//! Server-side control plane: the admin API for lease authority and offline
//! op-log replay. Backed by Postgres (op-log) + Valkey (leases) per ADR 004 —
//! the SQLite fallback is gone from the server entirely. The endpoint daemon
//! is a client of this API; its local SQLite store stays the offline op-log,
//! not the lease source.
//!
//! The server is the single source of truth for session write leases: it
//! grants, renews, preempts, and releases them (Valkey), stamping every
//! timestamp with the SERVER clock (never the client's — device clocks drift
//! and are user-settable). Preemption is a presence signal, not a timestamp
//! race: the latest server-observed activity from a surface wins the lease.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

pub mod identity;
pub mod soul;

pub mod pg_store;
pub mod valkey_lease;

pub use pg_store::PostgresContextStore;
pub use valkey_lease::ValkeyLeaseAuthority;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use fabric_context::clock::now_ms;
use fabric_context::db::ms_to_timestamp;
use fabric_context::{
    ContextStore, LeaseAuthority, ReconcileReport, StoreError, DEFAULT_LEASE_TTL_MS,
    MAX_LEASE_TTL_MS,
};
use fabric_types::context::{ContextEntry, Locus, SessionMeta, SessionState};
use fabric_types::lease::{
    AcquireLeaseRequest, ActiveLeaseRequest, Lease, PreemptRequest, PresenceRequest,
    ReleaseLeaseRequest, RenewLeaseRequest, ReplayRequest,
};
use fabric_types::policy::EndpointPolicy;
use serde_json::{json, Value};
use tracing::{info, warn};

use async_trait::async_trait;

use crate::identity::{identity_middleware, Identity, IdentityContext};
use crate::soul::SoulRegistry;

/// Resolve and validate a caller-supplied TTL. Absent means the default;
/// out-of-range is a 400. The bounds are the context crate's
/// [`DEFAULT_LEASE_TTL_MS`] / [`MAX_LEASE_TTL_MS`] (single definition,
/// shared with the core store's turn-scoped safety-net posture).
fn resolve_ttl(ttl_ms: Option<i64>) -> Result<i64, (StatusCode, Json<Value>)> {
    let ttl = ttl_ms.unwrap_or(DEFAULT_LEASE_TTL_MS);
    if !(1..=MAX_LEASE_TTL_MS).contains(&ttl) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("ttl_ms must be between 1 and {MAX_LEASE_TTL_MS}")
            })),
        ));
    }
    Ok(ttl)
}

/// State shared by every control-plane handler. `pg` is the op-log
/// ([`PostgresContextStore`]); `kv` is the lease authority
/// ([`ValkeyLeaseAuthority`]); `souls` is the SOUL + device registry on the
/// same Postgres pool; `identity` is stamped into `granted_by` on every lease
/// the server issues.
pub struct ControlState {
    pub pg: PostgresContextStore,
    pub kv: ValkeyLeaseAuthority,
    pub souls: SoulRegistry,
    pub identity: String,
    /// The effective policy replayed entries are re-evaluated against
    /// (ADR 006). `None` is dev mode: replay merges without re-evaluation
    /// and logs a warning.
    policy: std::sync::RwLock<Option<EndpointPolicy>>,
}

impl ControlState {
    pub fn new(
        pg: PostgresContextStore,
        kv: ValkeyLeaseAuthority,
        souls: SoulRegistry,
        identity: impl Into<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            kv,
            souls,
            identity: identity.into(),
            policy: std::sync::RwLock::new(None),
        })
    }

    /// Identity from `FABRIC_SERVER_IDENTITY`, defaulting to "fabric-server".
    pub fn from_env(
        pg: PostgresContextStore,
        kv: ValkeyLeaseAuthority,
        souls: SoulRegistry,
    ) -> Arc<Self> {
        let identity =
            std::env::var("FABRIC_SERVER_IDENTITY").unwrap_or_else(|_| "fabric-server".into());
        Self::new(pg, kv, souls, identity)
    }

    /// Install the effective policy used for replay re-evaluation (ADR 006).
    pub fn set_policy(&self, policy: EndpointPolicy) {
        *self.policy.write().expect("policy lock poisoned") = Some(policy);
    }

    /// The current effective policy, if one is loaded.
    pub fn policy(&self) -> Option<EndpointPolicy> {
        self.policy.read().expect("policy lock poisoned").clone()
    }
}

pub fn router(state: Arc<ControlState>) -> Router {
    // Identity is mandatory on every route except the liveness probe (C1):
    // the middleware resolves the IdentityContext server-side from headers
    // and rejects requests missing them before any handler runs.
    let protected = Router::new()
        .route("/identity", get(get_identity))
        .route("/lease/acquire", post(acquire))
        .route("/lease/preempt", post(preempt))
        .route("/lease/renew", post(renew))
        .route("/lease/release", delete(release))
        .route("/lease/active", get(active))
        .route("/presence", post(presence))
        .route("/context/replay", post(replay))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            identity_middleware,
        ));
    Router::new()
        .route("/healthz", get(healthz))
        .merge(protected)
        .with_state(state)
}

/// Bind to `addr` and serve until `shutdown` resolves, then drain in-flight
/// requests before returning.
pub async fn serve(
    state: Arc<ControlState>,
    addr: SocketAddr,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, identity = %state.identity, "control plane listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
}

/// Resolve an optional proto-encoded locus to the enum, defaulting to
/// `Unspecified` for absent or out-of-range values.
fn locus_or_default(locus: Option<i32>) -> Locus {
    locus
        .and_then(|l| Locus::try_from(l).ok())
        .unwrap_or(Locus::Unspecified)
}

/// Map a store error onto an HTTP status. Conflicts and holder mismatches
/// are client-visible; everything else is a 500 with no internals leaked.
fn store_err(e: StoreError) -> Response {
    let (code, msg) = match &e {
        StoreError::LeaseConflict(_) => (StatusCode::CONFLICT, e.to_string()),
        StoreError::SessionNotFound(_)
        | StoreError::LeaseNotFound(_)
        | StoreError::NoActiveLease(_) => (StatusCode::NOT_FOUND, e.to_string()),
        StoreError::NotLeaseHolder { .. } => (StatusCode::FORBIDDEN, e.to_string()),
        StoreError::LeaseExpired(_) | StoreError::LeaseNotActive(_) => {
            (StatusCode::GONE, e.to_string())
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "store error".to_string()),
    };
    (code, Json(json!({ "error": msg }))).into_response()
}

/// Ensure the session row exists so lease FK constraints hold. Idempotent
/// (ON CONFLICT DO NOTHING): first writer to touch a session creates it
/// bound to the caller's identity (user/org/soul), with server-stamped
/// timestamps. Returns the (possibly pre-existing) session after the tenancy
/// check.
async fn ensure_session(
    pg: &PostgresContextStore,
    session_id: &str,
    ctx: &IdentityContext,
) -> Result<SessionMeta, Response> {
    let meta = SessionMeta {
        session_id: session_id.to_string(),
        soul_id: ctx.soul_id.clone(),
        user_id: ctx.user_id.clone(),
        state: SessionState::Active as i32,
        active_lease: String::new(),
        created_at: Some(ms_to_timestamp(now_ms())),
        last_activity: Some(ms_to_timestamp(now_ms())),
        labels: Default::default(),
        org_id: ctx.org_id.clone(),
    };
    pg.create_session(&meta).await.map_err(store_err)?;
    let session = ContextStore::session(pg, session_id)
        .await
        .map_err(store_err)?;
    authorize_session(&session, ctx).map_err(IntoResponse::into_response)?;
    Ok(session)
}

/// Tenancy check (H5): a session belongs to the identity that created it.
/// Sessions with empty user/org (created before tenancy binding) are treated
/// as unowned and stay accessible; all sessions created through
/// [`ensure_session`] are bound to the creator.
fn authorize_session(
    session: &SessionMeta,
    ctx: &IdentityContext,
) -> Result<(), (StatusCode, Json<Value>)> {
    let user_ok = session.user_id.is_empty() || session.user_id == ctx.user_id;
    let org_ok = session.org_id.is_empty() || session.org_id == ctx.org_id;
    if user_ok && org_ok {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": format!("session {} belongs to a different identity", session.session_id)
        })),
    ))
}

/// Tenancy check for handlers operating on an existing session (release,
/// renew, active): authorize when the session exists; when it does not, let
/// the store surface its own 404.
async fn authorize_if_session_exists(
    pg: &PostgresContextStore,
    session_id: &str,
    ctx: &IdentityContext,
) -> Result<(), Response> {
    match ContextStore::session(pg, session_id).await {
        Ok(session) => {
            authorize_session(&session, ctx).map_err(IntoResponse::into_response)?;
            Ok(())
        }
        Err(StoreError::SessionNotFound(_)) => Ok(()),
        Err(e) => Err(store_err(e)),
    }
}

/// Stamp the lease with the server's identity and persist the attribution.
/// Timestamps already came from the server clock inside the Valkey authority
/// — so the returned lease is fully server-stamped.
async fn stamp_granted_by(
    kv: &ValkeyLeaseAuthority,
    lease: &mut Lease,
    identity: &str,
) -> Result<(), StoreError> {
    kv.set_granted_by(&lease.lease_id, identity).await?;
    lease.granted_by = identity.to_string();
    Ok(())
}

async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Returns the server-resolved identity context (ADR 007). Demonstrates the
/// identity extractor: the client supplies no identity fields in the body;
/// all four are derived from headers + registry.
async fn get_identity(Identity(ctx): Identity) -> Json<IdentityContext> {
    Json(ctx)
}

/// Grant a write lease, server-stamped. 409 while another holder's
/// unexpired lease is active — preemption (presence) is the way to take
/// over a live session, never a raw acquire race. The holder is the
/// caller's authenticated device identity, never a body field.
async fn acquire(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Json(req): Json<AcquireLeaseRequest>,
) -> Result<Json<Lease>, Response> {
    ensure_session(&state.pg, &req.session_id, &ctx).await?;
    let ttl = resolve_ttl(req.ttl_ms).map_err(IntoResponse::into_response)?;
    let mut lease = LeaseAuthority::acquire_lease(
        &state.kv,
        &req.session_id,
        &ctx.holder_id,
        locus_or_default(req.locus),
        ttl,
    )
    .await
    .map_err(store_err)?;
    stamp_granted_by(&state.kv, &mut lease, &state.identity)
        .await
        .map_err(store_err)?;
    info!(session = %req.session_id, holder = %ctx.holder_id, lease = %lease.lease_id, "lease granted");
    Ok(Json(lease))
}

/// Presence-driven preemption: the surface with the latest server-observed
/// activity takes the lease. The outgoing lease is revoked atomically (Valkey
/// Lua) and a fresh server-stamped lease is granted to the caller's device —
/// so a failure can never leave the session writerless. If the caller
/// already holds the lease this is a no-op returning the current lease.
async fn preempt(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Json(req): Json<PreemptRequest>,
) -> Result<Json<Lease>, Response> {
    ensure_session(&state.pg, &req.session_id, &ctx).await?;
    let locus = locus_or_default(req.locus);
    let ttl = resolve_ttl(req.ttl_ms).map_err(IntoResponse::into_response)?;

    if let Some(old) = LeaseAuthority::active_lease(&state.kv, &req.session_id)
        .await
        .map_err(store_err)?
    {
        if old.holder_id == ctx.holder_id {
            return Ok(Json(old));
        }
        let mut lease = state
            .kv
            .preempt(&req.session_id, &ctx.holder_id, locus, ttl)
            .await
            .map_err(store_err)?;
        stamp_granted_by(&state.kv, &mut lease, &state.identity)
            .await
            .map_err(store_err)?;
        info!(
            session = %req.session_id,
            old_holder = %old.holder_id,
            new_holder = %ctx.holder_id,
            "lease preempted"
        );
        return Ok(Json(lease));
    }

    let mut lease =
        LeaseAuthority::acquire_lease(&state.kv, &req.session_id, &ctx.holder_id, locus, ttl)
            .await
            .map_err(store_err)?;
    stamp_granted_by(&state.kv, &mut lease, &state.identity)
        .await
        .map_err(store_err)?;
    Ok(Json(lease))
}

/// Extend an ACTIVE lease's expiry. The caller's device identity must match
/// the lease holder; the new expiry is stamped with the server clock.
async fn renew(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Json(req): Json<RenewLeaseRequest>,
) -> Result<Json<Lease>, Response> {
    let ttl = resolve_ttl(req.ttl_ms).map_err(IntoResponse::into_response)?;
    // Tenancy: the lease's session must belong to the caller's identity.
    let existing = LeaseAuthority::lease(&state.kv, &req.lease_id)
        .await
        .map_err(store_err)?;
    authorize_if_session_exists(&state.pg, &existing.session_id, &ctx).await?;
    let lease = state
        .kv
        .renew_lease(&req.lease_id, &ctx.holder_id, ttl)
        .await
        .map_err(store_err)?;
    Ok(Json(lease))
}

/// Release the lease at the end of a turn. The caller's device identity must
/// match the holder. 204 on success; the session stays ACTIVE without a
/// writer.
async fn release(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Json(req): Json<ReleaseLeaseRequest>,
) -> Result<StatusCode, Response> {
    authorize_if_session_exists(&state.pg, &req.session_id, &ctx).await?;
    LeaseAuthority::release_lease(&state.kv, &req.session_id, &ctx.holder_id)
        .await
        .map_err(store_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The session's ACTIVE lease, or 404 when there is no writer.
async fn active(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Query(q): Query<ActiveLeaseRequest>,
) -> Result<Json<Lease>, Response> {
    authorize_if_session_exists(&state.pg, &q.session_id, &ctx).await?;
    LeaseAuthority::active_lease(&state.kv, &q.session_id)
        .await
        .map_err(store_err)?
        .map(Json)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("no active lease for session {}", q.session_id) })),
            )
                .into_response()
        })
}

/// A surface reports user activity. Latest server-observed activity wins the
/// lease: if the reporting surface is not the current holder, the lease is
/// preempted to it. This IS the preemption mechanism — presence, not clock
/// races. The reporting surface is the caller's authenticated device
/// identity. Returns the lease the surface now holds (or already held).
async fn presence(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Json(req): Json<PresenceRequest>,
) -> Result<Json<Lease>, Response> {
    preempt(
        State(state),
        Identity(ctx.clone()),
        Json(PreemptRequest {
            session_id: req.session_id,
            new_holder_id: String::new(),
            reason: format!("presence from {}", ctx.holder_id),
            locus: req.locus,
            ttl_ms: None,
        }),
    )
    .await
}

/// A throwaway in-memory replica holding only the replayed entries, fed to
/// [`fabric_context::reconcile`] as the "remote" side. Reconcile reads only
/// `entries_since` from the remote; every other method is unreachable from
/// that path and returns an error.
struct EntryStaging(Vec<ContextEntry>);

#[async_trait]
impl ContextStore for EntryStaging {
    async fn entries_since(
        &self,
        session_id: &str,
        after_seq: u64,
    ) -> Result<Vec<ContextEntry>, StoreError> {
        let mut out: Vec<ContextEntry> = self
            .0
            .iter()
            .filter(|e| e.session_id == session_id && e.seq > after_seq)
            .cloned()
            .collect();
        out.sort_by_key(|e| e.seq);
        Ok(out)
    }

    async fn append_entry(&self, _entry: &mut ContextEntry) -> Result<u64, StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
    async fn insert_entry_raw(&self, _entry: &ContextEntry) -> Result<(), StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
    async fn entry_by_id(&self, _entry_id: &str) -> Result<Option<ContextEntry>, StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
    async fn entry_at_seq(
        &self,
        _session_id: &str,
        _seq: u64,
    ) -> Result<Option<ContextEntry>, StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
    async fn head_seq(&self, _session_id: &str) -> Result<u64, StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
    async fn reassign_seq(&self, _entry_id: &str, _new_seq: u64) -> Result<(), StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
    async fn session(&self, _session_id: &str) -> Result<SessionMeta, StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
    async fn set_session_state(&self, _session_id: &str, _state: i32) -> Result<(), StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
    async fn set_disposition(&self, _entry_id: &str, _disposition: &str) -> Result<(), StoreError> {
        Err(StoreError::Valkey("staging replica is read-only".into()))
    }
}

/// Offline-reconnect ingest: an endpoint replays its local op-log after an
/// offline stretch. Entries were already validated by the endpoint's locus,
/// so they merge through the deterministic `reconcile` path: duplicates
/// skipped, seq collisions resolved by (received_at, entry_id) — where
/// `received_at` is stamped HERE with the server clock before merge, never
/// trusted from the client (ADR 006). Replayed entries are re-evaluated
/// against the current effective policy; DENY matches are quarantined.
/// Returns the reconcile report.
async fn replay(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Json(req): Json<ReplayRequest>,
) -> Result<Json<ReconcileReport>, Response> {
    // Validate the seq range before touching the store: seq 0 is not a
    // valid op-log position (seqs start at 1) and anything above i64::MAX
    // cannot be represented in Postgres' BIGINT column.
    for entry in &req.entries {
        if entry.seq == 0 || entry.seq > i64::MAX as u64 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!(
                        "entry {} has out-of-range seq {}; must be 1..=i64::MAX",
                        entry.entry_id, entry.seq
                    )
                })),
            )
                .into_response());
        }
    }

    ensure_session(&state.pg, &req.session_id, &ctx).await?;

    // ADR 006: the server clock is authoritative. Overwrite received_at on
    // every replayed entry BEFORE the merge — a client-supplied value could
    // forge priority in (received_at, entry_id) conflict resolution.
    let received_ms = now_ms();

    let mut staged: Vec<ContextEntry> = Vec::with_capacity(req.entries.len());
    for entry in &req.entries {
        // created_at is an untrusted endpoint claim: flag insane clocks
        // (pre-2020 or far-future) for the audit log. ADR 006 is
        // "accept everything" — warn, never reject; merge ordering uses
        // the server-stamped received_at, so a garbage created_at cannot
        // forge priority.
        let created_ms = entry.created_at.as_ref().map_or(0, |t| {
            t.seconds
                .saturating_mul(1000)
                .saturating_add(i64::from(t.nanos) / 1_000_000)
        });
        if !fabric_context::clock::is_timestamp_sane(created_ms) {
            warn!(
                session = %req.session_id,
                entry = %entry.entry_id,
                created_ms,
                "replayed entry has insane created_at clock; accepted per ADR 006"
            );
        }
        let mut stamped = entry.clone();
        stamped.received_at = Some(ms_to_timestamp(received_ms));
        staged.push(stamped);
    }
    let staging = EntryStaging(staged);

    // Re-evaluate replayed entries against the CURRENT effective policy
    // (ADR 006): what was legal under the write-time policy version may now
    // be denied. Dev mode (no policy loaded) merges without re-evaluation.
    let policy = state.policy();
    if policy.is_none() {
        warn!(
            session = %req.session_id,
            "no policy loaded; replaying without policy re-evaluation (dev mode)"
        );
    }
    let report = fabric_context::reconcile(&state.pg, &staging, &req.session_id, policy.as_ref())
        .await
        .map_err(store_err)?;
    info!(
        session = %req.session_id,
        applied = report.applied,
        duplicates = report.duplicates,
        conflicts = report.conflicts.len(),
        violations = report.policy_violations.len(),
        "offline op-log replayed"
    );
    Ok(Json(report))
}
