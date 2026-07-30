# ADR 006: Replay trust model — accept everything, verify on reconnect

- Status: accepted
- Date: 2026-07-29

## Context

Offline-first is the fabric's premise (ADR 002): an endpoint that loses
connectivity keeps working and commits real turns to its local op-log.
When it reconnects, `reconcile()` merges the endpoint's diverged branch
into the server's authoritative log.

The current implementation (`core/context/src/reconcile.rs`) accepts all
remote entries via `insert_entry_raw()`, which explicitly bypasses lease
checks. The endpoint's `created_at` timestamp is preserved verbatim and
used as the deterministic tiebreaker for seq collision resolution:
`(created_at, entry_id)` — earlier wins.

This creates three trust gaps:

1. **Timestamp forgery**: `created_at` is an untrusted endpoint claim.
   A malicious or compromised endpoint can backdate entries to win seq
   collisions and control merge ordering.
2. **Policy drift**: an endpoint offline during a policy change can
   replay entries that violate the current policy. No re-evaluation
   occurs on ingest.
3. **No origin attribution**: entries are accepted from "a store" with
   no binding to the authenticated identity presenting them (ADR 003
   authenticates the API call but the reconcile path doesn't use it).

### Constraint

Offline-first means the endpoint IS sovereign while dark. The server
cannot prevent offline work. It can only decide what it *believes* and
*how it orders* entries on reconnect. Dropping entries violates the
core premise — all work is preserved.

## Decision

### Accept everything, never drop

All entries presented by a reconnecting endpoint are accepted into the
op-log. Offline-first is non-negotiable. The server's job is to order,
verify, and flag — not to gatekeep what the endpoint did while sovereign.

### Server-stamped `received_at` is authoritative for ordering

Every entry ingested via reconcile gets a server-stamped `received_at`
timestamp. This is the authoritative time for cross-replica ordering.

| Timestamp | Source | Authority |
|---|---|---|
| `created_at` | Endpoint clock | Preserved as a *claim*. Useful for auditing and display. NOT used for merge ordering. |
| `received_at` | Server clock | Authoritative for cross-replica seq collision resolution. |

Seq collision resolution changes from `(created_at, entry_id)` to
`(received_at, entry_id)`. A backdated endpoint clock no longer
influences merge ordering. Within a single replica, local seq is already
authoritative and unchanged.

### Policy re-evaluation on replay

Replayed entries are re-evaluated against the *current* effective policy
(MDM ceiling merged with server policy, deny-wins). This catches:

- Entries that were legal under the old policy but violate a tightened
  policy (e.g., tool access revoked while endpoint was offline)
- Entries that violate DLP patterns added during the offline window

Violating entries are NOT dropped. They are flagged with
`disposition: QUARANTINE` in the op-log. The entry is preserved,
auditable, and visible to admins. The quarantine disposition feeds into
the existing audit/SIEM export path.

### Origin attribution via ADR 003 identity

The reconcile API endpoint requires authentication (ADR 003). The `sub`
claim from the validated JWT identifies the presenting device. All
replayed entries are attributed to that identity. The device registry
(ADR 003) maps `sub → endpoint`, so the server knows which physical
device is presenting which entries.

### Structural conflict detection (unchanged)

Tier 1 structural detection already runs over the merged region
(`detect_in_region`). Same tool + same target + different params →
conflict. This is model-free and deterministic. Ambiguous conflicts
escalate to the model pipeline (decoder → mediator → policy veto).
No change needed.

### Dedup (unchanged)

`entry_id` dedup prevents naive replay attacks (re-presenting the same
entries). Already implemented. No change needed.

### Summary of changes

| What | Current | New |
|---|---|---|
| Seq collision tiebreaker | `(created_at, entry_id)` | `(received_at, entry_id)` |
| `received_at` field | Does not exist | Server-stamped on reconcile ingest |
| Policy check on replay | None | Re-evaluate against current effective policy |
| Violating entries | Accepted silently | Accepted + `disposition: QUARANTINE` |
| Origin attribution | None | `sub` from ADR 003 JWT → device registry |
| Entry acceptance | All accepted | All accepted (unchanged) |
| Structural detection | Runs on merged region | Runs on merged region (unchanged) |
| Dedup | By `entry_id` | By `entry_id` (unchanged) |

### What this does NOT do

- **Drop entries.** Never. Offline work is preserved.
- **Reject replay.** The endpoint is sovereign while offline.
- **Validate endpoint clock accuracy.** Clock skew is expected.
  `received_at` makes it irrelevant for ordering.
- **Cryptographic entry signing.** The endpoint's JWT (ADR 003)
  authenticates the *session*. Individual entries are not signed.
  If the endpoint is compromised, the attacker can fabricate entries
  regardless of signing — they control the signing key.

### Future: EDR integration (not v1)

Enterprise endpoints run EDR agents (CrowdStrike, SentinelOne, Defender
for Endpoint, Carbon Black). These provide process-level attestation:
which binary made which syscall, when, with what arguments.

A future `EdrAttestation` trait could allow the server to cross-reference
replayed entries against EDR telemetry:

- "Endpoint claims it ran `shell.exec` at 14:32" → EDR confirms process
  creation event at 14:32 from the fabric daemon binary
- "Endpoint claims file write to `/etc/passwd`" → EDR flags this as
  anomalous regardless of what the op-log says

This is a verification *enrichment*, not a gate. Entries are still
accepted (offline-first). EDR data adds confidence metadata and can
upgrade a QUARANTINE disposition to a higher-severity flag.

Not a v1 priority. The trait boundary (`EdrAttestation`) can be defined
when the first customer asks for it. The reconcile pipeline already has
the extension point: policy re-evaluation is a pass over entries, and
EDR correlation would be another pass.

## Consequences

- `ContextEntry` proto gains a `received_at` field (wire-compatible
  addition, new tag number).
- `reconcile()` gains a policy re-evaluation step after merge.
- `insert_entry_raw()` stamps `received_at` when called from the
  reconcile path. Direct local inserts (endpoint-side) leave it unset.
- Seq collision resolution in `reconcile()` changes tiebreaker from
  `created_at` to `received_at`.
- Quarantine disposition added to the entry metadata (or a parallel
  `dispositions` table keyed by `entry_id`).
- The reconcile API handler extracts `sub` from the ADR 003 auth
  middleware and passes it through for attribution.
- Existing tests that assert `(created_at, entry_id)` ordering need
  updating to `(received_at, entry_id)`.
- New tests: backdated endpoint entries lose seq collisions; policy-
  violating replayed entries get QUARANTINE; dedup still works;
  reconcile is still idempotent.

## References

- ADR 002 (conflict resolution, offline-first, deterministic merge)
- ADR 003 (control plane auth, `sub` claim, device registry)
- ADR 005 (MDM policy delivery, dual policy model)
- Current implementation: `core/context/src/reconcile.rs`
- Review findings: replay trust model (item 4)
