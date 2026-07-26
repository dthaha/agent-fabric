# Conflict Decoder — Reference Model

Tier 2 of the conflict-resolution pipeline: the semantic sensor that classifies a
flagged entry pair into `SUPERSEDES` / `CONTRADICTS` / `INDEPENDENT` / `AMBIGUOUS`.
Classify-ONLY — the decoder never acts, never resolves, never calls policy.

## Contents

| File | What |
|---|---|
| `system_prompt.md` | The decoder system prompt. Versioned artifact, embedded into the binary via `include_str!` in `core/context/src/constrained_decoder.rs`. The single source of truth for model behavior. |
| `eval/*.json` | Hand-crafted eval scenarios: a `DecoderInput` + expected relation + a canned golden model output. |
| `RECIPE.md` | The fine-tune recipe (data generation, config, eval gates). Documentation, not executed. |

The implementation is `ConstrainedDecoder` in `core/context/src/constrained_decoder.rs`:
endpoint-agnostic (any OpenAI-compatible chat completions server), JSON-schema-constrained
decoding with a prompt-only fallback, all output piped through the locked `parse_verdict`.

## Base-model recommendation

**Primary: Qwen2.5-3B-Instruct** (Apache-2.0).

- **License:** Apache-2.0 — clean for an Apache-2.0 open-core repo with no usage
  restrictions, unlike Llama-3.2 (Llama Community License, acceptable-use clauses) or
  Gemma (Gemma Terms of Use).
- **Size:** ~3B params → ~6 GB at bf16, ~2 GB at Q4. Runs comfortably on a single
  server GPU next to other services, and on high-end endpoints later. The decoder is a
  cold path (fires on reconnect, not per-prompt), so 3B latency is fine.
- **Quality:** the strongest instruction-following and structured-output adherence in
  the Apache-2.0 ≤3B class; follows the four-relation rubric and the JSON contract
  reliably even before constrained decoding is applied.

**Alternatives:**

| Model | License | Note |
|---|---|---|
| Phi-3.5-mini-instruct (3.8B) | MIT | Solid fallback; slightly weaker JSON discipline. |
| Qwen2.5-1.5B-Instruct | Apache-2.0 | Use when latency/memory is tight; expect a small drop on borderline SUPERSEDES-vs-CONTRADICTS cases. |
| Llama-3.2-3B-Instruct | Llama Community License | Quality is fine; license is not Apache-2.0 — customer-deployable but not our reference pick. |

## Sizing rationale

With JSON-schema-constrained decoding (the schema is enforced at the sampler, so the
output contract is structural, not aspirational) plus the reference system prompt, a
1.5B–3B instruct model suffices for v1. The prompt carries the judgment — the
SUPERSEDES/CONTRADICTS discriminator, the calibration anchors, the AMBIGUOUS bias — so
the base model only needs competent instruction following, not conflict-domain
knowledge.

The fine-tune (`RECIPE.md`) is a **later optimization pass**, not a prerequisite: it
buys latency (shorter prompt, no few-shot), reliability on the boundary cases, and
calibration stability across base-model upgrades. Ship the constrained-decoding
baseline first, measure it with the live eval, then fine-tune against the gates in
`RECIPE.md`.

## Running the evals

```bash
# DRY (default, CI-safe, no network): golden outputs through parse_verdict.
cargo test -p fabric-context --test decoder_eval

# LIVE (opt-in): real endpoint, scored against expected relations.
OPENAI_BASE_URL=http://localhost:8000 \
FABRIC_DECODER_MODEL=Qwen/Qwen2.5-3B-Instruct \
make eval-decoder
```

Serve the base model with any OpenAI-compatible server, e.g. vLLM:

```bash
vllm serve Qwen/Qwen2.5-3B-Instruct --port 8000
```
