# Port Spec: Fabric terminology + Rust→Go

Status: draft — governs the Go reimplementation. The Rust tree is frozen;
do not apply these changes there.

## 1. Terminology (frozen by proto, source of truth)

"endpoint" has one meaning in Fabric: **the managed device daemon**
(the agent's local hands). Proto freezes it:

| Wire symbol | File | Sense |
|---|---|---|
| `EndpointPolicy` | policy.proto | MDM-shipped policy ceiling for the device |
| `LOCUS_ENDPOINT` | context.proto | locus = run on the device |
| `endpoint_url`, `endpoint_version` | policy.proto | device policy pack identity |
| `TOOL_LOCALITY_ENDPOINT_ONLY` | tools.proto | tool must not leave the device |
| `SAFETY_ACTION_FORCE_ENDPOINT` | policy.proto | safety action: force device-side |

Never rename these. Wire compat and AGENTS rule #1 (proto is source of
truth) both forbid it.

## 2. Banned collisions in Go prose/comments

"endpoint" must NOT mean:

- **HTTP route** → say **route** (`/healthz`, `/readyz`, `/policy`, ...)
- **inference API URL** → say **inference server** / **provider**
  (OpenAI-compatible chat completions server; provider-agnostic)

Device-sense usages that are fine: "the endpoint replays its op-log",
"endpoint policy", "weak-endpoint case", `Locus::Endpoint`.

## 3. Vocabulary glossary (Go port)

| Term | Meaning |
|---|---|
| endpoint | the managed device running fabric-daemon |
| surface | a place a user interacts from (device, web, CLI) — presence signal |
| locus | where a turn runs: endpoint / server / split / offline |
| route | an HTTP handler on a control-plane or daemon API |
| inference server | any OpenAI-compatible completions API (local or hosted) |
| lease | server-granted single-writer authority over session context |

## 4. Go port layout (mirrors repo structure)

```
core/      shared engine (context, policy, classifier, telemetry, types)
endpoint/  device binaries (daemon, cli, mdm) — CGO_ENABLED=0 static
server/    leased services (control, agent, catalog) — Postgres + Valkey
sdk/       published packages
proto/     buf-managed, unchanged — generates Go via protoc-gen-go
```

Inference backends (MLX / ONNX / llama.cpp) run as **sidecar processes**
behind their localhost HTTP APIs; the daemon never embeds them.

## 5. Context assembly (ADR 009) — governs the Go context plane

Conflict decode/mediate never receives an unbounded raw window. Assembly
is: `checkpoint + per-locus tails + raw[-N] + conflict pair`, token-
budgeted, oldest-first trim.

Frozen vocabulary for the Go port:

| Term | Meaning |
|---|---|
| checkpoint | server-computed summary of the converged spine `[0..covered_through_seq]`; chain-versioned; distributed via catch-up/handoff; clients cache, never compute |
| tail summary | ephemeral per-locus digest of that locus's unsynced branch; mediation-scoped; server recomputes or accepts per policy; never persisted as authority |
| assembly | the bounded context handed to decoder/mediator tiers |
| coverage | seq-range `[start, end]` + version carried by every summary node |

Rules the Go port must preserve:

1. Summaries are **derived** — seq-range-keyed store, never op-log entries.
   The spine stays append-only, verbatim, single-writer.
2. Checkpoint authority is server-only. Clients that compute their own
   checkpoint are divergent by definition (non-deterministic LLM digests).
3. Tail summaries are untrusted endpoint claims — treat like `created_at`
   (ADR 006): accept or recompute per policy.
4. In-flight structure is a depth-2 tree (checkpoint → tails → raw), not a
   DAG store. Drill-down is seq-range fetch against the spine.
5. Summarizer is an interface with a deterministic truncate default; LLM
   summarizers plug into the same seam. Assembly never requires a model.
6. Handoff invariant unchanged: lease-transfer + catch-up, never
   summarize-and-restart.
