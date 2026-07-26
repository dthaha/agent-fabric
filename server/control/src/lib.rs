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

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use fabric_context::clock::now_ms;
use fabric_context::db::ms_to_timestamp;
use fabric_context::{ContextStore, ReconcileReport, SqliteContextStore, StoreError};
use fabric_types::context::{ContextEntry, Locus, SessionMeta, SessionState};
use fabric_types::lease::Lease;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

/// Default TTL for leases granted via preempt/presence, where the caller does
/// not specify one. Matches the turn-scoped safety-net posture of the core
/// store: generous, only firing when a holder crashes without releasing.
const DEFAULT_LEASE_TTL_MS: i64 = 3_600_000;

/// State shared by every control-plane handler. The store is the lease
/// authority; `identity` is stamped into `granted_by` on every lease the
/// server issues.
pub struct ControlState {
    pub store: SqliteContextStore,
    pub identity: String,
}

impl ControlState {
    pub fn new(store: SqliteContextStore, identity: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            store,
            identity: identity.into(),
        })
    }

    /// Identity from `FABRIC_SERVER_IDENTITY`, defaulting to "fabric-server".
    pub fn from_env(store: SqliteContextStore) -> Arc<Self> {
        let identity =
            std::env::var("FABRIC_SERVER_IDENTITY").unwrap_or_else(|_| "fabric-server".into());
        Self::new(store, identity)
    }
}

pub fn router(state: Arc<ControlState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/lease/acquire", post(acquire))
        .route("/lease/preempt", post(preempt))
        .route("/lease/renew", post(renew))
        .route("/lease/release", axum::routing::delete(release))
        .route("/lease/active", get(active))
        .route("/presence", post(presence))
        .route("/context/replay", post(replay))
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
/// (INSERT OR IGNORE): first writer to touch a session creates it with
/// server-stamped timestamps.
async fn ensure_session(store: &SqliteContextStore, session_id: &str) -> Result<(), StoreError> {
    let store = store.clone();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || {
        store.create_session(&SessionMeta {
            session_id,
            soul_id: String::new(),
            user_id: String::new(),
            state: SessionState::Active as i32,
            active_lease: String::new(),
            created_at: Some(ms_to_timestamp(now_ms())),
            last_activity: Some(ms_to_timestamp(now_ms())),
            labels: Default::default(),
            org_id: String::new(),
        })
    })
    .await?
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

#[derive(Debug, Deserialize)]
struct AcquireRequest {
    session_id: String,
    holder_id: String,
    #[serde(default)]
    locus: Option<Locus>,
    #[serde(default)]
    ttl_ms: Option<i64>,
}

/// Grant a write lease, server-stamped. 409 while another holder's
/// unexpired lease is active — preemption (presence) is the way to take
/// over a live session, never a raw acquire race.
async fn acquire(
    State(state): State<Arc<ControlState>>,
    Json(req): Json<AcquireRequest>,
) -> Result<Json<Lease>, Response> {
    ensure_session(&state.store, &req.session_id)
        .await
        .map_err(store_err)?;
    let mut lease = ContextStore::acquire_lease(
        &state.store,
        &req.session_id,
        &req.holder_id,
        req.locus.unwrap_or(Locus::Unspecified),
        req.ttl_ms.unwrap_or(DEFAULT_LEASE_TTL_MS),
    )
    .await
    .map_err(store_err)?;
    stamp_granted_by(&state.store, &mut lease, &state.identity)
        .await
        .map_err(store_err)?;
    info!(session = %req.session_id, holder = %req.holder_id, lease = %lease.lease_id, "lease granted");
    Ok(Json(lease))
}

#[derive(Debug, Deserialize)]
struct PreemptRequest {
    session_id: String,
    new_holder_id: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    locus: Option<Locus>,
    #[serde(default)]
    ttl_ms: Option<i64>,
}

/// Presence-driven preemption: the surface with the latest server-observed
/// activity takes the lease. The outgoing lease is revoked with
/// `preempted_by` recorded for audit, then a fresh server-stamped lease is
/// granted to the new holder. If the new holder already holds the lease this
/// is a no-op returning the current lease.
async fn preempt(
    State(state): State<Arc<ControlState>>,
    Json(req): Json<PreemptRequest>,
) -> Result<Json<Lease>, Response> {
    ensure_session(&state.store, &req.session_id)
        .await
        .map_err(store_err)?;
    let locus = req.locus.unwrap_or(Locus::Unspecified);
    let ttl = req.ttl_ms.unwrap_or(DEFAULT_LEASE_TTL_MS);

    if let Some(old) = ContextStore::active_lease(&state.store, &req.session_id)
        .await
        .map_err(store_err)?
    {
        if old.holder_id == req.new_holder_id {
            return Ok(Json(old));
        }
        let store = state.store.clone();
        let old_id = old.lease_id.clone();
        let new_holder = req.new_holder_id.clone();
        tokio::task::spawn_blocking(move || store.set_preempted_by(&old_id, &new_holder))
            .await
            .map_err(StoreError::from)
            .map_err(store_err)?
            .map_err(store_err)?;
        let reason = if req.reason.is_empty() {
            format!("preempted by presence from {}", req.new_holder_id)
        } else {
            req.reason.clone()
        };
        let store = state.store.clone();
        let session_id = req.session_id.clone();
        tokio::task::spawn_blocking(move || store.revoke_lease(&session_id, &reason))
            .await
            .map_err(StoreError::from)
            .map_err(store_err)?
            .map_err(store_err)?;
        info!(
            session = %req.session_id,
            old_holder = %old.holder_id,
            new_holder = %req.new_holder_id,
            "lease preempted"
        );
    }

    let mut lease = ContextStore::acquire_lease(
        &state.store,
        &req.session_id,
        &req.new_holder_id,
        locus,
        ttl,
    )
    .await
    .map_err(store_err)?;
    stamp_granted_by(&state.store, &mut lease, &state.identity)
        .await
        .map_err(store_err)?;
    Ok(Json(lease))
}

#[derive(Debug, Deserialize)]
struct RenewRequest {
    lease_id: String,
    holder_id: String,
    #[serde(default)]
    ttl_ms: Option<i64>,
}

/// Extend an ACTIVE lease's expiry. Holder must match; the new expiry is
/// stamped with the server clock.
async fn renew(
    State(state): State<Arc<ControlState>>,
    Json(req): Json<RenewRequest>,
) -> Result<Json<Lease>, Response> {
    let store = state.store.clone();
    let ttl = req.ttl_ms.unwrap_or(DEFAULT_LEASE_TTL_MS);
    let lease =
        tokio::task::spawn_blocking(move || store.renew_lease(&req.lease_id, &req.holder_id, ttl))
            .await
            .map_err(StoreError::from)
            .map_err(store_err)?
            .map_err(store_err)?;
    Ok(Json(lease))
}

#[derive(Debug, Deserialize)]
struct ReleaseRequest {
    session_id: String,
    holder_id: String,
}

/// Release the lease at the end of a turn. Holder must match. 204 on
/// success; the session stays ACTIVE without a writer.
async fn release(
    State(state): State<Arc<ControlState>>,
    Json(req): Json<ReleaseRequest>,
) -> Result<StatusCode, Response> {
    ContextStore::release_lease(&state.store, &req.session_id, &req.holder_id)
        .await
        .map_err(store_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ActiveQuery {
    session_id: String,
}

/// The session's ACTIVE lease, or 404 when there is no writer.
async fn active(
    State(state): State<Arc<ControlState>>,
    Query(q): Query<ActiveQuery>,
) -> Result<Json<Lease>, Response> {
    ContextStore::active_lease(&state.store, &q.session_id)
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

#[derive(Debug, Deserialize)]
struct PresenceRequest {
    session_id: String,
    surface_id: String,
    #[serde(default)]
    locus: Option<Locus>,
}

/// A surface reports user activity. Latest server-observed activity wins the
/// lease: if the reporting surface is not the current holder, the lease is
/// preempted to it. This IS the preemption mechanism — presence, not clock
/// races. Returns the lease the surface now holds (or already held).
async fn presence(
    State(state): State<Arc<ControlState>>,
    Json(req): Json<PresenceRequest>,
) -> Result<Json<Lease>, Response> {
    preempt(
        State(state),
        Json(PreemptRequest {
            session_id: req.session_id,
            new_holder_id: req.surface_id.clone(),
            reason: format!("presence from {}", req.surface_id),
            locus: req.locus,
            ttl_ms: None,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
struct ReplayRequest {
    session_id: String,
    #[serde(default)]
    entries: Vec<ContextEntry>,
}

/// Offline-reconnect ingest: an endpoint replays its local op-log after an
/// offline stretch. Entries were already validated by the endpoint's locus,
/// so they merge through the deterministic `reconcile` path (same merge as
/// store-to-store replicas): duplicates skipped, seq collisions resolved by
/// (created_at, entry_id). Returns the reconcile report.
async fn replay(
    State(state): State<Arc<ControlState>>,
    Json(req): Json<ReplayRequest>,
) -> Result<Json<ReconcileReport>, Response> {
    ensure_session(&state.store, &req.session_id)
        .await
        .map_err(store_err)?;

    // Stage the replayed entries in a throwaway in-memory replica and run
    // the standard reconcile merge into the authoritative store.
    let staging = SqliteContextStore::open_in_memory().map_err(store_err)?;
    let staging_session = SessionMeta {
        session_id: req.session_id.clone(),
        soul_id: String::new(),
        user_id: String::new(),
        state: SessionState::Active as i32,
        active_lease: String::new(),
        created_at: Some(ms_to_timestamp(now_ms())),
        last_activity: Some(ms_to_timestamp(now_ms())),
        labels: Default::default(),
        org_id: String::new(),
    };
    let staging_create = staging.clone();
    tokio::task::spawn_blocking(move || staging_create.create_session(&staging_session))
        .await
        .map_err(StoreError::from)
        .map_err(store_err)?
        .map_err(store_err)?;
    for entry in &req.entries {
        ContextStore::insert_entry_raw(&staging, entry)
            .await
            .map_err(store_err)?;
    }

    let report = fabric_context::reconcile(&state.store, &staging, &req.session_id)
        .await
        .map_err(store_err)?;
    info!(
        session = %req.session_id,
        applied = report.applied,
        duplicates = report.duplicates,
        conflicts = report.conflicts.len(),
        "offline op-log replayed"
    );
    Ok(Json(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use fabric_types::lease::LeaseState;
    use tower::ServiceExt;

    fn test_state() -> Arc<ControlState> {
        let store = SqliteContextStore::open_in_memory().unwrap();
        ControlState::new(store, "fabric-server-test")
    }

    async fn request(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
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

    #[tokio::test]
    async fn acquire_active_renew_preempt_release_cycle() {
        let state = test_state();
        let app = router(Arc::clone(&state));

        // Acquire: the server stamps identity + timestamps.
        let (code, lease) = request(
            &app,
            "POST",
            "/lease/acquire",
            Some(json!({
                "session_id": "s1",
                "holder_id": "endpoint-1",
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

        // A competing raw acquire conflicts: preemption is the only way in.
        let (code, body) = request(
            &app,
            "POST",
            "/lease/acquire",
            Some(json!({
                "session_id": "s1",
                "holder_id": "web-1",
                "locus": "LOCUS_SERVER",
                "ttl_ms": 60_000,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");

        // Active: returns the holder's lease.
        let (code, active_lease) = request(&app, "GET", "/lease/active?session_id=s1", None).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(active_lease["leaseId"], lease_id);
        assert_eq!(active_lease["grantedBy"], "fabric-server-test");

        // Renew: holder matches, expiry extends.
        let (code, renewed) = request(
            &app,
            "POST",
            "/lease/renew",
            Some(json!({
                "lease_id": lease_id,
                "holder_id": "endpoint-1",
                "ttl_ms": 120_000,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{renewed}");
        assert!(expires_ms(&renewed) > first_expiry);

        // Renew by a non-holder is rejected.
        let (code, _) = request(
            &app,
            "POST",
            "/lease/renew",
            Some(json!({
                "lease_id": lease_id,
                "holder_id": "mallory",
                "ttl_ms": 120_000,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN);

        // Preempt: user moved to the web surface. Presence wins the lease.
        let (code, new_lease) = request(
            &app,
            "POST",
            "/lease/preempt",
            Some(json!({
                "session_id": "s1",
                "new_holder_id": "web-1",
                "reason": "user active on web",
                "locus": "LOCUS_SERVER",
            })),
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

        // Preempting to the current holder is a no-op.
        let (code, same) = request(
            &app,
            "POST",
            "/lease/preempt",
            Some(json!({
                "session_id": "s1",
                "new_holder_id": "web-1",
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(same["leaseId"], new_lease["leaseId"]);

        // Release by a non-holder is rejected.
        let (code, _) = request(
            &app,
            "DELETE",
            "/lease/release",
            Some(json!({ "session_id": "s1", "holder_id": "endpoint-1" })),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN);

        // Release by the holder: 204, then no active lease.
        let (code, _) = request(
            &app,
            "DELETE",
            "/lease/release",
            Some(json!({ "session_id": "s1", "holder_id": "web-1" })),
        )
        .await;
        assert_eq!(code, StatusCode::NO_CONTENT);
        let (code, _) = request(&app, "GET", "/lease/active?session_id=s1", None).await;
        assert_eq!(code, StatusCode::NOT_FOUND);
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
                "holder_id": "endpoint-1",
                "locus": "LOCUS_ENDPOINT",
                "ttl_ms": 60_000,
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK);

        // Presence from the holder: no-op, same lease.
        let (code, same) = request(
            &app,
            "POST",
            "/presence",
            Some(json!({ "session_id": "s1", "surface_id": "endpoint-1" })),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(same["leaseId"], lease["leaseId"]);

        // Presence from the web client: latest activity wins the lease.
        let (code, new_lease) = request(
            &app,
            "POST",
            "/presence",
            Some(json!({
                "session_id": "s1",
                "surface_id": "web-1",
                "locus": "LOCUS_SERVER",
            })),
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

        let entry = |id: &str, seq: u64, created_ms: i64| {
            json!({
                "entryId": id,
                "sessionId": "s1",
                "seq": seq.to_string(),
                "kind": "ENTRY_KIND_USER_MESSAGE",
                "payload": "aGVsbG8=",
                "leaseHolder": "endpoint-1",
                "locus": "LOCUS_ENDPOINT",
                "createdAt": pbjson_types::Timestamp {
                    seconds: created_ms / 1000,
                    nanos: ((created_ms % 1000) * 1_000_000) as i32,
                },
            })
        };

        // First replay: both entries apply cleanly.
        let (code, report) = request(
            &app,
            "POST",
            "/context/replay",
            Some(json!({
                "session_id": "s1",
                "entries": [entry("e1", 1, 1_000), entry("e2", 2, 2_000)],
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
                "entries": [entry("e1", 1, 1_000), entry("e2", 2, 2_000)],
            })),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(report["applied"], 0);
        assert_eq!(report["duplicates"], 2);

        // A diverged offline entry at a contested seq merges
        // deterministically: e3 loses to e2 on (created_at, entry_id) and
        // moves to the tail.
        let (code, report) = request(
            &app,
            "POST",
            "/context/replay",
            Some(json!({
                "session_id": "s1",
                "entries": [entry("e1", 1, 1_000), entry("e3", 2, 3_000)],
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
}
