//! Localhost-only health, readiness, and status HTTP server. Bound to
//! 127.0.0.1 so supervisors (launchd/systemd/MDM agents) and the admin CLI
//! can probe the daemon without exposing anything off-device.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tracing::info;

use crate::state::DaemonState;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/status", get(status))
        .with_state(state)
}

/// Bind to `addr` and serve until `shutdown` resolves, then drain in-flight
/// requests before returning.
pub async fn serve(
    state: Arc<DaemonState>,
    addr: SocketAddr,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "health/status server listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
}

/// Liveness: always 200, even mid-startup. Answers "is the process up".
async fn healthz() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": VERSION,
    }))
}

/// Readiness: 200 only when the context store is open and a policy is
/// loaded; 503 otherwise.
async fn readyz(State(state): State<Arc<DaemonState>>) -> (StatusCode, Json<Value>) {
    let store_open = state.store_open();
    let policy_loaded = state.policy_loaded();
    let ready = store_open && policy_loaded;
    let code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(json!({
            "ready": ready,
            "context_store": store_open,
            "policy_loaded": policy_loaded,
        })),
    )
}

/// Human/admin-facing daemon status snapshot.
async fn status(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let active_sessions = state
        .store
        .lock()
        .ok()
        .and_then(|store| store.active_session_count().ok())
        .unwrap_or(0);
    let (endpoint_version, hosted_version) = state
        .policy
        .read()
        .map(|p| {
            (
                p.endpoint_version().unwrap_or("").to_string(),
                p.hosted_version().unwrap_or("").to_string(),
            )
        })
        .unwrap_or_default();
    Json(json!({
        "device_id": state.cfg.device_id,
        "version": VERSION,
        "uptime_secs": state.started.elapsed().as_secs(),
        "policy_endpoint_version": endpoint_version,
        "policy_hosted_version": hosted_version,
        "context_db_path": state.cfg.context_db.display().to_string(),
        "hosted_url": state.cfg.hosted_url,
        "active_sessions": active_sessions,
        "tool_bridge_port": state.cfg.tool_bridge_port,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use fabric_types::policy::EndpointPolicy;
    use tower::ServiceExt;

    use crate::config::DaemonConfig;

    fn test_state() -> Arc<DaemonState> {
        let store = fabric_context::ContextStore::open_in_memory().unwrap();
        DaemonState::new(DaemonConfig::default(), store)
    }

    async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
        let res = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn healthz_always_ok() {
        let app = router(test_state());
        let (code, body) = get(&app, "/healthz").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], VERSION);
    }

    #[tokio::test]
    async fn readyz_503_until_policy_loaded() {
        let state = test_state();
        let app = router(Arc::clone(&state));

        let (code, body) = get(&app, "/readyz").await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["ready"], false);
        assert_eq!(body["context_store"], true);
        assert_eq!(body["policy_loaded"], false);

        state.policy.write().unwrap().load_endpoint(EndpointPolicy {
            policy_id: "ep".into(),
            version: "v1".into(),
            org_id: "org".into(),
            ..Default::default()
        });

        let (code, body) = get(&app, "/readyz").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["ready"], true);
    }

    #[tokio::test]
    async fn status_reports_config_and_policy_versions() {
        let state = test_state();
        state.policy.write().unwrap().load_endpoint(EndpointPolicy {
            policy_id: "ep".into(),
            version: "v3".into(),
            org_id: "org".into(),
            ..Default::default()
        });
        let app = router(state);

        let (code, body) = get(&app, "/status").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["policy_endpoint_version"], "v3");
        assert_eq!(body["policy_hosted_version"], "");
        assert_eq!(body["active_sessions"], 0);
        assert_eq!(body["tool_bridge_port"], 47771);
        assert!(body["uptime_secs"].is_number());
        assert!(!body["device_id"].as_str().unwrap().is_empty());
    }
}
