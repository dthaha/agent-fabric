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

## Safety pipeline

Agent Fabric is a **control plane**, not an inference plane. It never runs safety models itself. Instead:

1. The customer deploys a safety model on their own inference cluster (vLLM, TGI, Bedrock, etc.)
2. The fabric calls the customer's safety endpoint via HTTP
3. The fabric **parses** the model's output into a canonical `SafetyVerdict`
4. The **policy engine** maps detected categories to enforcement actions (block / force-endpoint / warn / allow)

The safety taxonomy is a **policy artifact**, not a model artifact. The same model serves different organizations with different policy packs — a hospital flags PHI, a law firm flags privileged comms, a family flags minor-safety.

```
intent_text
    ↓
[Customer's safety endpoint] → raw model output
    ↓
[SafetyParser] → SafetyVerdict { verdict, categories, explanation }
    ↓
[Policy engine] → per-category enforcement (block / force-endpoint / warn / allow)
    ↓
[Load classifier] → locus suggestion (RouteLLM-style, on safety-cleared intent)
    ↓
[Rules engine] → hard constraints + advisory merge
    ↓
[Policy wrapper] → final veto
    ↓
LocusDecision
```

### Supported safety models

The fabric ships output parsers for these models. Deploy any of them on your inference infrastructure and point `safety_endpoint_url` in your policy pack at it.

| Model | Params | License | Output format | Parser |
|---|---|---|---|---|
| [IBM Granite Guardian 3.1 2B](https://huggingface.co/ibm-granite/granite-guardian-3.1-2b) | 2B | Apache-2.0 | `safe` / `unsafe` + risk categories (harm, PII, injection, profanity) | `GraniteGuardianParser` |
| [IBM Granite Guardian 3.3 8B](https://huggingface.co/ibm-granite/granite-guardian-3.3-8b) | 8B | Apache-2.0 | Same as 2B, higher accuracy | `GraniteGuardianParser` |
| [Meta Llama Guard 3 1B](https://huggingface.co/meta-llama/Llama-Guard-3-1B) | 1B | Llama 3.2 Community | `safe` / `unsafe\nS1,S2,...` category codes | `LlamaGuardParser` |
| [Meta Llama Guard 3 8B](https://huggingface.co/meta-llama/Llama-Guard-3-8B) | 8B | Llama 3.2 Community | Same as 1B | `LlamaGuardParser` |
| [Google ShieldGemma 2B](https://huggingface.co/google/shieldgemma-2b) | 2B | Gemma | Probability-based safe/unsafe with harm categories | `ShieldGemmaParser` |

All parsers implement the `SafetyParser` trait. Adding a new model = implement one trait method that maps raw text → `SafetyVerdict`.

### Configuration

```jsonc
// In your endpoint policy pack:
{
  "safety": {
    "endpoint_url": "https://your-inference-cluster/v1/chat/completions",
    "model": "ibm-granite/granite-guardian-3.1-2b",
    "parser": "granite_guardian",
    "timeout_ms": 5000,
    "fail_mode": "closed"  // "closed" = block on error, "open" = allow on error
  }
}
```

### Endpoint-side (future)

When the endpoint daemon runs client-side, the safety model is seeded locally via the model catalog and inferred on-device through llama.cpp. Same `SafetyVerdict` schema, same policy rules — but inference is local, no round-trip to hosted. The endpoint never calls home to decide if content is safe.

## License

All code in this repository is [Apache-2.0](LICENSE). VyOS-style model: the code is fully open; the commercial product is signed binaries + SLA + support, not code access. Enterprise features are compile-time cargo feature flags, never runtime paywalls.
