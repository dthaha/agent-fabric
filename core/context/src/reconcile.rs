//! Offline reconcile. After an offline stretch, two replicas of the same
//! session op-log may both have appended entries. Reconcile merges a remote
//! replica into the local store deterministically:
//!
//! - Entries already known (by entry_id) are skipped.
//! - Remote entries whose seq is free locally are inserted in place.
//! - Seq collisions are resolved deterministically: the entry with the
//!   earlier (created_at, entry_id) keeps the contested seq; the loser is
//!   re-appended at the tail with a fresh seq. Both replicas converge to the
//!   same final log regardless of merge direction.

use serde::{Deserialize, Serialize};

use crate::conflict::{detect_in_region, StructuralConflict};
use crate::db::{timestamp_to_ms, Result};
use crate::store::ContextStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeqConflict {
    pub seq: u64,
    pub kept_entry_id: String,
    pub moved_entry_id: String,
    pub moved_to_seq: u64,
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
}

/// Merge all remote entries for `session_id` into the local store.
pub async fn reconcile(
    local: &impl ContextStore,
    remote: &impl ContextStore,
    session_id: &str,
) -> Result<ReconcileReport> {
    let mut report = ReconcileReport {
        session_id: session_id.to_string(),
        ..Default::default()
    };

    let remote_entries = remote.entries_since(session_id, 0).await?;
    let mut applied_ids = std::collections::HashSet::new();
    for remote_entry in remote_entries {
        if local.entry_by_id(&remote_entry.entry_id).await?.is_some() {
            report.duplicates += 1;
            continue;
        }
        applied_ids.insert(remote_entry.entry_id.clone());

        match local.entry_at_seq(session_id, remote_entry.seq).await? {
            None => {
                local.insert_entry_raw(&remote_entry).await?;
                report.applied += 1;
            }
            Some(local_entry) => {
                // Deterministic winner: earlier (created_at, entry_id) keeps
                // the contested seq.
                let remote_ms = timestamp_to_ms(remote_entry.created_at.as_ref());
                let local_ms = timestamp_to_ms(local_entry.created_at.as_ref());
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
                } else {
                    let mut moved = remote_entry.clone();
                    moved.seq = tail;
                    local.insert_entry_raw(&moved).await?;
                    report.conflicts.push(SeqConflict {
                        seq: remote_entry.seq,
                        kept_entry_id: local_entry.entry_id,
                        moved_entry_id: remote_entry.entry_id,
                        moved_to_seq: tail,
                    });
                }
                report.applied += 1;
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

        let report = reconcile(&endpoint, &server, "s1").await.unwrap();
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
        // keeps seq 1, from-endpoint (t=2000) is moved to the tail.
        let report_ab = reconcile(&a, &b, "s1").await.unwrap();
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
        let report_ba = reconcile(&b, &a, "s1").await.unwrap();
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

        let first = reconcile(&a, &b, "s1").await.unwrap();
        let second = reconcile(&a, &b, "s1").await.unwrap();
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

        let report = reconcile(&endpoint, &server, "s1").await.unwrap();
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

        let report = reconcile(&endpoint, &server, "s1").await.unwrap();
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

        let report = reconcile(&endpoint, &server, "s1").await.unwrap();
        assert_eq!(report.applied, 1);
        assert!(report.structural_conflicts.is_empty());
    }
}
