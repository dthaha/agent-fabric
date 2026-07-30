//! Tier 1 structural conflict detector. Deterministic and model-free: it
//! compares `ENTRY_KIND_TOOL_CALL` entries in a merged region and flags
//! obvious collisions — same tool, same target, different params. Idempotent
//! pairs are resolved by last-write-wins here; state-mutating collisions are
//! flagged for escalation to the model tiers (decoder/mediator, Phase E/F).
//!
//! Detection rule:
//! - different tool or different target => [`StructuralVerdict::Independent`]
//! - same tool + same target + same params => [`StructuralVerdict::Duplicate`]
//! - same tool + same target + different params =>
//!   [`StructuralVerdict::Conflict`], disposition [`StructuralDisposition::LastWriteWins`]
//!   when both calls are idempotent (idempotency_key present), else
//!   [`StructuralDisposition::Escalate`].
//!
//! The detector performs no I/O, no inference, and no clock reads; ordering
//! derives only from the entries' own `(received_at, entry_id)` — with
//! `created_at` as the fallback for entries predating the field — matching
//! the reconcile merge order (ADR 006).

use fabric_types::conflict::{ConflictRelation, ConflictVerdict, SharedEntity};
use fabric_types::context::{ContextEntry, EntryKind, ToolCall};
use serde::{Deserialize, Serialize};

use crate::db::timestamp_to_ms;
use crate::tool_call;

/// What the detector concluded about a pair of entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuralVerdict {
    /// Different tool, different target, non-tool-call entries, or an
    /// undecodable (opaque) payload: nothing to act on.
    Independent,
    /// Same tool + same target + same params: a duplicate, not a conflict.
    Duplicate,
    /// Same tool + same target + different params.
    Conflict(StructuralConflict),
}

/// How a structural conflict can be handled without a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructuralDisposition {
    /// Both calls are idempotent: last-write-wins resolves deterministically
    /// (winner in `lww_winner_entry_id`). No escalation needed.
    LastWriteWins,
    /// State-mutating calls with divergent params: flag for tiers 2-4.
    Escalate,
}

/// A deterministic, structurally-detected collision between two tool calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralConflict {
    pub session_id: String,
    pub entry_id_a: String,
    pub entry_id_b: String,
    pub tool_name: String,
    pub target: String,
    pub relation: ConflictRelation,
    pub disposition: StructuralDisposition,
    /// Set when disposition is `LastWriteWins`: the surviving entry under the
    /// deterministic `(received_at, entry_id)` order (`created_at` fallback).
    pub lww_winner_entry_id: Option<String>,
}

impl StructuralConflict {
    /// Convert to the Phase A decoder-output shape. Deterministic detection
    /// reports full confidence.
    pub fn to_verdict(&self) -> ConflictVerdict {
        ConflictVerdict {
            session_id: self.session_id.clone(),
            entry_id_a: self.entry_id_a.clone(),
            entry_id_b: self.entry_id_b.clone(),
            relation: self.relation as i32,
            shared_entities: vec![
                SharedEntity {
                    entity_type: "tool".into(),
                    entity_id: self.tool_name.clone(),
                },
                SharedEntity {
                    entity_type: "target".into(),
                    entity_id: self.target.clone(),
                },
            ],
            confidence: 1.0,
            explanation: format!(
                "structural: same tool '{}' + same target '{}' with divergent params",
                self.tool_name, self.target
            ),
        }
    }
}

/// Decode the `ToolCall` payload of an entry, if it is a decodable tool call.
fn as_tool_call(entry: &ContextEntry) -> Option<ToolCall> {
    if entry.kind != EntryKind::ToolCall as i32 {
        return None;
    }
    tool_call::decode(&entry.payload).ok()
}

/// Deterministic last-write-wins winner: later `(received_at, entry_id)`
/// writes last and wins, with `created_at` as the fallback for entries
/// predating the field. Mirrors reconcile's merge ordering (ADR 006), so
/// the structural-detector winner always matches the merge winner.
fn lww_winner<'a>(a: &'a ContextEntry, b: &'a ContextEntry) -> &'a ContextEntry {
    let key = |e: &ContextEntry| {
        (
            timestamp_to_ms(e.received_at.as_ref().or(e.created_at.as_ref())),
            e.entry_id.clone(),
        )
    };
    if key(a) >= key(b) {
        a
    } else {
        b
    }
}

/// Compare two entries structurally. Pure, deterministic, model-free.
pub fn detect_pair(a: &ContextEntry, b: &ContextEntry) -> StructuralVerdict {
    let (Some(call_a), Some(call_b)) = (as_tool_call(a), as_tool_call(b)) else {
        return StructuralVerdict::Independent;
    };
    if call_a.tool_name != call_b.tool_name || call_a.target != call_b.target {
        return StructuralVerdict::Independent;
    }
    if call_a.params == call_b.params {
        return StructuralVerdict::Duplicate;
    }

    let both_idempotent = !call_a.idempotency_key.is_empty() && !call_b.idempotency_key.is_empty();
    let (disposition, lww_winner_entry_id) = if both_idempotent {
        (
            StructuralDisposition::LastWriteWins,
            Some(lww_winner(a, b).entry_id.clone()),
        )
    } else {
        (StructuralDisposition::Escalate, None)
    };

    StructuralVerdict::Conflict(StructuralConflict {
        session_id: a.session_id.clone(),
        entry_id_a: a.entry_id.clone(),
        entry_id_b: b.entry_id.clone(),
        tool_name: call_a.tool_name,
        target: call_a.target,
        relation: ConflictRelation::Contradicts,
        disposition,
        lww_winner_entry_id,
    })
}

/// Run the detector over a merged region (entries from both replicas after
/// reconcile). Returns every pairwise structural conflict in log order.
/// Pure and deterministic: no I/O, no clock reads.
pub fn detect_in_region(entries: &[ContextEntry]) -> Vec<StructuralConflict> {
    let tool_calls: Vec<&ContextEntry> = entries
        .iter()
        .filter(|e| as_tool_call(e).is_some())
        .collect();
    let mut conflicts = Vec::new();
    for (i, a) in tool_calls.iter().enumerate() {
        for b in &tool_calls[i + 1..] {
            if let StructuralVerdict::Conflict(c) = detect_pair(a, b) {
                conflicts.push(c);
            }
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ms_to_timestamp;
    use fabric_types::context::Locus;
    use std::collections::HashMap;

    fn tool_entry(
        id: &str,
        tool: &str,
        target: &str,
        params: &[(&str, &str)],
        idempotency_key: &str,
        ms: i64,
    ) -> ContextEntry {
        let call = ToolCall {
            tool_name: tool.into(),
            target: target.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
            idempotency_key: idempotency_key.into(),
        };
        ContextEntry {
            entry_id: id.into(),
            session_id: "s1".into(),
            seq: 0,
            kind: EntryKind::ToolCall as i32,
            payload: tool_call::encode(&call),
            lease_holder: "h".into(),
            policy_version: "v1".into(),
            locus: Locus::Endpoint as i32,
            created_at: Some(ms_to_timestamp(ms)),
            received_at: None,
            disposition: String::new(),
        }
    }

    #[test]
    fn same_tool_target_diff_params_is_conflict() {
        let a = tool_entry(
            "a",
            "set_config",
            "ui.theme",
            &[("value", "dark")],
            "",
            1000,
        );
        let b = tool_entry(
            "b",
            "set_config",
            "ui.theme",
            &[("value", "light")],
            "",
            2000,
        );
        let StructuralVerdict::Conflict(c) = detect_pair(&a, &b) else {
            panic!("expected conflict");
        };
        assert_eq!(c.relation, ConflictRelation::Contradicts);
        assert_eq!(c.disposition, StructuralDisposition::Escalate);
        assert_eq!(c.lww_winner_entry_id, None);
        let v = c.to_verdict();
        assert_eq!(v.confidence, 1.0);
        assert_eq!(v.relation, ConflictRelation::Contradicts as i32);
        assert_eq!(v.shared_entities.len(), 2);
    }

    #[test]
    fn same_params_is_duplicate() {
        let a = tool_entry("a", "get", "api/x", &[("q", "1")], "", 1000);
        let b = tool_entry("b", "get", "api/x", &[("q", "1")], "", 2000);
        assert_eq!(detect_pair(&a, &b), StructuralVerdict::Duplicate);
    }

    #[test]
    fn different_tool_or_target_is_independent() {
        let a = tool_entry(
            "a",
            "set_config",
            "ui.theme",
            &[("value", "dark")],
            "",
            1000,
        );
        let b = tool_entry(
            "b",
            "send_email",
            "ui.theme",
            &[("value", "dark")],
            "",
            2000,
        );
        let c = tool_entry("c", "set_config", "ui.font", &[("value", "dark")], "", 2000);
        assert_eq!(detect_pair(&a, &b), StructuralVerdict::Independent);
        assert_eq!(detect_pair(&a, &c), StructuralVerdict::Independent);
    }

    #[test]
    fn idempotent_pair_resolves_lww() {
        // Later (received_at, entry_id) writes last and wins; created_at is
        // the fallback when received_at is unset.
        let a = tool_entry("a", "cache_put", "k1", &[("v", "1")], "key-a", 1000);
        let b = tool_entry("b", "cache_put", "k1", &[("v", "2")], "key-b", 2000);
        let StructuralVerdict::Conflict(c) = detect_pair(&a, &b) else {
            panic!("expected conflict");
        };
        assert_eq!(c.disposition, StructuralDisposition::LastWriteWins);
        assert_eq!(c.lww_winner_entry_id.as_deref(), Some("b"));
        // Symmetric.
        let StructuralVerdict::Conflict(c2) = detect_pair(&b, &a) else {
            panic!("expected conflict");
        };
        assert_eq!(c2.lww_winner_entry_id.as_deref(), Some("b"));
    }

    #[test]
    fn lww_orders_by_received_at_before_created_at() {
        // Same created_at, different received_at: the later server-stamped
        // received_at wins (ADR 006 — created_at is an untrusted claim).
        let mut a = tool_entry("a", "cache_put", "k1", &[("v", "1")], "key-a", 1000);
        a.received_at = Some(ms_to_timestamp(500));
        let mut b = tool_entry("b", "cache_put", "k1", &[("v", "2")], "key-b", 1000);
        b.received_at = Some(ms_to_timestamp(900));
        let StructuralVerdict::Conflict(c) = detect_pair(&a, &b) else {
            panic!("expected conflict");
        };
        assert_eq!(c.lww_winner_entry_id.as_deref(), Some("b"));

        // A backdated created_at cannot override received_at ordering.
        let mut c_entry = tool_entry("c", "cache_put", "k1", &[("v", "3")], "key-c", 1);
        c_entry.received_at = Some(ms_to_timestamp(950));
        let StructuralVerdict::Conflict(c2) = detect_pair(&b, &c_entry) else {
            panic!("expected conflict");
        };
        assert_eq!(c2.lww_winner_entry_id.as_deref(), Some("c"));
    }

    #[test]
    fn one_idempotent_one_mutating_escalates() {
        let a = tool_entry("a", "cache_put", "k1", &[("v", "1")], "key-a", 1000);
        let b = tool_entry("b", "cache_put", "k1", &[("v", "2")], "", 2000);
        let StructuralVerdict::Conflict(c) = detect_pair(&a, &b) else {
            panic!("expected conflict");
        };
        assert_eq!(c.disposition, StructuralDisposition::Escalate);
    }

    #[test]
    fn non_tool_call_and_opaque_payloads_are_independent() {
        let a = tool_entry(
            "a",
            "set_config",
            "ui.theme",
            &[("value", "dark")],
            "",
            1000,
        );
        let mut opaque = a.clone();
        opaque.entry_id = "opaque".into();
        opaque.payload = b"hello".to_vec();
        let mut not_tool = a.clone();
        not_tool.entry_id = "msg".into();
        not_tool.kind = EntryKind::UserMessage as i32;
        assert_eq!(detect_pair(&a, &opaque), StructuralVerdict::Independent);
        assert_eq!(detect_pair(&a, &not_tool), StructuralVerdict::Independent);
    }

    #[test]
    fn region_detection_collects_all_pairs() {
        let a = tool_entry(
            "a",
            "set_config",
            "ui.theme",
            &[("value", "dark")],
            "",
            1000,
        );
        let b = tool_entry(
            "b",
            "set_config",
            "ui.theme",
            &[("value", "light")],
            "",
            2000,
        );
        let c = tool_entry("c", "set_config", "ui.font", &[("value", "mono")], "", 3000);
        let d = tool_entry(
            "d",
            "set_config",
            "ui.theme",
            &[("value", "dark")],
            "",
            4000,
        );
        let conflicts = detect_in_region(&[a.clone(), b.clone(), c.clone(), d.clone()]);
        // a-b (dark vs light) and b-d (light vs dark); a-d is a duplicate,
        // c is on a different target.
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0].entry_id_a, "a");
        assert_eq!(conflicts[0].entry_id_b, "b");
        assert_eq!(conflicts[1].entry_id_a, "b");
        assert_eq!(conflicts[1].entry_id_b, "d");
    }
}
