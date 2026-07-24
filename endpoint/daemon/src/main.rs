//! Endpoint daemon: the long-running agent service shipped to managed
//! laptops via MDM. Single static binary, no runtime dependencies. Owns the
//! local context store, the offline classifier, seeded models, the tool
//! bridge, and the CUA actuator.

mod config;
mod http;
mod state;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::config::DaemonConfig;
use crate::state::DaemonState;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = DaemonConfig::load()?;
    info!(
        device_id = %cfg.device_id,
        context_db = %cfg.context_db.display(),
        health_port = cfg.health_port,
        tool_bridge_port = cfg.tool_bridge_port,
        "fabric-endpoint starting"
    );

    let store = fabric_context::ContextStore::open(&cfg.context_db)
        .with_context(|| format!("opening context store {}", cfg.context_db.display()))?;
    info!("context store ready (WAL mode)");

    let state = DaemonState::new(cfg.clone(), store);
    load_policy(&state);

    #[cfg(feature = "enterprise")]
    info!("enterprise features compiled in (mdm, audit-siem, ha, private-registry)");

    if cfg.hosted_url.is_empty() {
        warn!("no hosted URL configured — running offline-only");
    }

    // Health/status server on localhost, with graceful shutdown: it stops
    // accepting connections and drains in-flight requests when signaled.
    // The cancellation token is shared with every long-running service
    // (classifier, tool bridge, CUA, seeding) as they attach in later
    // phases.
    let token = CancellationToken::new();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, cfg.health_port));
    let server = tokio::spawn(http::serve(Arc::clone(&state), addr, {
        let token = token.clone();
        async move { token.cancelled().await }
    }));

    wait_for_shutdown_signal().await;
    info!("shutdown signal received, draining");
    token.cancel();

    server
        .await
        .context("health server task panicked")?
        .context("health server error")?;

    // Close the context store, flushing the WAL. The server task has
    // finished by now, so main is the only remaining state holder.
    match Arc::try_unwrap(state) {
        Ok(state) => {
            let store = state.store.into_inner().expect("store lock poisoned");
            store.close().context("closing context store")?;
            info!("context store closed (WAL checkpointed)");
        }
        Err(_) => warn!("state still referenced at shutdown; skipping explicit store close"),
    }
    info!("shutdown complete");
    Ok(())
}

/// Block until SIGINT (ctrl-c) or SIGTERM arrives.
#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("installing SIGTERM handler");
    tokio::select! {
        res = tokio::signal::ctrl_c() => {
            res.expect("installing SIGINT handler");
            info!("received SIGINT");
        }
        _ = sigterm.recv() => info!("received SIGTERM"),
    }
}

/// Block until ctrl-c arrives (non-unix fallback).
#[cfg(not(unix))]
async fn wait_for_shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("installing ctrl-c handler");
    info!("received ctrl-c");
}

/// Load the endpoint policy from disk at startup. A missing file is not
/// fatal: the daemon starts with an empty PolicyStore, whose fail-closed
/// gate denies everything until policy arrives.
fn load_policy(state: &DaemonState) {
    let path = &state.cfg.policy_path;
    if !path.exists() {
        warn!(
            path = %path.display(),
            "no endpoint policy found — starting fail-closed (everything denied)"
        );
        return;
    }
    match read_endpoint_policy(path) {
        Ok(policy) => {
            let version = policy.version.clone();
            state
                .policy
                .write()
                .expect("policy lock poisoned")
                .load_endpoint(policy);
            info!(path = %path.display(), version, "endpoint policy loaded");
        }
        Err(e) => warn!(
            path = %path.display(),
            error = %e,
            "failed to load endpoint policy — starting fail-closed"
        ),
    }
}

/// Read the endpoint policy document: MDM wrapper packs (detected by the
/// `fabric-mdm/` format marker) go through the MDM ingest layer, everything
/// else is treated as a bare `EndpointPolicy` JSON document.
fn read_endpoint_policy(path: &std::path::Path) -> Result<fabric_types::policy::EndpointPolicy> {
    let bytes =
        std::fs::read(path).with_context(|| format!("reading policy {}", path.display()))?;
    let policy = if fabric_endpoint_mdm::is_policy_pack(&bytes) {
        fabric_endpoint_mdm::parse_policy_pack(&bytes)
            .with_context(|| format!("parsing MDM policy pack {}", path.display()))?
    } else {
        serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing endpoint policy {}", path.display()))?
    };
    Ok(policy)
}
