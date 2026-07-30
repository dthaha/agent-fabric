//! Offline reconcile. After an offline stretch, two replicas of the same
//! session op-log may both have appended entries. Reconcile merges a remote
//! replica into the local store deterministically:
//!
//! - Entries already known (by entry_id) are skipped.
//! - Remote entries whose seq is free locally are inserted in place.
//! - Seq collisions are resolved deterministically: the entry with the
//!   earlier (received_at, entry_id) keeps the contested seq; the loser is
//!   re-appended at the tail with a fresh seq. Both replicas converge to the
//!   same final log regardless of merge direction. `received_at` is
//!   server-stamped on ingest (ADR 006) and authoritative; `created_at` is
//!   an untrusted endpoint claim used only as a fallback for entries
//!   predating the field.
//! - When a policy is supplied, replayed entries are re-evaluated against
//!   its tool rules: a tool call matching a DENY rule is preserved but
//!   marked `QUARANTINE` (ADR 006).

use serde::{Deserialize, Serialize};

use fabric_types::context::ContextEntry;
use fabric_types::policy::{EndpointPolicy, ToolAction};

use crate::conflict::{detect_in_region, StructuralConflict};
use crate::db::{timestamp_to_ms, Result};
use crate::store::ContextStore;

/// Disposition stamped on replayed entries that violate policy (ADR 006).
pub const DISPOSITION_QUARANTINE: &str = "QUARANTINE";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeqConflict {
    pub seq: u64,
    pub kept_entry_id: String,
    pub moved_entry_id: String,
    pub moved_to_seq: u64,
}

/// A replayed entry that violated a policy tool rule on re-evaluation
/// (ADR 006). The entry is kept in the log with `QUARANTINE` disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyViolation {
    pub entry_id: String,
    /// The tool_pattern of the DENY rule that matched.
    pub rule: String,
    /// The rule's action (always `TOOL_ACTION_DENY` for quarantines).
    pub action: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub session_id: String,
    pub applied: u64,
    pub duplicates: u64,
    /// Seq collisions resolved by the deterministic merge (reordered entries).
    pub conflicts: Vec<SeqConflict>,
    /// Tier 1 structural conflicts detected over the merged region: same
    /// tool + same target + different params. Model-free; `Escalate`
    /// dispositions are candidates for the model tiers (Phase E/F).
    pub structural_conflicts: Vec<StructuralConflict>,
    /// Replayed entries quarantined by policy re-evaluation (ADR 006).
    /// Empty when reconcile ran without a policy.
    pub policy_violations: Vec<PolicyViolation>,
}

/// The timestamp that orders an entry in seq-collision resolution:
/// `received_at` (server-stamped on ingest, authoritative) with a
/// `created_at` fallback for entries predating the field (ADR 006).
fn ordering_ms(entry: &ContextEntry) -> i64 {
    timestamp_to_ms(entry.received_at.as_ref().or(entry.created_at.as_ref()))
}

/// Merge all remote entries for `session_id` into the local store. When
/// `policy` is supplied, replayed tool-call entries are re-evaluated against
/// its tool rules and DENY matches are quarantined.
pub async fn reconcile(
    local: &impl ContextStore,
    remote: &impl ContextStore,
    session_id: &str,
    policy: Option<&EndpointPolicy>,
) -> Result<ReconcileReport> {
    let mut report = ReconcileReport {
        session_id: session_id.to_string(),
        ..Default::default()
    };

    let remote_entries = remote.entries_since(session_id, 0).await?;
    let mut applied_ids = std::collections::HashSet::new();
    let mut applied_entries = Vec::new();
    for remote_entry in remote_entries {
        if local.entry_by_id(&remote_entry.entry_id).await?.is_some() {
            report.duplicates += 1;
            continue;
        }
        applied_ids.insert(remote_entry.entry_id.clone());

        match local.entry_at_seq(session_id, remote_entry.seq).await? {
            None => {
                local.insert_entry_raw(&remote_entry).await?;
                applied_entries.push(remote_entry);
                report.applied += 1;
            }
            Some(local_entry) => {
                // Deterministic winner: earlier (received_at, entry_id) keeps
                // the contested seq. received_at falls back to created_at
                // for entries predating the field (ADR 006).
                let remote_ms = ordering_ms(&remote_entry);
                let local_ms = ordering_ms(&local_entry);
                let remote_wins =
                    (remote_ms, &remote_entry.entry_id) < (local_ms, &local_entry.entry_id);

                let tail = local.head_seq(session_id).await? + 1;
                if remote_wins {
                    // Move the local occupant to the tail, insert remote in place.
                    local.reassign_seq(&local_entry.entry_id, tail).await?;
                    local.insert_entry_raw(&remote_entry).await?;
                    report.conflicts.push(SeqConflict {
                        seq: remote_entry.seq,
                        kept_entry_id: remote_entry.entry_id.clone(),
                        moved_entry_id: local_entry.entry_id,
                        moved_to_seq: tail,
                    });
                    applied_entries.push(remote_entry);
                } else {
                    let mut moved = remote_entry.clone();
                    moved.seq = tail;
                    local.insert_entry_raw(&moved).await?;
                    report.conflicts.push(SeqConflict {
                        seq: remote_entry.seq,
                        kept_entry_id: local_entry.entry_id,
                        moved_entry_id: remote_entry.entry_id.clone(),
                        moved_to_seq: tail,
                    });
                    applied_entries.push(moved);
                }
                report.applied += 1;
            }
        }
    }

    // ADR 006 policy re-evaluation: entries that were legal under the
    // policy version at write time may violate the current policy. Denied
    // tool calls are quarantined — preserved in the log, never dropped.
    if let Some(policy) = policy {
        for entry in &applied_entries {
            if entry.kind != fabric_types::context::EntryKind::ToolCall as i32 {
                continue;
            }
            let Ok(call) = crate::tool_call::decode(&entry.payload) else {
                continue;
            };
            if let Some(rule) = policy.tool_rules.iter().find(|r| {
                r.action == ToolAction::Deny as i32
                    && fabric_policy::eval::glob_matches(&r.tool_pattern, &call.tool_name)
            }) {
                local
                    .set_disposition(&entry.entry_id, DISPOSITION_QUARANTINE)
                    .await?;
                report.policy_violations.push(PolicyViolation {
                    entry_id: entry.entry_id.clone(),
                    rule: rule.tool_pattern.clone(),
                    action: ToolAction::Deny.as_str_name().to_string(),
                });
            }
        }
    }

    // Tier 1 structural detection over the merged region, restricted to
    // pairs involving at least one entry that arrived from the remote side
    // (a pre-existing collision within one replica is not a merge conflict).
    // Deterministic, model-free, no I/O beyond the local store read.
    if !applied_ids.is_empty() {
        let merged = local.entries_since(session_id, 0).await?;
        report.structural_conflicts = detect_in_region(&merged)
            .into_iter()
            .filter(|c| applied_ids.contains(&c.entry_id_a) || applied_ids.contains(&c.entry_id_b))
            .collect();
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ms_to_timestamp;
    use crate::db::tests::{test_entry, test_lease, test_session};
    use crate::db::SqliteContextStore;
    use fabric_types::context::{ContextEntry, EntryKind, ToolCall};

    fn replica(session_id: &str, lease_id: &str, holder: &str) -> SqliteContextStore {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session(session_id)).unwrap();
        store
            .grant_lease(&test_lease(lease_id, session_id, holder))
            .unwrap();
        store
    }

    fn entry_at(id: &str, session: &str, holder: &str, ms: i64) -> ContextEntry {
        let mut e = test_entry(id, session, holder);
        e.created_at = Some(ms_to_timestamp(ms));
        e
    }

    #[tokio::test]
    async fn clean_tail_merge_no_conflicts() {
        // Endpoint wrote seq 1..2, went offline; server continued 3..4 from
        // a handoff. Reconcile just fills the gap.
        let endpoint = replica("s1", "l1", "endpoint-1");
        for i in 1..=2 {
            let mut e = test_entry(&format!("e{i}"), "s1", "endpoint-1");
            endpoint.append_entry(&mut e).unwrap();
        }
        let server = replica("s1", "l2", "server-1");
        for e in endpoint.entries_since("s1", 0).unwrap() {
            server.insert_entry_raw(&e).unwrap();
        }
        for i in 3..=4 {
            let mut e = entry_at(&format!("h{i}"), "s1", "server-1", 1000 + i);
            e.seq = i as u64;
            server.insert_entry_raw(&e).unwrap();
        }

        let report = reconcile(&endpoint, &server, "s1", None).await.unwrap();
        assert_eq!(report.applied, 2);
        assert_eq!(report.duplicates, 2);
        assert!(report.conflicts.is_empty());
        assert_eq!(endpoint.head_seq("s1").unwrap(), 4);
    }

    #[tokio::test]
    async fn divergent_replicas_conflict_deterministically() {
        // Both replicas appended at seq 1 while partitioned.
        let a = replica("s1", "l1", "endpoint-1");
        let b = replica("s1", "l2", "server-1");

        let mut ea = entry_at("from-endpoint", "s1", "endpoint-1", 2000);
        ea.seq = 1;
        a.insert_entry_raw(&ea).unwrap();

        let mut eb = entry_at("from-server", "s1", "server-1", 1000);
        eb.seq = 1;
        b.insert_entry_raw(&eb).unwrap();

        // Earlier created_at wins the contested seq: from-server (t=1000)
        // keeps seq 1, from-endpoint (t=2000) is moved to the tail. Neither
        // entry has received_at, so created_at is the fallback ordering.
        let report_ab = reconcile(&a, &b, "s1", None).await.unwrap();
        assert_eq!(report_ab.applied, 1);
        assert_eq!(report_ab.conflicts.len(), 1);
        let c = &report_ab.conflicts[0];
        assert_eq!(c.kept_entry_id, "from-server");
        assert_eq!(c.moved_entry_id, "from-endpoint");
        assert_eq!(c.seq, 1);
        assert_eq!(c.moved_to_seq, 2);

        // Merging in the opposite direction converges to the same log. The
        // already-resolved remote log applies cleanly (no new conflict:
        // the moved entry arrives at its tail position).
        let report_ba = reconcile(&b, &a, "s1", None).await.unwrap();
        assert_eq!(report_ba.applied, 1);
        assert_eq!(report_ba.conflicts.len(), 0);

        let log_a: Vec<_> = a
            .entries_since("s1", 0)
            .unwrap()
            .into_iter()
            .map(|e| (e.seq, e.entry_id))
            .collect();
        let log_b: Vec<_> = b
            .entries_since("s1", 0)
            .unwrap()
            .into_iter()
            .map(|e| (e.seq, e.entry_id))
            .collect();
        assert_eq!(log_a, log_b);
        assert_eq!(log_a[0], (1, "from-server".to_string()));
        assert_eq!(log_a[1], (2, "from-endpoint".to_string()));
    }

    #[tokio::test]
    async fn reconcile_is_idempotent() {
        let a = replica("s1", "l1", "endpoint-1");
        let b = replica("s1", "l2", "server-1");

        let mut ea = entry_at("e1", "s1", "endpoint-1", 1000);
        ea.seq = 1;
        a.insert_entry_raw(&ea).unwrap();
        let mut eb = entry_at("h1", "s1", "server-1", 900);
        eb.seq = 1;
        b.insert_entry_raw(&eb).unwrap();

        let first = reconcile(&a, &b, "s1", None).await.unwrap();
        let second = reconcile(&a, &b, "s1", None).await.unwrap();
        assert_eq!(first.applied, 1);
        assert_eq!(second.applied, 0);
        assert_eq!(second.duplicates, 1);
        assert!(second.conflicts.is_empty());
    }

    fn tool_call(tool: &str, target: &str, value: &str, key: &str) -> ToolCall {
        ToolCall {
            tool_name: tool.into(),
            target: target.into(),
            params: std::collections::HashMap::from([("value".into(), value.into())]),
            idempotency_key: key.into(),
        }
    }

    fn tool_call_entry(id: &str, holder: &str, ms: i64, seq: u64, call: ToolCall) -> ContextEntry {
        let mut e = entry_at(id, "s1", holder, ms);
        e.kind = EntryKind::ToolCall as i32;
        e.payload = crate::tool_call::encode(&call);
        e.seq = seq;
        e
    }

    #[tokio::test]
    async fn reconcile_surfaces_structural_conflicts() {
        // Both replicas issued a state-mutating call on the same target with
        // divergent params while partitioned.
        let endpoint = replica("s1", "l1", "endpoint-1");
        let server = replica("s1", "l2", "server-1");

        let local_call = tool_call_entry(
            "ep-call",
            "endpoint-1",
            2000,
            1,
            tool_call("set_config", "ui.theme", "dark", ""),
        );
        endpoint.insert_entry_raw(&local_call).unwrap();
        let remote_call = tool_call_entry(
            "srv-call",
            "server-1",
            1000,
            1,
            tool_call("set_config", "ui.theme", "light", ""),
        );
        server.insert_entry_raw(&remote_call).unwrap();

        let report = reconcile(&endpoint, &server, "s1", None).await.unwrap();
        assert_eq!(report.applied, 1);
        // Seq-collision tracking is untouched and still populated.
        assert_eq!(report.conflicts.len(), 1);
        // The structural detector flags the param divergence for escalation.
        assert_eq!(report.structural_conflicts.len(), 1);
        let c = &report.structural_conflicts[0];
        assert_eq!(c.tool_name, "set_config");
        assert_eq!(c.target, "ui.theme");
        assert_eq!(
            c.disposition,
            crate::conflict::StructuralDisposition::Escalate
        );
        assert_eq!(
            (c.entry_id_a.as_str(), c.entry_id_b.as_str()),
            ("srv-call", "ep-call")
        );
        assert_eq!(c.to_verdict().confidence, 1.0);
    }

    #[tokio::test]
    async fn reconcile_lww_resolves_idempotent_collisions() {
        let endpoint = replica("s1", "l1", "endpoint-1");
        let server = replica("s1", "l2", "server-1");

        let local_call = tool_call_entry(
            "ep-call",
            "endpoint-1",
            2000,
            1,
            tool_call("cache_put", "k1", "1", "req-ep"),
        );
        endpoint.insert_entry_raw(&local_call).unwrap();
        let remote_call = tool_call_entry(
            "srv-call",
            "server-1",
            1000,
            1,
            tool_call("cache_put", "k1", "2", "req-srv"),
        );
        server.insert_entry_raw(&remote_call).unwrap();

        let report = reconcile(&endpoint, &server, "s1", None).await.unwrap();
        assert_eq!(report.structural_conflicts.len(), 1);
        let c = &report.structural_conflicts[0];
        assert_eq!(
            c.disposition,
            crate::conflict::StructuralDisposition::LastWriteWins
        );
        // Later (created_at, entry_id) writes last: ep-call (t=2000) wins.
        assert_eq!(c.lww_winner_entry_id.as_deref(), Some("ep-call"));
    }

    #[tokio::test]
    async fn reconcile_ignores_duplicate_tool_calls() {
        let endpoint = replica("s1", "l1", "endpoint-1");
        let server = replica("s1", "l2", "server-1");

        let local_call = tool_call_entry(
            "ep-call",
            "endpoint-1",
            2000,
            1,
            tool_call("get", "api/x", "1", ""),
        );
        endpoint.insert_entry_raw(&local_call).unwrap();
        // Same tool + same target + same params: a duplicate, not a conflict.
        let remote_call = tool_call_entry(
            "srv-call",
            "server-1",
            1000,
            1,
            tool_call("get", "api/x", "1", ""),
        );
        server.insert_entry_raw(&remote_call).unwrap();

        let report = reconcile(&endpoint, &server, "s1", None).await.unwrap();
        assert_eq!(report.applied, 1);
        assert!(report.structural_conflicts.is_empty());
    }

    #[tokio::test]
    async fn backdated_created_at_loses_to_received_at_ordering() {
        // ADR 006: created_at is an untrusted endpoint claim. A remote entry
        // with a backdated created_at would win under the old tiebreaker;
        // with server-stamped received_at it must lose.
        let a = replica("s1", "l1", "endpoint-1");
        let b = replica("s1", "l2", "server-1");

        let mut local = entry_at("local", "s1", "endpoint-1", 2000);
        local.seq = 1;
        local.received_at = Some(ms_to_timestamp(100));
        a.insert_entry_raw(&local).unwrap();

        let mut remote = entry_at("backdated", "s1", "server-1", 1);
        remote.seq = 1;
        remote.received_at = Some(ms_to_timestamp(200));
        b.insert_entry_raw(&remote).unwrap();

        // received_at ordering: local (100) keeps seq 1; the backdated
        // remote entry (created_at=1, received_at=200) moves to the tail.
        let report = reconcile(&a, &b, "s1", None).await.unwrap();
        assert_eq!(report.conflicts.len(), 1);
        let c = &report.conflicts[0];
        assert_eq!(c.kept_entry_id, "local");
        assert_eq!(c.moved_entry_id, "backdated");
        assert_eq!(c.moved_to_seq, 2);

        let log: Vec<_> = a
            .entries_since("s1", 0)
            .unwrap()
            .into_iter()
            .map(|e| (e.seq, e.entry_id))
            .collect();
        assert_eq!(
            log,
            vec![(1, "local".to_string()), (2, "backdated".to_string())]
        );
    }

    #[tokio::test]
    async fn received_at_falls_back_to_created_at_when_unset() {
        // Entries predating the received_at field order by created_at.
        let a = replica("s1", "l1", "endpoint-1");
        let b = replica("s1", "l2", "server-1");

        let mut local = entry_at("local", "s1", "endpoint-1", 2000);
        local.seq = 1;
        a.insert_entry_raw(&local).unwrap();
        let mut remote = entry_at("remote", "s1", "server-1", 1000);
        remote.seq = 1;
        b.insert_entry_raw(&remote).unwrap();

        let report = reconcile(&a, &b, "s1", None).await.unwrap();
        assert_eq!(report.conflicts[0].kept_entry_id, "remote");
        assert_eq!(report.conflicts[0].moved_entry_id, "local");
    }

    use fabric_types::policy::ToolRule;

    fn deny_policy(pattern: &str) -> fabric_types::policy::EndpointPolicy {
        fabric_types::policy::EndpointPolicy {
            policy_id: "p1".into(),
            version: "v1".into(),
            org_id: String::new(),
            data_rules: vec![],
            tool_rules: vec![ToolRule {
                tool_pattern: pattern.into(),
                action: ToolAction::Deny as i32,
                condition: String::new(),
            }],
            model_rules: vec![],
            cua: None,
            kill_switch: false,
            max_retention_hours: 0,
            dlp_patterns: vec![],
            safety: None,
        }
    }

    #[tokio::test]
    async fn policy_violating_replayed_entries_are_quarantined() {
        let endpoint = replica("s1", "l1", "endpoint-1");
        let server = replica("s1", "l2", "server-1");

        let mut local = entry_at("local-msg", "s1", "endpoint-1", 1000);
        local.seq = 1;
        endpoint.insert_entry_raw(&local).unwrap();

        // The remote replica executed shell.exec while partitioned; the
        // current policy denies it.
        let denied = tool_call_entry(
            "denied-call",
            "server-1",
            2000,
            2,
            tool_call("shell.exec", "/etc", "1", ""),
        );
        server.insert_entry_raw(&denied).unwrap();
        let allowed = tool_call_entry(
            "allowed-call",
            "server-1",
            3000,
            3,
            tool_call("fs.read", "/tmp/x", "1", ""),
        );
        server.insert_entry_raw(&allowed).unwrap();

        let policy = deny_policy("shell.*");
        let report = reconcile(&endpoint, &server, "s1", Some(&policy))
            .await
            .unwrap();
        assert_eq!(report.applied, 2);
        assert_eq!(report.policy_violations.len(), 1);
        let v = &report.policy_violations[0];
        assert_eq!(v.entry_id, "denied-call");
        assert_eq!(v.rule, "shell.*");
        assert_eq!(v.action, "TOOL_ACTION_DENY");

        // Quarantined entries are preserved in the log, never dropped.
        let stored = endpoint.entry_by_id("denied-call").unwrap().unwrap();
        assert_eq!(stored.disposition, DISPOSITION_QUARANTINE);
        let stored = endpoint.entry_by_id("allowed-call").unwrap().unwrap();
        assert_eq!(stored.disposition, "");
        assert_eq!(endpoint.head_seq("s1").unwrap(), 3);
    }

    #[tokio::test]
    async fn reconcile_without_policy_never_quarantines() {
        // None policy: identical merge behavior, no dispositions stamped.
        let endpoint = replica("s1", "l1", "endpoint-1");
        let server = replica("s1", "l2", "server-1");

        let denied = tool_call_entry(
            "denied-call",
            "server-1",
            1000,
            1,
            tool_call("shell.exec", "/etc", "1", ""),
        );
        server.insert_entry_raw(&denied).unwrap();

        let report = reconcile(&endpoint, &server, "s1", None).await.unwrap();
        assert_eq!(report.applied, 1);
        assert!(report.policy_violations.is_empty());
        let stored = endpoint.entry_by_id("denied-call").unwrap().unwrap();
        assert_eq!(stored.disposition, "");
    }

    #[tokio::test]
    async fn reconcile_with_policy_is_idempotent() {
        let endpoint = replica("s1", "l1", "endpoint-1");
        let server = replica("s1", "l2", "server-1");

        let denied = tool_call_entry(
            "denied-call",
            "server-1",
            1000,
            1,
            tool_call("shell.exec", "/etc", "1", ""),
        );
        server.insert_entry_raw(&denied).unwrap();

        let policy = deny_policy("shell.*");
        let first = reconcile(&endpoint, &server, "s1", Some(&policy))
            .await
            .unwrap();
        assert_eq!(first.applied, 1);
        assert_eq!(first.policy_violations.len(), 1);

        // Second run: the entry is a duplicate; no re-quarantine, and the
        // disposition stamped by the first run survives.
        let second = reconcile(&endpoint, &server, "s1", Some(&policy))
            .await
            .unwrap();
        assert_eq!(second.applied, 0);
        assert_eq!(second.duplicates, 1);
        assert!(second.policy_violations.is_empty());
        let stored = endpoint.entry_by_id("denied-call").unwrap().unwrap();
        assert_eq!(stored.disposition, DISPOSITION_QUARANTINE);
    }
}
