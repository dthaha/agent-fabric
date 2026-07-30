//! Server-side control plane: the admin API for policy CRUD, audit queries, and
//! the SOUL home (memory plane source of truth). Serves the admin console.
//!
//! Phase C lands the lease authority here. The server is the single source of
//! truth for session write leases: it grants, renews, preempts, and releases
//! leases, stamping every timestamp with the SERVER clock (never the client's
//! — device clocks drift and are user-settable). Preemption is a presence
//! signal, not a timestamp race: the latest server-observed activity from a
//! surface wins the lease. The endpoint daemon is a client of this API; its
//! local SQLite store stays the offline op-log, not the lease source.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

pub mod identity;
pub mod soul;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use fabric_context::clock::now_ms;
use fabric_context::db::ms_to_timestamp;
use fabric_context::{
    ContextStore, LeaseAuthority, ReconcileReport, SqliteContextStore, StoreError,
};
use fabric_types::context::{Locus, SessionMeta, SessionState};
use fabric_types::lease::{
    AcquireLeaseRequest, ActiveLeaseRequest, Lease, PreemptRequest, PresenceRequest,
    ReleaseLeaseRequest, RenewLeaseRequest, ReplayRequest,
};
use fabric_types::policy::EndpointPolicy;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::identity::{identity_middleware, Identity, IdentityContext};
use crate::soul::SoulRegistry;

/// Default TTL for leases granted via preempt/presence, where the caller does
/// not specify one. Matches the turn-scoped safety-net posture of the core
/// store: generous, only firing when a holder crashes without releasing.
const DEFAULT_LEASE_TTL_MS: i64 = 3_600_000;

/// Maximum TTL a caller may request (1 hour). Unbounded TTLs would let a
/// crashed holder lock a session forever.
const MAX_TTL_MS: i64 = 3_600_000;

/// Resolve and validate a caller-supplied TTL. Absent means the default;
/// out-of-range is a 400.
fn resolve_ttl(ttl_ms: Option<i64>) -> Result<i64, (StatusCode, Json<Value>)> {
    let ttl = ttl_ms.unwrap_or(DEFAULT_LEASE_TTL_MS);
    if !(1..=MAX_TTL_MS).contains(&ttl) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("ttl_ms must be between 1 and {MAX_TTL_MS}")
            })),
        ));
    }
    Ok(ttl)
}

/// State shared by every control-plane handler. The store is the lease
/// authority; `souls` is the SOUL + device registry (ADR 007); `identity`
/// is stamped into `granted_by` on every lease the server issues.
pub struct ControlState {
    pub store: SqliteContextStore,
    pub souls: SoulRegistry,
    pub identity: String,
    /// The effective policy replayed entries are re-evaluated against
    /// (ADR 006). `None` is dev mode: replay merges without re-evaluation
    /// and logs a warning.
    policy: std::sync::RwLock<Option<EndpointPolicy>>,
}

impl ControlState {
    pub fn new(
        store: SqliteContextStore,
        souls: SoulRegistry,
        identity: impl Into<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            souls,
            identity: identity.into(),
            policy: std::sync::RwLock::new(None),
        })
    }

    /// Identity from `FABRIC_SERVER_IDENTITY`, defaulting to "fabric-server".
    pub fn from_env(store: SqliteContextStore, souls: SoulRegistry) -> Arc<Self> {
        let identity =
            std::env::var("FABRIC_SERVER_IDENTITY").unwrap_or_else(|_| "fabric-server".into());
        Self::new(store, souls, identity)
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
        .route("/lease/release", axum::routing::delete(release))
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
/// (INSERT OR IGNORE): first writer to touch a session creates it bound to
/// the caller's identity (user/org/soul), with server-stamped timestamps.
/// Returns the (possibly pre-existing) session after the tenancy check.
async fn ensure_session(
    store: &SqliteContextStore,
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
    let store2 = store.clone();
    tokio::task::spawn_blocking(move || store2.create_session(&meta))
        .await
        .map_err(StoreError::from)
        .map_err(store_err)?
        .map_err(store_err)?;
    let store2 = store.clone();
    let session_id = session_id.to_string();
    let session = tokio::task::spawn_blocking(move || store2.session(&session_id))
        .await
        .map_err(StoreError::from)
        .map_err(store_err)?
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
    store: &SqliteContextStore,
    session_id: &str,
    ctx: &IdentityContext,
) -> Result<(), Response> {
    let store2 = store.clone();
    let session_id = session_id.to_string();
    let session = tokio::task::spawn_blocking(move || store2.session(&session_id))
        .await
        .map_err(StoreError::from)
        .map_err(store_err)?;
    match session {
        Ok(session) => {
            authorize_session(&session, ctx).map_err(IntoResponse::into_response)?;
            Ok(())
        }
        Err(StoreError::SessionNotFound(_)) => Ok(()),
        Err(e) => Err(store_err(e)),
    }
}

/// Stamp the lease with the server's identity and persist the attribution.
/// Timestamps already came from `now_ms` inside the store — which runs on
/// the server — so the returned lease is fully server-stamped.
async fn stamp_granted_by(
    store: &SqliteContextStore,
    lease: &mut Lease,
    identity: &str,
) -> Result<(), StoreError> {
    let store = store.clone();
    let lease_id = lease.lease_id.clone();
    let identity = identity.to_string();
    tokio::task::spawn_blocking({
        let identity = identity.clone();
        move || store.set_granted_by(&lease_id, &identity)
    })
    .await??;
    lease.granted_by = identity;
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
    ensure_session(&state.store, &req.session_id, &ctx).await?;
    let ttl = resolve_ttl(req.ttl_ms).map_err(IntoResponse::into_response)?;
    let mut lease = LeaseAuthority::acquire_lease(
        &state.store,
        &req.session_id,
        &ctx.holder_id,
        locus_or_default(req.locus),
        ttl,
    )
    .await
    .map_err(store_err)?;
    stamp_granted_by(&state.store, &mut lease, &state.identity)
        .await
        .map_err(store_err)?;
    info!(session = %req.session_id, holder = %ctx.holder_id, lease = %lease.lease_id, "lease granted");
    Ok(Json(lease))
}

/// Presence-driven preemption: the surface with the latest server-observed
/// activity takes the lease. The outgoing lease is revoked with
/// `preempted_by` recorded for audit and a fresh server-stamped lease is
/// granted to the caller's device — atomically, in one store transaction,
/// so a failure can never leave the session writerless. If the caller
/// already holds the lease this is a no-op returning the current lease.
async fn preempt(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Json(req): Json<PreemptRequest>,
) -> Result<Json<Lease>, Response> {
    ensure_session(&state.store, &req.session_id, &ctx).await?;
    let locus = locus_or_default(req.locus);
    let ttl = resolve_ttl(req.ttl_ms).map_err(IntoResponse::into_response)?;

    if let Some(old) = LeaseAuthority::active_lease(&state.store, &req.session_id)
        .await
        .map_err(store_err)?
    {
        if old.holder_id == ctx.holder_id {
            return Ok(Json(old));
        }
        let reason = if req.reason.is_empty() {
            format!("preempted by presence from {}", ctx.holder_id)
        } else {
            req.reason.clone()
        };
        let store = state.store.clone();
        let holder = ctx.holder_id.clone();
        let mut lease = tokio::task::spawn_blocking({
            let holder = holder.clone();
            let session_id = req.session_id.clone();
            let old_id = old.lease_id.clone();
            move || {
                store.preempt_lease(&fabric_context::Preemption {
                    session_id,
                    old_lease_id: old_id,
                    new_holder_id: holder.clone(),
                    new_surface_id: holder,
                    locus,
                    ttl_ms: ttl,
                    reason,
                })
            }
        })
        .await
        .map_err(StoreError::from)
        .map_err(store_err)?
        .map_err(store_err)?;
        stamp_granted_by(&state.store, &mut lease, &state.identity)
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
        LeaseAuthority::acquire_lease(&state.store, &req.session_id, &ctx.holder_id, locus, ttl)
            .await
            .map_err(store_err)?;
    stamp_granted_by(&state.store, &mut lease, &state.identity)
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
    let store = state.store.clone();
    let lease_id = req.lease_id.clone();
    let existing = tokio::task::spawn_blocking(move || store.lease(&lease_id))
        .await
        .map_err(StoreError::from)
        .map_err(store_err)?
        .map_err(store_err)?;
    authorize_if_session_exists(&state.store, &existing.session_id, &ctx).await?;
    let store = state.store.clone();
    let holder = ctx.holder_id.clone();
    let lease_id = req.lease_id.clone();
    let lease = tokio::task::spawn_blocking(move || store.renew_lease(&lease_id, &holder, ttl))
        .await
        .map_err(StoreError::from)
        .map_err(store_err)?
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
    authorize_if_session_exists(&state.store, &req.session_id, &ctx).await?;
    LeaseAuthority::release_lease(&state.store, &req.session_id, &ctx.holder_id)
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
    authorize_if_session_exists(&state.store, &q.session_id, &ctx).await?;
    LeaseAuthority::active_lease(&state.store, &q.session_id)
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

/// Offline-reconnect ingest: an endpoint replays its local op-log after an
/// offline stretch. Entries were already validated by the endpoint's locus,
/// so they merge through the deterministic `reconcile` path (same merge as
/// store-to-store replicas): duplicates skipped, seq collisions resolved by
/// (received_at, entry_id) — where `received_at` is stamped HERE with the
/// server clock before merge, never trusted from the client (ADR 006).
/// Replayed entries are re-evaluated against the current effective policy;
/// DENY matches are quarantined. Returns the reconcile report.
async fn replay(
    State(state): State<Arc<ControlState>>,
    Identity(ctx): Identity,
    Json(req): Json<ReplayRequest>,
) -> Result<Json<ReconcileReport>, Response> {
    ensure_session(&state.store, &req.session_id, &ctx).await?;

    // ADR 006: the server clock is authoritative. Overwrite received_at on
    // every replayed entry BEFORE the merge — a client-supplied value could
    // forge priority in (received_at, entry_id) conflict resolution.
    let received_ms = now_ms();

    // Stage the replayed entries in a throwaway in-memory replica and run
    // the standard reconcile merge into the authoritative store.
    let staging = SqliteContextStore::open_in_memory().map_err(store_err)?;
    let staging_session = SessionMeta {
        session_id: req.session_id.clone(),
        soul_id: ctx.soul_id.clone(),
        user_id: ctx.user_id.clone(),
        state: SessionState::Active as i32,
        active_lease: String::new(),
        created_at: Some(ms_to_timestamp(now_ms())),
        last_activity: Some(ms_to_timestamp(now_ms())),
        labels: Default::default(),
        org_id: ctx.org_id.clone(),
    };
    let staging_create = staging.clone();
    tokio::task::spawn_blocking(move || staging_create.create_session(&staging_session))
        .await
        .map_err(StoreError::from)
        .map_err(store_err)?
        .map_err(store_err)?;
    for entry in &req.entries {
        let mut stamped = entry.clone();
        stamped.received_at = Some(ms_to_timestamp(received_ms));
        ContextStore::insert_entry_raw(&staging, &stamped)
            .await
            .map_err(store_err)?;
    }

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
    let report =
        fabric_context::reconcile(&state.store, &staging, &req.session_id, policy.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use fabric_types::context::{ContextEntry, EntryKind, ToolCall};
    use fabric_types::lease::LeaseState;
    use fabric_types::policy::{ToolAction, ToolRule};
    use tower::ServiceExt;

    const USER: &str = "user-1";

    fn test_state() -> Arc<ControlState> {
        let store = SqliteContextStore::open_in_memory().unwrap();
        let souls = SoulRegistry::open_in_memory().unwrap();
        ControlState::new(store, souls, "fabric-server-test")
    }

    /// Request as the default caller: user-1 on device endpoint-1.
    async fn request(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        request_as(app, method, uri, body, "endpoint-1").await
    }

    /// Request as user-1 on a specific device.
    async fn request_as(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
        device: &str,
    ) -> (StatusCode, Value) {
        request_full(app, method, uri, body, USER, device).await
    }

    /// Request with an explicit caller identity (the `x-fabric-*` headers
    /// the middleware resolves; bodies carry no identity claims).
    async fn request_full(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
        user: &str,
        device: &str,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-fabric-user-sub", user)
            .header("x-fabric-device-sub", device);
        let payload = match body {
            Some(b) => {
                builder = builder.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let res = app
            .clone()
            .oneshot(builder.body(payload).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, body)
    }

    fn expires_ms(lease: &Value) -> i64 {
        // pbjson serializes timestamps as RFC3339 strings; compare via the
        // store instead. Here we only need ordering, so parse through
        // pbjson_types.
        let ts: pbjson_types::Timestamp =
            serde_json::from_value(lease["expiresAt"].clone()).unwrap();
        ts.seconds * 1000 + i64::from(ts.nanos) / 1_000_000
    }

    fn entry_json(id: &str, session: &str, seq: u64, created_ms: i64) -> Value {
        serde_json::to_value(ContextEntry {
            entry_id: id.into(),
            session_id: session.into(),
            seq,
            kind: EntryKind::UserMessage as i32,
            payload: b"hello".to_vec(),
            lease_holder: "endpoint-1".into(),
            policy_version: String::new(),
            locus: Locus::Endpoint as i32,
            created_at: Some(pbjson_types::Timestamp {
                seconds: created_ms / 1000,
                nanos: ((created_ms % 1000) * 1_000_000) as i32,
            }),
            received_at: None,
            disposition: String::new(),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn acquire_active_renew_preempt_release_cycle() {
        let state = test_state();
        let app = router(Arc::clone(&state));

        // Acquire: the server stamps identity + timestamps; the holder is
        // the caller's device identity, not a body field.
        let (code, lease) = request(
            &app,
            "POST",
            "/lease/acquire",
            Some(json!({
                "session_id": "s1",
                "locus": "LOCUS_ENDPOINT",
                "ttl_ms": 60_000,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{lease}");
        assert_eq!(lease["holderId"], "endpoint-1");
        assert_eq!(lease["grantedBy"], "fabric-server-test");
        assert_eq!(lease["state"], "LEASE_STATE_ACTIVE");
        let lease_id = lease["leaseId"].as_str().unwrap().to_string();
        let first_expiry = expires_ms(&lease);

        // The server clock stamped granted_at — within the last minute.
        let granted: pbjson_types::Timestamp =
            serde_json::from_value(lease["grantedAt"].clone()).unwrap();
        let granted_ms = granted.seconds * 1000;
        assert!((now_ms() - granted_ms).abs() < 60_000);

        // A competing raw acquire from the same user's other device
        // conflicts: preemption is the only way in.
        let (code, body) = request_as(
            &app,
            "POST",
            "/lease/acquire",
            Some(json!({
                "session_id": "s1",
                "locus": "LOCUS_SERVER",
                "ttl_ms": 60_000,
            })),
            "web-1",
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");

        // Active: returns the holder's lease.
        let (code, active_lease) = request(&app, "GET", "/lease/active?session_id=s1", None).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(active_lease["leaseId"], lease_id);
        assert_eq!(active_lease["grantedBy"], "fabric-server-test");

        // Renew: holder (device identity) matches, expiry extends.
        let (code, renewed) = request(
            &app,
            "POST",
            "/lease/renew",
            Some(json!({
                "lease_id": lease_id,
                "ttl_ms": 120_000,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{renewed}");
        assert!(expires_ms(&renewed) > first_expiry);

        // Renew from a different device is rejected.
        let (code, _) = request_as(
            &app,
            "POST",
            "/lease/renew",
            Some(json!({
                "lease_id": lease_id,
                "ttl_ms": 120_000,
            })),
            "mallory",
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN);

        // Preempt: user moved to the web surface. Presence wins the lease.
        let (code, new_lease) = request_as(
            &app,
            "POST",
            "/lease/preempt",
            Some(json!({
                "session_id": "s1",
                "reason": "user active on web",
                "locus": "LOCUS_SERVER",
            })),
            "web-1",
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{new_lease}");
        assert_eq!(new_lease["holderId"], "web-1");
        assert_eq!(new_lease["grantedBy"], "fabric-server-test");
        assert_ne!(new_lease["leaseId"], lease_id);

        // Audit: the old lease is REVOKED and records who preempted it.
        let old = state.store.lease(&lease_id).unwrap();
        assert_eq!(old.state, LeaseState::Revoked as i32);
        assert_eq!(old.preempted_by, "web-1");

        // Active now reports the web surface.
        let (code, active_lease) = request(&app, "GET", "/lease/active?session_id=s1", None).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(active_lease["holderId"], "web-1");

        // Preempting from the current holder's device is a no-op.
        let (code, same) = request_as(
            &app,
            "POST",
            "/lease/preempt",
            Some(json!({ "session_id": "s1" })),
            "web-1",
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(same["leaseId"], new_lease["leaseId"]);

        // Release by a non-holder is rejected.
        let (code, _) = request(
            &app,
            "DELETE",
            "/lease/release",
            Some(json!({ "session_id": "s1" })),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN);

        // Release by the holder: 204, then no active lease.
        let (code, _) = request_as(
            &app,
            "DELETE",
            "/lease/release",
            Some(json!({ "session_id": "s1" })),
            "web-1",
        )
        .await;
        assert_eq!(code, StatusCode::NO_CONTENT);
        let (code, _) = request(&app, "GET", "/lease/active?session_id=s1", None).await;
        assert_eq!(code, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn acquire_and_renew_reject_out_of_range_ttl() {
        let app = router(test_state());

        for ttl in [0, -5, MAX_TTL_MS + 1] {
            let (code, body) = request(
                &app,
                "POST",
                "/lease/acquire",
                Some(json!({
                    "session_id": "s1",
                    "ttl_ms": ttl,
                })),
            )
            .await;
            assert_eq!(code, StatusCode::BAD_REQUEST, "ttl {ttl}: {body}");
        }

        // Acquire a lease with a valid TTL, then renew with bad ones.
        let (code, lease) = request(
            &app,
            "POST",
            "/lease/acquire",
            Some(json!({
                "session_id": "s1",
                "ttl_ms": 60_000,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{lease}");
        let lease_id = lease["leaseId"].as_str().unwrap();

        for ttl in [0, -1, MAX_TTL_MS + 1] {
            let (code, body) = request(
                &app,
                "POST",
                "/lease/renew",
                Some(json!({
                    "lease_id": lease_id,
                    "ttl_ms": ttl,
                })),
            )
            .await;
            assert_eq!(code, StatusCode::BAD_REQUEST, "ttl {ttl}: {body}");
        }

        // Boundary value: exactly MAX_TTL_MS is accepted.
        let (code, body) = request(
            &app,
            "POST",
            "/lease/renew",
            Some(json!({
                "lease_id": lease_id,
                "ttl_ms": MAX_TTL_MS,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
    }

    #[tokio::test]
    async fn presence_from_new_surface_preempts_lease() {
        let app = router(test_state());

        let (code, lease) = request(
            &app,
            "POST",
            "/lease/acquire",
            Some(json!({
                "session_id": "s1",
                "locus": "LOCUS_ENDPOINT",
                "ttl_ms": 60_000,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK);

        // Presence from the holder's own device: no-op, same lease.
        let (code, same) = request(
            &app,
            "POST",
            "/presence",
            Some(json!({ "session_id": "s1" })),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(same["leaseId"], lease["leaseId"]);

        // Presence from the web client: latest activity wins the lease.
        let (code, new_lease) = request_as(
            &app,
            "POST",
            "/presence",
            Some(json!({
                "session_id": "s1",
                "locus": "LOCUS_SERVER",
            })),
            "web-1",
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{new_lease}");
        assert_eq!(new_lease["holderId"], "web-1");
        assert_ne!(new_lease["leaseId"], lease["leaseId"]);
    }

    #[tokio::test]
    async fn replay_merges_offline_entries_deterministically() {
        let state = test_state();
        let app = router(Arc::clone(&state));

        // First replay: both entries apply cleanly.
        let (code, report) = request(
            &app,
            "POST",
            "/context/replay",
            Some(json!({
                "session_id": "s1",
                "entries": [entry_json("e1", "s1", 1, 1_000), entry_json("e2", "s1", 2, 2_000)],
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{report}");
        assert_eq!(report["applied"], 2);
        assert_eq!(report["duplicates"], 0);

        // Replaying the same entries is idempotent.
        let (code, report) = request(
            &app,
            "POST",
            "/context/replay",
            Some(json!({
                "session_id": "s1",
                "entries": [entry_json("e1", "s1", 1, 1_000), entry_json("e2", "s1", 2, 2_000)],
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(report["applied"], 0);
        assert_eq!(report["duplicates"], 2);

        // A diverged offline entry at a contested seq merges
        // deterministically: e3 loses to e2 on (received_at, entry_id) —
        // both were server-stamped at ingest — and moves to the tail.
        let (code, report) = request(
            &app,
            "POST",
            "/context/replay",
            Some(json!({
                "session_id": "s1",
                "entries": [entry_json("e1", "s1", 1, 1_000), entry_json("e3", "s1", 2, 3_000)],
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{report}");
        assert_eq!(report["duplicates"], 1);
        assert_eq!(report["conflicts"].as_array().unwrap().len(), 1);
        let entries = ContextStore::entries_since(&state.store, "s1", 0)
            .await
            .unwrap();
        let ids: Vec<&str> = entries.iter().map(|e| e.entry_id.as_str()).collect();
        assert_eq!(ids, ["e1", "e2", "e3"]);
    }

    #[tokio::test]
    async fn identity_headers_required_on_all_routes_except_healthz() {
        let app = router(test_state());

        // No identity headers: every protected route rejects with 400.
        for (method, uri) in [
            ("POST", "/lease/acquire"),
            ("POST", "/lease/preempt"),
            ("POST", "/lease/renew"),
            ("DELETE", "/lease/release"),
            ("GET", "/lease/active?session_id=s1"),
            ("POST", "/presence"),
            ("POST", "/context/replay"),
            ("GET", "/identity"),
        ] {
            let req = Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                res.status(),
                StatusCode::BAD_REQUEST,
                "{method} {uri} must reject requests without identity headers"
            );
        }

        // The liveness probe stays unauthenticated.
        let req = Request::builder()
            .method("GET")
            .uri("/healthz")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn session_is_bound_to_the_creating_identity() {
        let state = test_state();
        let app = router(Arc::clone(&state));

        // user-a creates the session by acquiring the first lease.
        let (code, _) = request_full(
            &app,
            "POST",
            "/lease/acquire",
            Some(json!({ "session_id": "s1", "ttl_ms": 60_000 })),
            "user-a",
            "dev-a",
        )
        .await;
        assert_eq!(code, StatusCode::OK);

        // The session row is bound to user-a's identity and org.
        let session = state.store.session("s1").unwrap();
        assert_eq!(session.user_id, "user-a");
        assert_eq!(session.org_id, "default");
        assert!(!session.soul_id.is_empty());

        // user-b cannot acquire, preempt, release, replay, or even READ the
        // active lease on user-a's session.
        for (method, uri, body) in [
            (
                "POST",
                "/lease/acquire",
                json!({ "session_id": "s1", "ttl_ms": 60_000 }),
            ),
            ("POST", "/lease/preempt", json!({ "session_id": "s1" })),
            ("DELETE", "/lease/release", json!({ "session_id": "s1" })),
            (
                "POST",
                "/context/replay",
                json!({ "session_id": "s1", "entries": [] }),
            ),
        ] {
            let (code, body) = request_full(&app, method, uri, Some(body), "user-b", "dev-b").await;
            assert_eq!(code, StatusCode::FORBIDDEN, "{method} {uri}: {body}");
        }
        let (code, _) = request_full(
            &app,
            "GET",
            "/lease/active?session_id=s1",
            None,
            "user-b",
            "dev-b",
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN);

        // user-a's OTHER device is fine: tenancy is per user+org, not per
        // device (device switch is the whole point of the fabric).
        let (code, lease) = request_full(
            &app,
            "POST",
            "/lease/preempt",
            Some(json!({ "session_id": "s1" })),
            "user-a",
            "dev-a2",
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{lease}");
        assert_eq!(lease["holderId"], "dev-a2");
    }

    #[tokio::test]
    async fn replay_restamps_received_at_with_the_server_clock() {
        let state = test_state();
        let app = router(Arc::clone(&state));

        // A forged received_at (1970) would win every conflict resolution
        // if the server trusted it. ADR 006: the server clock is
        // authoritative; the client claim is overwritten before merge.
        let mut forged = entry_json("forged", "s1", 1, 1_000);
        forged["receivedAt"] = serde_json::to_value(pbjson_types::Timestamp {
            seconds: 0,
            nanos: 1_000_000,
        })
        .unwrap();
        let before = now_ms();
        let (code, report) = request(
            &app,
            "POST",
            "/context/replay",
            Some(json!({ "session_id": "s1", "entries": [forged] })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{report}");
        assert_eq!(report["applied"], 1);

        let stored = ContextStore::entries_since(&state.store, "s1", 0)
            .await
            .unwrap()
            .pop()
            .unwrap();
        let stamped = stored.received_at.as_ref().unwrap();
        let stamped_ms = stamped.seconds * 1000 + i64::from(stamped.nanos) / 1_000_000;
        assert_ne!(stamped_ms, 1, "client-supplied received_at was trusted");
        assert!(
            (before..=now_ms() + 5_000).contains(&stamped_ms),
            "received_at must be server-stamped at ingest: {stamped_ms}"
        );
    }

    fn deny_policy(pattern: &str) -> EndpointPolicy {
        EndpointPolicy {
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

    fn tool_call_entry_json(id: &str, session: &str, seq: u64, tool: &str, target: &str) -> Value {
        serde_json::to_value(ContextEntry {
            entry_id: id.into(),
            session_id: session.into(),
            seq,
            kind: EntryKind::ToolCall as i32,
            payload: fabric_context::tool_call::encode(&ToolCall {
                tool_name: tool.into(),
                target: target.into(),
                params: Default::default(),
                idempotency_key: String::new(),
            }),
            lease_holder: "endpoint-1".into(),
            policy_version: String::new(),
            locus: Locus::Endpoint as i32,
            created_at: Some(pbjson_types::Timestamp {
                seconds: 1,
                nanos: 0,
            }),
            received_at: None,
            disposition: String::new(),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn replay_reevaluates_entries_against_current_policy() {
        let state = test_state();
        state.set_policy(deny_policy("shell.*"));
        let app = router(Arc::clone(&state));

        // The endpoint executed shell.exec while offline under an older
        // policy; the current policy denies it. Re-evaluated on replay, the
        // entry is preserved but QUARANTINED (ADR 006).
        let denied = tool_call_entry_json("denied-call", "s1", 1, "shell.exec", "/etc");

        let (code, report) = request(
            &app,
            "POST",
            "/context/replay",
            Some(json!({ "session_id": "s1", "entries": [denied] })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{report}");
        assert_eq!(report["applied"], 1);
        let violations = report["policy_violations"].as_array().unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0]["entry_id"], "denied-call");
        assert_eq!(violations[0]["rule"], "shell.*");

        let stored = state.store.entry_by_id("denied-call").unwrap().unwrap();
        assert_eq!(
            stored.disposition,
            fabric_context::reconcile::DISPOSITION_QUARANTINE
        );
    }

    #[tokio::test]
    async fn replay_without_policy_loaded_merges_without_quarantine() {
        let state = test_state();
        let app = router(Arc::clone(&state));

        let denied = tool_call_entry_json("denied-call", "s1", 1, "shell.exec", "/etc");

        let (code, report) = request(
            &app,
            "POST",
            "/context/replay",
            Some(json!({ "session_id": "s1", "entries": [denied] })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{report}");
        assert_eq!(report["applied"], 1);
        assert_eq!(report["policy_violations"].as_array().unwrap().len(), 0);
        let stored = state.store.entry_by_id("denied-call").unwrap().unwrap();
        assert_eq!(stored.disposition, "");
    }
}
