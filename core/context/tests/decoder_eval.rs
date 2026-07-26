//! Conflict-decoder eval harness (`models/conflict-decoder/eval/*.json`).
//!
//! Two modes:
//!
//! - DRY (default, CI-safe, no network): feeds each scenario's canned golden
//!   model output through [`parse_verdict`] and asserts the parsed verdict
//!   matches the expected relation. Validates the prompt contract + parser
//!   without a live model.
//! - LIVE (`make eval-decoder`, opt-in, `#[ignore]`d): calls the real
//!   [`ConstrainedDecoder`] against `OPENAI_BASE_URL` and scores verdicts vs
//!   expectations. Skips gracefully when the endpoint env vars are absent.

use std::path::PathBuf;

use fabric_context::decoder::{parse_verdict, DecoderInput};
use fabric_context::{ConflictDecoder, ConstrainedDecoder, ConstrainedDecoderConfig};
use fabric_types::conflict::ConflictRelation;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    expected_relation: String,
    input: DecoderInput,
    golden_output: String,
}

fn eval_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/conflict-decoder/eval")
}

fn load_scenarios() -> Vec<Scenario> {
    let dir = eval_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read eval dir {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no eval scenarios found in {}",
        dir.display()
    );
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()));
            serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("invalid scenario {}: {e}", p.display()))
        })
        .collect()
}

fn expected_relation(s: &Scenario) -> i32 {
    match s.expected_relation.as_str() {
        "SUPERSEDES" => ConflictRelation::Supersedes as i32,
        "CONTRADICTS" => ConflictRelation::Contradicts as i32,
        "INDEPENDENT" => ConflictRelation::Independent as i32,
        "AMBIGUOUS" => ConflictRelation::Ambiguous as i32,
        other => panic!("scenario {}: unknown expected_relation '{other}'", s.name),
    }
}

fn relation_name(relation: i32) -> &'static str {
    match ConflictRelation::try_from(relation) {
        Ok(ConflictRelation::Supersedes) => "SUPERSEDES",
        Ok(ConflictRelation::Contradicts) => "CONTRADICTS",
        Ok(ConflictRelation::Independent) => "INDEPENDENT",
        Ok(ConflictRelation::Ambiguous) => "AMBIGUOUS",
        _ => "UNSPECIFIED",
    }
}

/// DRY mode: golden outputs through the real parser. No network.
#[test]
fn dry_eval_golden_outputs_match_expected() {
    let scenarios = load_scenarios();
    // The test set must cover all four relations.
    for rel in ["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"] {
        assert!(
            scenarios.iter().any(|s| s.expected_relation == rel),
            "eval set is missing coverage for {rel}"
        );
    }
    for s in &scenarios {
        let verdict = parse_verdict(&s.golden_output, &s.input)
            .unwrap_or_else(|e| panic!("scenario {}: golden output failed to parse: {e}", s.name));
        assert_eq!(
            verdict.relation,
            expected_relation(s),
            "scenario {}: parsed relation {} != expected {}",
            s.name,
            relation_name(verdict.relation),
            s.expected_relation
        );
        // Identity is injected from the input, never from model output.
        assert_eq!(verdict.session_id, s.input.session_id);
        assert_eq!(verdict.entry_id_a, s.input.entry_id_a);
        assert_eq!(verdict.entry_id_b, s.input.entry_id_b);
    }
}

/// LIVE mode: real endpoint, real decoding. Opt-in via `make eval-decoder`;
/// skipped (pass) when the endpoint env vars are not set, so CI never breaks.
#[tokio::test]
#[ignore = "live eval: requires OPENAI_BASE_URL + FABRIC_DECODER_MODEL"]
async fn live_eval_against_endpoint() {
    let config = match ConstrainedDecoderConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("live eval skipped: {e}");
            return;
        }
    };
    let decoder = ConstrainedDecoder::new(config);
    let scenarios = load_scenarios();

    let mut correct = 0usize;
    for s in &scenarios {
        match decoder.decode(s.input.clone()).await {
            Ok(v) => {
                let expected = expected_relation(s);
                let ok = v.relation == expected;
                correct += usize::from(ok);
                eprintln!(
                    "[{}] {}: got {} ({:.2}) expected {}",
                    if ok { "PASS" } else { "FAIL" },
                    s.name,
                    relation_name(v.relation),
                    v.confidence,
                    s.expected_relation
                );
            }
            Err(e) => {
                eprintln!("[ERROR] {}: {e}", s.name);
            }
        }
    }
    eprintln!(
        "live eval: {correct}/{} correct ({:.0}%)",
        scenarios.len(),
        100.0 * correct as f64 / scenarios.len() as f64
    );
}
