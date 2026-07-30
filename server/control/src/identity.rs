//! Identity middleware (ADR 007): resolves the four identity fields
//! server-side so clients supply none of them.
//!
//! v1 trusts `x-fabric-*` headers directly.
//! TODO: ADR 003: replace header trust with JWT validation via JWKS.

use std::sync::Arc;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{json, Value as JsonValue};

use crate::soul::SoulRegistry;
use crate::ControlState;

/// Extracted identity context, resolved server-side per ADR 007.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IdentityContext {
    pub user_id: String,
    pub holder_id: String,
    pub org_id: String,
    pub soul_id: String,
}

/// Resolve the org for a request: explicit header wins, then the
/// `FABRIC_ORG_ID` server config, then "default". Per ADR 007 the MDM
/// policy pack is the most authoritative source; the endpoint stamps it
/// into the header, so header-first is correct here.
pub fn resolve_org(header: Option<&str>, env: Option<&str>) -> String {
    [header, env]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("default")
        .to_string()
}

fn header<'a>(parts: &'a Parts, name: &str) -> Option<&'a str> {
    parts.headers.get(name).and_then(|v| v.to_str().ok())
}

/// Rejection returned when identity resolution fails. Small enough to stay
/// under clippy's `result_large_err` threshold and implements
/// [`IntoResponse`] directly.
pub type IdentityRejection = (StatusCode, Json<JsonValue>);

fn missing(field: &str) -> IdentityRejection {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": format!("missing required identity header: {field}") })),
    )
}

fn internal(context: &str, e: impl std::fmt::Display) -> IdentityRejection {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": format!("{context}: {e}") })),
    )
}

/// Resolve identity from request headers against the registry. Shared by
/// the [`Identity`] extractor and the [`identity_middleware`] layer.
fn resolve(
    registry: &SoulRegistry,
    parts: &Parts,
) -> std::result::Result<IdentityContext, IdentityRejection> {
    let user_id = header(parts, "x-fabric-user-sub")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| missing("x-fabric-user-sub"))?;
    let holder_id = header(parts, "x-fabric-device-sub")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| missing("x-fabric-device-sub"))?;
    let org_id = resolve_org(
        header(parts, "x-fabric-org-id"),
        std::env::var("FABRIC_ORG_ID").ok().as_deref(),
    );

    let soul = registry
        .resolve_or_create_soul(user_id, &org_id)
        .map_err(|e| internal("soul resolution failed", e))?;
    registry
        .record_device(
            holder_id,
            header(parts, "x-fabric-device-name").unwrap_or(""),
            &org_id,
            header(parts, "x-fabric-device-platform").unwrap_or("unknown"),
        )
        .map_err(|e| internal("device registration failed", e))?;

    Ok(IdentityContext {
        user_id: user_id.to_string(),
        holder_id: holder_id.to_string(),
        org_id,
        soul_id: soul.soul_id,
    })
}

/// Axum extractor: resolves the full identity context (and records the
/// device sighting) for any handler that takes `Identity` as a parameter.
pub struct Identity(pub IdentityContext);

impl FromRequestParts<Arc<ControlState>> for Identity {
    type Rejection = IdentityRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<ControlState>,
    ) -> std::result::Result<Self, Self::Rejection> {
        resolve(&state.souls, parts).map(Self)
    }
}

impl std::ops::Deref for Identity {
    type Target = IdentityContext;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Middleware variant: resolves identity once and inserts the
/// [`IdentityContext`] into request extensions for downstream handlers.
pub async fn identity_middleware(
    State(state): State<Arc<ControlState>>,
    req: Request,
    next: Next,
) -> Response {
    let (parts, body) = req.into_parts();
    match resolve(&state.souls, &parts) {
        Ok(ctx) => {
            let mut req = Request::from_parts(parts, body);
            req.extensions_mut().insert(ctx);
            next.run(req).await
        }
        Err(rejection) => rejection.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soul::SoulRegistry;
    use crate::{router, ControlState};
    use axum::body::Body;
    use fabric_context::SqliteContextStore;
    use serde_json::Value;
    use tower::ServiceExt;

    fn test_state() -> Arc<ControlState> {
        let store = SqliteContextStore::open_in_memory().unwrap();
        let souls = SoulRegistry::open_in_memory().unwrap();
        ControlState::new(store, souls, "fabric-server-test")
    }

    async fn get_identity(headers: &[(&str, &str)]) -> (StatusCode, Value) {
        let app = router(test_state());
        let mut builder = Request::builder().method("GET").uri("/identity");
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let res = app
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[test]
    fn org_fallback_header_then_env_then_default() {
        assert_eq!(resolve_org(Some("org-a"), Some("org-b")), "org-a");
        assert_eq!(resolve_org(Some(""), Some("org-b")), "org-b");
        assert_eq!(resolve_org(None, Some("org-b")), "org-b");
        assert_eq!(resolve_org(Some("  "), None), "default");
        assert_eq!(resolve_org(None, None), "default");
    }

    #[tokio::test]
    async fn extractor_resolves_all_four_fields() {
        let (code, body) = get_identity(&[
            ("x-fabric-user-sub", "user-1"),
            ("x-fabric-device-sub", "dev-1"),
            ("x-fabric-org-id", "org-1"),
        ])
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["user_id"], "user-1");
        assert_eq!(body["holder_id"], "dev-1");
        assert_eq!(body["org_id"], "org-1");
        assert!(body["soul_id"].as_str().unwrap().len() >= 32);
    }

    #[tokio::test]
    async fn extractor_is_idempotent_across_requests() {
        let state = test_state();
        let app = router(state);

        let call = || {
            let app = app.clone();
            async move {
                let req = Request::builder()
                    .method("GET")
                    .uri("/identity")
                    .header("x-fabric-user-sub", "user-1")
                    .header("x-fabric-device-sub", "dev-1")
                    .body(Body::empty())
                    .unwrap();
                let res = app.oneshot(req).await.unwrap();
                let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                    .await
                    .unwrap();
                serde_json::from_slice::<Value>(&bytes).unwrap()
            }
        };

        let first = call().await;
        let second = call().await;
        assert_eq!(first["soul_id"], second["soul_id"]);
        // Org fell back to "default" with no header and no env var.
        assert_eq!(first["org_id"], "default");
    }

    #[tokio::test]
    async fn extractor_records_device_sighting() {
        let state = test_state();
        let app = router(Arc::clone(&state));
        let req = Request::builder()
            .method("GET")
            .uri("/identity")
            .header("x-fabric-user-sub", "user-1")
            .header("x-fabric-device-sub", "dev-1")
            .header("x-fabric-device-name", "Hermes MacBook")
            .header("x-fabric-device-platform", "macos")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let device = state.souls.get_device("dev-1").unwrap().unwrap();
        assert_eq!(device.display_name, "Hermes MacBook");
        assert_eq!(device.platform, "macos");
        assert_eq!(device.status, "active");
    }

    #[tokio::test]
    async fn missing_user_or_device_header_is_400() {
        let (code, body) = get_identity(&[("x-fabric-device-sub", "dev-1")]).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("x-fabric-user-sub"));

        let (code, body) = get_identity(&[("x-fabric-user-sub", "user-1")]).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("x-fabric-device-sub"));
    }
}
