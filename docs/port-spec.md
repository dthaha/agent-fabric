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
