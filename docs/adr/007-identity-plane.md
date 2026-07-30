# ADR 007: Identity plane — Fabric as consumer, SOUL as the one native identity

- Status: accepted
- Date: 2026-07-29

## Context

The codebase has four identity strings with no formal model:

- `soul_id` on `SessionMeta` — the agent's persistent persona
- `user_id` on `SessionMeta` — the human user
- `holder_id` on `Lease` — the device holding the write lease
- `org_id` on `SessionMeta` and policy protos — the organization boundary

All are opaque strings. All are currently caller-supplied. There is no
registry, no resolution logic, and no defined relationship between them.
ADR 003 established that `holder_id` derives from the JWT `sub` claim,
but the broader identity model was left undefined.

### Constraint

Fabric is a control plane, not an identity provider. Customers bring
their own IdP (Entra, Okta, Authentik — ADR 003). Fabric must not
create a parallel user directory, device enrollment system, or org
hierarchy. That would be the mTLS anti-pattern applied to identity:
building a second trust kingdom alongside the customer's existing one.

## Decision

### Four identity types, one issuer each

| Identity | Issued by | Fabric's role |
|---|---|---|
| **User** | Customer's IdP | Consumer. Reads `sub` from validated JWT (ADR 003). Never mints users. |
| **Device** | Customer's IdP / MDM enrollment | Consumer + registry. Maps JWT `sub → endpoint record` in a server-side device registry. |
| **SOUL** | **Fabric** | **Creator and sole authority.** The one identity Fabric mints. |
| **Organization** | Customer (MDM policy packs, server config) | Consumer. Reads `org_id` from policy packs (ADR 005) and server configuration. |

### Relationship model

```
Org (customer-defined, from policy/config)
 └── Users (from IdP, identified by JWT sub)
      ├── SOULs (Fabric-created, 1:1 per user per org by default)
      │    └── Sessions (many per SOUL, transient)
      │         └── Lease → Device (one active writer, ADR 002/004)
      └── Devices (IdP-enrolled, Fabric device registry)
```

### SOUL: the Fabric-native identity

The SOUL is the agent's persistent persona, memory anchor, and continuity
identity. It is the only identity concept that no external system
understands. Fabric creates it, Fabric manages it, Fabric is the
authority.

Lifecycle:

- **Creation**: on first authenticated session for a `(user_sub, org_id)`
  pair. The server resolves or creates the SOUL automatically. The client
  never supplies `soul_id`.
- **Identity**: `soul_id` is a Fabric-generated UUIDv4, stored server-side
  (Postgres, ADR 004).
- **Cardinality**: one SOUL per user per org by default. This is the
  "one memory, one context, one SOUL" principle from the architecture.
  Multiple SOULs per user is a future capability (e.g., separate work
  and personal personas), not v1.
- **Persistence**: sessions come and go; the SOUL persists. The SOUL is
  the anchor for memory (Honcho-class memory plane), not the session.
  Context entries belong to sessions; memories belong to SOULs.
- **Deletion**: SOUL deletion = full memory wipe. This is the GDPR
  right-to-erasure implementation. Deleting a SOUL cascades to all
  sessions and memories. The op-log entries are retained for audit
  (immutable) but stripped of SOUL attribution.
- **Offline**: the endpoint caches `soul_id` locally. It does not need
  to resolve the SOUL on every request — the server resolves it from
  the authenticated identity.

### All identities are server-derived

The client supplies none of the four identity fields. All are resolved
server-side:

| Field | Source | Resolution |
|---|---|---|
| `user_id` | JWT `sub` claim (ADR 003) | Extracted by auth middleware |
| `holder_id` | JWT `sub` claim (ADR 003) | Same as `user_id` for user-auth; device `sub` for client-creds |
| `org_id` | Policy pack / server config | Mapped from IdP claims or MDM policy |
| `soul_id` | Fabric SOUL registry | Resolved from `(user_id, org_id)` |

The proto fields remain for wire compatibility but are ignored when
auth is enabled (same pattern as ADR 003's `holder_id` deprecation).

### Device registry

The server maintains a `devices` table (ADR 003) mapping:

```
device_sub (JWT sub) → {
    device_id:      Fabric-generated UUID,
    display_name:   from JWT claims or admin-set,
    org_id:         from policy/config,
    enrolled_at:    first-seen timestamp,
    last_seen_at:   most recent authenticated request,
    platform:       from JWT claims or user-agent (macos/windows/linux),
    status:         active | revoked
}
```

This is a *cache* of IdP-enrolled devices, not a directory. Fabric does
not enroll devices. The IdP/MDM does that. Fabric records "this device
talked to me" and lets admins revoke access.

### Org resolution

`org_id` is resolved from:

1. **MDM policy pack** (ADR 005): the `OrgID` key in the endpoint policy
2. **Server config**: `FABRIC_ORG_ID` for single-org deployments
3. **JWT claims**: `org_id` or tenant claim if the IdP provides one
   (Entra: `tid`, Okta: `org_id` custom claim)

Priority: policy pack > server config > JWT claim. The MDM pack is the
most authoritative source because it's admin-deployed and device-bound.

### What Fabric does NOT build

- **User directory / SCIM provisioning.** The IdP owns users. Fabric
  reads claims.
- **Device enrollment.** MDM/IdP enrolls devices. Fabric records them.
- **Org hierarchy.** The customer's IdP or MDM defines org structure.
  Fabric reads `org_id`.
- **Group/role management.** The IdP manages groups. Fabric can read
  group claims from JWTs for policy evaluation, but does not manage
  them.
- **SOUL federation across orgs.** A user in two orgs has two SOULs.
  Cross-org SOUL merging is not a thing.

### Future considerations

- **Multiple SOULs per user**: work/personal/project personas. The
  `(user_id, org_id)` key becomes `(user_id, org_id, persona)`. Not v1.
- **SOUL transfer**: migrating a SOUL (and its memories) between orgs.
  GDPR data portability. Not v1.
- **Group-based policy**: reading IdP group claims from JWTs to apply
  group-level policy rules. The policy engine (ADR 002/005) already
  supports arbitrary rule matching; group claims would feed into it.
- **SCIM event ingestion**: listening to IdP SCIM events for real-time
  user/device lifecycle (deprovisioning → immediate SOUL revocation).
  Not v1; polling `last_seen_at` + admin revocation is sufficient.

## Consequences

- Server gains a `souls` table: `soul_id`, `user_id`, `org_id`,
  `created_at`, `deleted_at` (soft delete for GDPR).
- Server gains SOUL resolution middleware: after auth (ADR 003), resolve
  `(user_id, org_id) → soul_id`, create if absent.
- `SessionMeta.soul_id` becomes server-resolved, not caller-supplied.
- `SessionMeta.user_id` becomes the JWT `sub`, not caller-supplied.
- `SessionMeta.org_id` becomes policy/config-resolved, not caller-supplied.
- The device registry (ADR 003) is formalized with the schema above.
- Memory plane keys on `soul_id`, not `session_id` or `user_id`.
- GDPR erasure: `DELETE /souls/{soul_id}` cascades to sessions and
  memories, retains audit log.
- Proto fields remain for wire compatibility but are server-overridden
  when auth is enabled.

## References

- ADR 002 (conflict resolution, offline-first, session model)
- ADR 003 (control plane auth, JWT `sub`, device registry)
- ADR 004 (server store split, Postgres for relational state)
- ADR 005 (MDM policy delivery, `org_id` from policy packs)
- ADR 006 (replay trust model, origin attribution via `sub`)
- AGENTS.md ("one SOUL per session", memory plane architecture)
