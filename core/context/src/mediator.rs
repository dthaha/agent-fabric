//! Tier 3 conflict mediator seam. Propose-ONLY: a [`ConflictMediator`] reads
//! a Tier 2 [`ConflictVerdict`] plus the full two-branch context and either
//! proposes a concrete resolution or asks ONE targeted clarifying question.
//! It never acts, never enforces, never calls policy — the Tier 4 policy gate
//! holds the veto.
//!
//! This module ships the socket, not the model: the trait, the input contract
//! ([`MediatorInput`] + locked prompt render), the output contract
//! ([`parse_proposal`] + [`PROPOSAL_OUTPUT_SCHEMA`]), and a deterministic
//! [`StubMediator`] so the pipeline runs end-to-end with no model wired.
//!
//! The parser mirrors [`crate::decoder::parse_verdict`]'s robustness: tolerant
//! of real-model messiness (prose, fences, extra fields, sloppy confidence),
//! fail-closed on garbage, and anti-spoof on identity — `session_id` is
//! injected from the input and an invented `winning_entry_id` (not one of the
//! two real entry IDs) is cleared, never trusted.

use async_trait::async_trait;
use fabric_types::conflict::{
    ClarifyingQuestion, ConflictRelation, ConflictResolution, ConflictVerdict, ResolutionProposal,
    SharedEntity,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::decoder::DecoderInput;

/// Errors from the mediator seam. Parsing failures are fail-closed: an
/// unparseable model output is an error, never a fabricated proposal.
#[derive(Debug, Error)]
pub enum MediatorError {
    #[error("unparseable mediator output: {0}")]
    Parse(String),
    #[error("mediator endpoint misconfigured: {0}")]
    Config(String),
    #[error("mediator endpoint error: {0}")]
    Http(String),
    #[error("mediator endpoint timed out after {0}ms")]
    Timeout(u64),
}

/// The output contract every `ConflictMediator` impl (and its prompt) is held
/// to. The model emits a single JSON object matching this shape; `session_id`
/// is NOT part of the model's output — it is injected from the
/// [`MediatorInput`] by [`parse_proposal`], and `winning_entry_id` is trusted
/// only if it is one of the two real entry IDs.
pub const PROPOSAL_OUTPUT_SCHEMA: &str = r#"{
  "relation": "SUPERSEDES" | "CONTRADICTS" | "INDEPENDENT" | "AMBIGUOUS",
  "winning_entry_id": string,
  "proposed_resolution": "LAST_WRITE_WINS" | "COMPENSATE" | "ROLLBACK" | "ESCALATE" | "QUARANTINE",
  "confidence": number in [0, 1],
  "rationale": string,
  "clarifying_question": {"question_text": string, "options": [string]} | null
}"#;

/// A serde-friendly view of the decoder's [`ConflictVerdict`] for the
/// mediator prompt. Identity fields are intentionally absent — the mediator
/// reads identity from [`MediatorInput`], not from the verdict, so a verdict
/// cannot relabel which entries were judged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerdictView {
    pub relation: String,
    #[serde(default)]
    pub shared_entities: Vec<SharedEntityView>,
    pub confidence: f32,
    #[serde(default)]
    pub explanation: String,
}

impl From<&ConflictVerdict> for VerdictView {
    fn from(v: &ConflictVerdict) -> Self {
        VerdictView {
            relation: relation_name(v.relation).to_string(),
            shared_entities: v
                .shared_entities
                .iter()
                .map(SharedEntityView::from)
                .collect(),
            confidence: v.confidence,
            explanation: v.explanation.clone(),
        }
    }
}

/// Serde view of a proto [`SharedEntity`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedEntityView {
    pub entity_type: String,
    pub entity_id: String,
}

impl From<&SharedEntity> for SharedEntityView {
    fn from(e: &SharedEntity) -> Self {
        SharedEntityView {
            entity_type: e.entity_type.clone(),
            entity_id: e.entity_id.clone(),
        }
    }
}

/// The mediator input contract: the Tier 2 verdict plus the full
/// [`DecoderInput`] (the two flagged entries, their decoded tool calls, and
/// the bounded context window), optionally tagged with the tool category so
/// the high-stakes fail-closed bias can fire. Serializable so a future
/// HTTP/LLM impl can ship it to a provider; `render_prompt` locks the exact
/// prompt template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediatorInput {
    pub verdict: VerdictView,
    #[serde(flatten)]
    pub input: DecoderInput,
    /// e.g. "financial", "deployment", "filesystem". Drives the high-stakes
    /// QUARANTINE/ESCALATE bias. None = no category signal.
    #[serde(default)]
    pub tool_category: Option<String>,
}

impl MediatorInput {
    /// Build from a Tier 2 verdict and the [`DecoderInput`] that produced it.
    pub fn new(verdict: &ConflictVerdict, input: DecoderInput) -> Self {
        MediatorInput {
            verdict: VerdictView::from(verdict),
            input,
            tool_category: None,
        }
    }

    /// Render the exact prompt a model would receive. Part of the contract —
    /// locked. The prompt instructs the model to emit only
    /// [`PROPOSAL_OUTPUT_SCHEMA`] JSON; [`parse_proposal`] is the tolerant
    /// reader for what it actually emits.
    pub fn render_prompt(&self) -> String {
        let mut p = String::new();
        p.push_str(
            "A conflict classifier has judged two diverged tool calls. Given its verdict \
             and the full context, either propose a resolution or ask ONE targeted \
             clarifying question.\n\
             \n\
             Respond with a single JSON object matching this schema (no other fields \
             are required; extra fields are ignored):\n",
        );
        p.push_str(PROPOSAL_OUTPUT_SCHEMA);
        p.push_str(
            "\n\nResolution meanings:\n\
             - LAST_WRITE_WINS: the newer entry stands, the older is superseded\n\
             - COMPENSATE: undo the older action and apply the newer\n\
             - ROLLBACK: revert to a prior known-good state\n\
             - ESCALATE: route to a human/higher authority (also use when asking a question)\n\
             - QUARANTINE: mark unresolved, act on neither (fail-closed default)\n",
        );
        p.push_str(&format!(
            "\nDecoder verdict: relation {}, confidence {:.2}",
            self.verdict.relation, self.verdict.confidence
        ));
        if !self.verdict.explanation.is_empty() {
            p.push_str(&format!(", \"{}\"", self.verdict.explanation));
        }
        p.push('\n');
        if let Some(cat) = &self.tool_category {
            p.push_str(&format!("Tool category: {cat}\n"));
        }
        p.push_str("\nConversation context (entries preceding the conflict, oldest first):\n");
        if self.input.context.is_empty() {
            p.push_str("(none)\n");
        }
        for turn in &self.input.context {
            p.push_str(&format!(
                "[seq {}] {}: {}\n",
                turn.seq, turn.kind, turn.content
            ));
        }
        p.push_str(&format!(
            "\nTool call A (entry {}):\n{}\n",
            self.input.entry_id_a,
            crate::decoder::render_call(&self.input.call_a)
        ));
        p.push_str(&format!(
            "\nTool call B (entry {}):\n{}\n",
            self.input.entry_id_b,
            crate::decoder::render_call(&self.input.call_b)
        ));
        p.push_str(&format!(
            "\nThe two real entry IDs are \"{}\" and \"{}\". winning_entry_id MUST be one of these, or \"\".\n",
            self.input.entry_id_a, self.input.entry_id_b
        ));
        p.push_str("\nEmit only the JSON object.\n");
        p
    }
}

/// The raw shape [`parse_proposal`] accepts from model output. Extra/unknown
/// fields are ignored by serde's default behavior.
#[derive(Debug, Deserialize)]
struct RawProposal {
    relation: String,
    #[serde(default)]
    winning_entry_id: String,
    proposed_resolution: String,
    confidence: serde_json::Value,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    clarifying_question: Option<RawQuestion>,
}

#[derive(Debug, Deserialize)]
struct RawQuestion {
    question_text: String,
    #[serde(default)]
    options: Vec<String>,
}

/// Extract the JSON object from messy model output: tolerates leading and
/// trailing prose and markdown code fences by taking the slice from the
/// first `{` to the last `}`.
fn extract_json(raw: &str) -> Result<&str, MediatorError> {
    let start = raw
        .find('{')
        .ok_or_else(|| MediatorError::Parse("no JSON object found in output".into()))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| MediatorError::Parse("no JSON object found in output".into()))?;
    if end < start {
        return Err(MediatorError::Parse(
            "malformed JSON braces in output".into(),
        ));
    }
    Ok(&raw[start..=end])
}

/// Map a relation string to a [`ConflictRelation`]. Accepts the bare enum
/// name ("CONTRADICTS") or the proto name ("CONFLICT_RELATION_CONTRADICTS"),
/// case-insensitive, with spaces/hyphens normalized to underscores. Requires
/// a concrete relation: UNSPECIFIED and anything unknown are rejected.
fn parse_relation(s: &str) -> Result<ConflictRelation, MediatorError> {
    let norm = s.trim().to_uppercase().replace([' ', '-'], "_");
    let bare = norm.strip_prefix("CONFLICT_RELATION_").unwrap_or(&norm);
    match bare {
        "SUPERSEDES" => Ok(ConflictRelation::Supersedes),
        "CONTRADICTS" => Ok(ConflictRelation::Contradicts),
        "INDEPENDENT" => Ok(ConflictRelation::Independent),
        "AMBIGUOUS" => Ok(ConflictRelation::Ambiguous),
        other => Err(MediatorError::Parse(format!("unknown relation '{other}'"))),
    }
}

/// Map a resolution string to a [`ConflictResolution`]. Accepts the bare enum
/// name ("LAST_WRITE_WINS") or the proto name
/// ("CONFLICT_RESOLUTION_LAST_WRITE_WINS"), case-insensitive, with
/// spaces/hyphens normalized to underscores. Requires a concrete resolution:
/// UNSPECIFIED and anything unknown are rejected.
fn parse_resolution(s: &str) -> Result<ConflictResolution, MediatorError> {
    let norm = s.trim().to_uppercase().replace([' ', '-'], "_");
    let bare = norm.strip_prefix("CONFLICT_RESOLUTION_").unwrap_or(&norm);
    match bare {
        "LAST_WRITE_WINS" => Ok(ConflictResolution::LastWriteWins),
        "COMPENSATE" => Ok(ConflictResolution::Compensate),
        "ROLLBACK" => Ok(ConflictResolution::Rollback),
        "ESCALATE" => Ok(ConflictResolution::Escalate),
        "QUARANTINE" => Ok(ConflictResolution::Quarantine),
        other => Err(MediatorError::Parse(format!(
            "unknown resolution '{other}'"
        ))),
    }
}

/// Coerce a confidence value: accepts a JSON number or a numeric string.
/// Must be finite; clamped into [0, 1].
fn parse_confidence(v: &serde_json::Value) -> Result<f32, MediatorError> {
    let n = match v {
        serde_json::Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| MediatorError::Parse("confidence is not a real number".into()))?,
        serde_json::Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| MediatorError::Parse(format!("confidence '{s}' is not a number")))?,
        other => {
            return Err(MediatorError::Parse(format!(
                "confidence must be a number, got {other}"
            )))
        }
    };
    if !n.is_finite() {
        return Err(MediatorError::Parse("confidence is not finite".into()));
    }
    Ok(n.clamp(0.0, 1.0) as f32)
}

/// Parse raw model output text into a validated [`ResolutionProposal`].
///
/// Robust to real-model messiness: prose around the JSON, ```json fences,
/// whitespace, confidence as a number or numeric string, and unknown extra
/// fields. Anti-spoof: `session_id` is always injected from `input`, and a
/// `winning_entry_id` that is not one of the two real entry IDs is cleared to
/// empty (never trusted). A `clarifying_question` that is null, absent, or
/// has empty `question_text` collapses to `None`. On unparseable output
/// returns a clear [`MediatorError::Parse`]; it never panics and never
/// fabricates a proposal.
pub fn parse_proposal(
    raw: &str,
    input: &MediatorInput,
) -> Result<ResolutionProposal, MediatorError> {
    let json = extract_json(raw)?;
    let parsed: RawProposal = serde_json::from_str(json)
        .map_err(|e| MediatorError::Parse(format!("invalid JSON object: {e}")))?;

    let winner = parsed.winning_entry_id.trim();
    let winning_entry_id = if winner.is_empty() {
        String::new()
    } else if winner == input.input.entry_id_a || winner == input.input.entry_id_b {
        winner.to_string()
    } else {
        String::new()
    };

    let clarifying_question = parsed
        .clarifying_question
        .filter(|q| !q.question_text.trim().is_empty())
        .map(|q| ClarifyingQuestion {
            question_text: q.question_text,
            options: q.options,
        });

    Ok(ResolutionProposal {
        session_id: input.input.session_id.clone(),
        relation: parse_relation(&parsed.relation)? as i32,
        winning_entry_id,
        proposed_resolution: parse_resolution(&parsed.proposed_resolution)? as i32,
        confidence: parse_confidence(&parsed.confidence)?,
        rationale: parsed.rationale,
        clarifying_question,
    })
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

/// The Tier 3 seam: mediate a judged conflict into a [`ResolutionProposal`]
/// or a clarifying question. Propose-ONLY — implementations emit a proposal
/// and never act, never enforce, never call policy. Independently
/// swappable/post-trainable: a customer (or our later reference model)
/// provides the impl behind this trait. Async and `Send + Sync`, matching
/// `ConflictDecoder`.
#[async_trait]
pub trait ConflictMediator: Send + Sync {
    async fn resolve(&self, input: MediatorInput) -> Result<ResolutionProposal, MediatorError>;
}

/// Deterministic placeholder mediator. Proposes ESCALATE with zero
/// confidence for every input so the pipeline runs end-to-end in tests with
/// no model wired. No randomness, no I/O, no inference.
#[derive(Debug, Default, Clone, Copy)]
pub struct StubMediator;

#[async_trait]
impl ConflictMediator for StubMediator {
    async fn resolve(&self, input: MediatorInput) -> Result<ResolutionProposal, MediatorError> {
        Ok(ResolutionProposal {
            session_id: input.input.session_id,
            relation: parse_relation(&input.verdict.relation).unwrap_or(ConflictRelation::Ambiguous)
                as i32,
            winning_entry_id: String::new(),
            proposed_resolution: ConflictResolution::Escalate as i32,
            confidence: 0.0,
            rationale: "stub mediator: model not wired".into(),
            clarifying_question: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{ContextTurn, ToolCallView};
    use std::collections::BTreeMap;

    fn sample_input() -> MediatorInput {
        MediatorInput {
            verdict: VerdictView {
                relation: "CONTRADICTS".into(),
                shared_entities: vec![SharedEntityView {
                    entity_type: "trip".into(),
                    entity_id: "denver-2026".into(),
                }],
                confidence: 0.72,
                explanation: "book vs cancel the same trip".into(),
            },
            input: DecoderInput {
                session_id: "s1".into(),
                entry_id_a: "a".into(),
                entry_id_b: "b".into(),
                call_a: ToolCallView {
                    tool_name: "book_flight".into(),
                    target: "trip:denver-2026".into(),
                    params: BTreeMap::from([("date".into(), "2026-03-01".into())]),
                    idempotency_key: String::new(),
                },
                call_b: ToolCallView {
                    tool_name: "cancel_trip".into(),
                    target: "trip:denver-2026".into(),
                    params: BTreeMap::new(),
                    idempotency_key: String::new(),
                },
                context: vec![ContextTurn {
                    entry_id: "u1".into(),
                    seq: 1,
                    kind: "ENTRY_KIND_USER_MESSAGE".into(),
                    content: "the offsite might move".into(),
                }],
            },
            tool_category: None,
        }
    }

    #[test]
    fn parses_clean_resolution() {
        let raw = r#"{
            "relation": "SUPERSEDES",
            "winning_entry_id": "b",
            "proposed_resolution": "LAST_WRITE_WINS",
            "confidence": 0.95,
            "rationale": "newer booking is a clean revision",
            "clarifying_question": null
        }"#;
        let p = parse_proposal(raw, &sample_input()).unwrap();
        assert_eq!(p.relation, ConflictRelation::Supersedes as i32);
        assert_eq!(p.winning_entry_id, "b");
        assert_eq!(
            p.proposed_resolution,
            ConflictResolution::LastWriteWins as i32
        );
        assert!((p.confidence - 0.95).abs() < 1e-6);
        assert!(p.clarifying_question.is_none());
        assert_eq!(p.session_id, "s1");
    }

    #[test]
    fn parses_clarifying_question() {
        let raw = r#"{
            "relation": "CONTRADICTS",
            "winning_entry_id": "",
            "proposed_resolution": "ESCALATE",
            "confidence": 0.45,
            "rationale": "cannot tell if cancel was deliberate",
            "clarifying_question": {"question_text": "did you mean to cancel?", "options": ["yes", "no"]}
        }"#;
        let p = parse_proposal(raw, &sample_input()).unwrap();
        let q = p.clarifying_question.unwrap();
        assert_eq!(q.question_text, "did you mean to cancel?");
        assert_eq!(q.options, vec!["yes", "no"]);
    }

    #[test]
    fn empty_question_text_collapses_to_none() {
        let raw = r#"{"relation": "CONTRADICTS", "proposed_resolution": "QUARANTINE",
            "confidence": 0.5, "clarifying_question": {"question_text": "  ", "options": []}}"#;
        let p = parse_proposal(raw, &sample_input()).unwrap();
        assert!(p.clarifying_question.is_none());
    }

    #[test]
    fn invented_winning_entry_id_is_cleared() {
        let raw = r#"{"relation": "SUPERSEDES", "winning_entry_id": "evil-id",
            "proposed_resolution": "LAST_WRITE_WINS", "confidence": 0.9,
            "session_id": "evil", "entry_id_a": "evil"}"#;
        let p = parse_proposal(raw, &sample_input()).unwrap();
        assert_eq!(p.winning_entry_id, "");
        assert_eq!(p.session_id, "s1");
    }

    #[test]
    fn parses_json_in_fence_with_prose() {
        let raw = "Reasoning: the cancel is provisional.\n```json\n{\"relation\":\"CONTRADICTS\",\"winning_entry_id\":\"\",\"proposed_resolution\":\"ESCALATE\",\"confidence\":\"0.4\",\"rationale\":\"unclear\",\"clarifying_question\":{\"question_text\":\"cancel entirely?\",\"options\":[]}}\n```\nDone.";
        let p = parse_proposal(raw, &sample_input()).unwrap();
        assert_eq!(p.proposed_resolution, ConflictResolution::Escalate as i32);
        assert!((p.confidence - 0.4).abs() < 1e-6);
        assert_eq!(
            p.clarifying_question.unwrap().question_text,
            "cancel entirely?"
        );
    }

    #[test]
    fn accepts_resolution_name_variants() {
        for raw_resolution in [
            "last_write_wins",
            "COMPENSATE",
            "CONFLICT_RESOLUTION_ROLLBACK",
            "quarantine",
            "escalate",
        ] {
            let raw = format!(
                r#"{{"relation": "CONTRADICTS", "proposed_resolution": "{raw_resolution}", "confidence": 0.5}}"#
            );
            assert!(
                parse_proposal(&raw, &sample_input()).is_ok(),
                "resolution {raw_resolution}"
            );
        }
    }

    #[test]
    fn rejects_unknown_and_unspecified_resolution() {
        for bad in ["MAYBE", "UNSPECIFIED", "CONFLICT_RESOLUTION_UNSPECIFIED"] {
            let raw = format!(
                r#"{{"relation": "CONTRADICTS", "proposed_resolution": "{bad}", "confidence": 0.5}}"#
            );
            assert!(matches!(
                parse_proposal(&raw, &sample_input()),
                Err(MediatorError::Parse(_))
            ));
        }
    }

    #[test]
    fn clamps_and_validates_confidence() {
        let high = parse_proposal(
            r#"{"relation": "CONTRADICTS", "proposed_resolution": "QUARANTINE", "confidence": 1.9}"#,
            &sample_input(),
        )
        .unwrap();
        assert_eq!(high.confidence, 1.0);
        assert!(matches!(
            parse_proposal(
                r#"{"relation": "CONTRADICTS", "proposed_resolution": "QUARANTINE", "confidence": true}"#,
                &sample_input()
            ),
            Err(MediatorError::Parse(_))
        ));
    }

    #[test]
    fn garbage_input_is_err_not_panic() {
        assert!(matches!(
            parse_proposal("", &sample_input()),
            Err(MediatorError::Parse(_))
        ));
        assert!(matches!(
            parse_proposal("no json here", &sample_input()),
            Err(MediatorError::Parse(_))
        ));
        assert!(matches!(
            parse_proposal("} backwards {", &sample_input()),
            Err(MediatorError::Parse(_))
        ));
        assert!(matches!(
            parse_proposal(r#"{"confidence": 0.5}"#, &sample_input()),
            Err(MediatorError::Parse(_))
        ));
    }

    #[test]
    fn prompt_contains_verdict_calls_context_and_ids() {
        let input = sample_input();
        let prompt = input.render_prompt();
        assert!(prompt.contains("CONTRADICTS"));
        assert!(prompt.contains("book vs cancel the same trip"));
        assert!(prompt.contains("book_flight"));
        assert!(prompt.contains("cancel_trip"));
        assert!(prompt.contains("the offsite might move"));
        assert!(prompt.contains("LAST_WRITE_WINS"));
        assert!(prompt.contains("QUARANTINE"));
        // Deterministic: same input, same prompt.
        assert_eq!(prompt, input.render_prompt());
    }

    #[tokio::test]
    async fn stub_mediator_is_deterministic_escalate() {
        let input = sample_input();
        let p1 = StubMediator.resolve(input.clone()).await.unwrap();
        let p2 = StubMediator.resolve(input).await.unwrap();
        assert_eq!(p1, p2);
        assert_eq!(p1.proposed_resolution, ConflictResolution::Escalate as i32);
        assert_eq!(p1.confidence, 0.0);
        assert_eq!(p1.rationale, "stub mediator: model not wired");
        assert!(p1.clarifying_question.is_none());
        assert_eq!(p1.relation, ConflictRelation::Contradicts as i32);
        assert_eq!(p1.session_id, "s1");
    }

    #[test]
    fn mediator_input_new_converts_verdict() {
        let verdict = ConflictVerdict {
            session_id: "s1".into(),
            entry_id_a: "a".into(),
            entry_id_b: "b".into(),
            relation: ConflictRelation::Contradicts as i32,
            shared_entities: vec![SharedEntity {
                entity_type: "trip".into(),
                entity_id: "denver-2026".into(),
            }],
            confidence: 0.72,
            explanation: "book vs cancel".into(),
        };
        let mi = MediatorInput::new(&verdict, sample_input().input);
        assert_eq!(mi.verdict.relation, "CONTRADICTS");
        assert_eq!(mi.verdict.shared_entities.len(), 1);
        assert_eq!(mi.tool_category, None);
    }
}
