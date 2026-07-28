# Baseline Model Evaluation

Standalone research tool that measures `poolside/laguna-xs-2.1`'s conflict
classification (decoder) and mediation quality *before* fine-tuning.

**Not product code.** Not in CI. Does not touch the Rust codebase.

## Setup

```bash
cd eval/
pip install -r requirements.txt
export OPENROUTER_API_KEY=sk-or-...
```

## Run

```bash
python run_baseline.py                    # run both tracks
python run_baseline.py --track decoder    # decoder only
python run_baseline.py --track mediator   # mediator only
python run_baseline.py --dry-run          # validate scenarios, no API calls
```

Results are written to `results/baseline-{timestamp}.json` (gitignored).

## Regenerating scenarios

Scenarios in `scenarios/` are generated and pinned. To regenerate after
editing the generator:

```bash
python generate_safety_scenarios.py
```

## Layout

```
scenarios/decoder/   scored decoder scenarios (DecoderInput + expected relation)
scenarios/mediator/  scored mediator scenarios (MediatorInput + expected resolution/question)
prompts/             verbatim copies of the production system prompts
run_baseline.py      runner/scorer
generate_safety_scenarios.py  scenario generator (source of truth for scenarios)
```
