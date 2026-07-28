# Baseline Model Evaluation — Spec

**Purpose:** Measure Laguna XS 2.1's conflict classification and mediation quality *before* fine-tuning, so we know what to fix and can measure improvement after.

**This is NOT product code.** It's a standalone Python research tool. It does not ship with the fabric, does not go in CI, and does not touch the Rust codebase.

## What it does

1. Reads scored conflict scenarios from JSON files
2. Calls OpenRouter (`poolside/laguna-xs-2.1`) with the project's actual system prompts
3. Scores responses against ground truth labels
4. Outputs a metrics report (accuracy, F1, schema compliance, calibration, latency)

## Directory layout

```
eval/                          # standalone, NOT in core/ or server/
  README.md                    # how to run
  requirements.txt             # openai, numpy, scikit-learn
  run_baseline.py              # main script
  scenarios/
    decoder/                   # 50-100 scored decoder scenarios
      *.json
    mediator/                  # 30-50 scored mediator scenarios
      *.json
  prompts/
    decoder_system.md          # COPY of models/conflict-decoder/system_prompt.md
    mediator_system.md         # COPY of models/conflict-mediator/system_prompt.md
  results/                     # gitignored, output goes here
    *.json
```

## Scenario schema

### Decoder scenario

```json
{
  "id": "dec-001",
  "category": "supersedes|contradicts|independent|ambiguous|adversarial",
  "difficulty": "easy|medium|hard",
  "input": {
    "session_id": "string",
    "entry_id_a": "string",
    "entry_id_b": "string",
    "call_a": {
      "tool_name": "string",
      "target": "string",
      "params": {},
      "idempotency_key": "string"
    },
    "call_b": { "...same shape..." },
    "context": [
      {"entry_id": "string", "seq": 0, "kind": "ENTRY_KIND_USER_MESSAGE|ENTRY_KIND_ASSISTANT_MESSAGE", "content": "string"}
    ]
  },
  "expected": {
    "relation": "SUPERSEDES|CONTRADICTS|INDEPENDENT|AMBIGUOUS",
    "min_confidence": 0.0,
    "max_confidence": 1.0
  },
  "notes": "why this scenario exists, what it tests"
}
```

### Mediator scenario

```json
{
  "id": "med-001",
  "category": "lww|compensate|rollback|escalate|quarantine|question",
  "difficulty": "easy|medium|hard",
  "input": {
    "verdict": {
      "relation": "string",
      "shared_entities": [],
      "confidence": 0.0,
      "explanation": "string"
    },
    "session_id": "string",
    "entry_id_a": "string",
    "entry_id_b": "string",
    "call_a": { "...same as decoder..." },
    "call_b": { "...same as decoder..." },
    "context": [ "...same as decoder..." ]
  },
  "expected": {
    "kind": "resolution|question",
    "resolution": "LAST_WRITE_WINS|COMPENSATE|ROLLBACK|ESCALATE|QUARANTINE|null",
    "winning_entry_id": "string|null",
    "min_confidence": 0.0,
    "max_confidence": 1.0
  },
  "notes": "string"
}
```

## Scoring metrics

### Decoder
- **Accuracy**: relation match rate
- **Per-class F1**: precision/recall per relation
- **Schema compliance**: % responses that parse as valid JSON matching OUTPUT_SCHEMA
- **Confidence calibration**: binned (0.5-0.6, 0.6-0.7, ..., 0.9-1.0) — actual accuracy vs stated confidence per bin
- **Latency**: p50, p95, p99 per request

### Mediator
- **Resolution accuracy**: does the proposed_resolution match expected (for resolution cases)
- **Question rate**: % of cases where model asks a question (should be high for ambiguous/hard)
- **Question quality**: manual review flag (script outputs the questions for human scoring)
- **Schema compliance**: same as decoder but for PROPOSAL_OUTPUT_SCHEMA
- **Confidence calibration**: same binning
- **Winning entry accuracy**: does winning_entry_id match expected (when applicable)
- **Latency**: p50, p95, p99

## API parameters

### Decoder calls
```
model: poolside/laguna-xs-2.1
temperature: 0.1
top_k: 20
top_p: 0.9
max_tokens: 300
enable_thinking: false
```

### Mediator calls
```
model: poolside/laguna-xs-2.1
temperature: 0.7
top_k: 20
top_p: 0.9
max_tokens: 2048
enable_thinking: true
```

## Scenario generation requirements

Generate scenarios that cover:

### Decoder (target: 60-80 scenarios)
- 15-20 clear SUPERSEDES (revisions, "actually make it X instead")
- 15-20 clear CONTRADICTS (lock/unlock, cancel/expedite, incompatible params)
- 10-15 INDEPENDENT (same tool, different targets — structural false positives)
- 10-15 genuinely AMBIGUOUS (missing context, could go either way)
- 10-15 adversarial edge cases:
  - Near-miss supersession (looks like revision but is actually contradiction)
  - Implicit contradiction via side effects (not obvious from params alone)
  - Multi-param conflicts where some params supersede and others contradict
  - Idempotent calls (same tool, same params — should be INDEPENDENT/LWW not conflict)
  - Temporal ordering traps (B is newer but A is the correct intent)

### Mediator (target: 40-50 scenarios)
- 10-12 clean LAST_WRITE_WINS (decoder said SUPERSEDES high-conf, context confirms)
- 8-10 COMPENSATE (undo + redo semantics, tool supports it)
- 5-8 ESCALATE (genuine disagreement, context doesn't disambiguate)
- 5-8 QUARANTINE (high-stakes: financial, deployment, irreversible)
- 8-10 clarifying questions (ambiguous, the question IS the right output)
- 5 adversarial:
  - Looks like LWW but is actually QUARANTINE (high stakes hidden in params)
  - Decoder said SUPERSEDES but context reveals CONTRADICTS (mediator should override)
  - Both entries are wrong (neither should win)

### Realism requirements
- Tool names should be realistic agent tools: file ops, email, calendar, smart home, financial, deployment, config management, messaging
- Context windows should vary: some with rich conversation history, some with minimal/no context
- Params should be realistic (not "foo"/"bar")
- Include multi-surface scenarios (phone + laptop, different users)
- Include offline-then-reconnect scenarios (timestamps hours apart)

## Output format

`results/baseline-{timestamp}.json`:
```json
{
  "model": "poolside/laguna-xs-2.1",
  "timestamp": "ISO8601",
  "decoder": {
    "total": 0,
    "accuracy": 0.0,
    "per_class_f1": {},
    "schema_compliance": 0.0,
    "calibration": [{"bin": "0.5-0.6", "count": 0, "avg_confidence": 0.0, "actual_accuracy": 0.0}],
    "latency_p50_ms": 0,
    "latency_p95_ms": 0,
    "latency_p99_ms": 0,
    "failures": [{"id": "dec-001", "expected": "SUPERSEDES", "got": "CONTRADICTS", "confidence": 0.7}]
  },
  "mediator": {
    "total": 0,
    "resolution_accuracy": 0.0,
    "question_rate": 0.0,
    "schema_compliance": 0.0,
    "winning_entry_accuracy": 0.0,
    "calibration": [],
    "latency_p50_ms": 0,
    "latency_p95_ms": 0,
    "latency_p99_ms": 0,
    "failures": [],
    "questions_for_review": [{"id": "med-001", "question": "...", "expected_kind": "question"}]
  }
}
```

## Usage

```bash
cd eval/
pip install -r requirements.txt
export OPENAI_API_KEY=sk-...
python run_baseline.py                    # run both tracks
python run_baseline.py --track decoder    # decoder only
python run_baseline.py --track mediator   # mediator only
python run_baseline.py --dry-run          # validate scenarios without API calls
```
