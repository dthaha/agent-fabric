# Conflict Mediator — Fine-Tune Recipe

**Status: documentation only. Do NOT run training from this phase.**

The constrained-decoding baseline (reference system prompt + JSON-schema-constrained
sampling on Qwen2.5-7B-Instruct) ships first. This recipe is the later optimization
pass: it buys latency (no few-shot prompt on the wire), better resolve-vs-ask judgment,
higher clarifying-question quality, and — critically — **honest confidence calibration**,
the property the Tier 4 policy auto-approve threshold depends on. This document — the
data shapes, the config, the gates — is the durable open-core asset.

---

## 1. Training target

The training target is exactly the locked output contract (`PROPOSAL_OUTPUT_SCHEMA` in
`core/context/src/mediator.rs`, mirrored in `system_prompt.md`):

```json
{"relation": "SUPERSEDES"|"CONTRADICTS"|"INDEPENDENT"|"AMBIGUOUS",
 "winning_entry_id": str,
 "proposed_resolution": "LAST_WRITE_WINS"|"COMPENSATE"|"ROLLBACK"|"ESCALATE"|"QUARANTINE",
 "confidence": float 0..1,
 "rationale": str,
 "clarifying_question": {"question_text": str, "options": [str]} | null}
```

One JSON object per example, nothing else. `session_id` is NEVER in the target — it is
injected by `parse_proposal` at inference. `winning_entry_id` is always either empty or
one of the two real entry IDs from the input; training examples with invented IDs are
invalid and must be dropped (the parser clears them at inference, but training must not
teach the habit).

Input format: the same `MediatorInput::render_prompt()` rendering used at inference —
the decoder verdict, both tool calls, the context window, the optional tool category —
so train/test formats match byte-for-byte. The system prompt for training is a
**shortened** version of `system_prompt.md` (role + resolution definitions +
resolve-vs-ask judgment + calibration + hard constraints; few-shot examples dropped).

## 2. Synthetic data generation

Mediator pairs (divergent branches → resolution or clarifying question) are harder to
generate than decoder pairs because the label is a judgment, not a class. Three
generators, mixed:

### 2a. Divergent-branch scenario synthesis (seed + template expansion)

- Take the 6 hand-crafted scenarios in `eval/*.json` as seeds.
- Build fork generators: a coherent session up to seq N, then TWO continuations that
  diverge (different surfaces, one offline, clock gap). Vary the fork cause: user changed
  mind (→ SUPERSEDES/LWW), two actors with opposing intents (→ CONTRADICTS + resolution),
  ambiguous note + action (→ clarifying question), high-stakes tool category (→
  QUARANTINE/ESCALATE bias).
- Labels come from the generator: the fork cause determines whether the gold output is a
  resolution (and which) or a question, and the winning entry is known by construction.
- Target: ~40% of the dataset.

### 2b. Teacher distillation with a disambiguation filter

- Generate divergent-branch skeletons where the gold outcome is NOT rule-decidable.
- Prompt a large frontier teacher with the reference system prompt; keep only outputs
  where the teacher is confident AND a second independent teacher agrees on the outcome
  kind (resolution vs question) and, for resolutions, the resolution enum.
- For question golds, require the two teachers' questions to target the same missing
  fact (embedding-similarity or entailment check), not just both be questions —
  question-targetedness is the moat and must be taught precisely.
- **Hold out a slice of teacher-labeled data for validation, never train on it.**
- Target: ~30% of the dataset.

### 2c. Calibration-stress generation (the point of the exercise)

- The property that matters most is honest confidence, because policy compares mediator
  confidence against `auto_approve_threshold`. Generate:
  - **Near-miss SUPERSEDES**: revision-looking phrasing that is actually provisional —
    gold is a question or ESCALATE, not LWW. Teaches "decoder said SUPERSEDES" is not
    sufficient to resolve.
  - **High-stakes traps**: CONTRADICTS on financial/deployment/irreversible tools where
    context weakly favors one side — gold is QUARANTINE at ≤0.6 confidence, never an
    auto-approvable resolution. Teaches the fail-closed bias.
  - **Context-starvation sweeps**: progressively strip the context window so gold slides
    from resolution → question → QUARANTINE, with confidence dropping accordingly.
- Target: ~30% of the dataset, with question/escalate golds at ~35% overall so the model
  internalizes "asking is cheap, wrong auto-approval is expensive."

### Class balance (outcome kind)

Approximate: resolution 65% (LWW 25%, COMPENSATE 15%, QUARANTINE 15%, ROLLBACK 5%,
ESCALATE-as-resolution 5%), clarifying question 35%. Deliberately question-heavy
relative to natural frequency: the cost asymmetry must survive fine-tuning.

### Validation

Every generated example round-trips through `parse_proposal` against its own
`MediatorInput` with zero error, and additionally: `winning_entry_id` is empty or one of
the two real IDs; question golds have non-empty `question_text`; resolution golds have
`clarifying_question: null`. Anything failing is dropped.

## 3. Fine-tune config

| Knob | Value | Why |
|---|---|---|
| Base model | Qwen2.5-7B-Instruct | Matches the constrained-decoding baseline (§README); Apache-2.0; reasoning-capable. |
| Method | **LoRA**, rank 32, alpha 64, dropout 0.05, all linear layers | Slightly higher rank than the decoder: the judgment + question-generation task is richer. Full FT only if LoRA plateaus below the gates. |
| Precision | bf16 (QLoRA 4-bit base acceptable for iteration) | 7B + LoRA fits on one 48 GB GPU (or 24 GB with QLoRA). |
| Sequence length | 4096 | Mediator prompts carry the decoder verdict + two full branches; longer than the decoder's. |
| Batch / accumulation | 8 × grad-accum 4 (effective 32) | Stable on small datasets. |
| LR / schedule | 1e-4, cosine, warmup 3% | Standard LoRA regime. |
| Epochs | 3, early stop on val outcome-kind accuracy + calibration | Watch the over-confidence curve, not just accuracy. |
| Loss | standard causal LM loss, **completion-only** (mask the prompt tokens) | Teach the proposal, not the prompt. |
| Output contract | training targets are schema-exact JSON (see §1) | The schema is the label space. |

Keep the run reproducible: pin the dataset hash, seed, and base-model revision in the
artifact manifest alongside the adapter weights.

## 4. Serving the fine-tune

The adapter slots behind the **same `ConflictMediator` trait** — `ConstrainedMediator`
with `FABRIC_MEDIATOR_MODEL` pointed at the fine-tuned model. Keep
`response_format: json_schema` ON even after fine-tuning: constrained decoding is a free
correctness floor and makes schema drift impossible. The shortened system prompt (still
versioned alongside `system_prompt.md`) replaces the few-shot version.

## 5. Eval gates (must beat the baseline to ship)

The fine-tune ships only if it beats the constrained-decoding baseline on all gates,
measured on the held-out teacher-labeled validation set (§2b) **plus** the hand-crafted
`eval/*.json` set, both run through the LIVE eval harness (`make eval-mediator`):

| Gate | Threshold |
|---|---|
| Outcome-kind accuracy (resolve vs ask) | ≥ baseline + 3 points absolute, and ≥ 92% overall |
| Resolution accuracy (correct enum + winner, given resolve) | ≥ baseline + 3 points; LWW winner must be the newer entry 100% of the time |
| Question-targetedness | on question golds, the asked question targets the gold missing fact (teacher-judged) ≥ baseline + 5 points |
| **Honest calibration — over-confidence rate** | fraction of proposals with confidence ≥ 0.8 that are WRONG ≤ baseline, and ≤ 5% absolute. **Release blocker regardless of other gates**: over-confidence drives policy auto-approve, so a confident-wrong mediator silently auto-resolves real conflicts |
| High-stakes fail-closed | on financial/deployment scenarios, no auto-approvable (≥0.8 confidence) resolution when gold is QUARANTINE/question — zero tolerance |
| Schema validity | 100% of outputs parse via `parse_proposal` (structural with constrained decoding, but verify) |
| Latency | p50 proposal latency ≤ 70% of baseline (the few-shot prompt is gone) |

The over-confidence gate is the release blocker: a mediator that is right 95% of the
time but confident when wrong is worse than the baseline, because policy will
auto-approve its mistakes. Under-confidence only costs a clarifying question.

## 6. What this recipe deliberately does not do

- No training on `session_id` or entry-ID invention — identity is injected/cleared by
  `parse_proposal`, never learned.
- No enforcement behavior — the mediator stays propose-only after fine-tuning. Policy
  (Tier 4) holds the veto; that seam is code, not a model.
- No decoder collapse — decoder and mediator remain separate models with separate
  recipes; sharing one model with two prompts is a v1 serving convenience, not the
  target architecture.
- No customer data leaves the customer — the recipe is runnable entirely on customer
  infra; 2b's teacher runs against the customer's own (consented, anonymized) op-logs.
