//! Postgres-backed op-log (ADR 004). The server's [`ContextStore`] runs against
//! Postgres: real transactions for multi-step operations, a real connection
//! pool, and row-level locking (`SELECT ... FOR UPDATE`) instead of the
//! endpoint's mutex-guarded SQLite connection.
//!
//! The write lease lives in Valkey ([`crate::valkey_lease`]); on this side
//! `append_entry` enforces only the session-ACTIVE invariant and the seq
//! assignment. Lease verification is the handler's job — it calls
//! [`LeaseAuthority::verify_writer`] on the Valkey authority before mutating
//! the op-log. Splitting the two stores (ADR 004) means there is no single
//! cross-store transaction; the lease is the gate, exactly as on the endpoint,
//! and the op-log trusts the caller once past it.

#![cfg(feature = "server-store")]

use async_trait::async_trait;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::{query, Row};
use tracing::instrument;

use fabric_context::clock::now_ms;
use fabric_context::db::{ms_to_timestamp, Result, StoreError};
use fabric_types::context::{ContextEntry, EntryKind, SessionMeta, SessionState};

/// Embed the migration SQL so the binary is self-contained: `make proto`-style
/// build steps and a live `DATABASE_URL` at compile time are not required. The
/// file is the same one checked into `migrations/` for `sqlx migrate`.
const SCHEMA: &str = include_str!("../migrations/20260728000001_init.sql");

/// Convert a protobuf timestamp into epoch milliseconds. `received_at` may be
/// absent (direct local appends carry none — ADR 006).
fn ts_to_ms(ts: Option<&pbjson_types::Timestamp>) -> i64 {
    ts.map(|t| {
        t.seconds
            .saturating_mul(1000)
            .saturating_add(i64::from(t.nanos) / 1_000_000)
    })
    .unwrap_or(0)
}

/// The stable name for an entry kind, written into the denormalized
/// `entry_type` column for human-friendly queries.
fn entry_kind_name(kind: i32) -> String {
    EntryKind::try_from(kind)
        .map(|k| k.as_str_name().to_string())
        .unwrap_or_default()
}

/// Human-readable session-state name for errors. Replicates the
/// crate-private `session_state_name` in `fabric_context::db` without
/// exporting it (the helper there is `pub(crate)`).
fn state_name(state: i32) -> String {
    SessionState::try_from(state)
        .map(|s| s.as_str_name().to_string())
        .unwrap_or_else(|_| format!("UNKNOWN({state})"))
}

/// Server-side context store backed by a Postgres connection pool.
///
/// Cheap to clone (an `Arc` inside [`PgPool`]) and `Send + Sync`; handlers hold
/// one alongside the [`crate::valkey_lease::ValkeyLeaseAuthority`].
#[derive(Clone)]
pub struct PostgresContextStore {
    pool: PgPool,
}

impl PostgresContextStore {
    /// Connect to `url`, run the embedded schema, and return a pool of
    /// `pool_size` connections (default 16 — see [`Self::connect`]).
    pub async fn connect_with(url: &str, pool_size: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_size)
            .connect(url)
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?;
        // Idempotent: CREATE TABLE IF NOT EXISTS. `raw_sql` runs the whole
        // multi-statement init (tables + indexes) — `query()` only prepares a
        // single statement. Equivalent to `sqlx migrate` without a
        // compile-time `DATABASE_URL` or the macros feature.
        sqlx::raw_sql(SCHEMA)
            .execute(&pool)
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?;
        Ok(Self { pool })
    }

    /// Connect with the default pool size (16).
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_with(url, 16).await
    }

    /// Wrap an existing pool (e.g. shared with the SOUL registry). The schema
    /// is assumed already migrated by the pool's owner.
    pub fn from_pool(pool: PgPool) -> Result<Self> {
        Ok(Self { pool })
    }

    /// The raw pool, shared with other server stores (SOUL registry).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Lightweight liveness probe for health/readiness endpoints.
    pub async fn ping(&self) -> Result<()> {
        query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?;
        Ok(())
    }

    /// Create a session row, idempotent on `session_id`. First writer to touch
    /// a session binds it to the caller's identity (user/org/soul); re-creates
    /// are no-ops so offline replicas converge. Mirrors the endpoint's
    /// `CREATE TABLE IF NOT EXISTS`-backed `INSERT OR IGNORE` semantics.
    pub async fn create_session(&self, meta: &SessionMeta) -> Result<()> {
        query(
            "INSERT INTO sessions (session_id, soul_id, user_id, org_id, state, created_at_ms, updated_at_ms)
             VALUES ($1, $2, $3, $4, $5, $6, $6)
             ON CONFLICT (session_id) DO NOTHING",
        )
        .bind(&meta.session_id)
        .bind(&meta.soul_id)
        .bind(&meta.user_id)
        .bind(&meta.org_id)
        .bind(meta.state)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?;
        Ok(())
    }

    fn session_row(row: &PgRow) -> SessionMeta {
        SessionMeta {
            session_id: row.get("session_id"),
            soul_id: row.get("soul_id"),
            user_id: row.get("user_id"),
            state: row.get("state"),
            // Lease authority is Valkey; the session row carries no active
            // lease pointer (the endpoint's SQLite does, but the server does
            // not — Valkey IS the authority).
            active_lease: String::new(),
            created_at: Some(ms_to_timestamp(row.get("created_at_ms"))),
            last_activity: Some(ms_to_timestamp(row.get("updated_at_ms"))),
            labels: Default::default(),
            org_id: row.get("org_id"),
        }
    }

    fn entry_row(row: &PgRow) -> ContextEntry {
        ContextEntry {
            entry_id: row.get("entry_id"),
            session_id: row.get("session_id"),
            seq: row.get::<i64, _>("seq") as u64,
            kind: row.get("kind"),
            payload: row.get("payload"),
            lease_holder: row.get("lease_holder"),
            policy_version: row.get("policy_version"),
            locus: row.get("locus"),
            created_at: Some(ms_to_timestamp(row.get("created_at_ms"))),
            received_at: row
                .get::<Option<i64>, _>("received_at_ms")
                .map(ms_to_timestamp),
            disposition: row.get("disposition"),
        }
    }
}

#[async_trait]
impl fabric_context::store::ContextStore for PostgresContextStore {
    /// Append in a single transaction: lock the session row (`FOR UPDATE`),
    /// verify ACTIVE, assign `head_seq + 1`, and insert. The writer's clock
    /// stamps `created_at` (forging is impossible — ADR 006); `received_at`
    /// is unset for a direct local append. The lease is verified upstream by
    /// the handler against the Valkey authority.
    #[instrument(skip(self, entry), fields(session = %entry.session_id))]
    async fn append_entry(&self, entry: &mut ContextEntry) -> Result<u64> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?;

        let row: PgRow = query("SELECT state FROM sessions WHERE session_id = $1 FOR UPDATE")
            .bind(&entry.session_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?
            .ok_or_else(|| StoreError::SessionNotFound(entry.session_id.clone()))?;
        let state: i32 = row.get("state");
        if state != SessionState::Active as i32 {
            return Err(StoreError::SessionNotActive {
                session_id: entry.session_id.clone(),
                state: state_name(state),
            });
        }

        let head: i64 = query(
            "SELECT COALESCE(MAX(seq), 0) AS head FROM context_entries WHERE session_id = $1",
        )
        .bind(&entry.session_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?
        .get("head");
        let seq = head as u64 + 1;
        entry.seq = seq;
        entry.created_at = Some(ms_to_timestamp(now_ms()));
        entry.received_at = None;

        query(
            "INSERT INTO context_entries
             (entry_id, session_id, seq, entry_type, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&entry.entry_id)
        .bind(&entry.session_id)
        .bind(seq as i64)
        .bind(entry_kind_name(entry.kind))
        .bind(entry.kind)
        .bind(&entry.payload)
        .bind(&entry.lease_holder)
        .bind(&entry.policy_version)
        .bind(entry.locus)
        .bind(now_ms())
        .bind::<Option<i64>>(entry.received_at.as_ref().map(|t| ts_to_ms(Some(t))))
        .bind(&entry.disposition)
        .execute(&mut *tx)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?;

        query("UPDATE sessions SET updated_at_ms = $1 WHERE session_id = $2")
            .bind(now_ms())
            .bind(&entry.session_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?;
        Ok(seq)
    }

    /// Insert an entry verbatim, bypassing the lease gate. Replication path
    /// only (reconcile / catch-up): the entries were already validated by the
    /// writer's locus. Idempotent on `(session_id, seq)` (replay is a no-op).
    async fn insert_entry_raw(&self, entry: &ContextEntry) -> Result<()> {
        query(
            "INSERT INTO context_entries
             (entry_id, session_id, seq, entry_type, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (session_id, seq) DO NOTHING",
        )
        .bind(&entry.entry_id)
        .bind(&entry.session_id)
        .bind(entry.seq as i64)
        .bind(entry_kind_name(entry.kind))
        .bind(entry.kind)
        .bind(&entry.payload)
        .bind(&entry.lease_holder)
        .bind(&entry.policy_version)
        .bind(entry.locus)
        .bind(ts_to_ms(entry.created_at.as_ref()))
        .bind::<Option<i64>>(entry.received_at.as_ref().map(|t| ts_to_ms(Some(t))))
        .bind(&entry.disposition)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?;
        Ok(())
    }

    async fn entries_since(&self, session_id: &str, after_seq: u64) -> Result<Vec<ContextEntry>> {
        let rows = query(
            "SELECT entry_id, session_id, seq, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition
             FROM context_entries WHERE session_id = $1 AND seq > $2 ORDER BY seq ASC",
        )
        .bind(session_id)
        .bind(after_seq as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?;
        Ok(rows.iter().map(Self::entry_row).collect())
    }

    async fn entry_by_id(&self, entry_id: &str) -> Result<Option<ContextEntry>> {
        let row = query(
            "SELECT entry_id, session_id, seq, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition
             FROM context_entries WHERE entry_id = $1",
        )
        .bind(entry_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?;
        Ok(row.as_ref().map(Self::entry_row))
    }

    async fn entry_at_seq(&self, session_id: &str, seq: u64) -> Result<Option<ContextEntry>> {
        let row = query(
            "SELECT entry_id, session_id, seq, kind, payload, lease_holder, policy_version, locus, created_at_ms, received_at_ms, disposition
             FROM context_entries WHERE session_id = $1 AND seq = $2",
        )
        .bind(session_id)
        .bind(seq as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?;
        Ok(row.as_ref().map(Self::entry_row))
    }

    async fn head_seq(&self, session_id: &str) -> Result<u64> {
        let head: i64 = query(
            "SELECT COALESCE(MAX(seq), 0) AS head FROM context_entries WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?
        .get("head");
        Ok(head as u64)
    }

    async fn reassign_seq(&self, entry_id: &str, new_seq: u64) -> Result<()> {
        let n = query("UPDATE context_entries SET seq = $1 WHERE entry_id = $2")
            .bind(new_seq as i64)
            .bind(entry_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?
            .rows_affected();
        if n == 0 {
            return Err(StoreError::LeaseNotFound(entry_id.to_string()));
        }
        Ok(())
    }

    async fn session(&self, session_id: &str) -> Result<SessionMeta> {
        let row = query(
            "SELECT session_id, soul_id, user_id, org_id, state, created_at_ms, updated_at_ms
             FROM sessions WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StoreError::Postgres(e.to_string()))?
        .ok_or_else(|| StoreError::SessionNotFound(session_id.to_string()))?;
        Ok(Self::session_row(&row))
    }

    async fn set_session_state(&self, session_id: &str, state: i32) -> Result<()> {
        let n = query("UPDATE sessions SET state = $1, updated_at_ms = $2 WHERE session_id = $3")
            .bind(state)
            .bind(now_ms())
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?
            .rows_affected();
        if n == 0 {
            return Err(StoreError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    async fn set_disposition(&self, entry_id: &str, disposition: &str) -> Result<()> {
        let n = query("UPDATE context_entries SET disposition = $1 WHERE entry_id = $2")
            .bind(disposition)
            .bind(entry_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::Postgres(e.to_string()))?
            .rows_affected();
        if n == 0 {
            return Err(StoreError::LeaseNotFound(entry_id.to_string()));
        }
        Ok(())
    }
}
