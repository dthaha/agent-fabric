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
/// the [`Identity`] extractor and the [`identity_middleware`] layer. ADR 007:
/// all four identity fields are derived server-side; the body carries none.
async fn resolve(
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
        .await
        .map_err(|e| internal("soul resolution failed", e))?;
    registry
        .record_device(
            holder_id,
            header(parts, "x-fabric-device-name").unwrap_or(""),
            &org_id,
            header(parts, "x-fabric-device-platform").unwrap_or("unknown"),
        )
        .await
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
        resolve(&state.souls, parts).await.map(Self)
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
    match resolve(&state.souls, &parts).await {
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

    #[test]
    fn org_fallback_header_then_env_then_default() {
        assert_eq!(resolve_org(Some("org-a"), Some("org-b")), "org-a");
        assert_eq!(resolve_org(Some(""), Some("org-b")), "org-b");
        assert_eq!(resolve_org(None, Some("org-b")), "org-b");
        assert_eq!(resolve_org(Some("  "), None), "default");
        assert_eq!(resolve_org(None, None), "default");
    }
}

#[cfg(all(test, feature = "server-store"))]
mod server_tests {
    // The full-extractor tests require Postgres + Valkey (FABRIC_PG_URL +
    // FABRIC_KV_URL) and live in the workspace's ignored integration tests.
}
