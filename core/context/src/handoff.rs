//! Lease handoff protocol. Handoff = transfer the write lease + catch-up,
//! never summarize-and-restart. The old holder freezes at a sequence, a
//! HANDOFF_MARKER is appended, the lease is transferred, and the new holder
//! acks once it has replayed the log up to the freeze point.

use tracing::instrument;
use uuid::Uuid;

use crate::db::{Result, StoreError};
use crate::store::ContextStore;
use fabric_types::context::{ContextEntry, EntryKind, Locus, SessionState};
use fabric_types::lease::{HandoffAck, HandoffRequest, Lease};

/// Safety net for crashed holders. Primary release is explicit via
/// `release_lease` / `release_with_rollback`. The harness calls release at
/// end of every agent turn. Agent turns with tool calls routinely run
/// 2-5+ minutes, so the TTL is deliberately generous: it only fires when a
/// holder crashes without releasing.
pub const DEFAULT_LEASE_TTL_MS: i64 = 3_600_000;

/// Execute a handoff from the current lease holder to a new holder.
///
/// Steps (atomic in effect; each is idempotent by id):
/// 1. Verify the requester is the current writer.
/// 2. Append a HANDOFF_MARKER at the freeze sequence.
/// 3. Release the old lease (the old holder's turn is over).
/// 4. The new holder acquires a fresh lease with granted_seq = freeze point.
/// 5. Mark the session HANDED_OFF until the ack arrives.
///
/// Returns the new lease. The new holder then calls [`ack_handoff`] after
/// catching up.
#[instrument(skip(store, request), fields(session = %request.session_id))]
pub async fn execute_handoff(
    store: &impl ContextStore,
    request: &HandoffRequest,
    to_locus: Locus,
    ttl_ms: i64,
) -> Result<Lease> {
    let old_lease = store
        .verify_writer(&request.session_id, &request.from_holder)
        .await?;

    let head = store.head_seq(&request.session_id).await?;
    let freeze_seq = if request.freeze_at_seq == 0 {
        head
    } else {
        request.freeze_at_seq
    };
    if freeze_seq > head {
        return Err(StoreError::InvalidTransition(format!(
            "freeze_at_seq {freeze_seq} is beyond head {head}"
        )));
    }

    // 1. The old holder records the handoff in the log itself. The marker is
    //    the last entry the old holder will ever write for this session.
    let mut marker = ContextEntry {
        entry_id: format!("handoff-{}", Uuid::now_v7()),
        session_id: request.session_id.clone(),
        seq: 0,
        kind: EntryKind::HandoffMarker as i32,
        payload: request.reason.clone().into_bytes(),
        lease_holder: request.from_holder.clone(),
        policy_version: String::new(),
        locus: old_lease.locus,
        created_at: None,
    };
    store.append_entry(&mut marker).await?;

    // 2. Transfer the lease: release the old holder's turn-scoped lease,
    //    then the new holder acquires fresh, pinned to the freeze point.
    store
        .release_lease(&request.session_id, &request.from_holder)
        .await?;

    let mut new_lease = store
        .acquire_lease(&request.session_id, &request.to_holder, to_locus, ttl_ms)
        .await?;
    new_lease.granted_seq = freeze_seq;
    store
        .set_granted_seq(&new_lease.lease_id, freeze_seq)
        .await?;
    store
        .set_session_state(&request.session_id, SessionState::HandedOff as i32)
        .await?;

    Ok(new_lease)
}

/// Acknowledge a handoff. The new holder reports the sequence it has caught
/// up to; on success the session returns to ACTIVE and the new holder may
/// begin writing. The ack must target the new lease's own session, and the
/// session must still be in the HANDED_OFF state — an ack for any other
/// session, or a duplicate/late ack after reactivation, is rejected.
pub async fn ack_handoff(
    store: &impl ContextStore,
    new_lease: &Lease,
    ack: &HandoffAck,
) -> Result<()> {
    if ack.session_id != new_lease.session_id {
        return Err(StoreError::InvalidTransition(format!(
            "ack session '{}' does not match lease session '{}'",
            ack.session_id, new_lease.session_id
        )));
    }
    if ack.new_holder != new_lease.holder_id {
        return Err(StoreError::NotLeaseHolder {
            writer: ack.new_holder.clone(),
            holder: new_lease.holder_id.clone(),
        });
    }
    if !ack.success {
        return Err(StoreError::InvalidTransition(format!(
            "handoff rejected by {}: {}",
            ack.new_holder, ack.error
        )));
    }
    if ack.caught_up_to_seq < new_lease.granted_seq {
        return Err(StoreError::InvalidTransition(format!(
            "new holder caught up to {} but freeze was at {}",
            ack.caught_up_to_seq, new_lease.granted_seq
        )));
    }
    let session = store.session(&ack.session_id).await?;
    if session.state != SessionState::HandedOff as i32 {
        return Err(StoreError::InvalidTransition(format!(
            "session {} is not HANDED_OFF (state = {}); ack rejected",
            ack.session_id,
            crate::db::session_state_name(session.state),
        )));
    }
    store
        .set_session_state(&ack.session_id, SessionState::Active as i32)
        .await?;
    Ok(())
}

/// Replay entries the new holder missed while offline into its local store.
/// Returns the sequence the target is now caught up to.
pub async fn catch_up(
    source: &impl ContextStore,
    target: &impl ContextStore,
    session_id: &str,
) -> Result<u64> {
    let target_head = target.head_seq(session_id).await?;
    let missing = source.entries_since(session_id, target_head).await?;
    for entry in &missing {
        if target.entry_by_id(&entry.entry_id).await?.is_none() {
            target.insert_entry_raw(entry).await?;
        }
    }
    source.head_seq(session_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{test_entry, test_lease, test_session};
    use crate::db::SqliteContextStore;
    use fabric_types::lease::LeaseState;

    fn setup() -> (SqliteContextStore, Lease) {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let lease = test_lease("l1", "s1", "endpoint-1");
        store.grant_lease(&lease).unwrap();
        (store, lease)
    }

    #[tokio::test]
    async fn handoff_transfers_write_lease() {
        let (store, old) = setup();
        for i in 1..=3 {
            let mut e = test_entry(&format!("e{i}"), "s1", "endpoint-1");
            store.append_entry(&mut e).unwrap();
        }

        let req = HandoffRequest {
            session_id: "s1".into(),
            from_holder: "endpoint-1".into(),
            to_holder: "server-1".into(),
            freeze_at_seq: 3,
            reason: "long-horizon task".into(),
        };
        let new_lease = execute_handoff(&store, &req, Locus::Server, DEFAULT_LEASE_TTL_MS)
            .await
            .unwrap();

        // Old lease released, new lease active from the freeze point.
        assert_eq!(
            store.lease(&old.lease_id).unwrap().state,
            LeaseState::Released as i32
        );
        assert_eq!(new_lease.holder_id, "server-1");
        assert_eq!(new_lease.granted_seq, 3);

        // The handoff marker is the last entry from the old holder.
        let head = store.entries_since("s1", 0).unwrap();
        assert_eq!(head.len(), 4);
        assert_eq!(head[3].kind, EntryKind::HandoffMarker as i32);
        assert_eq!(head[3].lease_holder, "endpoint-1");

        // Old holder can no longer write (session is HANDED_OFF until ack).
        let mut stale = test_entry("stale", "s1", "endpoint-1");
        assert!(matches!(
            store.append_entry(&mut stale),
            Err(StoreError::SessionNotActive { .. })
        ));

        // Ack from the new holder reactivates the session.
        let ack = HandoffAck {
            session_id: "s1".into(),
            new_holder: "server-1".into(),
            caught_up_to_seq: 4,
            success: true,
            error: String::new(),
        };
        ack_handoff(&store, &new_lease, &ack).await.unwrap();
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Active as i32
        );

        // New holder writes at seq 5 — continuity preserved, no restart.
        let mut e5 = test_entry("e5", "s1", "server-1");
        assert_eq!(store.append_entry(&mut e5).unwrap(), 5);
    }

    #[tokio::test]
    async fn handoff_rejects_non_holder_initiator() {
        let (store, _old) = setup();
        let req = HandoffRequest {
            session_id: "s1".into(),
            from_holder: "mallory".into(),
            to_holder: "server-1".into(),
            freeze_at_seq: 0,
            reason: "hostile takeover".into(),
        };
        assert!(matches!(
            execute_handoff(&store, &req, Locus::Server, DEFAULT_LEASE_TTL_MS).await,
            Err(StoreError::NotLeaseHolder { .. })
        ));
    }

    #[tokio::test]
    async fn ack_rejects_insufficient_catch_up() {
        let (store, _old) = setup();
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        store.append_entry(&mut e1).unwrap();

        let req = HandoffRequest {
            session_id: "s1".into(),
            from_holder: "endpoint-1".into(),
            to_holder: "server-1".into(),
            freeze_at_seq: 1,
            reason: String::new(),
        };
        let new_lease = execute_handoff(&store, &req, Locus::Server, DEFAULT_LEASE_TTL_MS)
            .await
            .unwrap();

        let ack = HandoffAck {
            session_id: "s1".into(),
            new_holder: "server-1".into(),
            caught_up_to_seq: 0,
            success: true,
            error: String::new(),
        };
        assert!(matches!(
            ack_handoff(&store, &new_lease, &ack).await,
            Err(StoreError::InvalidTransition(_))
        ));
    }

    #[tokio::test]
    async fn ack_rejects_mismatched_session_id() {
        let (store, _old) = setup();
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        store.append_entry(&mut e1).unwrap();

        let req = HandoffRequest {
            session_id: "s1".into(),
            from_holder: "endpoint-1".into(),
            to_holder: "server-1".into(),
            freeze_at_seq: 1,
            reason: String::new(),
        };
        let new_lease = execute_handoff(&store, &req, Locus::Server, DEFAULT_LEASE_TTL_MS)
            .await
            .unwrap();

        // An ack naming a different session must not reactivate s1 — and
        // must not touch the other session at all.
        store.create_session(&test_session("s2")).unwrap();
        let ack = HandoffAck {
            session_id: "s2".into(),
            new_holder: "server-1".into(),
            caught_up_to_seq: 1,
            success: true,
            error: String::new(),
        };
        assert!(matches!(
            ack_handoff(&store, &new_lease, &ack).await,
            Err(StoreError::InvalidTransition(_))
        ));
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::HandedOff as i32
        );
        assert_eq!(
            store.session("s2").unwrap().state,
            SessionState::Active as i32
        );
    }

    #[tokio::test]
    async fn ack_rejected_when_session_not_handed_off() {
        let (store, _old) = setup();
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        store.append_entry(&mut e1).unwrap();

        let req = HandoffRequest {
            session_id: "s1".into(),
            from_holder: "endpoint-1".into(),
            to_holder: "server-1".into(),
            freeze_at_seq: 1,
            reason: String::new(),
        };
        let new_lease = execute_handoff(&store, &req, Locus::Server, DEFAULT_LEASE_TTL_MS)
            .await
            .unwrap();

        let ack = HandoffAck {
            session_id: "s1".into(),
            new_holder: "server-1".into(),
            caught_up_to_seq: 2,
            success: true,
            error: String::new(),
        };
        ack_handoff(&store, &new_lease, &ack).await.unwrap();
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Active as i32
        );

        // A duplicate/late ack after reactivation is rejected: the session
        // is no longer HANDED_OFF.
        assert!(matches!(
            ack_handoff(&store, &new_lease, &ack).await,
            Err(StoreError::InvalidTransition(_))
        ));
    }

    #[tokio::test]
    async fn catch_up_replays_missing_entries() {
        let (source, _old) = setup();
        for i in 1..=4 {
            let mut e = test_entry(&format!("e{i}"), "s1", "endpoint-1");
            source.append_entry(&mut e).unwrap();
        }

        // Target replica has only the first two entries.
        let target = SqliteContextStore::open_in_memory().unwrap();
        target.create_session(&test_session("s1")).unwrap();
        for e in source.entries_since("s1", 0).unwrap().into_iter().take(2) {
            target.insert_entry_raw(&e).unwrap();
        }

        let caught = catch_up(&source, &target, "s1").await.unwrap();
        assert_eq!(caught, 4);
        assert_eq!(target.head_seq("s1").unwrap(), 4);
        // Re-running is a no-op (idempotent).
        assert_eq!(catch_up(&source, &target, "s1").await.unwrap(), 4);
    }
}
