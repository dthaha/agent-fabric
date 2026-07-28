#!/usr/bin/env python3
"""Run baseline eval via OpenRouter API using the MCP OAuth token.
Saves raw responses + scored metrics. Matches run_baseline.py format exactly."""

import argparse, json, os, re, sys, time, statistics
from pathlib import Path
from datetime import datetime, timezone

try:
    import httpx
except ImportError:
    os.system(f"{sys.executable} -m pip install httpx -q")
    import httpx

BASE = Path(__file__).resolve().parent
SCENARIOS = BASE / "scenarios"
PROMPTS = BASE / "prompts"
RESULTS = BASE / "results"
RESULTS.mkdir(exist_ok=True)

parser = argparse.ArgumentParser(description="Baseline eval runner (decoder + mediator)")
parser.add_argument(
    "--base-url",
    default="https://openrouter.ai/api/v1",
    help="OpenAI-compatible API base URL (vLLM, NIM, OpenRouter, ...)",
)
ARGS = parser.parse_args()

TOKEN = os.environ.get("OPENROUTER_API_KEY")
if not TOKEN:
    sys.exit("OPENROUTER_API_KEY not set.")
API_URL = ARGS.base_url.rstrip("/") + "/chat/completions"
MODEL = os.environ.get("EVAL_MODEL", "poolside/laguna-xs-2.1:free")

DECODER_SYSTEM = (PROMPTS / "decoder_system.md").read_text()
MEDIATOR_SYSTEM = (PROMPTS / "mediator_system.md").read_text()

RELATIONS = ["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"]
RESOLUTIONS = ["LAST_WRITE_WINS", "COMPENSATE", "ROLLBACK", "ESCALATE", "QUARANTINE"]

DECODER_BODY = {"temperature": 0.1, "top_p": 0.9, "max_tokens": 1024, "top_k": 20, "reasoning": {"effort": "none"}, "provider": {"sort": "throughput"}}
MEDIATOR_BODY = {"temperature": 0.7, "top_p": 0.9, "max_tokens": 4096, "top_k": 20, "reasoning": {"effort": "high"}, "provider": {"sort": "throughput"}}

client = httpx.Client(timeout=60.0, headers={
    "Authorization": f"Bearer {TOKEN}",
    "Content-Type": "application/json",
    "HTTP-Referer": "https://agent.hafamily.tech",
    "X-Title": "agent-fabric-eval",
})

def extract_json(text):
    if not text:
        return None
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
            return json.loads(text[start:end + 1])
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

def check_mediator_schema(obj, eid_a, eid_b):
    if not isinstance(obj, dict):
        return False
    expected_keys = {"relation", "winning_entry_id", "proposed_resolution", "confidence", "rationale", "clarifying_question"}
    if set(obj.keys()) != expected_keys:
        return False
    if obj["relation"] not in RELATIONS:
        return False
    if obj["proposed_resolution"] not in RESOLUTIONS:
        return False
    if obj["winning_entry_id"] not in ("", eid_a, eid_b):
        return False
    if not isinstance(obj["confidence"], (int, float)) or not (0.0 <= obj["confidence"] <= 1.0):
        return False
    if not isinstance(obj["rationale"], str):
        return False
    cq = obj["clarifying_question"]
    if cq is not None:
        if not isinstance(cq, dict) or set(cq.keys()) != {"question_text", "options"}:
            return False
    return True

def call_api(system_prompt, user_payload, body_params, retries=3):
    payload = {
        "model": MODEL,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_payload},
        ],
        **body_params,
    }
    last_err = None
    for attempt in range(retries):
        try:
            start = time.monotonic()
            resp = client.post(API_URL, json=payload)
            latency_ms = (time.monotonic() - start) * 1000.0
            if resp.status_code == 429:
                wait = 5 * (2 ** attempt)
                print(f"    rate limited, waiting {wait}s...")
                time.sleep(wait)
                continue
            resp.raise_for_status()
            data = resp.json()
            choice = data["choices"][0]
            content = choice["message"].get("content") or ""
            # If content is null but reasoning exists, note it
            reasoning = choice["message"].get("reasoning", "")
            usage = data.get("usage", {})
            return {
                "content": content,
                "reasoning": reasoning,
                "latency_ms": latency_ms,
                "usage": usage,
                "finish_reason": choice.get("finish_reason", ""),
                "raw_response": data,
            }
        except Exception as e:
            last_err = e
            if attempt < retries - 1:
                wait = 3 * (2 ** attempt)
                print(f"    error ({e}), retry in {wait}s...")
                time.sleep(wait)
    return {"content": "", "reasoning": "", "latency_ms": 0, "usage": {}, "finish_reason": "ERROR", "error": str(last_err), "raw_response": {}}

def run_track(track, scenarios, system_prompt, body_params, raw_log):
    results = []
    for i, s in enumerate(scenarios):
        sid = s["id"]
        user_payload = json.dumps(s["input"], indent=2)
        print(f"  [{i+1}/{len(scenarios)}] {sid} ...", end=" ", flush=True)

        resp = call_api(system_prompt, user_payload, body_params)
        raw_log.write(json.dumps({"id": sid, "track": track, "scenario": s, "response": resp}) + "\n")
        raw_log.flush()

        obj = extract_json(resp["content"])
        latency = resp["latency_ms"]

        if track == "decoder":
            schema_ok = obj is not None and check_decoder_schema(obj)
            got = obj.get("relation") if schema_ok else "SCHEMA_VIOLATION"
            conf = float(obj.get("confidence", 0.0)) if schema_ok else 0.0
            correct = schema_ok and got == s["expected"]["relation"]
            exp = s["expected"]
            rec = {
                "id": sid, "category": s["category"], "difficulty": s["difficulty"],
                "expected": exp["relation"], "got": got, "correct": correct,
                "schema_ok": schema_ok, "confidence": conf,
                "confidence_in_bounds": exp["min_confidence"] <= conf <= exp["max_confidence"],
                "latency_ms": latency, "finish_reason": resp["finish_reason"],
                "explanation": obj.get("explanation", "") if schema_ok else resp["content"][:200],
            }
        else:
            eid_a = s["input"]["entry_id_a"]
            eid_b = s["input"]["entry_id_b"]
            schema_ok = obj is not None and check_mediator_schema(obj, eid_a, eid_b)
            exp = s["expected"]
            if schema_ok:
                got_kind = "question" if obj.get("clarifying_question") is not None else "resolution"
                got_res = obj.get("proposed_resolution", "")
                got_winner = obj.get("winning_entry_id", "")
                conf = float(obj.get("confidence", 0.0))
            else:
                got_kind = "SCHEMA_VIOLATION"
                got_res = "SCHEMA_VIOLATION"
                got_winner = ""
                conf = 0.0

            kind_correct = schema_ok and got_kind == exp["kind"]
            res_correct = kind_correct and (exp["kind"] == "question" or got_res == exp.get("resolution"))
            winner_correct = kind_correct and exp["kind"] == "resolution" and got_winner == exp.get("winning_entry_id", "")

            rec = {
                "id": sid, "category": s["category"], "difficulty": s["difficulty"],
                "expected_kind": exp["kind"], "got_kind": got_kind,
                "expected_resolution": exp.get("resolution"), "got_resolution": got_res,
                "expected_winner": exp.get("winning_entry_id"), "got_winner": got_winner,
                "kind_correct": kind_correct, "resolution_correct": res_correct,
                "winner_correct": winner_correct,
                "schema_ok": schema_ok, "confidence": conf,
                "confidence_in_bounds": exp["min_confidence"] <= conf <= exp["max_confidence"],
                "latency_ms": latency, "finish_reason": resp["finish_reason"],
                "rationale": obj.get("rationale", "") if schema_ok else resp["content"][:200],
                "clarifying_question": obj.get("clarifying_question") if schema_ok else None,
            }

        results.append(rec)
        status = "OK" if (rec.get("correct") or rec.get("resolution_correct")) else "MISS"
        print(f"{status} ({latency:.0f}ms)")
        time.sleep(0.5)  # be nice to free tier

    return results

def main():
    ts = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
    model_tag = MODEL.replace("/", "_").replace(":", "_")
    raw_path = RESULTS / f"raw_responses_{model_tag}_{ts}.jsonl"
    metrics_path = RESULTS / f"baseline_{model_tag}_{ts}.json"

    decoder_scenarios = sorted((SCENARIOS / "decoder").glob("*.json"))
    mediator_scenarios = sorted((SCENARIOS / "mediator").glob("*.json"))
    decoder_data = [json.loads(f.read_text()) for f in decoder_scenarios]
    mediator_data = [json.loads(f.read_text()) for f in mediator_scenarios]

    print(f"Model: {MODEL}")
    print(f"Decoder: {len(decoder_data)} scenarios | Mediator: {len(mediator_data)} scenarios")
    print(f"Raw log: {raw_path}")
    print()

    with open(raw_path, "w") as raw_log:
        print("=== DECODER TRACK ===")
        dec_results = run_track("decoder", decoder_data, DECODER_SYSTEM, DECODER_BODY, raw_log)
        print()
        print("=== MEDIATOR TRACK ===")
        med_results = run_track("mediator", mediator_data, MEDIATOR_SYSTEM, MEDIATOR_BODY, raw_log)

    # --- Decoder metrics ---
    dec_total = len(dec_results)
    dec_correct = sum(1 for r in dec_results if r["correct"])
    dec_schema = sum(1 for r in dec_results if r["schema_ok"])
    dec_latencies = [r["latency_ms"] for r in dec_results if r["schema_ok"]]

    # Per-class F1
    per_class = {}
    for rel in RELATIONS:
        tp = sum(1 for r in dec_results if r["schema_ok"] and r["expected"] == rel and r["got"] == rel)
        fp = sum(1 for r in dec_results if r["schema_ok"] and r["expected"] != rel and r["got"] == rel)
        fn = sum(1 for r in dec_results if r["schema_ok"] and r["expected"] == rel and r["got"] != rel)
        prec = tp / (tp + fp) if (tp + fp) > 0 else 0.0
        rec = tp / (tp + fn) if (tp + fn) > 0 else 0.0
        f1 = 2 * prec * rec / (prec + rec) if (prec + rec) > 0 else 0.0
        per_class[rel] = {"precision": round(prec, 4), "recall": round(rec, 4), "f1": round(f1, 4), "support": tp + fn}

    # Calibration
    cal_bins = [(0.0, 0.5), (0.5, 0.6), (0.6, 0.7), (0.7, 0.8), (0.8, 0.9), (0.9, 1.01)]
    dec_cal = []
    for lo, hi in cal_bins:
        in_bin = [(r["confidence"], r["correct"]) for r in dec_results if r["schema_ok"] and lo <= r["confidence"] < hi]
        if in_bin:
            dec_cal.append({
                "bin": f"{lo:.1f}-{min(hi, 1.0):.1f}",
                "count": len(in_bin),
                "avg_confidence": round(statistics.mean(c for c, _ in in_bin), 4),
                "actual_accuracy": round(statistics.mean(ok for _, ok in in_bin), 4),
            })

    # --- Mediator metrics ---
    med_total = len(med_results)
    med_schema = sum(1 for r in med_results if r["schema_ok"])
    med_kind_correct = sum(1 for r in med_results if r["kind_correct"])
    med_res_correct = sum(1 for r in med_results if r["resolution_correct"])
    med_winner_correct = sum(1 for r in med_results if r.get("winner_correct"))
    med_question_rate = sum(1 for r in med_results if r["got_kind"] == "question") / med_total if med_total else 0
    med_latencies = [r["latency_ms"] for r in med_results if r["schema_ok"]]

    med_cal = []
    for lo, hi in cal_bins:
        in_bin = [(r["confidence"], r["resolution_correct"]) for r in med_results if r["schema_ok"] and lo <= r["confidence"] < hi]
        if in_bin:
            med_cal.append({
                "bin": f"{lo:.1f}-{min(hi, 1.0):.1f}",
                "count": len(in_bin),
                "avg_confidence": round(statistics.mean(c for c, _ in in_bin), 4),
                "actual_accuracy": round(statistics.mean(ok for _, ok in in_bin), 4),
            })

    def pct(vals, p):
        if not vals:
            return 0.0
        s = sorted(vals)
        idx = int(len(s) * p / 100)
        return round(s[min(idx, len(s) - 1)], 1)

    metrics = {
        "model": MODEL,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "decoder": {
            "total": dec_total,
            "correct": dec_correct,
            "accuracy": round(dec_correct / dec_total, 4) if dec_total else 0,
            "schema_compliance": round(dec_schema / dec_total, 4) if dec_total else 0,
            "per_class_f1": per_class,
            "calibration": dec_cal,
            "latency_p50_ms": pct(dec_latencies, 50),
            "latency_p95_ms": pct(dec_latencies, 95),
            "latency_p99_ms": pct(dec_latencies, 99),
            "failures": [{"id": r["id"], "category": r["category"], "expected": r["expected"], "got": r["got"], "confidence": r["confidence"]} for r in dec_results if not r["correct"]],
        },
        "mediator": {
            "total": med_total,
            "schema_compliance": round(med_schema / med_total, 4) if med_total else 0,
            "kind_accuracy": round(med_kind_correct / med_total, 4) if med_total else 0,
            "resolution_accuracy": round(med_res_correct / med_total, 4) if med_total else 0,
            "winner_accuracy": round(med_winner_correct / med_total, 4) if med_total else 0,
            "question_rate": round(med_question_rate, 4),
            "calibration": med_cal,
            "latency_p50_ms": pct(med_latencies, 50),
            "latency_p95_ms": pct(med_latencies, 95),
            "latency_p99_ms": pct(med_latencies, 99),
            "failures": [{"id": r["id"], "category": r["category"], "expected_kind": r["expected_kind"], "got_kind": r["got_kind"], "expected_res": r.get("expected_resolution"), "got_res": r.get("got_resolution"), "confidence": r["confidence"]} for r in med_results if not r["resolution_correct"]],
            "questions_for_review": [{"id": r["id"], "question": r.get("clarifying_question", {}).get("question_text", "") if r.get("clarifying_question") else "", "expected_kind": r["expected_kind"]} for r in med_results if r["got_kind"] == "question"],
        },
        "all_results": {"decoder": dec_results, "mediator": med_results},
    }

    metrics_path.write_text(json.dumps(metrics, indent=2))
    print(f"\n{'='*60}")
    print(f"RESULTS: {metrics_path}")
    print(f"RAW LOG: {raw_path}")
    print(f"\nDECODER: {dec_correct}/{dec_total} correct ({metrics['decoder']['accuracy']:.1%}) | schema {metrics['decoder']['schema_compliance']:.1%}")
    print(f"MEDIATOR: resolution {metrics['mediator']['resolution_accuracy']:.1%} | kind {metrics['mediator']['kind_accuracy']:.1%} | schema {metrics['mediator']['schema_compliance']:.1%}")
    print(f"{'='*60}")

if __name__ == "__main__":
    main()
