#!/usr/bin/env python3
"""Mediator-only eval with prompt variants.
EVAL_MODEL = model slug
EVAL_VARIANT = baseline | strip_conf | adversarial
"""

import argparse, json, os, re, sys, time, statistics
from pathlib import Path
from datetime import datetime, timezone

try:
    import httpx
except ImportError:
    sys.exit("pip install httpx")

parser = argparse.ArgumentParser(description="Mediator-only eval with prompt variants")
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
MODEL = os.environ.get("EVAL_MODEL", "poolside/laguna-xs-2.1:free")
VARIANT = os.environ.get("EVAL_VARIANT", "baseline")

PROMPTS = Path(__file__).parent / "prompts"
SCENARIOS = Path(__file__).parent / "scenarios"
RESULTS = Path(__file__).parent / "results"
RESULTS.mkdir(exist_ok=True)

MEDIATOR_SYSTEM_BASE = (PROMPTS / "mediator_system.md").read_text()

# --- Variant system prompts ---
ADVERSARIAL_ADDENDUM = """

## CRITICAL: Decoder Independence Rule

The decoder verdict below is ONE input, not ground truth. Decoders are
classification-only sensors with known biases — particularly an over-tendency
toward SUPERSEDES. You MUST independently verify the relation against the raw
context before accepting it.

Before proposing a resolution:
1. Read the raw context entries and tool calls FIRST, ignoring the verdict.
2. Form your own hypothesis about what happened.
3. ONLY THEN compare against the decoder's verdict.
4. If your independent read disagrees with the decoder, trust YOUR reasoning
   and note the disagreement in your rationale.

A decoder saying "SUPERSEDES, confidence 0.93" does NOT mean it's a clean
revision. The decoder cannot see intent, stakes, or domain semantics — you can.
"""

def get_system_prompt():
    if VARIANT == "adversarial":
        return MEDIATOR_SYSTEM_BASE + ADVERSARIAL_ADDENDUM
    return MEDIATOR_SYSTEM_BASE

def transform_input(scenario_input):
    """Apply variant transformation to the mediator input."""
    inp = json.loads(json.dumps(scenario_input))  # deep copy
    if VARIANT == "strip_conf":
        if "verdict" in inp and "confidence" in inp["verdict"]:
            del inp["verdict"]["confidence"]
    # adversarial: no input transform, just the system prompt change
    return inp

RESOLUTIONS = ["LAST_WRITE_WINS", "COMPENSATE", "ROLLBACK", "ESCALATE", "QUARANTINE"]

MEDIATOR_BODY = {"temperature": 0.7, "top_p": 0.9, "max_tokens": 4096, "top_k": 20, "reasoning": {"effort": "high"}}

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
    if text.startswith("```"):
        text = re.sub(r"^```(?:json)?\s*\n?", "", text)
        text = re.sub(r"\n?```\s*$", "", text)
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        m = re.search(r"\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}", text, re.DOTALL)
        if m:
            try:
                return json.loads(m.group())
            except json.JSONDecodeError:
                pass
    return None

def check_mediator_schema(obj, eid_a, eid_b):
    if not isinstance(obj, dict):
        return False
    expected_keys = {"relation", "winning_entry_id", "proposed_resolution", "confidence", "rationale", "clarifying_question"}
    if set(obj.keys()) != expected_keys:
        return False
    if obj.get("relation") not in ["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"]:
        return False
    if obj.get("proposed_resolution") not in RESOLUTIONS:
        return False
    if not isinstance(obj.get("confidence"), (int, float)) or not (0.0 <= obj["confidence"] <= 1.0):
        return False
    if not isinstance(obj.get("rationale"), str) or len(obj["rationale"]) < 5:
        return False
    return True

def call_api(system_prompt, user_payload, body_params):
    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": user_payload},
    ]
    body = {"model": MODEL, "messages": messages, **body_params}
    last_err = None
    for attempt in range(5):
        try:
            t0 = time.monotonic()
            resp = client.post(API_URL, json=body)
            latency = (time.monotonic() - t0) * 1000
            if resp.status_code == 429:
                wait = 5 * (attempt + 1)
                print(f"    rate limited, waiting {wait}s...", flush=True)
                time.sleep(wait)
                continue
            data = resp.json()
            if "choices" not in data:
                raise ValueError(f"no choices: {list(data.keys())}")
            c = data["choices"][0]
            msg = c["message"]
            return {
                "content": msg.get("content") or "",
                "reasoning": msg.get("reasoning") or "",
                "latency_ms": latency,
                "usage": data.get("usage", {}),
                "finish_reason": c.get("finish_reason", ""),
                "raw_response": data,
            }
        except Exception as e:
            last_err = e
            wait = 3 * (attempt + 1)
            print(f"    error ({e}), retry in {wait}s...", flush=True)
            time.sleep(wait)
    return {"content": "", "reasoning": "", "latency_ms": 0, "usage": {}, "finish_reason": "ERROR", "error": str(last_err), "raw_response": {}}

def main():
    ts = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
    model_tag = MODEL.replace("/", "_").replace(":", "_")
    tag = f"{model_tag}_{VARIANT}_{ts}"
    raw_path = RESULTS / f"raw_responses_{tag}.jsonl"
    metrics_path = RESULTS / f"baseline_{tag}.json"

    mediator_files = sorted((SCENARIOS / "mediator").glob("*.json"))
    mediator_data = [json.loads(f.read_text()) for f in mediator_files]

    system_prompt = get_system_prompt()
    print(f"Model: {MODEL} | Variant: {VARIANT}")
    print(f"Mediator: {len(mediator_data)} scenarios")
    print(f"Raw log: {raw_path}\n")

    results = []
    with open(raw_path, "w") as raw_log:
        for i, s in enumerate(mediator_data):
            sid = s["id"]
            transformed = transform_input(s["input"])
            user_payload = json.dumps(transformed, indent=2)
            print(f"  [{i+1}/{len(mediator_data)}] {sid} ...", end=" ", flush=True)

            resp = call_api(system_prompt, user_payload, MEDIATOR_BODY)
            raw_log.write(json.dumps({"id": sid, "variant": VARIANT, "scenario": s, "response": resp}) + "\n")
            raw_log.flush()

            obj = extract_json(resp["content"])
            latency = resp["latency_ms"]
            exp = s["expected"]
            eid_a = s["input"]["entry_id_a"]
            eid_b = s["input"]["entry_id_b"]

            schema_ok = obj is not None and check_mediator_schema(obj, eid_a, eid_b)
            got_res = obj.get("proposed_resolution") if schema_ok else "SCHEMA_VIOLATION"
            got_kind = "question" if (schema_ok and obj.get("clarifying_question")) else "resolution"
            exp_res = exp["resolution"]
            exp_kind = exp["kind"]

            res_correct = schema_ok and got_res == exp_res
            kind_correct = got_kind == exp_kind

            # Winner check for LWW
            winner_correct = None
            if schema_ok and got_res == "LAST_WRITE_WINS":
                winner_correct = obj.get("winning_entry_id") in [eid_a, eid_b]
                if "winning_entry_id" in exp:
                    winner_correct = obj.get("winning_entry_id") == exp["winning_entry_id"]

            conf = float(obj.get("confidence", 0.0)) if schema_ok else 0.0

            rec = {
                "id": sid, "category": s["category"],
                "expected_resolution": exp_res, "got_resolution": got_res,
                "expected_kind": exp_kind, "got_kind": got_kind,
                "resolution_correct": res_correct, "kind_correct": kind_correct,
                "winner_correct": winner_correct,
                "schema_ok": schema_ok, "confidence": conf,
                "latency_ms": latency, "finish_reason": resp["finish_reason"],
            }
            results.append(rec)
            status = "OK" if res_correct else "MISS"
            print(f"{status} ({latency:.0f}ms)", flush=True)

    # Metrics
    total = len(results)
    schema_ok_count = sum(1 for r in results if r["schema_ok"])
    res_correct = sum(1 for r in results if r["resolution_correct"])
    kind_correct = sum(1 for r in results if r["kind_correct"])
    q_count = sum(1 for r in results if r["got_kind"] == "question")
    latencies = [r["latency_ms"] for r in results if r["latency_ms"] > 0]

    metrics = {
        "model": MODEL,
        "variant": VARIANT,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "total": total,
        "schema_compliance": schema_ok_count / total if total else 0,
        "resolution_accuracy": res_correct / total if total else 0,
        "kind_accuracy": kind_correct / total if total else 0,
        "question_rate": q_count / total if total else 0,
        "latency_p50_ms": statistics.median(latencies) if latencies else 0,
        "latency_p95_ms": sorted(latencies)[int(len(latencies) * 0.95)] if latencies else 0,
        "failures": [
            {"id": r["id"], "category": r["category"],
             "expected": r["expected_resolution"], "got": r["got_resolution"],
             "expected_kind": r["expected_kind"], "got_kind": r["got_kind"]}
            for r in results if not r["resolution_correct"]
        ],
    }
    metrics_path.write_text(json.dumps(metrics, indent=2))

    print(f"\n{'='*60}")
    print(f"RESULTS: {metrics_path}")
    print(f"\n{MODEL} [{VARIANT}]")
    print(f"  Resolution: {res_correct}/{total} ({res_correct/total:.1%})")
    print(f"  Kind: {kind_correct}/{total} ({kind_correct/total:.1%})")
    print(f"  Schema: {schema_ok_count}/{total} ({schema_ok_count/total:.1%})")
    print(f"  Question rate: {q_count}/{total} ({q_count/total:.1%})")
    print(f"{'='*60}")

if __name__ == "__main__":
    main()
