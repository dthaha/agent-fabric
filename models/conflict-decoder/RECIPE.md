# Conflict Decoder — Fine-Tune Recipe

**Status: documentation only. Do NOT run training from this phase.**

The constrained-decoding baseline (reference system prompt + JSON-schema-constrained
sampling on Qwen2.5-3B-Instruct) ships first. This recipe is the later optimization
pass: it buys latency (no few-shot prompt on the wire), reliability on the
SUPERSEDES/CONTRADICTS boundary, and calibration stability across base-model upgrades.
This document — the data shapes, the config, the gates — is the durable open-core
asset.

---

## 1. Training target

The training target is exactly the locked output contract (`OUTPUT_SCHEMA` in
`core/context/src/decoder.rs`, mirrored in `system_prompt.md`):

```json
{"relation": "SUPERSEDES"|"CONTRADICTS"|"INDEPENDENT"|"AMBIGUOUS",
 "shared_entities": [{"entity_type": str, "entity_id": str}],
 "confidence": float 0..1,
 "explanation": str}
```

One JSON object per example, nothing else. Identity fields
(`session_id`, `entry_id_a`, `entry_id_b`) are NEVER in the target — they are injected
by `parse_verdict` at inference, and training must not teach the model to emit them.

Input format: the same `DecoderInput::render_prompt()` rendering used at inference, so
train/test formats match byte-for-byte. The system prompt for training is a
**shortened** version of `system_prompt.md` (role + four-relation definitions +
discriminator + calibration + hard constraints; few-shot examples dropped) — the
examples are distilled into the weights instead of the context window.

## 2. Synthetic data generation

Conflict-scenario → verdict pairs are synthetic-friendly because the label semantics
are rule-expressible. Three generators, mixed:

### 2a. Template expansion of the eval scenarios (seed set)

- Take the 8 hand-crafted scenarios in `eval/*.json` as seeds.
- Parameterize: tool names, targets, param keys/values, context phrasing, surfaces,
  timestamps. Build a domain pack (smart-home, email/calendar, travel, devops, retail,
  filesystem) with 10–20 tools per domain and realistic param vocabularies.
- Labels come from the template: a "revision" template instantiates SUPERSEDES, an
  "opposing intents" template instantiates CONTRADICTS, a "different logical target"
  template instantiates INDEPENDENT, and a "context withheld" template instantiates
  AMBIGUOUS.
- Target: ~40% of the dataset. Cheap, high-precision labels.

### 2b. Perturbation of real op-logs (highest fidelity)

- From (consented, anonymized) real session op-logs, mine entry pairs the Tier 1
  structural detector flags: same tool + same target + different params.
- Auto-label the clear cases with the rules already in the prompt:
  - Same tool+target, later entry is a param revision within one user turn-chain →
    SUPERSEDES.
  - Param values that are logical negations (`locked: true/false`, `enabled/disabled`)
    or opposing verbs on one target (cancel/expedite, book/cancel) → CONTRADICTS.
  - Same tool, disjoint logical targets → INDEPENDENT.
- Pairs the rules can't label go to a teacher model (a large frontier model) with the
  reference system prompt; keep only teacher outputs where the teacher is confident
  AND a second teacher agrees (two-teacher agreement filter).
- **Hold out the real-derived pairs for validation, never train on them** — they are
  the closest thing to ground truth and must stay uncontaminated for the gates in §5.
- Target: ~30% of the dataset (train split only from auto-labeled clear cases).

### 2c. Boundary-case generation (the point of the exercise)

- Deliberately generate the near-misses, since they decide real-world quality:
  - SUPERSEDES-vs-CONTRADICTS: revision phrasing ("instead", "actually", "change it
    to") vs. independent-actor phrasing (different surfaces, no acknowledgment of the
    first action). Generate both over the same tool+target skeleton.
  - Low-context variants: strip the context window progressively so the same pair
    slides from a confident relation toward AMBIGUOUS — teaches the calibration bias,
    not just the labels.
  - Adversarial INDEPENDENT: same tool, same nominal target type, different logical
    target (two emails, two files, two orders).
- Target: ~30% of the dataset, with the AMBIGUOUS share pushed to ~20% overall so the
  model internalizes "escalation is cheap."

### Class balance

Approximate: SUPERSEDES 30%, CONTRADICTS 25%, INDEPENDENT 25%, AMBIGUOUS 20%. Skewed
toward AMBIGUOUS relative to natural frequency on purpose — the cost asymmetry (false
resolution is expensive, escalation is cheap) must survive fine-tuning.

### Validation

Every generated pair is validated before entering the dataset: the target JSON must
round-trip through `parse_verdict` against its own `DecoderInput` with zero error.
Anything `parse_verdict` rejects is dropped — training data is held to the same
contract as inference output.

## 3. Fine-tune config

| Knob | Value | Why |
|---|---|---|
| Base model | Qwen2.5-3B-Instruct | Matches the constrained-decoding baseline (§README); Apache-2.0. |
| Method | **LoRA**, rank 16, alpha 32, dropout 0.05, all linear layers | The task is narrow; full fine-tune risks catastrophic forgetting of general instruction following for zero measurable gain. LoRA adapters also ship cleanly as customer artifacts. Revisit full FT only if LoRA plateaus below the gates. |
| Precision | bf16 (QLoRA 4-bit base acceptable for iteration) | 3B fits on one 24 GB consumer GPU with LoRA. |
| Sequence length | 2048 | Covers shortened system prompt + bounded context window + verdict with headroom. |
| Batch / accumulation | 8 × grad-accum 4 (effective 32) | Stable on small datasets. |
| LR / schedule | 1e-4, cosine, warmup 3% | Standard LoRA regime. |
| Epochs | 3, early stop on val relation-accuracy | Small synthetic sets overfit fast; watch the AMBIGUOUS precision curve. |
| Loss | standard causal LM loss, **completion-only** (mask the prompt tokens) | Teach the verdict, not the prompt. |
| Output contract | training targets are schema-exact JSON (see §1) | The schema is the label space. |

Keep the run reproducible: pin the dataset hash, seed, and base-model revision in the
artifact manifest alongside the adapter weights.

## 4. Serving the fine-tune

The adapter slots behind the **same `ConflictDecoder` trait** — `ConstrainedDecoder`
with `FABRIC_DECODER_MODEL` pointed at the fine-tuned model. Keep
`response_format: json_schema` ON even after fine-tuning: constrained decoding is a
free correctness floor and makes schema drift impossible. The shortened system prompt
(still versioned alongside `system_prompt.md`) replaces the few-shot version.

## 5. Eval gates (must beat the baseline to ship)

The fine-tune ships only if it beats the constrained-decoding baseline on all gates,
measured on the held-out real-derived validation set (§2b) **plus** the hand-crafted
`eval/*.json` set, both run through the LIVE eval harness (`make eval-decoder`):

| Gate | Threshold |
|---|---|
| Relation accuracy | ≥ baseline + 3 points absolute, and ≥ 92% overall |
| SUPERSEDES/CONTRADICTS boundary F1 | ≥ baseline + 5 points; this is the discriminator that matters |
| CONTRADICTS recall | ≥ baseline, and never below 90% — a missed contradiction silently last-write-wins; this is the expensive error |
| AMBIGUOUS calibration | on low-context variants, AMBIGUOUS rate ≥ baseline's; overconfident specific relations below true 0.5-confidence cases = fail |
| Schema validity | 100% of outputs parse via `parse_verdict` (with constrained decoding this should be structural, but verify) |
| Latency | p50 verdict latency ≤ 60% of baseline (the few-shot prompt is gone) |

Any regression on CONTRADICTS recall or AMBIGUOUS calibration blocks the release
regardless of aggregate accuracy. Accuracy on the easy 80% is not the product; the
boundary is.

## 6. What this recipe deliberately does not do

- No training on `session_id`/entry IDs — identity is injected, never learned.
- No resolution behavior — the decoder stays classify-only after fine-tuning. The
  mediator (Tier 3, Phase F) is a separate model with a separate recipe.
- No customer data leaves the customer — the recipe is runnable entirely on customer
  infra; 2b's op-logs are the customer's own.
