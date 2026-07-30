# ADR 004: Server store split — Valkey (RESP) leases + Postgres op-log

- Status: accepted
- Date: 2026-07-28

## Context

The server-side control plane (`fabric-control`) currently uses SQLite for
all state: leases, sessions, op-log entries, handoff markers. The code
review (findings C2, S2, M6) identified that every lease operation is
check-then-act with independent mutex lock/unlock per step — zero
`Connection::transaction()` calls despite the docstring claiming them.
Two concurrent `acquire_lease` calls both pass the conflict check, producing
dual writers on the spine.

SQLite is the correct store for the **endpoint** (embedded, offline,
single-writer per device). It is the wrong store for the **server**:

- Single-node file lock; no horizontal scaling
- No native TTL; lease expiry requires clock math and periodic sweeps
- No pub/sub; presence broadcasting requires polling or a sidecar
- Mutex-guarded `Connection` serializes all access through one thread
- WAL mode helps read concurrency but writers still serialize on the file

The lease authority is a coordination primitive: atomic check-and-set,
TTL-based expiry, presence events. This is the canonical use case for an
in-memory KV store with native TTL and pub/sub.

The op-log (context entries, sessions, audit trail) is a durable, ordered,
queryable append-only log with relational integrity. This is the canonical
use case for a relational database.

## Decision

### Two stores, one trait boundary

The server store splits into two backends behind separate traits:

```rust
/// Lease authority — hot path, KV-backed.
/// Atomic coordination: acquire, preempt, renew, release.
/// TTL-native. Sub-millisecond. Presence via pub/sub.
trait LeaseAuthority {
    async fn acquire(&self, session: &str, holder: &str, ttl: Duration) -> Result<Lease>;
    async fn preempt(&self, session: &str, new_holder: &str, ttl: Duration) -> Result<Lease>;
    async fn renew(&self, session: &str, holder: &str, ttl: Duration) -> Result<()>;
    async fn release(&self, session: &str, holder: &str) -> Result<()>;
    async fn active_lease(&self, session: &str) -> Result<Option<Lease>>;
}

/// Context store — cold path, relational.
/// Durable op-log: append, query, reconcile, replay.
trait ContextStore {
    async fn append_entry(&self, entry: &ContextEntry) -> Result<Seq>;
    async fn entries_since(&self, session: &str, seq: Seq) -> Result<Vec<ContextEntry>>;
    async fn session(&self, id: &str) -> Result<Option<Session>>;
    // ... reconcile, replay, handoff markers, etc.
}
```

The endpoint keeps `SqliteContextStore` for both roles (embedded, offline,
unchanged). The server uses `ValkeyLeaseAuthority` + `PostgresContextStore`.

### KV layer: RESP wire protocol, Valkey recommended

The KV layer targets the **RESP wire protocol** (Redis 7.2.4 command
syntax). The recommended implementation is **Valkey** (Linux Foundation,
BSD-3-Clause). Any RESP-compatible server works: Valkey, Redis ≤7.2,
managed offerings (ElastiCache, Memorystore, Azure Cache).

Why Valkey over Redis 7.4+:

- Redis 7.4+ is dual-licensed RSALv2 + SSPLv1 — neither is OSI-approved
  open source. Source-available with restrictions, not open source.
- Valkey is BSD-3-Clause, forked from Redis 7.2.4 (the last BSD release),
  maintained under the Linux Foundation with AWS/GCP/Oracle/Snap backing.
- Same wire protocol, same commands, same Lua scripting, same RDB/AOF
  persistence. The Rust client (`fred` or `redis-rs`) speaks RESP over
  TCP and cannot distinguish which server is on the other end.
- Fabric's ethos is Apache-2.0 code, no lock-in, OSI-clean dependencies.
  Recommending a non-OSI-licensed service dependency contradicts the
  narrative and triggers enterprise legal review.

This follows the same pattern as all other Fabric dependencies: target
the spec/protocol, recommend the permissively-licensed implementation,
accept anything compatible.

### Lease operations in Valkey

| Operation | Valkey primitive | Atomicity |
|---|---|---|
| `acquire` | `SET lease:{session} {holder} NX PX {ttl_ms}` | Single command, atomic by construction |
| `preempt` | Lua script: verify old holder → DEL → SET NX PX | Script is atomic (single-threaded) |
| `renew` | Lua script: verify holder → PEXPIRE | Script is atomic |
| `release` | Lua script: verify holder → DEL | Script is atomic |
| `active_lease` | `GET lease:{session}` | Single command |
| Presence | `PUBLISH presence:{session} {event}` | Fire-and-forget pub/sub |

TTL handles lease expiry natively. No clock math, no `is_expired()` helper,
no periodic sweep. When the key expires, the lease is gone. This eliminates
review findings C2 (no transactions), S2 (dual writers), and M6 (expiry
boundary inconsistency) by construction.

### Op-log in Postgres

Sessions, context entries, handoff markers, and audit trail live in
Postgres. Real transactions (`BEGIN` / `COMMIT` / `ROLLBACK`) for
multi-step operations:

- `append_entry`: verify writer + check seq + insert + tick clock — one txn
- `execute_handoff`: acquire + set state + mark old + insert marker — one txn
- `release_with_rollback`: find boundary + delete scoped entries + release — one txn
- `transition`: check state + validate + update — one txn

`created_at` is always server-stamped in `append_entry` (fixes the forging
issue from the review). Only `insert_entry_raw` (replay/reconcile path)
preserves the original timestamp.

### What this kills from the review

| Finding | Resolution |
|---|---|
| C2 — no transactions | Valkey SETNX is atomic; Postgres has real txns |
| S2 — dual writers via non-atomic preempt | Lua script or SETNX, single-threaded |
| M6 — expiry boundary inconsistency | Valkey TTL is the expiry, no clock math |
| Mutex serialization | Gone from server path; Valkey is the lock |

### What stays (still needs fixing, now in Postgres)

| Finding | Where |
|---|---|
| C1 — `ack_handoff` missing validation | Postgres session-state check |
| H — `release_with_rollback` scoping | Postgres txn with seq clamp |
| H — `created_at` forging | Postgres server-stamp in `append_entry` |

### Endpoint unchanged

The endpoint keeps SQLite for both lease and context roles. Embedded,
offline, single-writer per device. The `SqliteContextStore` remains as
the endpoint impl and as the test/dev default for the server. No endpoint
code changes.

### Configuration

```toml
# fabric-control config
[store]
# Lease authority
kv_url = "redis://valkey:6379"      # RESP endpoint (Valkey recommended)
kv_pool_size = 8

# Op-log
pg_url = "postgres://fabric:***@postgres:5432/fabric"
pg_pool_size = 16

# Dev/test fallback (single-node, no external deps)
# sqlite_path = "/var/lib/fabric/control.db"
```

When `sqlite_path` is set and `kv_url`/`pg_url` are absent, the server
falls back to `SqliteContextStore` for everything (dev mode, single-node
deployments, CI). This preserves the "single static binary, no runtime
deps" property for simple deployments.

### Future considerations

- **Valkey modules**: if presence needs become complex (presence history,
  last-seen queries), Valkey's data structures (sorted sets, streams)
  handle it without adding another dependency.
- **Postgres logical replication**: for multi-region server deployments,
  the op-log can replicate natively. Lease authority stays per-region
  (leases are inherently local to the coordinating server).
- **etcd**: if a customer's infra is Kubernetes-native and they prefer
  etcd over Valkey, the `LeaseAuthority` trait can be implemented against
  etcd's lease primitives (`lease grant`, `lease keepalive`, `lease
  revoke`). Not a v1 target, but the trait boundary allows it.

## Consequences

- `core/context` gains a `LeaseAuthority` trait alongside `ContextStore`.
- `server/control` wires `ValkeyLeaseAuthority` + `PostgresContextStore`.
- `endpoint/daemon` continues using `SqliteContextStore` (unchanged).
- New crate or module: `core/context/src/valkey.rs` (or a separate
  `server/store/` crate) for the Valkey and Postgres impls.
- `Cargo.toml` gains `fred` (or `redis-rs`) and `tokio-postgres` (or
  `sqlx`) as optional deps behind a `server-store` feature flag.
- Deploy manifests (Helm, docker-compose) add Valkey and Postgres services.
- CI tests use `SqliteContextStore` fallback (no external services needed).
- Integration tests for Valkey/Postgres impls run in a separate CI job
  with service containers.

## References

- Review findings C2 (no transactions), S2 (dual writers), M6 (expiry
  boundary inconsistency)
- ADR 001 (monorepo, Rust, licensing)
- ADR 002 (conflict resolution, offline-first, lease authority)
- ADR 003 (control plane auth, server as sole OIDC RP)
- Valkey: https://valkey.io (Linux Foundation, BSD-3-Clause)
- Redis license change: March 2024, RSALv2 + SSPLv1 (non-OSI)
- RESP protocol: Redis 7.2.4 command syntax (wire-compatible)
