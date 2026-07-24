# ADR 001: Monorepo, Rust core, VyOS-style open licensing

- Status: accepted
- Date: 2026-07-24

## Context

Agent Fabric is an enterprise agent continuity fabric spanning endpoint
binaries, a hosted runtime, shared wire contracts, SDKs, a reference agent,
and an admin console. We needed to decide on repo topology, the primary
implementation language, and the licensing/distribution model before
scaffolding.

## Decision

### Monorepo

One repository holds all planes: `proto/`, `core/`, `endpoint/`, `hosted/`,
`enterprise/`, `sdk/`, `harness/`, `admin/`, `deploy/`, `tests/`.

- The product is defined by cross-plane invariants (leased context,
  deny-wins policy, device-sticky tools). A change to a lease invariant
  touches proto, context plane, endpoint daemon, and hosted runtime
  atomically; a monorepo makes that one commit and one CI run.
- Proto contracts are the source of truth; co-locating generated code and
  consumers eliminates version skew between repos.
- Integration and E2E tests (continuity, offline, policy, CUA) need every
  component at the same revision.

### Rust for core, endpoint, and hosted

- The endpoint daemon must be a **single static binary** with no runtime
  dependencies on managed laptops (MDM-shipped to macOS, Windows, Linux).
  Rust cross-compiles and statically links cleanly.
- The context plane is the spine: lease enforcement and offline reconcile
  must be memory-safe and race-free; Rust's ownership model and SQLite
  (WAL) embedding fit directly.
- Model backends (MLX, ONNX Runtime, llama.cpp) all expose C APIs that Rust
  binds well, behind traits per our non-negotiables.
- TypeScript is used where it is genuinely better: the reference harness
  (agent DX) and the admin console (web UI).

### VyOS-style licensing (Apache-2.0, feature flags, signed binaries)

- **All code is Apache-2.0 in this one repo**, including enterprise
  features. There are no private mirrors of enterprise code.
- Enterprise capabilities are **compile-time cargo feature flags**
  (`mdm-intune`, `mdm-jamf`, `audit-siem`, `ha-failover`,
  `private-registry`), never runtime license checks, never phone-home.
- The commercial product is **signed binaries + SLA + support**: core
  binaries are free public downloads; enterprise binaries ship via a
  subscriber registry. Anyone can `cargo build --features core,enterprise`
  themselves — the value sold is trust, updates, and support, not secrecy.

## Consequences

- CI must build the full workspace; we pay with longer builds, mitigated by
  cargo caching and feature-gated enterprise compilation.
- Generated protobuf code is checked in under `core/context/src/gen/` and
  refreshed via `make proto`; drift is caught by CI (`buf lint` + build).
- License compliance is trivially auditable: one LICENSE file, one repo.
