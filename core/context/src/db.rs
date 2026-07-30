//! SQLite-backed storage for the context plane: sessions, the append-only
//! context op-log, and write leases. Lease enforcement happens here — an
//! entry can only be appended by the holder of the session's ACTIVE lease.

use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;
use tracing::instrument;

use fabric_types::context::{ContextEntry, EntryKind, Locus, SessionMeta, SessionState};
use fabric_types::lease::{Lease, LeaseState};

pub use crate::clock::now_ms;
use crate::clock::MonotonicClock;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("lease not found: {0}")]
    LeaseNotFound(String),
    #[error("no active lease for session: {0}")]
    NoActiveLease(String),
    #[error("session {session_id} is not ACTIVE (state = {state}); appends rejected")]
    SessionNotActive { session_id: String, state: String },
    #[error("an active lease already exists for session: {0}")]
    LeaseConflict(String),
    #[error("writer '{writer}' does not hold the active lease (held by '{holder}')")]
    NotLeaseHolder { writer: String, holder: String },
    #[error("lease {0} is not active")]
    LeaseNotActive(String),
    #[error("lease {0} has expired")]
    LeaseExpired(String),
    #[error("invalid lease state transition: {0}")]
    InvalidTransition(String),
    #[error("sequence conflict at seq {seq}: local entry {local} vs remote entry {remote}")]
    SeqConflict {
        seq: u64,
        local: String,
        remote: String,
    },
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("blocking store task failed to join: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Outcome of [`SqliteContextStore::release_with_rollback`]: the op-log was
/// truncated back to the last completed flow boundary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RollbackReport {
    /// Seq of the last ASSISTANT_MESSAGE entry (the completed flow
    /// boundary). 0 if the session had no assistant messages yet.
    pub rolled_back_to_seq: u64,
    /// Number of partial-turn entries removed.
    pub entries_removed: u64,
}

pub fn ms_to_timestamp(ms: i64) -> pbjson_types::Timestamp {
    pbjson_types::Timestamp {
        seconds: ms.div_euclid(1000),
        nanos: (ms.rem_euclid(1000) * 1_000_000) as i32,
    }
}

pub(crate) fn timestamp_to_ms(ts: Option<&pbjson_types::Timestamp>) -> i64 {
    ts.map(|t| {
        t.seconds
            .saturating_mul(1000)
            .saturating_add(i64::from(t.nanos) / 1_000_000)
    })
    .unwrap_or(0)
}

/// A lease is expired AT its deadline, not after it. `expires_at <= 0` means
/// no deadline (never expires).
pub(crate) fn is_expired(expires_at: i64, now: i64) -> bool {
    expires_at > 0 && now >= expires_at
}

/// Parameters for [`SqliteContextStore::preempt_lease`]. Grouped because
/// preemption touches both leases and the op-log at once; the fields mirror
/// the audit trail (who took over, from which surface, and why).
#[derive(Debug, Clone)]
pub struct Preemption {
    pub session_id: String,
    /// The lease being taken over. Must be the session's ACTIVE lease.
    pub old_lease_id: String,
    /// Device the fresh lease is granted to.
    pub new_holder_id: String,
    /// Surface recorded in `preempted_by` on the revoked lease (audit).
    pub new_surface_id: String,
    pub locus: Locus,
    pub ttl_ms: i64,
    /// Recorded in the revocation SYSTEM_EVENT.
    pub reason: String,
}

/// The SQLite-backed context store. Wraps a single SQLite connection behind
/// a mutex so the store is `Send + Sync` and cheap to clone into blocking
/// tasks (the async [`crate::store::ContextStore`] impl runs calls via
/// `spawn_blocking`). Mutating operations that must be atomic
/// ([`SqliteContextStore::acquire_lease`], [`SqliteContextStore::append_entry`],
/// [`SqliteContextStore::preempt_lease`], [`SqliteContextStore::transfer_lease`])
/// hold the connection lock for their whole duration and run inside a single
/// SQLite transaction, so a crash or contention mid-operation can never leave
/// a session half-written (dual writers, writerless sessions, or seq gaps).
/// The `idx_one_active_lease` partial unique index is defense-in-depth: the
/// database itself rejects a second ACTIVE lease for a session.
#[derive(Clone)]
pub struct SqliteContextStore {
    conn: Arc<Mutex<Connection>>,
    clock: Arc<MonotonicClock>,
}

impl SqliteContextStore {
    /// Open (or create) a store at `path` and run migrations.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Open an in-memory store. Used by tests and the endpoint cache.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            clock: Arc::new(MonotonicClock::new()),
        })
    }

    /// Lock the underlying connection. Never hold the guard across a call
    /// to another store method: every method takes the lock itself.
    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A poisoned mutex means a panicking writer mid-transaction; the
        // connection itself is still usable. Recover instead of cascading
        // the panic into a daemon crash-loop.
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Flush the WAL and close the store. Called on daemon shutdown so no
    /// entries linger in the -wal sidecar file. Dropping the store without
    /// `close` is also safe (SQLite checkpoints on last close), but this
    /// makes the flush explicit and surfaces errors.
    pub fn close(self) -> Result<()> {
        self.conn()
            .pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    }

    // ---- sessions ----

    /// Lightweight liveness check for health/readiness probes.
    pub fn ping(&self) -> Result<()> {
        self.conn().query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    }

    /// Number of sessions in the ACTIVE state.
    pub fn active_session_count(&self) -> Result<u64> {
        let n: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM sessions WHERE state = ?1",
            params![SessionState::Active as i32],
            |row| row.get(0),
        )?;
        Ok(n as u64)
    }

    /// All sessions in the ACTIVE state, oldest first. Powers the daemon's
    /// admin endpoints.
    pub fn list_active_sessions(&self) -> Result<Vec<SessionMeta>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, soul_id, user_id, state, active_lease, created_at_ms, last_activity_ms, labels, org_id
             FROM sessions WHERE state = ?1 ORDER BY created_at_ms ASC",
        )?;
        let rows = stmt.query_map(params![SessionState::Active as i32], |row| {
            let labels_json: String = row.get(7)?;
            Ok(SessionMeta {
                session_id: row.get(0)?,
                soul_id: row.get(1)?,
                user_id: row.get(2)?,
                state: row.get(3)?,
                active_lease: row.get(4)?,
                created_at: Some(ms_to_timestamp(row.get(5)?)),
                last_activity: Some(ms_to_timestamp(row.get(6)?)),
                labels: serde_json::from_str(&labels_json).unwrap_or_default(),
                org_id: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Create a session. Idempotent: re-creating an existing session is a
    /// no-op so offline replicas can converge.
    pub fn create_session(&self, meta: &SessionMeta) -> Result<()> {
        let labels = serde_json::to_string(&meta.labels)?;
        self.conn().execute(
            "INSERT OR IGNORE INTO sessions
             (session_id, soul_id, user_id, state, active_lease, created_at_ms, last_activity_ms, labels, org_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                meta.session_id,
                meta.soul_id,
                meta.user_id,
                meta.state,
                meta.active_lease,
                timestamp_to_ms(meta.created_at.as_ref()),
                timestamp_to_ms(meta.last_activity.as_ref()),
                labels,
                meta.org_id,
            ],
        )?;
        Ok(())
    }

    pub fn session(&self, session_id: &str) -> Result<SessionMeta> {
        session_conn(&self.conn(), session_id)
    }

    /// Raw state setter. Crate-internal: callers outside this module must
    /// use the validated transitions ([`SqliteContextStore::suspend`],
    /// [`SqliteContextStore::resume`], [`SqliteContextStore::complete`],
    /// [`SqliteContextStore::archive`]). Handoff uses this to set HANDED_OFF and
    /// to return to ACTIVE on ack.
    pub(crate) fn set_session_state(&self, session_id: &str, state: i32) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE sessions SET state = ?1, last_activity_ms = ?2 WHERE session_id = ?3",
            params![state, now_ms(), session_id],
        )?;
        if n == 0 {
            return Err(StoreError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    /// Validated lifecycle transition. Fails with
    /// [`StoreError::InvalidTransition`] unless the session is in `from`.
    fn transition(&self, session_id: &str, from: SessionState, to: SessionState) -> Result<()> {
        let current = self.session(session_id)?.state;
        if current != from as i32 {
            return Err(StoreError::InvalidTransition(format!(
                "cannot transition session {session_id} {} -> {}: current state is {}",
                from.as_str_name(),
                to.as_str_name(),
                session_state_name(current),
            )));
        }
        self.set_session_state(session_id, to as i32)
    }

    /// Suspend an ACTIVE session (e.g. user locked the device). Appends are
    /// rejected while SUSPENDED.
    pub fn suspend(&self, session_id: &str) -> Result<()> {
        self.transition(session_id, SessionState::Active, SessionState::Suspended)
    }

    /// Resume a SUSPENDED session.
    pub fn resume(&self, session_id: &str) -> Result<()> {
        self.transition(session_id, SessionState::Suspended, SessionState::Active)
    }

    /// Complete an ACTIVE session. The session must have no active lease:
    /// release (or revoke) the writer's lease first.
    pub fn complete(&self, session_id: &str) -> Result<()> {
        if let Some(lease) = self.active_lease(session_id)? {
            return Err(StoreError::InvalidTransition(format!(
                "cannot complete session {session_id}: active lease {} held by '{}' must be released first",
                lease.lease_id, lease.holder_id,
            )));
        }
        self.transition(session_id, SessionState::Active, SessionState::Completed)
    }

    /// Archive a COMPLETED session. Terminal state.
    pub fn archive(&self, session_id: &str) -> Result<()> {
        self.transition(session_id, SessionState::Completed, SessionState::Archived)
    }

    // ---- leases ----

    /// Acquire a turn-scoped write lease. Called at the START of an agent
    /// turn; the holder must call [`SqliteContextStore::release_lease`] when the
    /// turn completes. `ttl_ms` is a safety net only: if the holder crashes
    /// without releasing, the lease auto-expires after the TTL and a new
    /// holder may acquire it.
    ///
    /// Fails with [`StoreError::LeaseConflict`] if another holder already
    /// holds an unexpired ACTIVE lease. An expired ACTIVE lease (crashed
    /// holder) is marked EXPIRED and superseded.
    ///
    /// The conflict check, lease insert, and session update run in ONE
    /// transaction: two racing acquirers cannot both win (TOCTOU), and the
    /// `idx_one_active_lease` partial unique index is the database-level
    /// backstop if they somehow both try to insert.
    pub fn acquire_lease(
        &self,
        session_id: &str,
        holder_id: &str,
        locus: Locus,
        ttl_ms: i64,
    ) -> Result<Lease> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let lease = acquire_lease_conn(&tx, session_id, holder_id, locus, ttl_ms)?;
        tx.commit()?;
        Ok(lease)
    }

    /// Release the turn-scoped lease at the end of an agent turn. Marks the
    /// lease RELEASED and clears the session's active lease, leaving the
    /// session ACTIVE but without a writer until the next acquire.
    pub fn release_lease(&self, session_id: &str, holder_id: &str) -> Result<()> {
        let lease = self
            .active_lease(session_id)?
            .ok_or_else(|| StoreError::NoActiveLease(session_id.to_string()))?;
        if lease.holder_id != holder_id {
            return Err(StoreError::NotLeaseHolder {
                writer: holder_id.to_string(),
                holder: lease.holder_id,
            });
        }
        self.set_lease_state(&lease.lease_id, LeaseState::Released)?;
        self.conn().execute(
            "UPDATE sessions SET active_lease = '', last_activity_ms = ?1 WHERE session_id = ?2",
            params![now_ms(), session_id],
        )?;
        Ok(())
    }

    /// User-initiated release with rollback (e.g. stop/cancel in the
    /// harness). Discards partial entries from the current incomplete agent
    /// turn: the op-log is truncated back to the last completed flow
    /// boundary — the seq of the last ASSISTANT_MESSAGE entry. Everything
    /// after that boundary is in-progress and is deleted, clamped to the
    /// entries this lease could have written (`seq > granted_seq`): entries
    /// from earlier leases are never touched. SYSTEM_EVENT and
    /// HANDOFF_MARKER entries are never deleted. If the session has no
    /// assistant messages yet, the boundary is seq 0.
    ///
    /// Requires the caller to hold the active lease. Marks the lease
    /// RELEASED and clears the session's active lease. For normal turn
    /// completion (nothing to discard) use [`SqliteContextStore::release_lease`].
    pub fn release_with_rollback(
        &self,
        session_id: &str,
        holder_id: &str,
    ) -> Result<RollbackReport> {
        let lease = self
            .active_lease(session_id)?
            .ok_or_else(|| StoreError::NoActiveLease(session_id.to_string()))?;
        if lease.holder_id != holder_id {
            return Err(StoreError::NotLeaseHolder {
                writer: holder_id.to_string(),
                holder: lease.holder_id,
            });
        }
        let boundary: i64 = self.conn().query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM context_entries
                 WHERE session_id = ?1 AND kind = ?2",
            params![session_id, EntryKind::AssistantMessage as i32],
            |row| row.get(0),
        )?;
        let removed = self.conn().execute(
            "DELETE FROM context_entries
                 WHERE session_id = ?1 AND seq > ?2 AND seq > ?3
                   AND kind NOT IN (?4, ?5)",
            params![
                session_id,
                boundary,
                lease.granted_seq as i64,
                EntryKind::SystemEvent as i32,
                EntryKind::HandoffMarker as i32,
            ],
        )?;
        self.set_lease_state(&lease.lease_id, LeaseState::Released)?;
        self.conn().execute(
            "UPDATE sessions SET active_lease = '', last_activity_ms = ?1 WHERE session_id = ?2",
            params![now_ms(), session_id],
        )?;
        Ok(RollbackReport {
            rolled_back_to_seq: boundary as u64,
            entries_removed: removed as u64,
        })
    }

    /// Revoke the session's active lease. This is the admin kill-switch: it
    /// does NOT require the caller to be the lease holder. The lease is
    /// marked REVOKED, the session's active lease is cleared, and a
    /// SYSTEM_EVENT recording the revocation (with `reason`) is appended to
    /// the op-log. Further appends fail until a new lease is acquired.
    pub fn revoke_lease(&self, session_id: &str, reason: &str) -> Result<Lease> {
        let lease = self
            .active_lease(session_id)?
            .ok_or_else(|| StoreError::NoActiveLease(session_id.to_string()))?;
        self.set_lease_state(&lease.lease_id, LeaseState::Revoked)?;
        self.conn().execute(
            "UPDATE sessions SET active_lease = '', last_activity_ms = ?1 WHERE session_id = ?2",
            params![now_ms(), session_id],
        )?;

        // Record the revocation in the op-log itself, bypassing the lease
        // gate: the revoked holder must not be able to suppress the event.
        let event = ContextEntry {
            entry_id: format!("revoke-{}", uuid::Uuid::now_v7()),
            session_id: session_id.to_string(),
            seq: self.head_seq(session_id)? + 1,
            kind: EntryKind::SystemEvent as i32,
            payload: reason.as_bytes().to_vec(),
            lease_holder: "system".to_string(),
            policy_version: String::new(),
            locus: Locus::Unspecified as i32,
            created_at: Some(ms_to_timestamp(self.clock.tick())),
            received_at: None,
            disposition: String::new(),
        };
        self.insert_entry_raw(&event)?;
        Ok(lease)
    }

    /// Presence-driven preemption, atomically: mark the old ACTIVE lease
    /// REVOKED with `preempted_by` recorded for audit, log the revocation
    /// SYSTEM_EVENT, and grant a fresh lease to the new holder — all in ONE
    /// transaction. A crash between revoke and grant can never leave the
    /// session writerless.
    ///
    /// Fails with [`StoreError::LeaseNotActive`] if `old_lease_id` is not the
    /// session's ACTIVE lease (already revoked/released/expired): preemption
    /// of a dead lease is a no-op error, never a silent takeover.
    pub fn preempt_lease(&self, preemption: &Preemption) -> Result<Lease> {
        let Preemption {
            session_id,
            old_lease_id,
            new_holder_id,
            new_surface_id,
            locus,
            ttl_ms,
            reason,
        } = preemption;
        let mut conn = self.conn();
        let tx = conn.transaction()?;

        let old = lease_conn(&tx, old_lease_id)?;
        if &old.session_id != session_id || old.state != LeaseState::Active as i32 {
            return Err(StoreError::LeaseNotActive(old_lease_id.clone()));
        }
        tx.execute(
            "UPDATE leases SET state = ?1, preempted_by = ?2 WHERE lease_id = ?3",
            params![LeaseState::Revoked as i32, new_surface_id, old_lease_id],
        )?;

        // Record the revocation in the op-log itself, bypassing the lease
        // gate: the revoked holder must not be able to suppress the event.
        let event = ContextEntry {
            entry_id: format!("revoke-{}", uuid::Uuid::now_v7()),
            session_id: session_id.clone(),
            seq: head_seq_conn(&tx, session_id)? + 1,
            kind: EntryKind::SystemEvent as i32,
            payload: reason.as_bytes().to_vec(),
            lease_holder: "system".to_string(),
            policy_version: String::new(),
            locus: Locus::Unspecified as i32,
            created_at: Some(ms_to_timestamp(self.clock.tick())),
            received_at: None,
            disposition: String::new(),
        };
        insert_entry_conn(&tx, &event)?;

        let lease = grant_lease_conn(&tx, session_id, new_holder_id, *locus, *ttl_ms)?;
        tx.commit()?;
        Ok(lease)
    }

    /// Atomic lease transfer for handoff: release the current holder's lease
    /// and grant a fresh one to the new holder pinned to `freeze_seq`, in ONE
    /// transaction. Failure between release and acquire is impossible, so a
    /// handoff that errors never leaves the session writerless.
    pub fn transfer_lease(
        &self,
        session_id: &str,
        from_holder: &str,
        to_holder: &str,
        locus: Locus,
        ttl_ms: i64,
        freeze_seq: u64,
    ) -> Result<Lease> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;

        let old = active_lease_conn(&tx, session_id)?
            .ok_or_else(|| StoreError::NoActiveLease(session_id.to_string()))?;
        if old.holder_id != from_holder {
            return Err(StoreError::NotLeaseHolder {
                writer: from_holder.to_string(),
                holder: old.holder_id,
            });
        }
        set_lease_state_conn(&tx, &old.lease_id, LeaseState::Released)?;

        let mut lease = grant_lease_conn(&tx, session_id, to_holder, locus, ttl_ms)?;
        tx.execute(
            "UPDATE leases SET granted_seq = ?1 WHERE lease_id = ?2",
            params![freeze_seq as i64, lease.lease_id],
        )?;
        lease.granted_seq = freeze_seq;
        tx.commit()?;
        Ok(lease)
    }

    /// Revoke every active lease on sessions owned by `org_id`. Org-scoped
    /// kill-switch for enterprise admin. Returns the revoked leases.
    pub fn revoke_all(&self, org_id: &str) -> Result<Vec<Lease>> {
        let session_ids = {
            let conn = self.conn();
            let mut stmt = conn.prepare(
                "SELECT session_id FROM sessions WHERE org_id = ?1 AND active_lease != ''",
            )?;
            let ids = stmt
                .query_map(params![org_id], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids
        };
        let mut revoked = Vec::with_capacity(session_ids.len());
        for session_id in session_ids {
            revoked.push(self.revoke_lease(&session_id, "org-wide lease revocation")?);
        }
        Ok(revoked)
    }

    /// Grant a new lease. Fails if the session already has an ACTIVE lease —
    /// single writer is enforced here. Use handoff to transfer.
    pub fn grant_lease(&self, lease: &Lease) -> Result<()> {
        if self.active_lease(&lease.session_id)?.is_some() {
            return Err(StoreError::LeaseConflict(lease.session_id.clone()));
        }
        self.insert_lease(lease)?;
        self.conn().execute(
            "UPDATE sessions SET active_lease = ?1, last_activity_ms = ?2 WHERE session_id = ?3",
            params![lease.lease_id, now_ms(), lease.session_id],
        )?;
        Ok(())
    }

    pub(crate) fn insert_lease(&self, lease: &Lease) -> Result<()> {
        insert_lease_conn(&self.conn(), lease)
    }

    pub fn lease(&self, lease_id: &str) -> Result<Lease> {
        lease_conn(&self.conn(), lease_id)
    }

    /// The session's ACTIVE lease, or the most recently expired one that has
    /// not yet been superseded (an expired lease blocks new writers until a
    /// handoff or re-grant occurs).
    pub fn active_lease(&self, session_id: &str) -> Result<Option<Lease>> {
        active_lease_conn(&self.conn(), session_id)
    }

    pub(crate) fn set_lease_state(&self, lease_id: &str, state: LeaseState) -> Result<()> {
        set_lease_state_conn(&self.conn(), lease_id, state)
    }

    /// Record the lease authority that granted this lease. The server
    /// control plane stamps its own identity here; client-supplied values
    /// are never trusted.
    pub fn set_granted_by(&self, lease_id: &str, granted_by: &str) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE leases SET granted_by = ?1 WHERE lease_id = ?2",
            params![granted_by, lease_id],
        )?;
        if n == 0 {
            return Err(StoreError::LeaseNotFound(lease_id.to_string()));
        }
        Ok(())
    }

    /// Record the surface whose presence preempted this lease, for audit.
    pub fn set_preempted_by(&self, lease_id: &str, preempted_by: &str) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE leases SET preempted_by = ?1 WHERE lease_id = ?2",
            params![preempted_by, lease_id],
        )?;
        if n == 0 {
            return Err(StoreError::LeaseNotFound(lease_id.to_string()));
        }
        Ok(())
    }

    /// Renew an ACTIVE lease, extending its expiry to now + `ttl_ms` using
    /// the local clock (the server clock when this store backs the control
    /// plane). The holder must match; expired or non-active leases cannot
    /// be renewed — acquire a fresh lease instead.
    pub fn renew_lease(&self, lease_id: &str, holder_id: &str, ttl_ms: i64) -> Result<Lease> {
        let lease = self.lease(lease_id)?;
        if lease.holder_id != holder_id {
            return Err(StoreError::NotLeaseHolder {
                writer: holder_id.to_string(),
                holder: lease.holder_id,
            });
        }
        if lease.state != LeaseState::Active as i32 {
            return Err(StoreError::LeaseNotActive(lease_id.to_string()));
        }
        let expires_ms = timestamp_to_ms(lease.expires_at.as_ref());
        if is_expired(expires_ms, now_ms()) {
            return Err(StoreError::LeaseExpired(lease_id.to_string()));
        }
        let new_expires = now_ms() + ttl_ms;
        self.conn().execute(
            "UPDATE leases SET expires_at_ms = ?1 WHERE lease_id = ?2",
            params![new_expires, lease_id],
        )?;
        self.lease(lease_id)
    }

    /// Set the granted_seq of an existing lease. Used by handoff to pin the
    /// new holder's lease to the freeze point.
    pub(crate) fn set_granted_seq(&self, lease_id: &str, granted_seq: u64) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE leases SET granted_seq = ?1 WHERE lease_id = ?2",
            params![granted_seq as i64, lease_id],
        )?;
        if n == 0 {
            return Err(StoreError::LeaseNotFound(lease_id.to_string()));
        }
        Ok(())
    }

    /// Verify that `writer` currently holds the write lease for `session_id`.
    pub fn verify_writer(&self, session_id: &str, writer: &str) -> Result<Lease> {
        verify_writer_conn(&self.conn(), session_id, writer)
    }

    // ---- op-log ----

    /// Append an entry to the op-log. Assigns the next sequence number,
    /// enforces that the session is ACTIVE, and enforces the write lease.
    /// Returns the assigned seq.
    ///
    /// Writer verification, seq assignment, and the insert run in ONE
    /// transaction: a lease change between check and write (TOCTOU) or a
    /// crash mid-append can never produce a half-committed entry or a seq
    /// collision between racing writers.
    #[instrument(skip(self, entry), fields(session = %entry.session_id))]
    pub fn append_entry(&self, entry: &mut ContextEntry) -> Result<u64> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let session = session_conn(&tx, &entry.session_id)?;
        if session.state != SessionState::Active as i32 {
            return Err(StoreError::SessionNotActive {
                session_id: entry.session_id.clone(),
                state: session_state_name(session.state),
            });
        }
        verify_writer_conn(&tx, &entry.session_id, &entry.lease_holder)?;
        let seq = head_seq_conn(&tx, &entry.session_id)? + 1;
        entry.seq = seq;
        // The writer always stamps created_at with its own monotonic clock;
        // a caller-supplied timestamp is never trusted (it could forge
        // priority in (created_at, entry_id) conflict resolution). Only
        // insert_entry_raw — the replay/reconcile path — preserves the
        // original timestamp.
        entry.created_at = Some(ms_to_timestamp(self.clock.tick()));
        // received_at is stamped on reconcile ingest by the receiving store,
        // never by the writer (ADR 006); a direct local append has none.
        entry.received_at = None;
        insert_entry_conn(&tx, entry)?;
        tx.execute(
            "UPDATE sessions SET last_activity_ms = ?1 WHERE session_id = ?2",
            params![now_ms(), entry.session_id],
        )?;
        tx.commit()?;
        Ok(seq)
    }

    /// Insert an entry as-is, bypassing lease checks. Used by reconcile and
    /// by replication catch-up, where entries were already validated by the
    /// writer's locus. Crate-internal: external writers must use
    /// [`SqliteContextStore::append_entry`] so the lease is always enforced.
    pub(crate) fn insert_entry_raw(&self, entry: &ContextEntry) -> Result<()> {
        insert_entry_conn(&self.conn(), entry)
    }

    pub fn entry_by_id(&self, entry_id: &str) -> Result<Option<ContextEntry>> {
        self.conn()
            .query_row(
                "SELECT session_id, seq, entry_id, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition
                 FROM context_entries WHERE entry_id = ?1",
                params![entry_id],
                row_to_entry,
            )
            .optional()
            .map_err(StoreError::from)
    }

    pub fn entry_at_seq(&self, session_id: &str, seq: u64) -> Result<Option<ContextEntry>> {
        self.conn()
            .query_row(
                "SELECT session_id, seq, entry_id, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition
                 FROM context_entries WHERE session_id = ?1 AND seq = ?2",
                params![session_id, seq as i64],
                row_to_entry,
            )
            .optional()
            .map_err(StoreError::from)
    }

    /// All entries with seq > `after_seq`, in order. Used for handoff
    /// catch-up and reconcile.
    pub fn entries_since(&self, session_id: &str, after_seq: u64) -> Result<Vec<ContextEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, seq, entry_id, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition
             FROM context_entries WHERE session_id = ?1 AND seq > ?2 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![session_id, after_seq as i64], row_to_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn head_seq(&self, session_id: &str) -> Result<u64> {
        head_seq_conn(&self.conn(), session_id)
    }

    /// Set an entry's disposition (ADR 006). Reconcile marks policy-violating
    /// replayed entries `QUARANTINE`; the entry is preserved, never dropped.
    pub(crate) fn set_disposition(&self, entry_id: &str, disposition: &str) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE context_entries SET disposition = ?1 WHERE entry_id = ?2",
            params![disposition, entry_id],
        )?;
        if n == 0 {
            return Err(StoreError::SessionNotFound(entry_id.to_string()));
        }
        Ok(())
    }

    /// Reassign the seq of an existing entry (conflict resolution moves the
    /// loser to the tail of the log).
    pub(crate) fn reassign_seq(&self, entry_id: &str, new_seq: u64) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE context_entries SET seq = ?1 WHERE entry_id = ?2",
            params![new_seq as i64, entry_id],
        )?;
        if n == 0 {
            return Err(StoreError::SessionNotFound(entry_id.to_string()));
        }
        Ok(())
    }
}

/// Connection-level helpers. Every store method above delegates to these so
/// the transactional paths (`acquire_lease`, `append_entry`, `preempt_lease`,
/// `transfer_lease`) can compose them inside one `Transaction` (which derefs
/// to `&Connection`) without re-taking the store's connection mutex — which
/// would deadlock, as `std::sync::Mutex` is not reentrant.
fn session_conn(conn: &Connection, session_id: &str) -> Result<SessionMeta> {
    conn.query_row(
        "SELECT session_id, soul_id, user_id, state, active_lease, created_at_ms, last_activity_ms, labels, org_id
         FROM sessions WHERE session_id = ?1",
        params![session_id],
        |row| {
            let labels_json: String = row.get(7)?;
            Ok(SessionMeta {
                session_id: row.get(0)?,
                soul_id: row.get(1)?,
                user_id: row.get(2)?,
                state: row.get(3)?,
                active_lease: row.get(4)?,
                created_at: Some(ms_to_timestamp(row.get(5)?)),
                last_activity: Some(ms_to_timestamp(row.get(6)?)),
                labels: serde_json::from_str(&labels_json).unwrap_or_default(),
                org_id: row.get(8)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| StoreError::SessionNotFound(session_id.to_string()))
}

fn lease_conn(conn: &Connection, lease_id: &str) -> Result<Lease> {
    conn.query_row(
        "SELECT lease_id, session_id, holder_id, locus, granted_seq, granted_at_ms, expires_at_ms, state, granted_by, preempted_by
         FROM leases WHERE lease_id = ?1",
        params![lease_id],
        |row| {
            Ok(Lease {
                lease_id: row.get(0)?,
                session_id: row.get(1)?,
                holder_id: row.get(2)?,
                locus: row.get(3)?,
                granted_seq: row.get::<_, i64>(4)? as u64,
                granted_at: Some(ms_to_timestamp(row.get(5)?)),
                expires_at: Some(ms_to_timestamp(row.get(6)?)),
                state: row.get(7)?,
                granted_by: row.get(8)?,
                preempted_by: row.get(9)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| StoreError::LeaseNotFound(lease_id.to_string()))
}

fn active_lease_conn(conn: &Connection, session_id: &str) -> Result<Option<Lease>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT lease_id FROM leases WHERE session_id = ?1 AND state = ?2
             ORDER BY granted_at_ms DESC LIMIT 1",
            params![session_id, LeaseState::Active as i32],
            |row| row.get(0),
        )
        .optional()?;
    match id {
        Some(id) => Ok(Some(lease_conn(conn, &id)?)),
        None => Ok(None),
    }
}

fn head_seq_conn(conn: &Connection, session_id: &str) -> Result<u64> {
    let seq: Option<i64> = conn
        .query_row(
            "SELECT MAX(seq) FROM context_entries WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(seq.unwrap_or(0) as u64)
}

fn insert_lease_conn(conn: &Connection, lease: &Lease) -> Result<()> {
    conn.execute(
        "INSERT INTO leases
         (lease_id, session_id, holder_id, locus, granted_seq, granted_at_ms, expires_at_ms, state, granted_by, preempted_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            lease.lease_id,
            lease.session_id,
            lease.holder_id,
            lease.locus,
            lease.granted_seq as i64,
            timestamp_to_ms(lease.granted_at.as_ref()),
            timestamp_to_ms(lease.expires_at.as_ref()),
            lease.state,
            lease.granted_by,
            lease.preempted_by,
        ],
    )?;
    Ok(())
}

fn insert_entry_conn(conn: &Connection, entry: &ContextEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO context_entries
         (session_id, seq, entry_id, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            entry.session_id,
            entry.seq as i64,
            entry.entry_id,
            entry.kind,
            entry.payload,
            entry.lease_holder,
            entry.policy_version,
            entry.locus,
            timestamp_to_ms(entry.created_at.as_ref()),
            entry.received_at.as_ref().map(|ts| timestamp_to_ms(Some(ts))),
            entry.disposition,
        ],
    )?;
    Ok(())
}

fn set_lease_state_conn(conn: &Connection, lease_id: &str, state: LeaseState) -> Result<()> {
    let n = conn.execute(
        "UPDATE leases SET state = ?1 WHERE lease_id = ?2",
        params![state as i32, lease_id],
    )?;
    if n == 0 {
        return Err(StoreError::LeaseNotFound(lease_id.to_string()));
    }
    Ok(())
}

fn verify_writer_conn(conn: &Connection, session_id: &str, writer: &str) -> Result<Lease> {
    let lease = active_lease_conn(conn, session_id)?
        .ok_or_else(|| StoreError::NoActiveLease(session_id.to_string()))?;
    if lease.holder_id != writer {
        return Err(StoreError::NotLeaseHolder {
            writer: writer.to_string(),
            holder: lease.holder_id,
        });
    }
    let expires_ms = timestamp_to_ms(lease.expires_at.as_ref());
    if is_expired(expires_ms, now_ms()) {
        return Err(StoreError::LeaseExpired(lease.lease_id));
    }
    Ok(lease)
}

/// Insert a fresh ACTIVE lease for `session_id` and point the session at it.
/// Caller must hold the transaction and have already established that no
/// unexpired ACTIVE lease exists.
fn grant_lease_conn(
    conn: &Connection,
    session_id: &str,
    holder_id: &str,
    locus: Locus,
    ttl_ms: i64,
) -> Result<Lease> {
    let now = now_ms();
    let lease = Lease {
        lease_id: format!("lease-{}", uuid::Uuid::now_v7()),
        session_id: session_id.to_string(),
        holder_id: holder_id.to_string(),
        locus: locus as i32,
        granted_seq: head_seq_conn(conn, session_id)?,
        granted_at: Some(ms_to_timestamp(now)),
        expires_at: Some(ms_to_timestamp(now + ttl_ms)),
        state: LeaseState::Active as i32,
        granted_by: String::new(),
        preempted_by: String::new(),
    };
    insert_lease_conn(conn, &lease)?;
    conn.execute(
        "UPDATE sessions SET active_lease = ?1, last_activity_ms = ?2 WHERE session_id = ?3",
        params![lease.lease_id, now, session_id],
    )?;
    Ok(lease)
}

/// The transactional body of [`SqliteContextStore::acquire_lease`].
fn acquire_lease_conn(
    conn: &Connection,
    session_id: &str,
    holder_id: &str,
    locus: Locus,
    ttl_ms: i64,
) -> Result<Lease> {
    if let Some(existing) = active_lease_conn(conn, session_id)? {
        let expires_ms = timestamp_to_ms(existing.expires_at.as_ref());
        if !is_expired(expires_ms, now_ms()) {
            return Err(StoreError::LeaseConflict(session_id.to_string()));
        }
        // Crashed holder: the safety-net TTL fired. Retire the stale
        // lease so a new writer can take over.
        set_lease_state_conn(conn, &existing.lease_id, LeaseState::Expired)?;
    }
    grant_lease_conn(conn, session_id, holder_id, locus, ttl_ms)
}

pub(crate) fn session_state_name(state: i32) -> String {
    SessionState::try_from(state)
        .map(|s| s.as_str_name().to_string())
        .unwrap_or_else(|_| format!("UNKNOWN({state})"))
}

/// Additive schema migrations for stores created by older builds. New
/// columns are added to schema.sql's CREATE TABLE for fresh stores; here we
/// only backfill columns that predate the current schema.
fn migrate(conn: &Connection) -> Result<()> {
    let has_org_id: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'org_id'")?
        .exists([])?;
    if !has_org_id {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN org_id TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    for column in ["granted_by", "preempted_by"] {
        let exists: bool = conn
            .prepare(&format!(
                "SELECT 1 FROM pragma_table_info('leases') WHERE name = '{column}'"
            ))?
            .exists([])?;
        if !exists {
            conn.execute(
                &format!("ALTER TABLE leases ADD COLUMN {column} TEXT NOT NULL DEFAULT ''"),
                [],
            )?;
        }
    }
    let has_disposition: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('context_entries') WHERE name = 'disposition'")?
        .exists([])?;
    if !has_disposition {
        conn.execute(
            "ALTER TABLE context_entries ADD COLUMN disposition TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    let has_received_at: bool = conn
        .prepare(
            "SELECT 1 FROM pragma_table_info('context_entries') WHERE name = 'received_at_ms'",
        )?
        .exists([])?;
    if !has_received_at {
        conn.execute(
            "ALTER TABLE context_entries ADD COLUMN received_at_ms INTEGER",
            [],
        )?;
    }
    Ok(())
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextEntry> {
    Ok(ContextEntry {
        session_id: row.get(0)?,
        seq: row.get::<_, i64>(1)? as u64,
        entry_id: row.get(2)?,
        kind: row.get(3)?,
        payload: row.get(4)?,
        lease_holder: row.get(5)?,
        policy_version: row.get(6)?,
        locus: row.get(7)?,
        created_at: Some(ms_to_timestamp(row.get(8)?)),
        received_at: row.get::<_, Option<i64>>(9)?.map(ms_to_timestamp),
        disposition: row.get(10)?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use fabric_types::context::{EntryKind, Locus, SessionState};

    pub(crate) fn test_session(session_id: &str) -> SessionMeta {
        SessionMeta {
            session_id: session_id.into(),
            soul_id: "soul-1".into(),
            user_id: "user-1".into(),
            state: SessionState::Active as i32,
            active_lease: String::new(),
            created_at: Some(ms_to_timestamp(now_ms())),
            last_activity: Some(ms_to_timestamp(now_ms())),
            labels: Default::default(),
            org_id: String::new(),
        }
    }

    pub(crate) fn test_lease(lease_id: &str, session_id: &str, holder: &str) -> Lease {
        Lease {
            lease_id: lease_id.into(),
            session_id: session_id.into(),
            holder_id: holder.into(),
            locus: Locus::Endpoint as i32,
            granted_seq: 0,
            granted_at: Some(ms_to_timestamp(now_ms())),
            expires_at: Some(ms_to_timestamp(now_ms() + 60_000)),
            state: LeaseState::Active as i32,
            granted_by: String::new(),
            preempted_by: String::new(),
        }
    }

    pub(crate) fn test_entry(entry_id: &str, session_id: &str, holder: &str) -> ContextEntry {
        ContextEntry {
            entry_id: entry_id.into(),
            session_id: session_id.into(),
            seq: 0,
            kind: EntryKind::UserMessage as i32,
            payload: b"hello".to_vec(),
            lease_holder: holder.into(),
            policy_version: "v1".into(),
            locus: Locus::Endpoint as i32,
            created_at: None,
            received_at: None,
            disposition: String::new(),
        }
    }

    #[test]
    fn close_checkpoints_wal() {
        let dir = std::env::temp_dir().join(format!("fabric-close-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ctx.db");

        let store = SqliteContextStore::open(&path).unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store.close().unwrap();

        // WAL was truncated on close; data survived in the main db file.
        let store = SqliteContextStore::open(&path).unwrap();
        assert_eq!(store.session("s1").unwrap().session_id, "s1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ping_and_active_session_count() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.ping().unwrap();
        assert_eq!(store.active_session_count().unwrap(), 0);

        store.create_session(&test_session("s1")).unwrap();
        store.create_session(&test_session("s2")).unwrap();
        assert_eq!(store.active_session_count().unwrap(), 2);

        store.suspend("s1").unwrap();
        assert_eq!(store.active_session_count().unwrap(), 1);
    }

    #[test]
    fn list_active_sessions_returns_only_active() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        assert!(store.list_active_sessions().unwrap().is_empty());

        store.create_session(&test_session("s1")).unwrap();
        store.create_session(&test_session("s2")).unwrap();
        store.suspend("s2").unwrap();

        let active = store.list_active_sessions().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].session_id, "s1");
        assert_eq!(active[0].state, SessionState::Active as i32);
    }

    #[test]
    fn acquire_append_release_cycle() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();

        // Turn 1: acquire, append, release.
        let lease = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        assert_eq!(lease.state, LeaseState::Active as i32);
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        assert_eq!(store.append_entry(&mut e1).unwrap(), 1);
        store.release_lease("s1", "endpoint-1").unwrap();

        // Lease is RELEASED, session has no active lease.
        assert_eq!(
            store.lease(&lease.lease_id).unwrap().state,
            LeaseState::Released as i32
        );
        assert!(store.active_lease("s1").unwrap().is_none());

        // Without a writer, appends fail.
        let mut e2 = test_entry("e2", "s1", "endpoint-1");
        assert!(matches!(
            store.append_entry(&mut e2),
            Err(StoreError::NoActiveLease(_))
        ));

        // Turn 2: a fresh acquire succeeds after release.
        let lease2 = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        assert_ne!(lease.lease_id, lease2.lease_id);
        assert_eq!(lease2.granted_seq, 1);
        let mut e3 = test_entry("e3", "s1", "endpoint-1");
        assert_eq!(store.append_entry(&mut e3).unwrap(), 2);
    }

    #[test]
    fn acquire_during_active_lease_conflicts() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        let err = store
            .acquire_lease("s1", "server-1", Locus::Server, 30_000)
            .unwrap_err();
        assert!(matches!(err, StoreError::LeaseConflict(_)));
    }

    #[test]
    fn acquire_after_holder_crash_succeeds_once_expired() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        // Holder crashes mid-turn without releasing; TTL is the safety net.
        let crashed = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 0)
            .unwrap();

        let lease = store
            .acquire_lease("s1", "server-1", Locus::Server, 30_000)
            .unwrap();
        assert_eq!(lease.holder_id, "server-1");
        assert_eq!(
            store.lease(&crashed.lease_id).unwrap().state,
            LeaseState::Expired as i32
        );
        let mut e1 = test_entry("e1", "s1", "server-1");
        assert_eq!(store.append_entry(&mut e1).unwrap(), 1);
    }

    #[test]
    fn release_rejects_non_holder() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        let err = store.release_lease("s1", "mallory").unwrap_err();
        assert!(matches!(err, StoreError::NotLeaseHolder { .. }));
        // The lease is still active.
        assert!(store.active_lease("s1").unwrap().is_some());
    }

    #[test]
    fn append_rejects_non_active_sessions() {
        for state in [
            SessionState::HandedOff,
            SessionState::Completed,
            SessionState::Archived,
            SessionState::Suspended,
        ] {
            let store = SqliteContextStore::open_in_memory().unwrap();
            store.create_session(&test_session("s1")).unwrap();
            store
                .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
                .unwrap();
            store.set_session_state("s1", state as i32).unwrap();

            let mut e = test_entry("e1", "s1", "endpoint-1");
            let err = store.append_entry(&mut e).unwrap_err();
            assert!(
                matches!(err, StoreError::SessionNotActive { .. }),
                "state {state:?} must reject appends: {err}"
            );
            assert_eq!(store.head_seq("s1").unwrap(), 0);
        }
    }

    #[test]
    fn release_keeps_session_active_without_writer() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        store.append_entry(&mut e1).unwrap();
        store.release_lease("s1", "endpoint-1").unwrap();

        // Session stays ACTIVE, just without a writer.
        let session = store.session("s1").unwrap();
        assert_eq!(session.state, SessionState::Active as i32);
        assert_eq!(session.active_lease, "");
    }

    #[test]
    fn revoke_lease_blocks_appends_and_logs_event() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let lease = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        store.append_entry(&mut e1).unwrap();

        // Admin kill-switch: caller is not the lease holder.
        let revoked = store.revoke_lease("s1", "policy violation").unwrap();
        assert_eq!(revoked.lease_id, lease.lease_id);
        assert_eq!(
            store.lease(&lease.lease_id).unwrap().state,
            LeaseState::Revoked as i32
        );
        assert!(store.active_lease("s1").unwrap().is_none());

        // The former holder can no longer append.
        let mut e2 = test_entry("e2", "s1", "endpoint-1");
        assert!(matches!(
            store.append_entry(&mut e2),
            Err(StoreError::NoActiveLease(_))
        ));

        // The revocation is recorded in the op-log as a SYSTEM_EVENT.
        let log = store.entries_since("s1", 0).unwrap();
        let event = log.last().unwrap();
        assert_eq!(event.kind, EntryKind::SystemEvent as i32);
        assert_eq!(event.payload, b"policy violation");
        assert_eq!(event.seq, 2);

        // Revoking again fails: there is no active lease.
        assert!(matches!(
            store.revoke_lease("s1", "again"),
            Err(StoreError::NoActiveLease(_))
        ));
    }

    #[test]
    fn revoke_all_revokes_every_active_lease_in_org() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        for (sid, org) in [("s1", "org-1"), ("s2", "org-1"), ("s3", "org-2")] {
            let mut meta = test_session(sid);
            meta.org_id = org.into();
            store.create_session(&meta).unwrap();
            store
                .acquire_lease(sid, "endpoint-1", Locus::Endpoint, 30_000)
                .unwrap();
        }
        // s4 is in org-1 but has no active lease: must be skipped.
        let mut meta = test_session("s4");
        meta.org_id = "org-1".into();
        store.create_session(&meta).unwrap();

        let revoked = store.revoke_all("org-1").unwrap();
        assert_eq!(revoked.len(), 2);
        assert!(store.active_lease("s1").unwrap().is_none());
        assert!(store.active_lease("s2").unwrap().is_none());
        // Other org untouched.
        assert!(store.active_lease("s3").unwrap().is_some());
    }

    #[test]
    fn lifecycle_happy_path() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();

        store.suspend("s1").unwrap();
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Suspended as i32
        );
        store.resume("s1").unwrap();
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Active as i32
        );
        store.complete("s1").unwrap();
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Completed as i32
        );
        store.archive("s1").unwrap();
        assert_eq!(
            store.session("s1").unwrap().state,
            SessionState::Archived as i32
        );
    }

    #[test]
    fn lifecycle_rejects_invalid_transitions() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();

        // Cannot resume an ACTIVE session.
        let err = store.resume("s1").unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidTransition(ref m) if m.contains("SESSION_STATE_SUSPENDED -> SESSION_STATE_ACTIVE"))
        );

        // Cannot archive an ACTIVE session.
        let err = store.archive("s1").unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidTransition(ref m) if m.contains("SESSION_STATE_COMPLETED -> SESSION_STATE_ARCHIVED"))
        );

        // Cannot suspend twice.
        store.suspend("s1").unwrap();
        let err = store.suspend("s1").unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidTransition(ref m) if m.contains("SESSION_STATE_ACTIVE -> SESSION_STATE_SUSPENDED"))
        );

        // Cannot complete a SUSPENDED session.
        let err = store.complete("s1").unwrap_err();
        assert!(matches!(err, StoreError::InvalidTransition(_)));

        // Unknown session.
        assert!(matches!(
            store.suspend("nope"),
            Err(StoreError::SessionNotFound(_))
        ));
    }

    #[test]
    fn complete_requires_no_active_lease() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();

        let err = store.complete("s1").unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidTransition(ref m) if m.contains("must be released")),
            "{err}"
        );

        store.release_lease("s1", "endpoint-1").unwrap();
        store.complete("s1").unwrap();
    }

    #[test]
    fn append_assigns_monotonic_seq() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .grant_lease(&test_lease("l1", "s1", "endpoint-1"))
            .unwrap();

        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        let mut e2 = test_entry("e2", "s1", "endpoint-1");
        assert_eq!(store.append_entry(&mut e1).unwrap(), 1);
        assert_eq!(store.append_entry(&mut e2).unwrap(), 2);
        assert_eq!(store.head_seq("s1").unwrap(), 2);
    }

    #[test]
    fn append_rejects_non_holder() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .grant_lease(&test_lease("l1", "s1", "endpoint-1"))
            .unwrap();

        let mut rogue = test_entry("e1", "s1", "attacker");
        let err = store.append_entry(&mut rogue).unwrap_err();
        assert!(matches!(err, StoreError::NotLeaseHolder { .. }));
        assert_eq!(store.head_seq("s1").unwrap(), 0);
    }

    #[test]
    fn append_rejects_expired_lease() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let mut lease = test_lease("l1", "s1", "endpoint-1");
        lease.expires_at = Some(ms_to_timestamp(now_ms() - 1));
        store.grant_lease(&lease).unwrap();

        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        let err = store.append_entry(&mut e1).unwrap_err();
        assert!(matches!(err, StoreError::LeaseExpired(_)));
    }

    #[test]
    fn append_always_restamps_created_at() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .grant_lease(&test_lease("l1", "s1", "endpoint-1"))
            .unwrap();

        // A caller-supplied timestamp far in the past would win every
        // (created_at, entry_id) conflict resolution. The store must not
        // trust it.
        let forged = ms_to_timestamp(1_000);
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        e1.created_at = Some(forged);
        store.append_entry(&mut e1).unwrap();

        let stored = store.entry_by_id("e1").unwrap().unwrap();
        let stored_ms = timestamp_to_ms(stored.created_at.as_ref());
        assert!(
            stored_ms >= now_ms() - 60_000,
            "created_at must be restamped by the writer's clock, got {stored_ms}"
        );

        // The raw replay path, by contrast, preserves the original
        // timestamp.
        let replayed = ContextEntry {
            created_at: Some(forged),
            ..test_entry("e2", "s1", "endpoint-1")
        };
        store.insert_entry_raw(&replayed).unwrap();
        let stored = store.entry_by_id("e2").unwrap().unwrap();
        assert_eq!(timestamp_to_ms(stored.created_at.as_ref()), 1_000);
    }

    #[test]
    fn single_active_lease_enforced() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .grant_lease(&test_lease("l1", "s1", "endpoint-1"))
            .unwrap();
        let err = store
            .grant_lease(&test_lease("l2", "s1", "server-1"))
            .unwrap_err();
        assert!(matches!(err, StoreError::LeaseConflict(_)));
    }

    #[test]
    fn entries_since_returns_ordered_tail() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .grant_lease(&test_lease("l1", "s1", "endpoint-1"))
            .unwrap();
        for i in 1..=5 {
            let mut e = test_entry(&format!("e{i}"), "s1", "endpoint-1");
            store.append_entry(&mut e).unwrap();
        }
        let tail = store.entries_since("s1", 3).unwrap();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].entry_id, "e4");
        assert_eq!(tail[1].entry_id, "e5");
    }

    fn append_kinds(
        store: &SqliteContextStore,
        session_id: &str,
        holder: &str,
        kinds: &[EntryKind],
    ) {
        for (i, kind) in kinds.iter().enumerate() {
            let mut e = test_entry(&format!("e{i}"), session_id, holder);
            e.kind = *kind as i32;
            store.append_entry(&mut e).unwrap();
        }
    }

    #[test]
    fn release_with_rollback_discards_partial_turn() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();

        // Partial turn: no ASSISTANT_MESSAGE yet.
        append_kinds(
            &store,
            "s1",
            "endpoint-1",
            &[
                EntryKind::UserMessage,
                EntryKind::ToolCall,
                EntryKind::ToolResult,
            ],
        );
        assert_eq!(store.head_seq("s1").unwrap(), 3);

        let report = store.release_with_rollback("s1", "endpoint-1").unwrap();
        assert_eq!(
            report,
            RollbackReport {
                rolled_back_to_seq: 0,
                entries_removed: 3,
            }
        );
        assert_eq!(store.head_seq("s1").unwrap(), 0);
        assert!(store.active_lease("s1").unwrap().is_none());

        // The log can be written again from seq 1.
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        let mut e = test_entry("e-retry", "s1", "endpoint-1");
        assert_eq!(store.append_entry(&mut e).unwrap(), 1);
    }

    #[test]
    fn release_with_rollback_preserves_completed_flows() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();

        // Completed flow (seq 1-4), then a partial turn (seq 5-6).
        append_kinds(
            &store,
            "s1",
            "endpoint-1",
            &[
                EntryKind::UserMessage,
                EntryKind::ToolCall,
                EntryKind::ToolResult,
                EntryKind::AssistantMessage,
                EntryKind::UserMessage,
                EntryKind::ToolCall,
            ],
        );

        let report = store.release_with_rollback("s1", "endpoint-1").unwrap();
        assert_eq!(
            report,
            RollbackReport {
                rolled_back_to_seq: 4,
                entries_removed: 2,
            }
        );
        assert_eq!(store.head_seq("s1").unwrap(), 4);
        assert_eq!(
            store.entry_at_seq("s1", 4).unwrap().unwrap().kind,
            EntryKind::AssistantMessage as i32
        );
        assert!(store.entry_at_seq("s1", 5).unwrap().is_none());
        assert!(store.active_lease("s1").unwrap().is_none());
    }

    #[test]
    fn release_lease_keeps_partial_entries() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        append_kinds(
            &store,
            "s1",
            "endpoint-1",
            &[EntryKind::UserMessage, EntryKind::ToolCall],
        );

        // Normal release: clean, no rollback.
        store.release_lease("s1", "endpoint-1").unwrap();
        assert_eq!(store.head_seq("s1").unwrap(), 2);
        assert!(store.active_lease("s1").unwrap().is_none());
    }

    #[test]
    fn release_with_rollback_never_touches_prior_lease_entries() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();

        // Lease A: a completed flow (user message + assistant message).
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        append_kinds(
            &store,
            "s1",
            "endpoint-1",
            &[EntryKind::UserMessage, EntryKind::AssistantMessage],
        );
        store.release_lease("s1", "endpoint-1").unwrap();
        assert_eq!(store.head_seq("s1").unwrap(), 2);

        // Lease B: a partial turn with no assistant message of its own.
        let lease_b = store
            .acquire_lease("s1", "server-1", Locus::Server, 30_000)
            .unwrap();
        assert_eq!(lease_b.granted_seq, 2);
        for (i, kind) in [EntryKind::UserMessage, EntryKind::ToolCall]
            .iter()
            .enumerate()
        {
            let mut e = test_entry(&format!("b{i}"), "s1", "server-1");
            e.kind = *kind as i32;
            store.append_entry(&mut e).unwrap();
        }
        assert_eq!(store.head_seq("s1").unwrap(), 4);

        // Rollback on lease B removes only lease B's entries — never lease
        // A's, even though the rollback boundary (seq 2) came from A.
        let report = store.release_with_rollback("s1", "server-1").unwrap();
        assert_eq!(
            report,
            RollbackReport {
                rolled_back_to_seq: 2,
                entries_removed: 2,
            }
        );
        assert_eq!(store.head_seq("s1").unwrap(), 2);
        assert_eq!(
            store.entry_at_seq("s1", 2).unwrap().unwrap().kind,
            EntryKind::AssistantMessage as i32
        );
    }

    #[test]
    fn release_with_rollback_preserves_system_events_and_handoff_markers() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        append_kinds(&store, "s1", "endpoint-1", &[EntryKind::UserMessage]);

        // A SYSTEM_EVENT and a HANDOFF_MARKER land mid-turn (written via the
        // raw path, as revoke/handoff do).
        for (id, kind) in [
            ("sys-1", EntryKind::SystemEvent),
            ("ho-1", EntryKind::HandoffMarker),
        ] {
            let event = ContextEntry {
                entry_id: id.into(),
                session_id: "s1".into(),
                seq: store.head_seq("s1").unwrap() + 1,
                kind: kind as i32,
                payload: b"admin".to_vec(),
                lease_holder: "system".into(),
                policy_version: String::new(),
                locus: Locus::Unspecified as i32,
                created_at: Some(ms_to_timestamp(now_ms())),
                received_at: None,
                disposition: String::new(),
            };
            store.insert_entry_raw(&event).unwrap();
        }
        let mut e = test_entry("tc1", "s1", "endpoint-1");
        e.kind = EntryKind::ToolCall as i32;
        store.append_entry(&mut e).unwrap();
        assert_eq!(store.head_seq("s1").unwrap(), 4);

        // Boundary is 0 (no assistant messages): everything the lease wrote
        // rolls back, but the event/marker entries survive.
        let report = store.release_with_rollback("s1", "endpoint-1").unwrap();
        assert_eq!(report.entries_removed, 2);
        let survivors = store.entries_since("s1", 0).unwrap();
        let kinds: Vec<i32> = survivors.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                EntryKind::SystemEvent as i32,
                EntryKind::HandoffMarker as i32
            ]
        );
    }

    #[test]
    fn release_with_rollback_rejects_non_holder() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        append_kinds(&store, "s1", "endpoint-1", &[EntryKind::UserMessage]);

        let err = store.release_with_rollback("s1", "mallory").unwrap_err();
        assert!(matches!(err, StoreError::NotLeaseHolder { .. }));
        // Nothing removed, lease still active.
        assert_eq!(store.head_seq("s1").unwrap(), 1);
        assert!(store.active_lease("s1").unwrap().is_some());
    }

    #[test]
    fn renew_lease_extends_expiry_for_holder() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let lease = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        let before = timestamp_to_ms(lease.expires_at.as_ref());

        let renewed = store
            .renew_lease(&lease.lease_id, "endpoint-1", 60_000)
            .unwrap();
        let after = timestamp_to_ms(renewed.expires_at.as_ref());
        assert!(
            after > before,
            "renewal must extend expiry: {before} -> {after}"
        );
        assert_eq!(renewed.state, LeaseState::Active as i32);
        assert_eq!(renewed.holder_id, "endpoint-1");
        // Persisted: re-reading from the store sees the new expiry.
        let reread = store.lease(&lease.lease_id).unwrap();
        assert_eq!(timestamp_to_ms(reread.expires_at.as_ref()), after);
    }

    #[test]
    fn renew_lease_rejects_non_holder_and_non_active() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let lease = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();

        let err = store
            .renew_lease(&lease.lease_id, "mallory", 60_000)
            .unwrap_err();
        assert!(matches!(err, StoreError::NotLeaseHolder { .. }));

        store.release_lease("s1", "endpoint-1").unwrap();
        let err = store
            .renew_lease(&lease.lease_id, "endpoint-1", 60_000)
            .unwrap_err();
        assert!(matches!(err, StoreError::LeaseNotActive(_)));

        assert!(matches!(
            store.renew_lease("nope", "endpoint-1", 60_000),
            Err(StoreError::LeaseNotFound(_))
        ));
    }

    #[test]
    fn renew_lease_rejects_expired() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        // TTL 0 expires immediately.
        let lease = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 0)
            .unwrap();
        let err = store
            .renew_lease(&lease.lease_id, "endpoint-1", 60_000)
            .unwrap_err();
        assert!(matches!(err, StoreError::LeaseExpired(_)));
    }

    #[test]
    fn disposition_and_received_at_roundtrip() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .grant_lease(&test_lease("l1", "s1", "endpoint-1"))
            .unwrap();

        // A replayed entry carries a server-stamped received_at.
        let mut replayed = test_entry("e1", "s1", "endpoint-1");
        replayed.seq = 1;
        replayed.created_at = Some(ms_to_timestamp(1_000));
        replayed.received_at = Some(ms_to_timestamp(2_000));
        store.insert_entry_raw(&replayed).unwrap();

        let stored = store.entry_by_id("e1").unwrap().unwrap();
        assert_eq!(timestamp_to_ms(stored.received_at.as_ref()), 2_000);
        assert_eq!(stored.disposition, "");

        // Quarantine persists and reads back.
        store.set_disposition("e1", "QUARANTINE").unwrap();
        let stored = store.entry_by_id("e1").unwrap().unwrap();
        assert_eq!(stored.disposition, "QUARANTINE");
        assert!(matches!(
            store.set_disposition("nope", "QUARANTINE"),
            Err(StoreError::SessionNotFound(_))
        ));

        // A direct local append has no received_at.
        let mut e2 = test_entry("e2", "s1", "endpoint-1");
        e2.received_at = Some(ms_to_timestamp(9_999));
        store.append_entry(&mut e2).unwrap();
        let stored = store.entry_by_id("e2").unwrap().unwrap();
        assert!(stored.received_at.is_none());
    }

    #[test]
    fn lease_attribution_roundtrips() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let lease = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        assert_eq!(lease.granted_by, "");
        assert_eq!(lease.preempted_by, "");

        store
            .set_granted_by(&lease.lease_id, "fabric-server")
            .unwrap();
        store.set_preempted_by(&lease.lease_id, "web-1").unwrap();

        let reread = store.lease(&lease.lease_id).unwrap();
        assert_eq!(reread.granted_by, "fabric-server");
        assert_eq!(reread.preempted_by, "web-1");

        assert!(matches!(
            store.set_granted_by("nope", "x"),
            Err(StoreError::LeaseNotFound(_))
        ));
    }

    #[test]
    fn one_active_lease_index_backstops_single_writer() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();

        // Bypass the transactional acquire path entirely: a raw insert of a
        // second ACTIVE lease is rejected by the database itself — the
        // partial unique index is the defense-in-depth behind acquire's
        // check-then-insert.
        let lease2 = test_lease("l2", "s1", "server-1");
        let err = store.insert_lease(&lease2).unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::Sqlite(rusqlite::Error::SqliteFailure(ref e, _))
                    if e.code == rusqlite::ErrorCode::ConstraintViolation
            ),
            "expected a unique-constraint violation, got {err}"
        );
        // The original lease is untouched.
        assert_eq!(
            store.active_lease("s1").unwrap().unwrap().holder_id,
            "endpoint-1"
        );
    }

    #[test]
    fn preempt_lease_revokes_and_grants_atomically() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let old = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        let mut e1 = test_entry("e1", "s1", "endpoint-1");
        store.append_entry(&mut e1).unwrap();

        let new = store
            .preempt_lease(&Preemption {
                session_id: "s1".into(),
                old_lease_id: old.lease_id.clone(),
                new_holder_id: "web-1".into(),
                new_surface_id: "web-1".into(),
                locus: Locus::Server,
                ttl_ms: 30_000,
                reason: "user active on web".into(),
            })
            .unwrap();
        assert_eq!(new.holder_id, "web-1");
        assert_eq!(new.state, LeaseState::Active as i32);

        // The old lease is REVOKED with preemption recorded for audit, in
        // the same transaction as the grant.
        let old = store.lease(&old.lease_id).unwrap();
        assert_eq!(old.state, LeaseState::Revoked as i32);
        assert_eq!(old.preempted_by, "web-1");

        // The revocation is recorded in the op-log as a SYSTEM_EVENT.
        let log = store.entries_since("s1", 0).unwrap();
        let event = log.last().unwrap();
        assert_eq!(event.kind, EntryKind::SystemEvent as i32);
        assert_eq!(event.payload, b"user active on web");

        // The new holder writes immediately — the session was never
        // writerless at any visible point.
        let mut e2 = test_entry("e2", "s1", "web-1");
        assert_eq!(store.append_entry(&mut e2).unwrap(), 3);
    }

    #[test]
    fn preempt_lease_rejects_already_revoked_lease() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let old = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        store.revoke_lease("s1", "policy violation").unwrap();

        // Preempting a dead lease fails cleanly: no takeover, no new lease.
        let err = store
            .preempt_lease(&Preemption {
                session_id: "s1".into(),
                old_lease_id: old.lease_id.clone(),
                new_holder_id: "web-1".into(),
                new_surface_id: "web-1".into(),
                locus: Locus::Server,
                ttl_ms: 30_000,
                reason: "too late".into(),
            })
            .unwrap_err();
        assert!(matches!(err, StoreError::LeaseNotActive(_)), "{err}");
        assert!(store.active_lease("s1").unwrap().is_none());
        assert_eq!(
            store.lease(&old.lease_id).unwrap().state,
            LeaseState::Revoked as i32
        );
    }

    #[test]
    fn transfer_lease_releases_and_grants_atomically() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let old = store
            .acquire_lease("s1", "endpoint-1", Locus::Endpoint, 30_000)
            .unwrap();
        for i in 1..=2 {
            let mut e = test_entry(&format!("e{i}"), "s1", "endpoint-1");
            store.append_entry(&mut e).unwrap();
        }

        let new = store
            .transfer_lease("s1", "endpoint-1", "server-1", Locus::Server, 30_000, 2)
            .unwrap();
        assert_eq!(new.holder_id, "server-1");
        assert_eq!(new.granted_seq, 2, "pinned to the handoff freeze point");
        assert_eq!(
            store.lease(&old.lease_id).unwrap().state,
            LeaseState::Released as i32
        );
        assert_eq!(
            store.active_lease("s1").unwrap().unwrap().lease_id,
            new.lease_id
        );

        // A transfer naming the wrong current holder fails and changes
        // nothing: the active lease and session writer are untouched.
        let err = store
            .transfer_lease("s1", "mallory", "attacker-1", Locus::Server, 30_000, 2)
            .unwrap_err();
        assert!(matches!(err, StoreError::NotLeaseHolder { .. }));
        assert_eq!(
            store.active_lease("s1").unwrap().unwrap().holder_id,
            "server-1"
        );
    }
}
