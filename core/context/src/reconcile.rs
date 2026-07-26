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

use crate::db::{timestamp_to_ms, Result};
use crate::store::ContextStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeqConflict {
    pub seq: u64,
    pub kept_entry_id: String,
    pub moved_entry_id: String,
    pub moved_to_seq: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub session_id: String,
    pub applied: u64,
    pub duplicates: u64,
    pub conflicts: Vec<SeqConflict>,
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
    for remote_entry in remote_entries {
        if local.entry_by_id(&remote_entry.entry_id).await?.is_some() {
            report.duplicates += 1;
            continue;
        }

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

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ms_to_timestamp;
    use crate::db::tests::{test_entry, test_lease, test_session};
    use crate::db::SqliteContextStore;
    use fabric_types::context::ContextEntry;

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
}
