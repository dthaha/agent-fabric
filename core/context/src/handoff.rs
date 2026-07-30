//! Lease handoff protocol. Handoff = transfer the write lease + catch-up,
//! never summarize-and-restart. The old holder freezes at a sequence, a
//! HANDOFF_MARKER is appended, the lease is transferred, and the new holder
//! acks once it has replayed the log up to the freeze point.

use tracing::instrument;
use uuid::Uuid;

use crate::db::{now_ms, timestamp_to_ms, Result, StoreError};
use crate::store::{ContextStore, LeaseAuthority};
use fabric_types::context::{ContextEntry, EntryKind, Locus, SessionState};
use fabric_types::lease::{HandoffAck, HandoffRequest, Lease};

/// Safety net for crashed holders. Primary release is explicit via
/// `release_lease` / `release_with_rollback`. The harness calls release at
/// end of every agent turn. Agent turns with tool calls routinely run
/// 2-5+ minutes, so the TTL is deliberately generous: it only fires when a
/// holder crashes without releasing.
pub const DEFAULT_LEASE_TTL_MS: i64 = 3_600_000;

/// Maximum TTL a caller may request (1 hour). Unbounded TTLs would let a
/// crashed holder lock a session forever. Currently equal to the default:
/// the turn-scoped safety net is the only posture the fabric supports.
pub const MAX_LEASE_TTL_MS: i64 = 3_600_000;

/// How long a HANDED_OFF session waits for [`ack_handoff`] before it may be
/// recovered via [`abort_handoff`]. Without an abort path, a new holder that
/// crashes after the transfer but before acking would wedge the session in
/// HANDED_OFF forever — appends are rejected in that state.
pub const DEFAULT_HANDOFF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

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
    store: &(impl ContextStore + LeaseAuthority),
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
        received_at: None,
        disposition: String::new(),
    };
    store.append_entry(&mut marker).await?;

    // 2. Transfer the lease atomically: the old holder's lease is released
    //    and the new holder's granted (pinned to the freeze point) in ONE
    //    transaction, so a failure here never leaves the session writerless.
    let new_lease = store
        .transfer_lease(
            &request.session_id,
            &request.from_holder,
            &request.to_holder,
            to_locus,
            ttl_ms,
            freeze_seq,
        )
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

/// Abort a handoff whose ack never arrived. Transitions the session from
/// HANDED_OFF back to ACTIVE so appends are accepted again (the new holder's
/// lease — granted by the transfer — is still ACTIVE and becomes usable, and
/// preemption remains available as the escape hatch).
///
/// The abort is only honored once `timeout` has elapsed since the session
/// entered HANDED_OFF (stamped into `last_activity` by the state change):
/// aborting an in-flight handoff that is merely slow would race a legitimate
/// ack. Fails with [`StoreError::InvalidTransition`] when the session is not
/// HANDED_OFF or the timeout has not yet elapsed.
pub async fn abort_handoff(
    store: &impl ContextStore,
    session_id: &str,
    timeout: std::time::Duration,
) -> Result<()> {
    let session = store.session(session_id).await?;
    if session.state != SessionState::HandedOff as i32 {
        return Err(StoreError::InvalidTransition(format!(
            "session {session_id} is not HANDED_OFF (state = {}); abort rejected",
            crate::db::session_state_name(session.state),
        )));
    }
    let elapsed_ms = now_ms() - timestamp_to_ms(session.last_activity.as_ref());
    let timeout_ms = i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX);
    if elapsed_ms < timeout_ms {
        return Err(StoreError::InvalidTransition(format!(
            "handoff for session {session_id} is still within the ack timeout \
             ({elapsed_ms}ms < {timeout_ms}ms); abort rejected"
        )));
    }
    store
        .set_session_state(session_id, SessionState::Active as i32)
        .await?;
    Ok(())
}

/// Catch-up failure. Divergence is NOT a store error: it means the two
/// replicas wrote different entries at the same seq while partitioned and
/// the caller must run [`crate::reconcile`] instead of raw catch-up.
#[derive(Debug, thiserror::Error)]
pub enum CatchUpError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("divergent entry at seq {seq}: target has {local_id}, source has {remote_id}")]
    Divergent {
        seq: u64,
        local_id: String,
        remote_id: String,
    },
}

/// Replay entries the new holder missed while offline into its local store.
/// Returns the sequence the target is now caught up to.
///
/// Every source entry is checked, not just those above the target's head:
/// if the target appended its own entries while partitioned, divergence can
/// sit BELOW the head. Stops with [`CatchUpError::Divergent`] when the
/// target holds a DIFFERENT entry at a seq the source occupies — a raw
/// insert would fail on the (session_id, seq) primary key, and silently
/// skipping it would fork the log. The caller decides: run reconcile to
/// merge deterministically.
pub async fn catch_up(
    source: &impl ContextStore,
    target: &impl ContextStore,
    session_id: &str,
) -> std::result::Result<u64, CatchUpError> {
    let source_entries = source.entries_since(session_id, 0).await?;
    for entry in &source_entries {
        if target.entry_by_id(&entry.entry_id).await?.is_some() {
            continue;
        }
        if let Some(existing) = target.entry_at_seq(session_id, entry.seq).await? {
            if existing.entry_id != entry.entry_id {
                return Err(CatchUpError::Divergent {
                    seq: entry.seq,
                    local_id: existing.entry_id,
                    remote_id: entry.entry_id.clone(),
                });
            }
        }
        target.insert_entry_raw(entry).await?;
    }
    Ok(source.head_seq(session_id).await?)
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

    #[tokio::test]
    async fn catch_up_stops_on_divergence() {
        let (source, _old) = setup();
        for i in 1..=4 {
            let mut e = test_entry(&format!("e{i}"), "s1", "endpoint-1");
            source.append_entry(&mut e).unwrap();
        }

        // Target has the first two entries, then wrote its OWN entry at
        // seq 3 while partitioned — the replicas have diverged.
        let target = SqliteContextStore::open_in_memory().unwrap();
        target.create_session(&test_session("s1")).unwrap();
        for e in source.entries_since("s1", 0).unwrap().into_iter().take(2) {
            target.insert_entry_raw(&e).unwrap();
        }
        let mut rogue = test_entry("local-3", "s1", "endpoint-1");
        rogue.seq = 3;
        target.insert_entry_raw(&rogue).unwrap();

        // Catch-up refuses to paper over the fork: the caller must run
        // reconcile instead of getting a raw sqlite PK violation.
        let err = catch_up(&source, &target, "s1").await.unwrap_err();
        match err {
            CatchUpError::Divergent {
                seq,
                local_id,
                remote_id,
            } => {
                assert_eq!(seq, 3);
                assert_eq!(local_id, "local-3");
                assert_eq!(remote_id, "e3");
            }
            other => panic!("expected divergence, got {other}"),
        }
        // Nothing at or past the divergence was inserted.
        assert!(target.entry_by_id("e3").unwrap().is_none());
        assert!(target.entry_by_id("e4").unwrap().is_none());
        assert_eq!(target.head_seq("s1").unwrap(), 3);
    }

    #[tokio::test]
    async fn failed_handoff_leaves_session_writer_intact() {
        let (store, old) = setup();
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        store.append_entry(&mut e1).unwrap();

        // A freeze point beyond the head fails BEFORE any mutation: no
        // marker, no transfer, session still ACTIVE with the old writer.
        let req = HandoffRequest {
            session_id: "s1".into(),
            from_holder: "endpoint-1".into(),
            to_holder: "server-1".into(),
            freeze_at_seq: 5,
            reason: String::new(),
        };
        assert!(matches!(
            execute_handoff(&store, &req, Locus::Server, DEFAULT_LEASE_TTL_MS).await,
            Err(StoreError::InvalidTransition(_))
        ));
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Active as i32
        );
        assert_eq!(
            store.active_lease("s1").unwrap().unwrap().lease_id,
            old.lease_id
        );
        assert_eq!(store.head_seq("s1").unwrap(), 1, "no marker appended");
        let mut e2 = test_entry("e2", "s1", "endpoint-1");
        assert_eq!(store.append_entry(&mut e2).unwrap(), 2);
    }

    #[tokio::test]
    async fn abort_handoff_recovers_unacked_session_after_timeout() {
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

        // The new holder crashes before acking. Aborting INSIDE the ack
        // timeout is rejected — a slow-but-alive ack must not race an abort.
        assert!(matches!(
            abort_handoff(&store, "s1", DEFAULT_HANDOFF_TIMEOUT).await,
            Err(StoreError::InvalidTransition(_))
        ));
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::HandedOff as i32
        );

        // Once the timeout has elapsed (zero here), abort recovers the
        // session: HANDED_OFF -> ACTIVE. The transferred lease is still
        // ACTIVE, so the new holder can write as soon as it is back.
        abort_handoff(&store, "s1", std::time::Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Active as i32
        );
        assert_eq!(
            store.active_lease("s1").unwrap().unwrap().lease_id,
            new_lease.lease_id
        );
        let mut e = test_entry("e5", "s1", "server-1");
        assert_eq!(store.append_entry(&mut e).unwrap(), 3);
    }

    #[tokio::test]
    async fn abort_handoff_rejects_non_handed_off_session() {
        let (store, _old) = setup();
        // ACTIVE session: there is nothing to abort.
        assert!(matches!(
            abort_handoff(&store, "s1", std::time::Duration::ZERO).await,
            Err(StoreError::InvalidTransition(_))
        ));
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Active as i32
        );
    }
}
