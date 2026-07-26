//! Tier 2 conflict decoder seam. Classify-ONLY: a [`ConflictDecoder`] reads a
//! flagged conflict plus its surrounding conversation context and emits a
//! [`ConflictVerdict`]. It never acts, never mutates state, and never calls
//! policy — the Tier 4 policy gate keeps the veto (Phase F).
//!
//! This module ships the socket, not the model: the trait, the input
//! contract ([`DecoderInput`] + locked prompt render), the output contract
//! ([`parse_verdict`] + [`OUTPUT_SCHEMA`]), and a deterministic
//! [`StubDecoder`] so the pipeline runs end-to-end with no model wired.
//!
//! Call path: a [`StructuralConflict`] with disposition
//! [`crate::conflict::StructuralDisposition::Escalate`] →
//! [`build_decoder_input`] → `decoder.decode(input)` → verdict.
//!
//! The parser is deliberately built before any real model exists: it must be
//! robust to real-model messiness (prose around the JSON, markdown fences,
//! extra fields, sloppy confidence values) and fail closed (clear error,
//! never a fabricated verdict) on unparseable output.

use std::collections::BTreeMap;

use async_trait::async_trait;
use fabric_types::conflict::{ConflictRelation, ConflictVerdict, SharedEntity};
use fabric_types::context::{ContextEntry, EntryKind, ToolCall};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::conflict::StructuralConflict;
use crate::db::StoreError;
use crate::store::ContextStore;
use crate::tool_call;

/// Errors from the decoder seam. Parsing failures are fail-closed: an
/// unparseable model output is an error, never a fabricated verdict.
#[derive(Debug, Error)]
pub enum DecoderError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("entry not found: {0}")]
    EntryNotFound(String),
    #[error("entry {0} has no decodable tool-call payload")]
    NotAToolCall(String),
    #[error("unparseable decoder output: {0}")]
    Parse(String),
    #[error("decoder endpoint misconfigured: {0}")]
    Config(String),
    #[error("decoder endpoint error: {0}")]
    Http(String),
    #[error("decoder endpoint timed out after {0}ms")]
    Timeout(u64),
}

/// The output contract every `ConflictDecoder` impl (and its prompt) is held
/// to. The model emits a single JSON object matching this shape; identity
/// fields (`session_id`, `entry_id_a`, `entry_id_b`) are NOT part of the
/// model's output — they are injected from the [`DecoderInput`] by
/// [`parse_verdict`], so a model cannot relabel which entries it judged.
pub const OUTPUT_SCHEMA: &str = r#"{
  "relation": "SUPERSEDES" | "CONTRADICTS" | "INDEPENDENT" | "AMBIGUOUS",
  "shared_entities": [{"entity_type": string, "entity_id": string}],
  "confidence": number in [0, 1],
  "explanation": string
}"#;

/// A tool call rendered for the decoder: the decoded Phase D payload with
/// params sorted for deterministic prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallView {
    pub tool_name: String,
    pub target: String,
    pub params: BTreeMap<String, String>,
    pub idempotency_key: String,
}

impl From<&ToolCall> for ToolCallView {
    fn from(call: &ToolCall) -> Self {
        ToolCallView {
            tool_name: call.tool_name.clone(),
            target: call.target.clone(),
            params: call
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            idempotency_key: call.idempotency_key.clone(),
        }
    }
}

/// One entry from the conversation window around the conflict, rendered as
/// text a model can read. Tool calls render structurally; other kinds render
/// their payload as UTF-8 (lossy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextTurn {
    pub entry_id: String,
    pub seq: u64,
    pub kind: String,
    pub content: String,
}

/// The input contract: everything a conflict-decoding model needs to judge a
/// flagged pair. Serializable so a future HTTP/LLM impl can ship it to a
/// provider; `render_prompt` locks the exact prompt template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderInput {
    pub session_id: String,
    pub entry_id_a: String,
    pub entry_id_b: String,
    pub call_a: ToolCallView,
    pub call_b: ToolCallView,
    /// Bounded window of entries immediately preceding the earlier of the
    /// two conflicting entries, oldest first.
    pub context: Vec<ContextTurn>,
}

impl DecoderInput {
    /// Render the exact prompt a model would receive. Part of the contract —
    /// locked. The prompt instructs the model to emit only [`OUTPUT_SCHEMA`]
    /// JSON; [`parse_verdict`] is the tolerant reader for what it actually
    /// emits.
    pub fn render_prompt(&self) -> String {
        let mut p = String::new();
        p.push_str(
            "You are a conflict decoder. Two tool calls from diverged branches of the \
             same session are flagged as potentially conflicting. Classify the relation \
             between them.\n\
             \n\
             Respond with a single JSON object matching this schema (no other fields \
             are required; extra fields are ignored):\n",
        );
        p.push_str(OUTPUT_SCHEMA);
        p.push_str(
            "\n\nRelation meanings:\n\
             - SUPERSEDES: one call clearly replaces the other's intent\n\
             - CONTRADICTS: the calls express opposing intents on the same underlying entity\n\
             - INDEPENDENT: the calls are unrelated; both can stand\n\
             - AMBIGUOUS: cannot tell from the context\n",
        );
        p.push_str("\nConversation context (entries preceding the conflict, oldest first):\n");
        if self.context.is_empty() {
            p.push_str("(none)\n");
        }
        for turn in &self.context {
            p.push_str(&format!(
                "[seq {}] {}: {}\n",
                turn.seq, turn.kind, turn.content
            ));
        }
        p.push_str(&format!(
            "\nTool call A (entry {}):\n{}\n",
            self.entry_id_a,
            render_call(&self.call_a)
        ));
        p.push_str(&format!(
            "\nTool call B (entry {}):\n{}\n",
            self.entry_id_b,
            render_call(&self.call_b)
        ));
        p.push_str("\nEmit only the JSON object.\n");
        p
    }
}

fn render_call(call: &ToolCallView) -> String {
    let params = serde_json::to_string(&call.params).unwrap_or_else(|_| "{}".into());
    let mut s = format!(
        "tool: {}\ntarget: {}\nparams: {}",
        call.tool_name, call.target, params
    );
    if !call.idempotency_key.is_empty() {
        s.push_str(&format!("\nidempotency_key: {}", call.idempotency_key));
    }
    s
}

/// Render a single store entry into a [`ContextTurn`].
fn context_turn(entry: &ContextEntry) -> ContextTurn {
    let kind = EntryKind::try_from(entry.kind)
        .map(|k| k.as_str_name())
        .unwrap_or("ENTRY_KIND_UNSPECIFIED")
        .to_string();
    let content = if entry.kind == EntryKind::ToolCall as i32 {
        tool_call::decode(&entry.payload)
            .map(|c| render_call(&ToolCallView::from(&c)))
            .unwrap_or_else(|_| String::from_utf8_lossy(&entry.payload).into_owned())
    } else {
        String::from_utf8_lossy(&entry.payload).into_owned()
    };
    ContextTurn {
        entry_id: entry.entry_id.clone(),
        seq: entry.seq,
        kind,
        content,
    }
}

/// Build a [`DecoderInput`] from an escalated structural conflict: fetch both
/// flagged entries, decode their tool-call payloads, and pull a bounded
/// window of `window` entries immediately preceding the earlier of the two.
/// This is the documented Tier 1 → Tier 2 handoff; no orchestration beyond
/// it lives here.
pub async fn build_decoder_input(
    store: &impl ContextStore,
    conflict: &StructuralConflict,
    window: u64,
) -> Result<DecoderInput, DecoderError> {
    let entry_a = store
        .entry_by_id(&conflict.entry_id_a)
        .await?
        .ok_or_else(|| DecoderError::EntryNotFound(conflict.entry_id_a.clone()))?;
    let entry_b = store
        .entry_by_id(&conflict.entry_id_b)
        .await?
        .ok_or_else(|| DecoderError::EntryNotFound(conflict.entry_id_b.clone()))?;

    let decode_call = |e: &ContextEntry| -> Result<ToolCall, DecoderError> {
        if e.kind != EntryKind::ToolCall as i32 {
            return Err(DecoderError::NotAToolCall(e.entry_id.clone()));
        }
        tool_call::decode(&e.payload).map_err(|_| DecoderError::NotAToolCall(e.entry_id.clone()))
    };
    let call_a = decode_call(&entry_a)?;
    let call_b = decode_call(&entry_b)?;

    let lo = entry_a.seq.min(entry_b.seq);
    let window_start = lo.saturating_sub(window);
    let after_seq = window_start.saturating_sub(1);
    let context = store
        .entries_since(&conflict.session_id, after_seq)
        .await?
        .into_iter()
        .filter(|e| e.seq >= window_start && e.seq < lo)
        .map(|e| context_turn(&e))
        .collect();

    Ok(DecoderInput {
        session_id: conflict.session_id.clone(),
        entry_id_a: conflict.entry_id_a.clone(),
        entry_id_b: conflict.entry_id_b.clone(),
        call_a: ToolCallView::from(&call_a),
        call_b: ToolCallView::from(&call_b),
        context,
    })
}

/// The raw shape [`parse_verdict`] accepts from model output. Extra/unknown
/// fields are ignored by serde's default behavior.
#[derive(Debug, Deserialize)]
struct RawVerdict {
    relation: String,
    #[serde(default)]
    shared_entities: Vec<RawEntity>,
    confidence: serde_json::Value,
    #[serde(default)]
    explanation: String,
}

#[derive(Debug, Deserialize)]
struct RawEntity {
    entity_type: String,
    entity_id: String,
}

/// Extract the JSON object from messy model output: tolerates leading and
/// trailing prose and markdown code fences by taking the slice from the
/// first `{` to the last `}`.
fn extract_json(raw: &str) -> Result<&str, DecoderError> {
    let start = raw
        .find('{')
        .ok_or_else(|| DecoderError::Parse("no JSON object found in output".into()))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| DecoderError::Parse("no JSON object found in output".into()))?;
    if end < start {
        return Err(DecoderError::Parse(
            "malformed JSON braces in output".into(),
        ));
    }
    Ok(&raw[start..=end])
}

/// Map a relation string to a [`ConflictRelation`]. Accepts the bare enum
/// name ("CONTRADICTS") or the proto name ("CONFLICT_RELATION_CONTRADICTS"),
/// case-insensitive, with spaces/hyphens normalized to underscores. Requires
/// a concrete relation: UNSPECIFIED and anything unknown are rejected.
fn parse_relation(s: &str) -> Result<ConflictRelation, DecoderError> {
    let norm = s.trim().to_uppercase().replace([' ', '-'], "_");
    let bare = norm.strip_prefix("CONFLICT_RELATION_").unwrap_or(&norm);
    match bare {
        "SUPERSEDES" => Ok(ConflictRelation::Supersedes),
        "CONTRADICTS" => Ok(ConflictRelation::Contradicts),
        "INDEPENDENT" => Ok(ConflictRelation::Independent),
        "AMBIGUOUS" => Ok(ConflictRelation::Ambiguous),
        other => Err(DecoderError::Parse(format!("unknown relation '{other}'"))),
    }
}

/// Coerce a confidence value: accepts a JSON number or a numeric string.
/// Must be finite; clamped into [0, 1].
fn parse_confidence(v: &serde_json::Value) -> Result<f32, DecoderError> {
    let n = match v {
        serde_json::Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| DecoderError::Parse("confidence is not a real number".into()))?,
        serde_json::Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| DecoderError::Parse(format!("confidence '{s}' is not a number")))?,
        other => {
            return Err(DecoderError::Parse(format!(
                "confidence must be a number, got {other}"
            )))
        }
    };
    if !n.is_finite() {
        return Err(DecoderError::Parse("confidence is not finite".into()));
    }
    Ok(n.clamp(0.0, 1.0) as f32)
}

/// Parse raw model output text into a validated [`ConflictVerdict`].
///
/// Robust to real-model messiness: prose around the JSON, ```json fences,
/// whitespace, confidence as a number or numeric string, and unknown extra
/// fields. Identity fields (`session_id`, `entry_id_a`, `entry_id_b`) are
/// always injected from `ctx` — never trusted from model output. On
/// unparseable output returns a clear [`DecoderError::Parse`]; it never
/// panics and never fabricates a verdict.
pub fn parse_verdict(raw: &str, ctx: &DecoderInput) -> Result<ConflictVerdict, DecoderError> {
    let json = extract_json(raw)?;
    let parsed: RawVerdict = serde_json::from_str(json)
        .map_err(|e| DecoderError::Parse(format!("invalid JSON object: {e}")))?;
    Ok(ConflictVerdict {
        session_id: ctx.session_id.clone(),
        entry_id_a: ctx.entry_id_a.clone(),
        entry_id_b: ctx.entry_id_b.clone(),
        relation: parse_relation(&parsed.relation)? as i32,
        shared_entities: parsed
            .shared_entities
            .into_iter()
            .map(|e| SharedEntity {
                entity_type: e.entity_type,
                entity_id: e.entity_id,
            })
            .collect(),
        confidence: parse_confidence(&parsed.confidence)?,
        explanation: parsed.explanation,
    })
}

/// The Tier 2 seam: classify a flagged conflict into a [`ConflictVerdict`].
/// Classify-ONLY — implementations emit a verdict and never act, never
/// mutate state, never call policy. Independently swappable/post-trainable:
/// a customer (or our later reference model) provides the impl behind this
/// trait. Async and `Send + Sync`, matching `ContextStore`/`Tool`.
#[async_trait]
pub trait ConflictDecoder: Send + Sync {
    async fn decode(&self, input: DecoderInput) -> Result<ConflictVerdict, DecoderError>;
}

/// Deterministic placeholder decoder. Returns `AMBIGUOUS` with zero
/// confidence for every input so the pipeline runs end-to-end in tests with
/// no model wired. No randomness, no I/O, no inference.
#[derive(Debug, Default, Clone, Copy)]
pub struct StubDecoder;

#[async_trait]
impl ConflictDecoder for StubDecoder {
    async fn decode(&self, input: DecoderInput) -> Result<ConflictVerdict, DecoderError> {
        Ok(ConflictVerdict {
            session_id: input.session_id,
            entry_id_a: input.entry_id_a,
            entry_id_b: input.entry_id_b,
            relation: ConflictRelation::Ambiguous as i32,
            shared_entities: vec![],
            confidence: 0.0,
            explanation: "stub decoder: model not wired".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::{detect_pair, StructuralDisposition, StructuralVerdict};
    use crate::db::ms_to_timestamp;
    use crate::db::tests::{test_lease, test_session};
    use crate::db::SqliteContextStore;
    use fabric_types::context::{EntryKind, Locus, ToolCall};
    use std::collections::HashMap;

    fn sample_input() -> DecoderInput {
        DecoderInput {
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
            context: vec![
                ContextTurn {
                    entry_id: "u1".into(),
                    seq: 1,
                    kind: "ENTRY_KIND_USER_MESSAGE".into(),
                    content: "book me a flight to denver".into(),
                },
                ContextTurn {
                    entry_id: "u2".into(),
                    seq: 4,
                    kind: "ENTRY_KIND_USER_MESSAGE".into(),
                    content: "actually, never mind the trip".into(),
                },
            ],
        }
    }

    #[test]
    fn parses_clean_json() {
        let raw = r#"{
            "relation": "CONTRADICTS",
            "shared_entities": [{"entity_type": "trip", "entity_id": "denver-2026"}],
            "confidence": 0.9,
            "explanation": "book vs cancel the same trip"
        }"#;
        let v = parse_verdict(raw, &sample_input()).unwrap();
        assert_eq!(v.relation, ConflictRelation::Contradicts as i32);
        assert_eq!(v.shared_entities.len(), 1);
        assert_eq!(v.shared_entities[0].entity_id, "denver-2026");
        assert!((v.confidence - 0.9).abs() < 1e-6);
        assert_eq!(v.explanation, "book vs cancel the same trip");
    }

    #[test]
    fn parses_json_in_code_fence() {
        let raw = "Here is my classification:\n```json\n{\"relation\": \"SUPERSEDES\", \"confidence\": 0.75, \"explanation\": \"cancel supersedes\"}\n```\nDone.";
        let v = parse_verdict(raw, &sample_input()).unwrap();
        assert_eq!(v.relation, ConflictRelation::Supersedes as i32);
        assert!((v.confidence - 0.75).abs() < 1e-6);
    }

    #[test]
    fn parses_json_with_surrounding_prose() {
        let raw = "Thinking about it... the answer is {\"relation\": \"INDEPENDENT\", \"confidence\": 0.1} — hope that helps!";
        let v = parse_verdict(raw, &sample_input()).unwrap();
        assert_eq!(v.relation, ConflictRelation::Independent as i32);
        assert_eq!(v.explanation, "");
    }

    #[test]
    fn ignores_unknown_extra_fields() {
        let raw = r#"{"relation": "AMBIGUOUS", "confidence": 0.5, "explanation": "x", "reasoning_trace": "...", "entry_id_a": "evil", "session_id": "evil"}"#;
        let v = parse_verdict(raw, &sample_input()).unwrap();
        assert_eq!(v.relation, ConflictRelation::Ambiguous as i32);
        // Identity always comes from ctx, never from model output.
        assert_eq!(v.session_id, "s1");
        assert_eq!(v.entry_id_a, "a");
        assert_eq!(v.entry_id_b, "b");
    }

    #[test]
    fn clamps_out_of_range_confidence() {
        let high = parse_verdict(
            r#"{"relation": "CONTRADICTS", "confidence": 1.7}"#,
            &sample_input(),
        )
        .unwrap();
        assert_eq!(high.confidence, 1.0);
        let low = parse_verdict(
            r#"{"relation": "CONTRADICTS", "confidence": -0.2}"#,
            &sample_input(),
        )
        .unwrap();
        assert_eq!(low.confidence, 0.0);
    }

    #[test]
    fn accepts_numeric_string_confidence() {
        let v = parse_verdict(
            r#"{"relation": "CONTRADICTS", "confidence": "0.8"}"#,
            &sample_input(),
        )
        .unwrap();
        assert!((v.confidence - 0.8).abs() < 1e-6);
    }

    #[test]
    fn rejects_non_numeric_confidence() {
        let err = parse_verdict(
            r#"{"relation": "CONTRADICTS", "confidence": true}"#,
            &sample_input(),
        );
        assert!(matches!(err, Err(DecoderError::Parse(_))));
    }

    #[test]
    fn accepts_relation_name_variants() {
        for raw_relation in [
            "contradicts",
            "Independent",
            "CONFLICT_RELATION_SUPERSEDES",
            "supersedes",
            "ambiguous",
        ] {
            let raw = format!(r#"{{"relation": "{raw_relation}", "confidence": 0.5}}"#);
            assert!(
                parse_verdict(&raw, &sample_input()).is_ok(),
                "relation {raw_relation}"
            );
        }
    }

    #[test]
    fn rejects_unknown_relation() {
        let err = parse_verdict(
            r#"{"relation": "MAYBE", "confidence": 0.5}"#,
            &sample_input(),
        );
        assert!(matches!(err, Err(DecoderError::Parse(_))));
        let err = parse_verdict(
            r#"{"relation": "UNSPECIFIED", "confidence": 0.5}"#,
            &sample_input(),
        );
        assert!(matches!(err, Err(DecoderError::Parse(_))));
    }

    #[test]
    fn garbage_input_is_err_not_panic() {
        assert!(matches!(
            parse_verdict("", &sample_input()),
            Err(DecoderError::Parse(_))
        ));
        assert!(matches!(
            parse_verdict("no json here", &sample_input()),
            Err(DecoderError::Parse(_))
        ));
        assert!(matches!(
            parse_verdict("{not valid json}", &sample_input()),
            Err(DecoderError::Parse(_))
        ));
        assert!(matches!(
            parse_verdict("} backwards {", &sample_input()),
            Err(DecoderError::Parse(_))
        ));
        assert!(matches!(
            parse_verdict(r#"{"confidence": 0.5}"#, &sample_input()),
            Err(DecoderError::Parse(_))
        ));
    }

    #[test]
    fn prompt_contains_tool_calls_and_context() {
        let prompt = sample_input().render_prompt();
        assert!(prompt.contains("book_flight"));
        assert!(prompt.contains("cancel_trip"));
        assert!(prompt.contains("trip:denver-2026"));
        assert!(prompt.contains("book me a flight to denver"));
        assert!(prompt.contains("actually, never mind the trip"));
        assert!(prompt.contains("CONTRADICTS"));
        // Deterministic: same input, same prompt.
        assert_eq!(prompt, sample_input().render_prompt());
    }

    fn tool_entry(
        id: &str,
        seq: u64,
        tool: &str,
        target: &str,
        params: &[(&str, &str)],
    ) -> ContextEntry {
        let call = ToolCall {
            tool_name: tool.into(),
            target: target.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
            idempotency_key: String::new(),
        };
        ContextEntry {
            entry_id: id.into(),
            session_id: "s1".into(),
            seq,
            kind: EntryKind::ToolCall as i32,
            payload: tool_call::encode(&call),
            lease_holder: "h".into(),
            policy_version: "v1".into(),
            locus: Locus::Endpoint as i32,
            created_at: Some(ms_to_timestamp(1000 + seq as i64)),
        }
    }

    fn msg_entry(id: &str, seq: u64, text: &str) -> ContextEntry {
        ContextEntry {
            entry_id: id.into(),
            session_id: "s1".into(),
            seq,
            kind: EntryKind::UserMessage as i32,
            payload: text.as_bytes().to_vec(),
            lease_holder: "h".into(),
            policy_version: "v1".into(),
            locus: Locus::Endpoint as i32,
            created_at: Some(ms_to_timestamp(1000 + seq as i64)),
        }
    }

    #[tokio::test]
    async fn escalate_feeds_stub_decoder_end_to_end() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store.grant_lease(&test_lease("l1", "s1", "h")).unwrap();
        store
            .insert_entry_raw(&msg_entry("u1", 1, "book me a flight to denver"))
            .unwrap();
        store
            .insert_entry_raw(&msg_entry("u2", 2, "window entry two"))
            .unwrap();
        store
            .insert_entry_raw(&msg_entry("u3", 3, "window entry three"))
            .unwrap();
        let a = tool_entry(
            "a",
            4,
            "book_flight",
            "trip:denver-2026",
            &[("date", "2026-03-01")],
        );
        let b = tool_entry(
            "b",
            5,
            "book_flight",
            "trip:denver-2026",
            &[("date", "2026-03-02")],
        );
        store.insert_entry_raw(&a).unwrap();
        store.insert_entry_raw(&b).unwrap();

        // Tier 1 flags the pair; mutating calls (no idempotency keys) escalate.
        let StructuralVerdict::Conflict(conflict) = detect_pair(&a, &b) else {
            panic!("expected structural conflict");
        };
        assert_eq!(conflict.disposition, StructuralDisposition::Escalate);

        // Tier 1 -> Tier 2 handoff.
        let input = build_decoder_input(&store, &conflict, 2).await.unwrap();
        assert_eq!(input.entry_id_a, "a");
        assert_eq!(input.entry_id_b, "b");
        assert_eq!(input.call_a.params["date"], "2026-03-01");
        assert_eq!(input.call_b.params["date"], "2026-03-02");
        // Window = 2 entries preceding seq 4: seqs 2 and 3.
        assert_eq!(input.context.len(), 2);
        assert_eq!(input.context[0].entry_id, "u2");
        assert_eq!(input.context[1].entry_id, "u3");

        let verdict = StubDecoder.decode(input).await.unwrap();
        assert_eq!(verdict.relation, ConflictRelation::Ambiguous as i32);
        assert_eq!(verdict.confidence, 0.0);
        assert_eq!(verdict.explanation, "stub decoder: model not wired");
        assert_eq!(verdict.entry_id_a, "a");
        assert_eq!(verdict.entry_id_b, "b");

        // Deterministic: same input decodes to the same verdict.
        let input2 = build_decoder_input(&store, &conflict, 2).await.unwrap();
        let verdict2 = StubDecoder.decode(input2).await.unwrap();
        assert_eq!(verdict, verdict2);
    }

    #[tokio::test]
    async fn build_input_errors_on_missing_or_non_tool_entries() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let a = tool_entry("a", 1, "t", "x", &[("v", "1")]);
        let b = tool_entry("b", 2, "t", "x", &[("v", "2")]);
        let StructuralVerdict::Conflict(conflict) = detect_pair(&a, &b) else {
            panic!("expected structural conflict");
        };
        let err = build_decoder_input(&store, &conflict, 2).await.unwrap_err();
        assert!(matches!(err, DecoderError::EntryNotFound(_)));

        store
            .insert_entry_raw(&msg_entry("a", 1, "not a tool call"))
            .unwrap();
        store.insert_entry_raw(&b).unwrap();
        let err = build_decoder_input(&store, &conflict, 2).await.unwrap_err();
        assert!(matches!(err, DecoderError::NotAToolCall(_)));
    }
}
