//! Conflict-mediator eval harness (`models/conflict-mediator/eval/*.json`).
//!
//! Two modes:
//!
//! - DRY (default, CI-safe, no network): feeds each scenario's canned golden
//!   model output through [`parse_proposal`] and asserts the parsed proposal
//!   matches the expected outcome (a resolution, or that a clarifying
//!   question was asked). Validates the prompt contract + parser without a
//!   live model.
//! - LIVE (`make eval-mediator`, opt-in, `#[ignore]`d): calls the real
//!   [`ConstrainedMediator`] against `OPENAI_BASE_URL` and scores proposals
//!   vs expectations. Skips gracefully when the endpoint env vars are absent.

use std::path::PathBuf;

use fabric_context::mediator::{parse_proposal, MediatorInput};
use fabric_context::{ConflictMediator, ConstrainedMediator, ConstrainedMediatorConfig};
use fabric_types::conflict::ConflictResolution;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    expected: Expected,
    input: MediatorInput,
    golden_output: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Expected {
    Resolution {
        resolution: String,
        #[serde(default)]
        winning_entry_id: String,
    },
    Question,
}

fn eval_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/conflict-mediator/eval")
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

fn resolution_i32(name: &str) -> i32 {
    match name {
        "LAST_WRITE_WINS" => ConflictResolution::LastWriteWins as i32,
        "COMPENSATE" => ConflictResolution::Compensate as i32,
        "ROLLBACK" => ConflictResolution::Rollback as i32,
        "ESCALATE" => ConflictResolution::Escalate as i32,
        "QUARANTINE" => ConflictResolution::Quarantine as i32,
        other => panic!("unknown resolution '{other}'"),
    }
}

fn resolution_name(resolution: i32) -> &'static str {
    match ConflictResolution::try_from(resolution) {
        Ok(ConflictResolution::LastWriteWins) => "LAST_WRITE_WINS",
        Ok(ConflictResolution::Compensate) => "COMPENSATE",
        Ok(ConflictResolution::Rollback) => "ROLLBACK",
        Ok(ConflictResolution::Escalate) => "ESCALATE",
        Ok(ConflictResolution::Quarantine) => "QUARANTINE",
        _ => "UNSPECIFIED",
    }
}

/// Score a parsed proposal against a scenario's expectation. Returns an error
/// string on mismatch.
fn check(s: &Scenario, p: &fabric_types::conflict::ResolutionProposal) -> Result<(), String> {
    // Identity is injected from the input, never from model output.
    if p.session_id != s.input.input.session_id {
        return Err(format!(
            "session_id {} not injected",
            s.input.input.session_id
        ));
    }
    // Anti-spoof: winner is either empty or one of the two real entry IDs.
    if !p.winning_entry_id.is_empty()
        && p.winning_entry_id != s.input.input.entry_id_a
        && p.winning_entry_id != s.input.input.entry_id_b
    {
        return Err(format!(
            "invented winning_entry_id '{}' survived parsing",
            p.winning_entry_id
        ));
    }
    match &s.expected {
        Expected::Resolution {
            resolution,
            winning_entry_id,
        } => {
            if p.proposed_resolution != resolution_i32(resolution) {
                return Err(format!(
                    "resolution {} != expected {}",
                    resolution_name(p.proposed_resolution),
                    resolution
                ));
            }
            if p.winning_entry_id != *winning_entry_id {
                return Err(format!(
                    "winning_entry_id '{}' != expected '{}'",
                    p.winning_entry_id, winning_entry_id
                ));
            }
            if p.clarifying_question.is_some() {
                return Err("unexpected clarifying_question on a resolution scenario".into());
            }
            Ok(())
        }
        Expected::Question => {
            let q = p
                .clarifying_question
                .as_ref()
                .ok_or_else(|| "expected a clarifying question, got none".to_string())?;
            if q.question_text.trim().is_empty() {
                return Err("clarifying question has empty question_text".into());
            }
            Ok(())
        }
    }
}

/// DRY mode: golden outputs through the real parser. No network.
#[test]
fn dry_eval_golden_outputs_match_expected() {
    let scenarios = load_scenarios();
    // The eval set must cover both outcome kinds and the high-stakes
    // fail-closed path.
    assert!(
        scenarios
            .iter()
            .any(|s| matches!(s.expected, Expected::Resolution { .. })),
        "eval set is missing resolution scenarios"
    );
    assert!(
        scenarios
            .iter()
            .any(|s| matches!(s.expected, Expected::Question)),
        "eval set is missing clarifying-question scenarios"
    );
    assert!(
        scenarios.iter().any(|s| matches!(
            &s.expected,
            Expected::Resolution { resolution, .. } if resolution == "QUARANTINE"
        )),
        "eval set is missing a QUARANTINE (fail-closed) scenario"
    );
    for s in &scenarios {
        let proposal = parse_proposal(&s.golden_output, &s.input)
            .unwrap_or_else(|e| panic!("scenario {}: golden output failed to parse: {e}", s.name));
        if let Err(msg) = check(s, &proposal) {
            panic!("scenario {}: {msg}", s.name);
        }
    }
}

/// LIVE mode: real endpoint, real mediation. Opt-in via `make eval-mediator`;
/// skipped (pass) when the endpoint env vars are not set, so CI never breaks.
#[tokio::test]
#[ignore = "live eval: requires OPENAI_BASE_URL (recommended: poolside/laguna-xs-2.1, thinking ON)"]
async fn live_eval_against_endpoint() {
    let config = match ConstrainedMediatorConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("live eval skipped: {e}");
            return;
        }
    };
    let mediator = ConstrainedMediator::new(config);
    let scenarios = load_scenarios();

    let mut correct = 0usize;
    for s in &scenarios {
        match mediator.resolve(s.input.clone()).await {
            Ok(p) => match check(s, &p) {
                Ok(()) => {
                    correct += 1;
                    eprintln!(
                        "[PASS] {}: {} ({:.2}) question={}",
                        s.name,
                        resolution_name(p.proposed_resolution),
                        p.confidence,
                        p.clarifying_question.is_some()
                    );
                }
                Err(msg) => eprintln!("[FAIL] {}: {msg}", s.name),
            },
            Err(e) => eprintln!("[ERROR] {}: {e}", s.name),
        }
    }
    eprintln!(
        "live eval: {correct}/{} correct ({:.0}%)",
        scenarios.len(),
        100.0 * correct as f64 / scenarios.len() as f64
    );
}
