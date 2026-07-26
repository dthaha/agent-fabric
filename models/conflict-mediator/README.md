# Conflict Mediator — Reference Model

Tier 3 of the conflict-resolution pipeline: the reasoning evaluator that reads a Tier 2
`ConflictVerdict` plus the full two-branch context and either proposes a concrete
resolution (`LAST_WRITE_WINS` / `COMPENSATE` / `ROLLBACK` / `ESCALATE` / `QUARANTINE`) or
asks ONE targeted clarifying question. Propose-ONLY — the mediator never acts, never
enforces, never calls policy; the Tier 4 policy gate holds the veto.

## Contents

| File | What |
|---|---|
| `system_prompt.md` | The mediator system prompt. Versioned artifact, embedded into the binary via `include_str!` in `core/context/src/constrained_mediator.rs`. The single source of truth for model behavior. |
| `eval/*.json` | Hand-crafted eval scenarios: a `MediatorInput` (decoder verdict + two tool calls + context window) + expected outcome (a resolution, or that a clarifying question is asked) + a canned golden model output. |
| `RECIPE.md` | The fine-tune recipe (data generation, config, eval gates). Documentation, not executed. |

The implementation is `ConstrainedMediator` in `core/context/src/constrained_mediator.rs`:
endpoint-agnostic (any OpenAI-compatible chat completions server), JSON-schema-constrained
decoding with a prompt-only fallback, all output piped through the locked `parse_proposal`.

## Base-model recommendation

**Primary: Qwen2.5-7B-Instruct** (Apache-2.0).

- **License:** Apache-2.0 — same clean posture as the decoder pick.
- **Capability:** the mediator does strictly more than the decoder. The decoder classifies
  a relation; the mediator must reason about user intent across two divergent branches,
  weigh a resolve-vs-ask judgment, apply a high-stakes fail-closed bias, and — the moat —
  write ONE surgical clarifying question whose answer flips the decision. That is
  reasoning + generation, not classification, so it wants a bigger, reasoning-capable
  base. 7B is the sweet spot: meaningfully stronger intent reasoning than the 3B decoder,
  still single-GPU server-deployable.
- **Cold-path economics:** the mediator fires only when the decoder flags a genuinely
  ambiguous conflict — roughly 5% of reconnects, never on the hot path. Seconds of
  latency are tolerable, so the extra capability is nearly free.

**Alternatives:**

| Model | License | Note |
|---|---|---|
| Qwen3-8B (reasoning) | Apache-2.0 | Stronger resolve-vs-ask judgment via reasoning traces; higher latency, still fine on the cold path. |
| Qwen2.5-14B-Instruct | Apache-2.0 | When question quality gates fail at 7B; ~2× serving cost. |
| Phi-4 (14B) | MIT | Solid reasoning fallback if the Qwen line underperforms on your domains. |

## Sizing rationale

The decoder needs competent instruction following; the mediator needs judgment. The
prompt carries the judgment framework — the resolve-vs-ask rubric, the confidence
anchors tied to policy auto-approve, the high-stakes QUARANTINE bias — but unlike
classification, clarifying-question quality is generative and improves with model size,
which is why the mediator base is deliberately bigger than the decoder's.

The fine-tune (`RECIPE.md`) is a **later optimization pass**, not a prerequisite: it
distills the few-shot judgment into the weights and hardens honest confidence
calibration (the property policy auto-approve depends on). Ship the constrained-decoding
baseline first, measure it with the live eval, then fine-tune against the gates in
`RECIPE.md`.

## Running the evals

```bash
# DRY (default, CI-safe, no network): golden outputs through parse_proposal.
cargo test -p fabric-context --test mediator_eval

# LIVE (opt-in): real endpoint, scored against expected resolutions/questions.
OPENAI_BASE_URL=http://localhost:8000 \
FABRIC_MEDIATOR_MODEL=Qwen/Qwen2.5-7B-Instruct \
make eval-mediator
```

Serve the base model with any OpenAI-compatible server, e.g. vLLM:

```bash
vllm serve Qwen/Qwen2.5-7B-Instruct --port 8000
```
