//! Async abstraction over the context-plane store. The endpoint runs the
//! SQLite backend ([`SqliteContextStore`]); server-side deployments can back
//! the same trait with a multi-replica database (e.g. Postgres) without
//! touching reconcile or handoff. The surface is exactly what the rest of
//! this crate uses: op-log append/read plus lease lifecycle.
//!
//! Uses the `async-trait` crate, matching the rest of the workspace
//! (`core/tools::Tool`), so the futures are `Send` and the trait stays
//! object-safe if a boxed backend is ever needed.

use async_trait::async_trait;
use fabric_types::context::{ContextEntry, Locus, SessionMeta};
use fabric_types::lease::Lease;
use tokio::task::spawn_blocking;

use crate::db::{Result, SqliteContextStore};

/// The context store surface used by reconcile, handoff, and catch-up.
///
/// Implementations must preserve the single-writer invariants documented on
/// [`SqliteContextStore`]: `append_entry` enforces session-ACTIVE and the
/// write lease; `insert_entry_raw` bypasses the lease gate and is only for
/// replicas merging already-validated entries (reconcile, catch-up).
#[async_trait]
pub trait ContextStore: Send + Sync {
    /// Append an entry to the op-log, assigning the next seq. Enforces the
    /// session state and write lease. Returns the assigned seq.
    async fn append_entry(&self, entry: &mut ContextEntry) -> Result<u64>;

    /// Insert an entry as-is, bypassing lease checks. Replication path only
    /// (reconcile / catch-up); writers must use [`ContextStore::append_entry`].
    async fn insert_entry_raw(&self, entry: &ContextEntry) -> Result<()>;

    /// All entries with seq > `after_seq`, in order.
    async fn entries_since(&self, session_id: &str, after_seq: u64) -> Result<Vec<ContextEntry>>;

    async fn entry_by_id(&self, entry_id: &str) -> Result<Option<ContextEntry>>;

    async fn entry_at_seq(&self, session_id: &str, seq: u64) -> Result<Option<ContextEntry>>;

    /// The highest seq in the session's op-log (0 when empty).
    async fn head_seq(&self, session_id: &str) -> Result<u64>;

    /// Reassign the seq of an existing entry. Conflict resolution moves the
    /// loser to the tail of the log.
    async fn reassign_seq(&self, entry_id: &str, new_seq: u64) -> Result<()>;

    /// Acquire a turn-scoped write lease. Fails with LeaseConflict while
    /// another holder's unexpired lease is active.
    async fn acquire_lease(
        &self,
        session_id: &str,
        holder_id: &str,
        locus: Locus,
        ttl_ms: i64,
    ) -> Result<Lease>;

    /// Release the turn-scoped lease at the end of an agent turn.
    async fn release_lease(&self, session_id: &str, holder_id: &str) -> Result<()>;

    /// Fetch a lease by id.
    async fn lease(&self, lease_id: &str) -> Result<Lease>;

    /// The session's ACTIVE lease, if any.
    async fn active_lease(&self, session_id: &str) -> Result<Option<Lease>>;

    /// Fetch session metadata.
    async fn session(&self, session_id: &str) -> Result<SessionMeta>;

    /// Verify that `writer` currently holds the session's write lease.
    async fn verify_writer(&self, session_id: &str, writer: &str) -> Result<Lease>;

    /// Pin an existing lease's granted_seq (handoff freeze point).
    async fn set_granted_seq(&self, lease_id: &str, granted_seq: u64) -> Result<()>;

    /// Raw session-state setter for the handoff protocol (HANDED_OFF, then
    /// back to ACTIVE on ack). Validated lifecycle transitions stay on the
    /// concrete store.
    async fn set_session_state(&self, session_id: &str, state: i32) -> Result<()>;
}

/// Run a blocking store call on the blocking thread pool and flatten the
/// join result into the store's [`Result`].
async fn run<T>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T>
where
    T: Send + 'static,
{
    spawn_blocking(f).await?
}

#[async_trait]
impl ContextStore for SqliteContextStore {
    async fn append_entry(&self, entry: &mut ContextEntry) -> Result<u64> {
        let store = self.clone();
        let mut owned = entry.clone();
        let (seq, owned) = run(move || {
            let seq = store.append_entry(&mut owned)?;
            Ok((seq, owned))
        })
        .await?;
        *entry = owned;
        Ok(seq)
    }

    async fn insert_entry_raw(&self, entry: &ContextEntry) -> Result<()> {
        let store = self.clone();
        let entry = entry.clone();
        run(move || store.insert_entry_raw(&entry)).await
    }

    async fn entries_since(&self, session_id: &str, after_seq: u64) -> Result<Vec<ContextEntry>> {
        let store = self.clone();
        let session_id = session_id.to_string();
        run(move || store.entries_since(&session_id, after_seq)).await
    }

    async fn entry_by_id(&self, entry_id: &str) -> Result<Option<ContextEntry>> {
        let store = self.clone();
        let entry_id = entry_id.to_string();
        run(move || store.entry_by_id(&entry_id)).await
    }

    async fn entry_at_seq(&self, session_id: &str, seq: u64) -> Result<Option<ContextEntry>> {
        let store = self.clone();
        let session_id = session_id.to_string();
        run(move || store.entry_at_seq(&session_id, seq)).await
    }

    async fn head_seq(&self, session_id: &str) -> Result<u64> {
        let store = self.clone();
        let session_id = session_id.to_string();
        run(move || store.head_seq(&session_id)).await
    }

    async fn reassign_seq(&self, entry_id: &str, new_seq: u64) -> Result<()> {
        let store = self.clone();
        let entry_id = entry_id.to_string();
        run(move || store.reassign_seq(&entry_id, new_seq)).await
    }

    async fn acquire_lease(
        &self,
        session_id: &str,
        holder_id: &str,
        locus: Locus,
        ttl_ms: i64,
    ) -> Result<Lease> {
        let store = self.clone();
        let session_id = session_id.to_string();
        let holder_id = holder_id.to_string();
        run(move || store.acquire_lease(&session_id, &holder_id, locus, ttl_ms)).await
    }

    async fn release_lease(&self, session_id: &str, holder_id: &str) -> Result<()> {
        let store = self.clone();
        let session_id = session_id.to_string();
        let holder_id = holder_id.to_string();
        run(move || store.release_lease(&session_id, &holder_id)).await
    }

    async fn lease(&self, lease_id: &str) -> Result<Lease> {
        let store = self.clone();
        let lease_id = lease_id.to_string();
        run(move || store.lease(&lease_id)).await
    }

    async fn active_lease(&self, session_id: &str) -> Result<Option<Lease>> {
        let store = self.clone();
        let session_id = session_id.to_string();
        run(move || store.active_lease(&session_id)).await
    }

    async fn session(&self, session_id: &str) -> Result<SessionMeta> {
        let store = self.clone();
        let session_id = session_id.to_string();
        run(move || store.session(&session_id)).await
    }

    async fn verify_writer(&self, session_id: &str, writer: &str) -> Result<Lease> {
        let store = self.clone();
        let session_id = session_id.to_string();
        let writer = writer.to_string();
        run(move || store.verify_writer(&session_id, &writer)).await
    }

    async fn set_granted_seq(&self, lease_id: &str, granted_seq: u64) -> Result<()> {
        let store = self.clone();
        let lease_id = lease_id.to_string();
        run(move || store.set_granted_seq(&lease_id, granted_seq)).await
    }

    async fn set_session_state(&self, session_id: &str, state: i32) -> Result<()> {
        let store = self.clone();
        let session_id = session_id.to_string();
        run(move || store.set_session_state(&session_id, state)).await
    }
}
