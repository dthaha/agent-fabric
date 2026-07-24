# Agent Fabric

Enterprise agent continuity fabric. Open source, Apache-2.0.

**One line:** MDM-managed endpoint hands + leased session context + opportunistic memory + swappable brains — an open source continuity fabric for enterprise agents, online or off.

## What this is

Agent Fabric solves one problem: **strict session context continuity across loci** (endpoint, hosted, split, offline), with memory soft and tools sticky to the endpoint. An agent turn can start on a managed laptop, hand off to a hosted runtime for a long-horizon task, survive a device switch, and reconcile after an offline stretch — without losing a single context entry.

## What this is NOT

- NOT an LLM router
- NOT an AI gateway
- NOT a copilot

## Architecture (the planes)

```
1. Identity & trust     — enterprise IdP + device attestation (MDM enrollment)
2. Policy plane         — dual: endpoint MDM ceiling + hosted additive. DENY WINS.
3. Memory plane         — NO LEASE. Hosted SoT (Honcho-class) + opportunistic endpoint cache.
4. Context plane        — LEASED. Single-writer op-log. THE SPINE.
5. Runtime plane        — LEASED with context. Who runs the loop this turn.
6. Tool plane           — device-sticky, NO LEASE. Remote bridge for hosted brain. CUA lives here.
7. Inference plane      — swappable commodity. Hosted = admin-configured. Endpoint = seeded.
8. Model plane          — endpoint seeding. OS-native runtimes behind unified catalog.
9. Agent integration    — adapter (BYO) + day-0 harness (full features).
```

### Key invariants

- Context is leased (single writer); memory is NOT
- Tools are device-sticky; brain is movable
- Policy deny-wins: endpoint can tighten, NEVER loosen
- Offline classifier lives ON the endpoint (never calls home to decide where to think)
- Endpoint models are seeded per-OS runtime (MLX mac, ONNX win, llama.cpp linux)
- CUA actuator stays on endpoint; hosted brain calls it via authenticated tool bridge
- Handoff = transfer write lease + catch-up, NOT summarize-and-restart

## Repo layout

```
proto/              shared contracts (protobuf, buf-managed)
core/               continuity engine (always compiled)
enterprise/         enterprise features (feature-flagged, OPEN SOURCE)
endpoint/           endpoint binary (MDM-shipped)
hosted/             hosted runtime (k8s/docker/VM)
harness/            first-party reference agent (TypeScript)
sdk/                published packages
admin/              admin console (TypeScript web app)
docs/               ADRs, API reference, guides
deploy/             helm, docker, terraform
tests/              integration + e2e
```

## Build commands

```bash
make proto      # buf generate (proto/ → Rust gen files)
make endpoint   # cargo build --release -p fabric-endpoint
make hosted     # docker build -f deploy/docker/Dockerfile.hosted
make test       # cargo test --workspace
make check      # clippy + fmt
```

## License

All code in this repository is [Apache-2.0](LICENSE). VyOS-style model: the code is fully open; the commercial product is signed binaries + SLA + support, not code access. Enterprise features are compile-time cargo feature flags, never runtime paywalls.
