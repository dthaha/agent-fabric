#!/usr/bin/env python3
"""Decoder-only eval duel. Env vars: EVAL_MODEL, EVAL_REASONING (low|high)."""

import argparse, json, os, re, sys, time, statistics
from pathlib import Path
from datetime import datetime, timezone

try:
    import httpx
except ImportError:
    sys.exit("httpx is required. Run: pip install -r requirements.txt")

BASE = Path(__file__).resolve().parent
SCENARIOS = BASE / "scenarios"
PROMPTS = BASE / "prompts"
RESULTS = BASE / "results"
RESULTS.mkdir(exist_ok=True)

parser = argparse.ArgumentParser(description="Decoder-only eval duel")
parser.add_argument(
    "--base-url",
    default="https://openrouter.ai/api/v1",
    help="OpenAI-compatible API base URL (vLLM, NIM, OpenRouter, ...)",
)
ARGS = parser.parse_args()

TOKEN = os.environ.get("OPENAI_API_KEY") or os.environ.get("OPENROUTER_API_KEY")
if not TOKEN:
    sys.exit("OPENAI_API_KEY (or OPENROUTER_API_KEY) not set.")
API_URL = ARGS.base_url.rstrip("/") + "/chat/completions"
MODEL = os.environ.get("EVAL_MODEL", "poolside/laguna-xs-2.1")
REASONING = os.environ.get("EVAL_REASONING", "low")

DECODER_SYSTEM = (PROMPTS / "decoder_system.md").read_text()
RELATIONS = ["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"]

BODY = {"temperature": 0.1, "top_p": 0.9, "max_tokens": 1024, "top_k": 20, "reasoning": {"effort": REASONING}}

client = httpx.Client(timeout=120.0, headers={
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

def call_api(system_prompt, user_payload, retries=3):
    payload = {
        "model": MODEL,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_payload},
        ],
        **BODY,
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
            usage = data.get("usage", {})
            return {
                "content": content,
                "latency_ms": latency_ms,
                "usage": usage,
                "finish_reason": choice.get("finish_reason", ""),
            }
        except Exception as e:
            last_err = e
            if attempt < retries - 1:
                wait = 3 * (2 ** attempt)
                print(f"    error ({e}), retry in {wait}s...")
                time.sleep(wait)
    return {"content": "", "latency_ms": 0, "usage": {}, "finish_reason": "ERROR", "error": str(last_err)}

def main():
    ts = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
    model_tag = MODEL.replace("/", "_").replace(":", "_")
    tag = f"{model_tag}_dec_{REASONING}"
    raw_path = RESULTS / f"raw_{tag}_{ts}.jsonl"
    metrics_path = RESULTS / f"results_{tag}_{ts}.json"

    decoder_files = sorted((SCENARIOS / "decoder").glob("*.json"))
    decoder_data = [json.loads(f.read_text()) for f in decoder_files]

    print(f"Model: {MODEL} | Reasoning: {REASONING}")
    print(f"Decoder: {len(decoder_data)} scenarios")
    print(f"Raw log: {raw_path}")
    print()

    results = []
    with open(raw_path, "w") as raw_log:
        for i, s in enumerate(decoder_data):
            sid = s["id"]
            user_payload = json.dumps(s["input"], indent=2)
            print(f"  [{i+1}/{len(decoder_data)}] {sid} ...", end=" ", flush=True)

            resp = call_api(DECODER_SYSTEM, user_payload)
            raw_log.write(json.dumps({"id": sid, "scenario": s, "response": resp}) + "\n")
            raw_log.flush()

            obj = extract_json(resp["content"])
            schema_ok = obj is not None and check_decoder_schema(obj)
            got = obj.get("relation") if schema_ok else "SCHEMA_VIOLATION"
            conf = float(obj.get("confidence", 0.0)) if schema_ok else 0.0
            exp = s["expected"]
            correct = schema_ok and got == exp["relation"]

            rec = {
                "id": sid, "category": s["category"], "difficulty": s["difficulty"],
                "expected": exp["relation"], "got": got, "correct": correct,
                "schema_ok": schema_ok, "confidence": conf,
                "confidence_in_bounds": exp["min_confidence"] <= conf <= exp["max_confidence"],
                "latency_ms": resp["latency_ms"], "finish_reason": resp["finish_reason"],
                "explanation": obj.get("explanation", "") if schema_ok else resp["content"][:200],
            }
            results.append(rec)
            status = "OK" if correct else "MISS"
            print(f"{status} ({resp['latency_ms']:.0f}ms)")
            time.sleep(0.3)

    # Metrics
    total = len(results)
    correct = sum(1 for r in results if r["correct"])
    schema = sum(1 for r in results if r["schema_ok"])
    latencies = [r["latency_ms"] for r in results if r["schema_ok"]]

    per_class = {}
    for rel in RELATIONS:
        tp = sum(1 for r in results if r["schema_ok"] and r["expected"] == rel and r["got"] == rel)
        fp = sum(1 for r in results if r["schema_ok"] and r["expected"] != rel and r["got"] == rel)
        fn = sum(1 for r in results if r["schema_ok"] and r["expected"] == rel and r["got"] != rel)
        prec = tp / (tp + fp) if (tp + fp) > 0 else 0.0
        rec_v = tp / (tp + fn) if (tp + fn) > 0 else 0.0
        f1 = 2 * prec * rec_v / (prec + rec_v) if (prec + rec_v) > 0 else 0.0
        per_class[rel] = {"precision": round(prec, 4), "recall": round(rec_v, 4), "f1": round(f1, 4), "support": tp + fn}

    cal_bins = [(0.0, 0.5), (0.5, 0.6), (0.6, 0.7), (0.7, 0.8), (0.8, 0.9), (0.9, 1.01)]
    calibration = []
    for lo, hi in cal_bins:
        in_bin = [(r["confidence"], r["correct"]) for r in results if r["schema_ok"] and lo <= r["confidence"] < hi]
        if in_bin:
            calibration.append({
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
        "reasoning": REASONING,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "total": total,
        "correct": correct,
        "accuracy": round(correct / total, 4) if total else 0,
        "schema_compliance": round(schema / total, 4) if total else 0,
        "per_class_f1": per_class,
        "calibration": calibration,
        "latency_p50_ms": pct(latencies, 50),
        "latency_p95_ms": pct(latencies, 95),
        "latency_p99_ms": pct(latencies, 99),
        "failures": [{"id": r["id"], "category": r["category"], "expected": r["expected"], "got": r["got"], "confidence": r["confidence"]} for r in results if not r["correct"]],
        "all_results": results,
    }

    metrics_path.write_text(json.dumps(metrics, indent=2))
    print(f"\n{'='*60}")
    print(f"RESULTS: {metrics_path}")
    print(f"\n{MODEL} [reasoning={REASONING}]")
    print(f"  Accuracy: {correct}/{total} ({metrics['accuracy']:.1%})")
    print(f"  Schema: {schema}/{total} ({metrics['schema_compliance']:.1%})")
    print(f"  p50: {metrics['latency_p50_ms']}ms | p95: {metrics['latency_p95_ms']}ms")
    for rel in RELATIONS:
        pc = per_class[rel]
        print(f"  {rel}: F1={pc['f1']:.3f} (P={pc['precision']:.3f} R={pc['recall']:.3f})")
    print(f"{'='*60}")

if __name__ == "__main__":
    main()
