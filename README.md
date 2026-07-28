# Agent Fabric

Enterprise agent continuity fabric. Open source, Apache-2.0.

**One line:** MDM-managed endpoint hands + leased session context + opportunistic memory + swappable brains — an open source continuity fabric for enterprise agents, online or off.

## What this is

Agent Fabric solves one problem: **strict session context continuity across loci** (endpoint, server, split, offline), with memory soft and tools sticky to the endpoint. An agent turn can start on a managed laptop, hand off to a server-side runtime for a long-horizon task, survive a device switch, and reconcile after an offline stretch — without losing a single context entry.

## What this is NOT

- NOT an LLM router
- NOT an AI gateway
- NOT a copilot

## Architecture (the planes)

```
1. Identity & trust     — enterprise IdP + device attestation (MDM enrollment)
2. Policy plane         — dual: endpoint MDM ceiling + server additive. DENY WINS.
3. Memory plane         — NO LEASE. Server-side SoT (Honcho-class) + opportunistic endpoint cache.
4. Context plane        — LEASED. Single-writer op-log. THE SPINE.
5. Runtime plane        — LEASED with context. Who runs the loop this turn.
6. Tool plane           — location-transparent. Same interface at every locus. Terminal = catch-all, CUA = escape hatch.
7. Inference plane      — swappable commodity. Server = admin-configured. Endpoint = seeded.
8. Model plane          — endpoint seeding. OS-native runtimes behind unified catalog.
9. Agent integration    — adapter (BYO) + day-0 harness (full features).
```

### Key invariants

- Context is leased (single writer); memory is NOT
- Tools are location-transparent; the brain never knows where a tool ran
- Policy deny-wins: endpoint can tighten, NEVER loosen
- Offline classifier lives ON the endpoint (never calls home to decide where to think)
- Endpoint models are seeded per-OS runtime (MLX mac, ONNX win, llama.cpp linux)
- The container image IS the capability manifest — same image at every locus
- Handoff = transfer write lease + catch-up, NOT summarize-and-restart

## Status: Landed vs Scaffold

| Component | Status | Notes |
|---|---|---|
| core/context (conflict pipeline) | ✅ Landed | Op-log, lease, handoff, reconcile, decoder/mediator, eval |
| core/classifier (safety) | ✅ Landed | Parsers, client, policy enforcer |
| core/policy | ✅ Landed | Deny-wins merge, veto, compensation |
| core/tools | ✅ Landed | Dispatch, terminal tool (K8s containers) |
| core/telemetry | ✅ Landed | OTel + JSON stdout |
| core/models | 🔨 Partial | Module registry + feature-gated discovery |
| server/control (lease authority) | ✅ Landed | `fabric-control` bin, axum routes, SQLite store |
| endpoint/daemon | 🔨 Partial | Health, classify, lease client; safety/pipeline not yet wired |
| server/agent | 📋 Scaffold | Stub main, lands in a later phase |
| core/inference, core/runtime, core/memory | 📋 Scaffold | Placeholder crates |
| sdk/*, admin/, harness/ | 📋 Scaffold | Empty, planned |

## Repo layout

```
proto/              shared contracts (protobuf, buf-managed)
core/               continuity engine (always compiled)
enterprise/         enterprise features (feature-flagged, OPEN SOURCE)
endpoint/           endpoint binary (MDM-shipped)
server/             server-side runtime (k8s/docker/VM)
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
make server     # docker build -f deploy/docker/Dockerfile.server
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
    // Base URL, API root, or full path — all three work:
    // "https://your-inference-cluster", ".../v1", or ".../v1/chat/completions"
    "endpoint_url": "https://your-inference-cluster/v1/chat/completions",
    "model": "ibm-granite/granite-guardian-3.1-2b",
    "parser": "granite_guardian",
    "timeout_ms": 5000,
    "fail_mode": "closed",  // "closed" = block on error, "open" = allow on error
    "api_key": "",          // optional Bearer token for the safety endpoint
    "extra_body_json": "",  // optional JSON object of vendor request extensions
    "system_prompt": ""     // optional override; empty = parser's default
  }
}
```

### Endpoint-side (future)

When the endpoint daemon runs client-side, the safety model is seeded locally via the model catalog and inferred on-device through llama.cpp. Same `SafetyVerdict` schema, same policy rules — but inference is local, no round-trip to the server. The endpoint never calls home to decide if content is safe.

## Model Modules

First-class model modules compile into the binary via Cargo features. Each module implements one of the three scoped inference traits (safety parser, conflict decoder, conflict mediator) and is enabled by default; strip a feature to remove the module from the build entirely.

| Module | Model | Task | Feature flag | Eval results (July 2026) |
|---|---|---|---|---|
| `nemotron_cs` | NVIDIA Nemotron 3.5 Content Safety | Content safety | `safety-nemotron-cs` | 100% recall, 97.3% precision, F1 0.986 (60 scenarios) |
| `llama_guard` | Meta Llama Guard 4 12B | Content safety | `safety-llama-guard` | 89.5% recall, 94.4% precision, F1 0.919, 0.33s p50 |
| `granite_guardian` | IBM Granite Guardian 3.x | Content safety | `safety-granite-guardian` | See `eval/results/` |
| `shield_gemma` | Google ShieldGemma 2B | Content safety | `safety-shield-gemma` (off by default) | See `eval/results/` |
| `constrained_decoder` | NVIDIA Nemotron 3 Nano 30B A3B | Conflict decoder (Tier 2) | `decoder-nemotron` | 65.3% accuracy, 100% schema compliance (`reasoning: none`) |
| `constrained_mediator` | NVIDIA Nemotron 3 Nano 30B A3B | Conflict mediator (Tier 3) | `mediator-nemotron` | 54.4% resolution, 84.8% kind accuracy (`reasoning: high`) |

Runtime discovery: `fabric_models::available_modules()` returns the `ModuleInfo` for every module compiled into the current binary, so pipelines and admin tooling can enumerate what's actually present.

**Adding a custom module:** implement the relevant trait (`SafetyParser`, `ConflictDecoder`, or `ConflictMediator`) in your own crate, construct it where the pipeline is built, and point it at your endpoint. No fork required.

**Certification:** the eval suite in `eval/` is the conformance bar. A module is "first-class" when it ships behind a feature flag AND passes its task's eval harness. Eval results are generated locally and gitignored — run the suite yourself to produce them.

## Extensibility

The fabric has exactly three scoped inference tasks, each a single pluggable trait:

| Task | Trait | Crate |
|---|---|---|
| Content safety | `SafetyParser` | `fabric-classifier` |
| Conflict decoder (Tier 2) | `ConflictDecoder` | `fabric-context` |
| Conflict mediator (Tier 3) | `ConflictMediator` | `fabric-context` |

**Bring your own model:** implement the trait, point it at your inference endpoint (OpenAI-compatible, NIM, vLLM, Bedrock, Foundry — anything), and wire it into your binary. The traits are small and stable; the eval harness treats your implementation exactly like a first-class module.

**Feature flag stripping for regulated environments:** every first-class module is gated behind a Cargo feature. Build with `--no-default-features --features safety-llama-guard` (for example) to produce a binary containing only the approved module — the stripped code is not in the artifact at all, which is auditable in a way runtime config is not.

**The eval harness as a conformance test:** `eval/` contains the scenario suites and runners for all three tasks. Run your implementation against the same scenarios; if it passes, it conforms. Eval results are generated locally and gitignored — run the suite yourself to produce them.

## Tool plane

Tools behave the same way regardless of where the brain runs. The brain calls `execute(ToolRequest)` and gets a `ToolResponse`. It never knows whether the tool ran on the endpoint or in a server-side container.

**Tool hierarchy:**

1. **Structured tools** (API/SDK) — Salesforce, Jira, Snowflake, internal APIs. Already centralized, reachable from any locus.
2. **Terminal** (catch-all) — sandboxed container shell. Anything with a CLI (~95% of enterprise work).
3. **Computer use** (escape hatch) — GUI-only legacy apps with no API or CLI. Out of scope until endpoint-side.

### Terminal tool

The terminal tool runs commands in an ephemeral OCI container. The org publishes one image with their tools (`kubectl`, `terraform`, `gh`, `aws-cli`, etc.), and the fabric runs it identically at every locus:

| Deployment | Runtime | Multi-host? |
|---|---|---|
| Enterprise server | Customer's K8s cluster | Yes — scheduler handles placement |
| Dev / CI | minikube | Single-node, same K8s API |
| Endpoint (future) | containerd direct (k3s or bare) | N/A — one device |

The container image is the capability manifest. `kubectl` is in the image or it isn't. No drift between loci.

### Dependencies

**Server-side requires a Kubernetes cluster.** The fabric talks to the K8s API (via [kube-rs](https://github.com/kube-rs/kube)) to create ephemeral containers for terminal execution. The fabric does not schedule containers — the cluster's scheduler handles placement. The fabric does not own the cluster — the customer does.

For development, [minikube](https://minikube.sigs.k8s.io/) provides a single-node K8s cluster locally.

**Endpoint-side (future)** talks to containerd directly via gRPC — no scheduler needed on a single device. macOS endpoints use Apple's OCI-compatible container runtime or Docker Desktop's embedded containerd.

### Container registry binding

Each org/user binds a container image and registry credentials in their policy pack:

```jsonc
{
  "terminal": {
    "image": "registry.customer.com/fabric/sandbox:latest",
    "registry_auth": "vault://org/registry-token",
    "resources": {
      "cpu": "2",
      "memory": "4g",
      "network": "restricted",
      "timeout_s": 300
    }
  }
}
```

The fabric pulls the image, creates an ephemeral container with the specified resource limits, executes the command, streams output, and tears down. The image is the customer's artifact — the fabric never owns it.

## Observability

The fabric emits OpenTelemetry. Set `OTEL_EXPORTER_OTLP_ENDPOINT` to ship traces to your backend (Tempo, Jaeger, Honeycomb, Datadog, etc.). Structured JSON logs always go to stdout for your collector. The fabric ships no log backend and no vendor SDK — you own the pipeline.

`RUST_LOG` controls verbosity (default `info`). Tool dispatch spans carry `request_id`, `session_id`, `lease_id`, and `tool_name` so traces correlate end-to-end across the tool plane.

## Conflict resolution model selection

Both the decoder (Tier 2) and mediator (Tier 3) use **NVIDIA Nemotron 3 Nano 30B A3B** via OpenRouter, selected after empirical evaluation (July 2026, 118 scenarios: 72 decoder + 46 mediator).

| Metric | Nemotron 3 Nano | Laguna XS 2.1 |
|---|---|---|
| Decoder accuracy | **65.3%** | 63.9% |
| Decoder schema compliance | **100%** | 91.7% |
| Mediator resolution | **54.4%** | 41.3% |
| Mediator kind accuracy | **84.8%** | 69.6% |
| Mediator schema compliance | **100%** | 76.1% |

Key findings:
- **Reasoning parameters are poison for classifiers.** Any reasoning effort (even `low`) causes models to burn tokens on CoT and mangle structured JSON output. The decoder MUST run with `reasoning: none`.
- **The mediator needs reasoning.** With `reasoning: high`, the mediator achieves 54.4% resolution and 84.8% kind accuracy — the policy veto (Tier 4) provides the safety floor.
- **Schema compliance is non-negotiable.** Nemotron achieves 100% on both tiers; Laguna breaks under reasoning (76.1% mediator schema).
- **Provider infrastructure drives latency, not model size.** Both are ~30B MoE / ~3B active params. Poolside serves Laguna first-party (606ms p50); Nemotron routes through Crusoe/DeepInfra/Novita (~3.6s p50). The 6x gap is serving infra, not architecture.
- **Nemotron is non-Chinese** (NVIDIA), satisfying the US government scrutiny constraint on Chinese open-weight models.

Configuration (env vars):
- `FABRIC_DECODER_MODEL` — default: `nvidia/nemotron-3-nano-30b-a3b`
- `FABRIC_MEDIATOR_MODEL` — default: falls back to decoder model
- `FABRIC_DECODER_EXTRA_BODY` — optional JSON object, vendor-specific request body extensions
- `FABRIC_MEDIATOR_EXTRA_BODY` — optional JSON object, vendor-specific request body extensions
- `OPENAI_BASE_URL` — OpenRouter: `https://openrouter.ai/api/v1`

### OpenRouter advisory

The eval results above were measured via OpenRouter. If you route through OpenRouter, set the following vendor extensions via `extra_body`:

**Decoder** (`FABRIC_DECODER_EXTRA_BODY`):
```json
{"reasoning": {"effort": "none"}, "provider": {"sort": "throughput"}, "top_k": 20}
```

**Mediator** (`FABRIC_MEDIATOR_EXTRA_BODY`):
```json
{"reasoning": {"effort": "high"}, "provider": {"sort": "throughput"}, "top_k": 20}
```

Why these matter on OpenRouter specifically:
- **`reasoning: {"effort": "none"}`** — OpenRouter defaults to some reasoning for Nemotron. Without explicitly disabling it, the decoder burns tokens on CoT and mangles structured JSON output (schema compliance drops from 100% to ~60%).
- **`reasoning: {"effort": "high"}`** — The mediator needs deep reasoning. Without it, resolution accuracy drops ~15 points.
- **`provider: {"sort": "throughput"}`** — Biases OpenRouter to the fastest-serving provider (Crusoe/DeepInfra/Novita for Nemotron).
- **`top_k: 20`** — Not in the OpenAI spec but supported by OpenRouter. Tightens sampling for the decoder.

If you use NVIDIA NIM, vLLM, Together, or another OpenAI-compatible backend, leave `extra_body` unset — the fabric sends only standard fields and the model works fine without these extensions.

## TODO

- [ ] Fine-tune Nemotron 3 Nano on synthetic conflict data (QLoRA, 4-bit) — same weights serve both decoder (reasoning off) and mediator (reasoning high) tiers
- [ ] Investigate first-party Nemotron serving (NVIDIA NIM) to close the latency gap vs Poolside

## License

All code in this repository is [Apache-2.0](LICENSE). VyOS-style model: the code is fully open; the commercial product is signed binaries + SLA + support, not code access. Enterprise features are compile-time cargo feature flags, never runtime paywalls.
