# Operations: standing up the server backends

How to run the two stateful dependencies the control plane needs — **Postgres**
(the op-log) and **Valkey** (the lease authority) — plus the inference
endpoints it calls. This is the operator's counterpart to
[ADR 004](adr/004-server-store-split.md): that document explains *why* the
stores are split; this one explains *how to run them safely*.

The inference side (conflict decoder/mediator, content safety) is documented
in the [README](../README.md#conflict-resolution-model-selection) and is not
repeated here.

## The one thing to internalize: the two stores are not symmetric

Postgres and Valkey play opposite roles, and they must be operated
differently. Getting this wrong — e.g. backing up Valkey hard, or treating
Postgres as disposable — is the most expensive mistake an operator can make
here.

| | Postgres | Valkey |
|---|---|---|
| Role | **The spine.** Authoritative op-log: sessions, `context_entries`, SOUL + device registry. | **Coordination only.** Write-lease authority: who may write the spine this turn. |
| Data character | Durable, append-only, irreplaceable. | Ephemeral, TTL-native, self-healing. |
| If it dies | **Data loss.** The session history is gone. | **No data loss.** Leases expire; endpoints re-acquire on their next turn. |
| Persistence | Mandatory (WAL + base backups + PITR). | Optional (see [below](#persistence-none-by-design)). |
| Backup | Yes — this is the thing you protect. | No — there is nothing worth keeping. |

The lease is a *gate*, not a record. Every fact that matters is in Postgres;
Valkey only answers "is this holder allowed to write right now?" When a lease
key expires (or Valkey restarts empty), the answer becomes "no active lease,"
and the next `acquire`/`preempt` from an endpoint re-establishes it. Nothing
is lost. This is by design, not an accident to be fixed with persistence.

## Postgres — the op-log

### What the binary does on connect

`fabric-control` connects with a sqlx pool and runs the embedded migration
(`server/control/migrations/20260728000001_init.sql`) automatically via
`CREATE TABLE IF NOT EXISTS`. You do **not** need to run `sqlx migrate` or
supply a compile-time `DATABASE_URL` — the schema is compiled into the binary
and applied idempotently on startup. The SOUL/device registry shares the same
pool (one pool per instance, not two).

Tables created: `sessions`, `context_entries` (`UNIQUE(session_id, seq)`,
payload `BYTEA`), `souls`, `devices`. Timestamps are epoch-millis `BIGINT`.

### Connection & pool

- DSN via `FABRIC_PG_URL` (required). Example:
  `postgres://fabric:***@postgres:5432/fabric`
- Default pool size is **16** connections per `fabric-control` instance
  (`PostgresContextStore::connect`). The SOUL registry reuses this pool.
- Sizing rule: `instances × 16` must stay under Postgres `max_connections`
  (default 100). Five instances at the default = 80 connections — leave
  headroom for `psql`, backups, and migrations. Either raise
  `max_connections` or front the fleet with PgBouncer and lower the per-app
  pool.
- The pool is lazy-connecting and auto-reconnects; a Postgres blip surfaces
  as request errors, not a crashed binary.

### TLS & auth

Both ride the DSN — no code changes, sqlx honors the standard libpq
parameters:

```
postgres://fabric:***@postgres:5432/fabric?sslmode=verify-full&sslrootcert=/etc/fabric/ca.pem
```

- **Production: `sslmode=verify-full`** (or at minimum `require`). The op-log
  carries full session content; do not let it cross a network in cleartext.
- Use a dedicated `fabric` role with privileges on the four tables only —
  not a superuser. The binary only ever issues DML + the idempotent
  `CREATE TABLE IF NOT EXISTS` on first run.
- Keep the password out of the compose file and the process list: inject
  `FABRIC_PG_URL` from a secret manager at runtime.

### Backup & recovery — this is the job

Postgres is the only place the session history lives. Treat it like the
source of truth it is:

- **WAL archiving + base backups for PITR.** `pg_basebackup` on a schedule
  plus continuous WAL archiving (or a managed offering / `pgBackRest` /
  `WAL-G`) gives you point-in-time recovery. The op-log is append-only, so a
  PITR target "just before the bad write" is a clean restore.
- **Test restores.** A backup you have never restored is a hope, not a
  backup.
- Retention: the op-log grows with agent activity. Partition or archive old
  sessions once you have a retention policy; the schema does not yet
  partition by time.

### Migrations

The embedded migration is additive and idempotent (`IF NOT EXISTS`). Future
schema changes ship as new migration files; until a `sqlx migrate` runner is
wired into the binary, apply forward migrations with `sqlx migrate run`
against `FABRIC_PG_URL` before rolling a new binary. There is no automated
down-migration — back up before any manual schema change.

## Valkey — the lease authority

### What it stores

Two keys per active lease, both carrying the lease TTL:

- `lease:{session_id}` → the lease JSON
- `leaseid:{lease_id}` → `session_id` (reverse index for renew/tenancy)

When the TTL lapses, both keys vanish and the lease is gone. There is no
expiry sweep and no `is_expired()` check in the code — **Valkey's native TTL
is the expiry mechanism.** Every mutating operation is a single Lua script,
atomic under Valkey's single-threaded execution.

### Connection

- URL via `FABRIC_KV_URL` (required). Example: `redis://valkey:6379`
  (`redis://` scheme; Valkey speaks RESP and is wire-indistinguishable from
  Redis here).
- The code uses a **single multiplexed `fred` client, not a connection
  pool.** `fred` pipelines many concurrent requests over one connection, so
  there is no pool to size. (ADR 004's illustrative `kv_pool_size = 8` is a
  config sketch, not implemented in the current code.) Lease operations are
  tiny and sub-millisecond; one multiplexed connection is not the bottleneck.
- The client auto-reconnects after init. A Valkey blip surfaces as transient
  lease errors; endpoints retry on their next turn.

### TLS & auth

- **Auth:** put a password in the URL — `redis://:***@valkey:6379`. The dev
  compose runs Valkey with **no password**; never do that on a reachable
  network.
- **TLS:** use the `rediss://` scheme (fred enables TLS on `rediss`). Pair
  with auth — an unauthenticated, unencrypted lease authority lets anyone on
  the path grant themselves a write lease.

### Persistence: none, by design

Do **not** enable AOF/RDB persistence for the lease authority, and do not
back it up. The leases are deliberately ephemeral:

- A persisted lease that outlives a restart is a *stale* lease — it can
  name a holder that is no longer present, which is exactly the split-brain
  the TTL model exists to prevent.
- Restarting Valkey empty is the *correct* recovery: every session briefly
  has "no active lease," and the next presence/acquire from the real holder
  re-establishes truth from the live surfaces.
- The durable record of what happened is in Postgres. Valkey only governs
  the present moment.

If you run Valkey highly-available (replication / sentinel), that is for
*availability* of the coordination primitive, not durability — and a
failover that drops in-flight leases is still safe, because re-acquire heals
it.

## Network exposure

`FABRIC_CONTROL_ADDR` defaults to **`127.0.0.1:47800` — loopback only.** This
is intentional: the control plane trusts identity headers (ADR 007) and has
no inbound auth of its own yet, so it must never be bound to a public
interface directly. To serve traffic:

- Put an authenticating reverse proxy in front (mTLS or OIDC-terminating),
  and bind the proxy — not `fabric-control` — to the network.
- Only widen `FABRIC_CONTROL_ADDR` (e.g. `0.0.0.0:47800`) behind that proxy,
  on a private network segment.

Postgres and Valkey should likewise live on a private segment, reachable only
from the control plane and your backup tooling — never port-forwarded to the
public internet as the dev compose does.

## Development compose ≠ production

`deploy/docker-compose.yaml` is **dev provisioning only.** It is correct for
local development and wrong for production. Specifically, it:

- hardcodes `POSTGRES_PASSWORD: fabric` (and user/db `fabric`),
- runs Valkey with **no password**,
- enables **no TLS** on either store,
- **publishes both ports to the host** (`5432`, `6379`).

Use it to develop against. Do not point production `FABRIC_PG_URL` /
`FABRIC_KV_URL` at it. Production means: secret-injected credentials,
`sslmode=verify-full` on Postgres, `rediss://` + password on Valkey, private
networking, and Postgres backups running.

```bash
# Dev only:
docker compose -f deploy/docker-compose.yaml up -d
export FABRIC_PG_URL='postgres://fabric:***@localhost:5432/fabric'
export FABRIC_KV_URL='redis://localhost:6379'
```

## Environment reference

| Variable | Required | Default | Purpose |
|---|---|---|---|
| `FABRIC_PG_URL` | **yes** | — | Postgres op-log DSN (supports `?sslmode=`, `?sslrootcert=`). |
| `FABRIC_KV_URL` | **yes** | — | RESP lease-authority URL (`redis://` / `rediss://`, password in URL). |
| `FABRIC_CONTROL_ADDR` | no | `127.0.0.1:47800` | Bind address. Loopback-only by default. |
| `FABRIC_SERVER_IDENTITY` | no | `fabric-server` | Identity stamped into every lease's `granted_by`. |
| `FABRIC_ORG_ID` | no | `default` | Org fallback for single-org deployments. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | no | — | Ship OpenTelemetry traces to your backend. |
| `RUST_LOG` | no | `info` | Log verbosity. |

Inference env vars (`FABRIC_DECODER_MODEL`, `FABRIC_MEDIATOR_MODEL`,
`FABRIC_*_EXTRA_BODY`, `OPENAI_BASE_URL`) are documented in the
[README](../README.md#conflict-resolution-model-selection).

## High availability — current posture

Honest status: HA is **not implemented** in the binary. `deploy/helm/` and
`deploy/terraform/` are empty stubs. The stores themselves are
HA-capable (Postgres streaming replication / a managed HA pair; Valkey
sentinel or a managed cluster), and the control plane is stateless apart from
its store connections, so running multiple `fabric-control` instances behind
a load balancer is the intended scale-out — each just opens its own 16-conn
Postgres pool and its own Valkey client. What does not exist yet is automated
failover, health-gated readiness, or multi-region placement. Those belong in
the (currently stubbed) deploy manifests, not in the binary.
