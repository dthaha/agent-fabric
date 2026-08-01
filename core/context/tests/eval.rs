//! Conflict-model eval harnesses (decoder + mediator).
//!
//! Two modes each:
//!
//! - DRY (default, CI-safe, no network): feeds each scenario's canned golden
//!   model output through the real parser and asserts the parsed result
//!   matches the expected outcome. Validates the prompt contract + parser
//!   without a live model.
//! - LIVE (opt-in, `#[ignore]`d): calls the real constrained model against
//!   `OPENAI_BASE_URL` and scores results vs expectations. Skips gracefully
//!   when the endpoint env vars are absent.

mod decoder {
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
            let verdict = parse_verdict(&s.golden_output, &s.input).unwrap_or_else(|e| {
                panic!("scenario {}: golden output failed to parse: {e}", s.name)
            });
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
    #[ignore = "live eval: requires OPENAI_BASE_URL (recommended: poolside/laguna-xs-2.1, thinking OFF)"]
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
}

mod mediator {
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
            let proposal = parse_proposal(&s.golden_output, &s.input).unwrap_or_else(|e| {
                panic!("scenario {}: golden output failed to parse: {e}", s.name)
            });
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
}
