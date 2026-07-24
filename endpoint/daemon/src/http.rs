//! Localhost-only health, readiness, and status HTTP server. Bound to
//! 127.0.0.1 so supervisors (launchd/systemd/MDM agents) and the admin CLI
//! can probe the daemon without exposing anything off-device.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use fabric_classifier::{
    ClassifyInput, LocusClassifier, LocusDecision, PolicyAwareClassifier, RulesClassifier,
};
use serde_json::{json, Value};
use tracing::info;

use crate::state::DaemonState;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/status", get(status))
        .route("/sessions", get(sessions))
        .route("/policy", get(policy))
        .route("/classify", post(classify))
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

/// List active sessions from the context store. Read-only admin endpoint
/// for the CLI.
async fn sessions(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let list = state
        .store
        .lock()
        .ok()
        .and_then(|store| {
            store.list_active_sessions().ok().map(|sessions| {
                sessions
                    .iter()
                    .map(|s| {
                        json!({
                            "session_id": s.session_id,
                            "state": fabric_types::context::SessionState::try_from(s.state)
                                .map(|st| st.as_str_name())
                                .unwrap_or("UNKNOWN"),
                            "created_at": s.created_at.as_ref().map(|t| t.seconds * 1000 + i64::from(t.nanos) / 1_000_000).unwrap_or(0),
                            "last_entry_seq": store.head_seq(&s.session_id).unwrap_or(0),
                        })
                    })
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default();
    Json(json!(list))
}

/// Summary of the current effective policy. Read-only admin endpoint for
/// the CLI.
async fn policy(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let policy = state.policy.read().expect("policy lock poisoned");
    let effective = policy.effective();
    Json(json!({
        "endpoint_version": policy.endpoint_version().unwrap_or(""),
        "hosted_version": policy.hosted_version().unwrap_or(""),
        "tool_rule_count": effective.tool_rules.len(),
        "kill_switch": effective.kill_switch,
        "cua_enabled": effective.cua.as_ref().is_some_and(|c| c.enabled),
    }))
}

/// Classify a turn's locus. Runs the rules engine wrapped in the policy
/// gate built from the current merged policy, so the answer already
/// reflects deny-wins downgrades. Callers may include a `model_advisory`
/// in the body; Phase 5 will populate it from the seeded on-device
/// classifier model.
async fn classify(
    State(state): State<Arc<DaemonState>>,
    Json(input): Json<ClassifyInput>,
) -> Json<LocusDecision> {
    let gate = state.policy.read().expect("policy lock poisoned").gate();
    let classifier = PolicyAwareClassifier::new(RulesClassifier::new(), gate);
    Json(classifier.classify(&input))
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
    async fn sessions_lists_active_sessions() {
        let state = test_state();
        {
            let store = state.store.lock().unwrap();
            store
                .create_session(&fabric_types::context::SessionMeta {
                    session_id: "s1".into(),
                    soul_id: "soul".into(),
                    user_id: "user".into(),
                    state: fabric_types::context::SessionState::Active as i32,
                    active_lease: String::new(),
                    created_at: Some(pbjson_types::Timestamp {
                        seconds: 100,
                        nanos: 0,
                    }),
                    last_activity: None,
                    labels: Default::default(),
                    org_id: String::new(),
                })
                .unwrap();
        }
        let app = router(state);

        let (code, body) = get(&app, "/sessions").await;
        assert_eq!(code, StatusCode::OK);
        let sessions = body.as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["session_id"], "s1");
        assert_eq!(sessions[0]["state"], "SESSION_STATE_ACTIVE");
        assert_eq!(sessions[0]["created_at"], 100_000);
        assert_eq!(sessions[0]["last_entry_seq"], 0);
    }

    #[tokio::test]
    async fn policy_reports_effective_summary() {
        let state = test_state();
        state.policy.write().unwrap().load_endpoint(EndpointPolicy {
            policy_id: "ep".into(),
            version: "v3".into(),
            org_id: "org".into(),
            kill_switch: true,
            tool_rules: vec![
                fabric_types::policy::ToolRule {
                    tool_pattern: "fs.*".into(),
                    action: fabric_types::policy::ToolAction::Allow as i32,
                    condition: String::new(),
                },
                fabric_types::policy::ToolRule {
                    tool_pattern: "shell.exec".into(),
                    action: fabric_types::policy::ToolAction::Deny as i32,
                    condition: String::new(),
                },
            ],
            cua: Some(fabric_types::policy::CuaPolicy {
                enabled: true,
                ..Default::default()
            }),
            ..Default::default()
        });
        let app = router(state);

        let (code, body) = get(&app, "/policy").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["endpoint_version"], "v3");
        assert_eq!(body["hosted_version"], "");
        assert_eq!(body["tool_rule_count"], 2);
        assert_eq!(body["kill_switch"], true);
        assert_eq!(body["cua_enabled"], true);
    }

    #[tokio::test]
    async fn classify_runs_rules_through_policy_gate() {
        let state = test_state();
        let app = router(state);

        // Rules say hosted (explicit preference, network up) but no policy is
        // loaded, so the gate has no inference rules and the wrapper
        // downgrades to endpoint.
        let body = json!({
            "intent_text": "summarize my emails",
            "required_tools": ["email.read"],
            "estimated_complexity": "low",
            "estimated_horizon": "single_turn",
            "data_classes": ["public"],
            "network_available": true,
            "local_model_available": true,
            "user_preference": "prefer_hosted",
        });
        let res = app
            .oneshot(
                Request::post("/classify")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let decision: fabric_classifier::LocusDecision = serde_json::from_slice(&body).unwrap();
        assert_eq!(decision.locus, fabric_types::context::Locus::Endpoint);
        assert!(decision.reason.contains("downgraded to endpoint"));
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
