# Agent Fabric — AGENTS.md

## What this is

Enterprise agent continuity fabric. NOT an LLM router, NOT an AI gateway, NOT a copilot.

**Core problem:** strict session context continuity across loci (endpoint, server, split, offline), with memory soft and tools sticky to the endpoint.

**One line:** MDM-managed endpoint hands + leased session context + opportunistic memory + swappable brains — open source continuity fabric for enterprise agents, online or off.

## Architecture (the planes)

```
1. Identity & trust     — enterprise IdP + device attestation (MDM enrollment)
2. Policy plane         — dual: endpoint MDM ceiling + server additive. DENY WINS.
3. Memory plane         — NO LEASE. Server-side SoT (Honcho-class) + opportunistic endpoint cache.
4. Context plane        — LEASED. Single-writer op-log. THE SPINE.
5. Runtime plane        — LEASED with context. Who runs the loop this turn.
6. Tool plane           — device-sticky, NO LEASE. Remote bridge for server-side brain. CUA lives here.
7. Inference plane      — swappable commodity. Server = admin-configured. Endpoint = seeded.
8. Model plane          — endpoint seeding. OS-native runtimes behind unified catalog.
9. Agent integration    — adapter (BYO) + day-0 harness (full features).
```

### Key invariants

- Context is leased (single writer); memory is NOT
- Tools are device-sticky; brain is movable
- Policy deny-wins: endpoint can tighten, NEVER loosen
- Offline classifier lives ON the endpoint (never calls home to decide where to think)
- Endpoint models are seeded per-OS runtime (MLX mac, ONNX win, llama.cpp linux)
- CUA actuator stays on endpoint; server-side brain calls it via authenticated tool bridge
- Handoff = transfer write lease + catch-up, NOT summarize-and-restart

### Locus decision matrix

| Condition | Loop | Inference | Tools |
|---|---|---|---|
| Default (endpoint capable) | endpoint | endpoint | endpoint |
| Endpoint too weak | endpoint/thin | server | endpoint |
| Long-horizon | server | server | endpoint via bridge |
| User "run in background" | server | server | endpoint/home |
| Offline | endpoint only | local only | local; defer rest |
| Device switch | lease handoff | per new device | new endpoint |

## Non-negotiables (coding rules)

1. **Proto is source of truth** for all wire formats. No hand-rolled serialization.
2. **All tool calls go through policy gate.** No bypasses.
3. **Tests for lease handoff + offline reconcile are MANDATORY** for any context changes.
4. **Model backends behind trait/interface.** No direct MLX/ORT/llama imports in router.
5. **Endpoint daemon = single static binary.** No runtime deps on managed laptops.
6. **All code Apache-2.0.** No license gates, no phone-home, no runtime checks.
7. **Enterprise features = compile-time cargo feature flags**, not runtime paywalls.
8. **Bite-sized commits.** One logical change per commit. Conventional commit messages.
9. **DRY, YAGNI, TDD.** Write failing test → implement → pass → commit.

## Repo layout

```
proto/              shared contracts (protobuf, buf-managed)
core/               continuity engine (always compiled)
  context/          session op-log, lease, handoff, reconcile (SQLite)
  policy/           dual policy engine (merge, eval, DLP)
  classifier/       offline locus classifier (rules + optional tiny model)
  memory/           soul loader, honcho adapter, endpoint cache
  runtime/          agent loop abstraction, handoff protocol, BYO adapters
  tools/            bridge server, CUA adapter, files, registry
  models/           catalog, seeding, router, backends (mlx/onnx/llama_cpp)
  inference/        server-side inference clients (openai_compat, bedrock, foundry)
enterprise/         enterprise features (feature-flagged, OPEN SOURCE)
  connectors/       Bedrock/Foundry deep auth, enterprise IdP
  mdm/              Intune/Jamf policy pack generators
  audit/            SIEM export, compliance reports
  ha/               multi-region failover, session replica
  catalog/          private model registry, air-gap signing
  admin/            polished console features
endpoint/           endpoint binary (MDM-shipped)
  daemon/           long-running agent service (Rust, static binary)
  cli/              admin/debug CLI
  mdm/              policy pack ingest
  installers/       pkg (mac), msi (win), deb/rpm (linux)
server/             server-side runtime (k8s/docker/VM)
  agent/            agent loop server
  control/          admin API (policy CRUD, audit, SOUL home)
  catalog/          model catalog service + artifact registry
harness/            first-party reference agent (TypeScript)
sdk/                published packages
  fabric-core/      core lib
  fabric-adapter/   BYO-agent adapter SDK
  fabric-tools/     tool bridge client SDK
  fabric-policy/    embeddable policy eval lib
admin/              admin console (TypeScript web app)
docs/               ADRs, API reference, guides
deploy/             helm, docker, terraform
tests/              integration + e2e (continuity, offline, policy, cua)
```

## Tech stack

| Component | Language | Why |
|---|---|---|
| Core + endpoint + server | **Rust** | Single static binary, cross-compile, no runtime deps |
| Harness + admin | **TypeScript** | Web UI, agent DX, fast iteration |
| Contracts | **Protobuf** (buf) | Language-neutral, schema evolution |
| Context store | **SQLite** (WAL mode) | Embeddable, op-log friendly, offline |
| Endpoint inference | MLX (mac), ONNX Runtime (win), llama.cpp (linux) | OS-native |
| CUA actuator | cua-driver (external) | Local desktop muscle |
| MDM delivery | Intune / Jamf | Existing enterprise truck |

## Build commands

```bash
make proto      # buf generate (proto/ → Rust gen files)
make endpoint   # cargo build --release -p fabric-endpoint
make server     # docker build -f deploy/docker/Dockerfile.server
make test       # cargo test --workspace
make check      # clippy + fmt
```

## Feature flags

```toml
# endpoint/daemon/Cargo.toml and server/agent/Cargo.toml
[features]
default = ["core"]
core = []
enterprise = ["mdm-intune", "mdm-jamf", "audit-siem", "ha-failover", "private-registry"]
mdm-intune = []
mdm-jamf = []
audit-siem = []
ha-failover = []
private-registry = []
```

Gate with `#[cfg(feature = "...")]`. Never runtime license checks.

## Licensing model (VyOS-style)

- ALL code is Apache-2.0 in one repo
- Core binaries: free, public download, signed
- Enterprise binaries: subscriber registry, signed
- The commercial product is signed binaries + SLA + support, NOT code access
- Anyone can `cargo build --features core,enterprise` themselves

## Implementation order

```
Phase 0:  Scaffold + proto contracts (context, lease, policy, catalog, tools)
Phase 1:  Context plane (op-log + lease + handoff + reconcile) ← SPINE
Phase 2:  Policy engine (dual, deny-wins, eval gate)
Phase 3:  Endpoint daemon skeleton
Phase 4:  Offline classifier
Phase 5:  Model plane (catalog + seeding + router)
Phase 6:  Tool bridge (remote RPC + CUA adapter)
Phase 7:  Server-side runtime
Phase 8:  Memory plane (SOUL + Honcho + cache)
Phase 9:  Reference harness
Phase 10: Admin console
Phase 11: Enterprise features (feature-flagged)
Phase 12: CI/CD + distribution
Phase 13: Integration tests + E2E
```

## Separate from

PAMAF / cf-think / family agent. No code or design reuse from those projects.
