#!/usr/bin/env python3
"""Baseline evaluation runner for the conflict pipeline (decoder + mediator).

Calls OpenRouter with poolside/laguna-xs-2.1 using the production system
prompts, scores responses against scenario ground truth, and writes a
metrics report to results/baseline-{timestamp}.json.

Usage:
    python run_baseline.py [--track decoder|mediator] [--dry-run]
"""

import argparse
import json
import os
import re
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
from openai import OpenAI
from sklearn.metrics import precision_recall_fscore_support

BASE_DIR = Path(__file__).resolve().parent
SCENARIOS_DIR = BASE_DIR / "scenarios"
PROMPTS_DIR = BASE_DIR / "prompts"
RESULTS_DIR = BASE_DIR / "results"

MODEL = "poolside/laguna-xs-2.1"
OPENROUTER_BASE_URL = "https://openrouter.ai/api/v1"

DECODER_PARAMS = {
    "temperature": 0.1,
    "top_p": 0.9,
    "max_tokens": 300,
    "extra_body": {"top_k": 20, "enable_thinking": False},
}
MEDIATOR_PARAMS = {
    "temperature": 0.7,
    "top_p": 0.9,
    "max_tokens": 2048,
    "extra_body": {"top_k": 20, "enable_thinking": True},
}

RELATIONS = ["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"]
RESOLUTIONS = ["LAST_WRITE_WINS", "COMPENSATE", "ROLLBACK", "ESCALATE", "QUARANTINE"]
CALIBRATION_BINS = [(0.5, 0.6), (0.6, 0.7), (0.7, 0.8), (0.8, 0.9), (0.9, 1.01)]

MAX_RETRIES = 3
RETRY_BACKOFF_S = 2.0


def load_scenarios(track):
    track_dir = SCENARIOS_DIR / track
    scenarios = []
    for path in sorted(track_dir.glob("*.json")):
        with open(path) as f:
            scenarios.append(json.load(f))
    return scenarios


def validate_scenario(track, s):
    errors = []
    for field in ("id", "category", "difficulty", "input", "expected", "notes"):
        if field not in s:
            errors.append(f"missing field: {field}")
    if errors:
        return errors
    inp = s["input"]
    for field in ("session_id", "entry_id_a", "entry_id_b", "call_a", "call_b", "context"):
        if field not in inp:
            errors.append(f"input missing: {field}")
    if track == "mediator" and "verdict" not in inp:
        errors.append("input missing: verdict")
    for call_key in ("call_a", "call_b"):
        call = inp.get(call_key, {})
        for field in ("tool_name", "target", "params", "idempotency_key"):
            if field not in call:
                errors.append(f"{call_key} missing: {field}")
    exp = s["expected"]
    if track == "decoder":
        if exp.get("relation") not in RELATIONS:
            errors.append(f"bad expected.relation: {exp.get('relation')}")
    else:
        if exp.get("kind") not in ("resolution", "question"):
            errors.append(f"bad expected.kind: {exp.get('kind')}")
        if exp.get("kind") == "resolution" and exp.get("resolution") not in RESOLUTIONS:
            errors.append(f"bad expected.resolution: {exp.get('resolution')}")
        winner = exp.get("winning_entry_id")
        if winner not in (None, "", inp.get("entry_id_a"), inp.get("entry_id_b")):
            errors.append(f"winning_entry_id not one of the two entries: {winner}")
    for bound in ("min_confidence", "max_confidence"):
        v = exp.get(bound)
        if not isinstance(v, (int, float)) or not (0.0 <= v <= 1.0):
            errors.append(f"bad expected.{bound}: {v}")
    if exp.get("min_confidence", 0) > exp.get("max_confidence", 1):
        errors.append("min_confidence > max_confidence")
    return errors


def extract_json(text):
    """Extract the first JSON object from a model response, tolerating
    markdown fences and surrounding prose."""
    text = text.strip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    fence = re.search(r"```(?:json)?\s*(\{.*?\})\s*```", text, re.DOTALL)
    if fence:
        try:
            return json.loads(fence.group(1))
        except json.JSONDecodeError:
            pass
    start = text.find("{")
    end = text.rfind("}")
    if start != -1 and end > start:
        try:
            return json.loads(text[start : end + 1])
        except json.JSONDecodeError:
            pass
    return None


def check_decoder_schema(obj):
    if not isinstance(obj, dict):
        return False
    if set(obj.keys()) != {"relation", "shared_entities", "confidence", "explanation"}:
        return False
    if obj["relation"] not in RELATIONS:
        return False
    if not isinstance(obj["shared_entities"], list):
        return False
    for ent in obj["shared_entities"]:
        if not isinstance(ent, dict) or set(ent.keys()) != {"entity_type", "entity_id"}:
            return False
    if not isinstance(obj["confidence"], (int, float)) or not (0.0 <= obj["confidence"] <= 1.0):
        return False
    if not isinstance(obj["explanation"], str):
        return False
    return True


def check_mediator_schema(obj, entry_id_a, entry_id_b):
    if not isinstance(obj, dict):
        return False
    expected_keys = {
        "relation",
        "winning_entry_id",
        "proposed_resolution",
        "confidence",
        "rationale",
        "clarifying_question",
    }
    if set(obj.keys()) != expected_keys:
        return False
    if obj["relation"] not in RELATIONS:
        return False
    if obj["proposed_resolution"] not in RESOLUTIONS:
        return False
    if obj["winning_entry_id"] not in ("", entry_id_a, entry_id_b):
        return False
    if not isinstance(obj["confidence"], (int, float)) or not (0.0 <= obj["confidence"] <= 1.0):
        return False
    if not isinstance(obj["rationale"], str):
        return False
    cq = obj["clarifying_question"]
    if cq is not None:
        if not isinstance(cq, dict) or set(cq.keys()) != {"question_text", "options"}:
            return False
        if not isinstance(cq["question_text"], str) or not isinstance(cq["options"], list):
            return False
    return True


def call_model(client, system_prompt, user_payload, params):
    """Call the model with retries (3x, exponential backoff). Returns
    (response_text, latency_ms). Raises after exhausting retries."""
    last_err = None
    for attempt in range(MAX_RETRIES):
        try:
            start = time.monotonic()
            resp = client.chat.completions.create(
                model=MODEL,
                messages=[
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_payload},
                ],
                **params,
            )
            latency_ms = (time.monotonic() - start) * 1000.0
            return resp.choices[0].message.content or "", latency_ms
        except Exception as e:  # noqa: BLE001 - retry any API error
            last_err = e
            if attempt < MAX_RETRIES - 1:
                wait = RETRY_BACKOFF_S * (2**attempt)
                print(f"    API error ({e}), retrying in {wait:.0f}s...")
                time.sleep(wait)
    raise RuntimeError(f"API call failed after {MAX_RETRIES} attempts: {last_err}")


def percentile(values, p):
    if not values:
        return 0.0
    return float(np.percentile(np.array(values), p))


def calibration_rows(records):
    """records: list of (confidence, correct_bool). One row per bin."""
    rows = []
    for lo, hi in CALIBRATION_BINS:
        in_bin = [(c, ok) for c, ok in records if lo <= c < hi]
        if not in_bin:
            continue
        rows.append(
            {
                "bin": f"{lo:.1f}-{min(hi, 1.0):.1f}",
                "count": len(in_bin),
                "avg_confidence": round(float(np.mean([c for c, _ in in_bin])), 4),
                "actual_accuracy": round(float(np.mean([ok for _, ok in in_bin])), 4),
            }
        )
    return rows


def run_decoder(client, scenarios, system_prompt, dry_run):
    results = []
    failures = []
    for i, s in enumerate(scenarios):
        sid = s["id"]
        if dry_run:
            print(f"  [{i + 1}/{len(scenarios)}] {sid} (dry-run, skipped)")
            continue
        print(f"  [{i + 1}/{len(scenarios)}] {sid} ...", end=" ", flush=True)
        user_payload = json.dumps(s["input"], indent=2)
        try:
            raw, latency_ms = call_model(client, system_prompt, user_payload, DECODER_PARAMS)
        except RuntimeError as e:
            print(f"API FAILURE: {e}")
            failures.append({"id": sid, "expected": s["expected"]["relation"], "got": "API_ERROR", "confidence": 0.0})
            continue
        obj = extract_json(raw)
        schema_ok = obj is not None and check_decoder_schema(obj)
        got_relation = obj.get("relation") if schema_ok else "SCHEMA_VIOLATION"
        confidence = float(obj.get("confidence", 0.0)) if schema_ok else 0.0
        correct = schema_ok and got_relation == s["expected"]["relation"]
        exp = s["expected"]
        results.append(
            {
                "id": sid,
                "expected": exp["relation"],
                "got": got_relation,
                "correct": correct,
                "schema_ok": schema_ok,
                "confidence": confidence,
                "confidence_in_bounds": exp["min_confidence"] <= confidence <= exp["max_confidence"],
                "latency_ms": latency_ms,
            }
        )
        if not correct:
            failures.append({"id": sid, "expected": exp["relation"], "got": got_relation, "confidence": round(confidence, 4)})
        print(f"{'OK' if correct else 'MISS'} (expected {exp['relation']}, got {got_relation}, conf {confidence:.2f}, {latency_ms:.0f}ms)")

    if dry_run:
        return None

    total = len(results)
    correct_n = sum(1 for r in results if r["correct"])
    schema_ok_n = sum(1 for r in results if r["schema_ok"])
    y_true = [r["expected"] for r in results if r["schema_ok"]]
    y_pred = [r["got"] for r in results if r["schema_ok"]]
    prec, rec, f1, _ = precision_recall_fscore_support(
        y_true, y_pred, labels=RELATIONS, zero_division=0
    )
    per_class_f1 = {
        rel: {"precision": round(float(p), 4), "recall": round(float(r), 4), "f1": round(float(f), 4)}
        for rel, p, r, f in zip(RELATIONS, prec, rec, f1)
    }
    latencies = [r["latency_ms"] for r in results]
    return {
        "total": total,
        "accuracy": round(correct_n / total, 4) if total else 0.0,
        "per_class_f1": per_class_f1,
        "schema_compliance": round(schema_ok_n / total, 4) if total else 0.0,
        "confidence_in_bounds_rate": round(
            sum(1 for r in results if r["confidence_in_bounds"]) / total, 4
        ) if total else 0.0,
        "calibration": calibration_rows([(r["confidence"], r["correct"]) for r in results]),
        "latency_p50_ms": round(percentile(latencies, 50), 1),
        "latency_p95_ms": round(percentile(latencies, 95), 1),
        "latency_p99_ms": round(percentile(latencies, 99), 1),
        "failures": failures,
    }


def run_mediator(client, scenarios, system_prompt, dry_run):
    results = []
    failures = []
    questions_for_review = []
    for i, s in enumerate(scenarios):
        sid = s["id"]
        if dry_run:
            print(f"  [{i + 1}/{len(scenarios)}] {sid} (dry-run, skipped)")
            continue
        print(f"  [{i + 1}/{len(scenarios)}] {sid} ...", end=" ", flush=True)
        exp = s["expected"]
        inp = s["input"]
        user_payload = json.dumps(inp, indent=2)
        try:
            raw, latency_ms = call_model(client, system_prompt, user_payload, MEDIATOR_PARAMS)
        except RuntimeError as e:
            print(f"API FAILURE: {e}")
            failures.append({"id": sid, "expected": exp, "got": "API_ERROR"})
            continue
        obj = extract_json(raw)
        schema_ok = obj is not None and check_mediator_schema(
            obj, inp["entry_id_a"], inp["entry_id_b"]
        )

        asked_question = False
        got_resolution = None
        got_winner = None
        confidence = 0.0
        if schema_ok:
            cq = obj["clarifying_question"]
            asked_question = cq is not None and bool(cq.get("question_text", "").strip())
            got_resolution = obj["proposed_resolution"]
            got_winner = obj["winning_entry_id"] or None
            confidence = float(obj["confidence"])

        expected_kind = exp["kind"]
        predicted_kind = "question" if asked_question else "resolution"
        kind_correct = schema_ok and predicted_kind == expected_kind

        resolution_correct = None
        winner_correct = None
        if expected_kind == "resolution" and schema_ok:
            resolution_correct = got_resolution == exp["resolution"]
            expected_winner = exp.get("winning_entry_id") or None
            if expected_winner is not None:
                winner_correct = got_winner == expected_winner

        overall_correct = kind_correct and (resolution_correct is not False) and (winner_correct is not False)

        results.append(
            {
                "id": sid,
                "expected_kind": expected_kind,
                "expected_resolution": exp.get("resolution"),
                "expected_winner": exp.get("winning_entry_id"),
                "predicted_kind": predicted_kind,
                "got_resolution": got_resolution,
                "got_winner": got_winner,
                "kind_correct": kind_correct,
                "resolution_correct": resolution_correct,
                "winner_correct": winner_correct,
                "overall_correct": overall_correct,
                "schema_ok": schema_ok,
                "confidence": confidence,
                "confidence_in_bounds": exp["min_confidence"] <= confidence <= exp["max_confidence"],
                "latency_ms": latency_ms,
            }
        )
        if not overall_correct:
            failures.append(
                {
                    "id": sid,
                    "expected": {"kind": expected_kind, "resolution": exp.get("resolution"), "winning_entry_id": exp.get("winning_entry_id")},
                    "got": {"kind": predicted_kind if schema_ok else "SCHEMA_VIOLATION", "resolution": got_resolution, "winning_entry_id": got_winner},
                    "confidence": round(confidence, 4),
                }
            )
        if asked_question and schema_ok:
            questions_for_review.append(
                {"id": sid, "question": obj["clarifying_question"]["question_text"], "expected_kind": expected_kind}
            )
        status = "OK" if overall_correct else "MISS"
        detail = f"expected {expected_kind}/{exp.get('resolution')}, got {predicted_kind}/{got_resolution}"
        print(f"{status} ({detail}, conf {confidence:.2f}, {latency_ms:.0f}ms)")

    if dry_run:
        return None

    total = len(results)
    res_cases = [r for r in results if r["expected_kind"] == "resolution" and r["schema_ok"]]
    q_expected = [r for r in results if r["expected_kind"] == "question"]
    winner_cases = [r for r in results if r["winner_correct"] is not None]
    schema_ok_n = sum(1 for r in results if r["schema_ok"])
    latencies = [r["latency_ms"] for r in results]
    return {
        "total": total,
        "resolution_accuracy": round(
            sum(1 for r in res_cases if r["resolution_correct"]) / len(res_cases), 4
        ) if res_cases else 0.0,
        "question_rate": round(
            sum(1 for r in q_expected if r["predicted_kind"] == "question") / len(q_expected), 4
        ) if q_expected else 0.0,
        "schema_compliance": round(schema_ok_n / total, 4) if total else 0.0,
        "winning_entry_accuracy": round(
            sum(1 for r in winner_cases if r["winner_correct"]) / len(winner_cases), 4
        ) if winner_cases else 0.0,
        "confidence_in_bounds_rate": round(
            sum(1 for r in results if r["confidence_in_bounds"]) / total, 4
        ) if total else 0.0,
        "calibration": calibration_rows([(r["confidence"], r["overall_correct"]) for r in results]),
        "latency_p50_ms": round(percentile(latencies, 50), 1),
        "latency_p95_ms": round(percentile(latencies, 95), 1),
        "latency_p99_ms": round(percentile(latencies, 99), 1),
        "failures": failures,
        "questions_for_review": questions_for_review,
    }


def main():
    parser = argparse.ArgumentParser(description="Baseline eval for the conflict pipeline")
    parser.add_argument("--track", choices=["decoder", "mediator"], default=None, help="run one track only")
    parser.add_argument("--dry-run", action="store_true", help="validate scenarios without API calls")
    args = parser.parse_args()

    tracks = [args.track] if args.track else ["decoder", "mediator"]

    all_valid = True
    for track in tracks:
        scenarios = load_scenarios(track)
        print(f"{track}: {len(scenarios)} scenarios loaded")
        for s in scenarios:
            errs = validate_scenario(track, s)
            if errs:
                all_valid = False
                for e in errs:
                    print(f"  INVALID {s.get('id', '?')}: {e}")
    if not all_valid:
        print("Scenario validation failed. Fix scenarios before running.")
        sys.exit(1)
    if args.dry_run:
        print("Dry-run: all scenarios valid. No API calls made.")
        sys.exit(0)

    api_key = os.environ.get("OPENROUTER_API_KEY")
    if not api_key:
        print("OPENROUTER_API_KEY not set.")
        sys.exit(1)
    client = OpenAI(base_url=OPENROUTER_BASE_URL, api_key=api_key)

    report = {
        "model": MODEL,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }
    if "decoder" in tracks:
        print("\n== DECODER ==")
        scenarios = load_scenarios("decoder")
        prompt = (PROMPTS_DIR / "decoder_system.md").read_text()
        report["decoder"] = run_decoder(client, scenarios, prompt, args.dry_run)
    if "mediator" in tracks:
        print("\n== MEDIATOR ==")
        scenarios = load_scenarios("mediator")
        prompt = (PROMPTS_DIR / "mediator_system.md").read_text()
        report["mediator"] = run_mediator(client, scenarios, prompt, args.dry_run)

    RESULTS_DIR.mkdir(exist_ok=True)
    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_path = RESULTS_DIR / f"baseline-{ts}.json"
    with open(out_path, "w") as f:
        json.dump(report, f, indent=2)
    print(f"\nReport written to {out_path}")


if __name__ == "__main__":
    main()
