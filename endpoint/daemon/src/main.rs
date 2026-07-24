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

    #[cfg(feature = "enterprise")]
    info!("enterprise features compiled in (mdm, audit-siem, ha, private-registry)");

    if cfg.hosted_url.is_empty() {
        warn!("no hosted URL configured — running offline-only");
    }

    // Health/status server on localhost, with graceful shutdown: it stops
    // accepting connections and drains in-flight requests when signaled.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, cfg.health_port));
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(http::serve(Arc::clone(&state), addr, async move {
        shutdown_rx.await.ok();
    }));

    // The daemon's long-running services (classifier, tool bridge, CUA,
    // seeding) attach here in later phases.
    tokio::signal::ctrl_c().await?;
    info!("shutdown signal received");
    let _ = shutdown_tx.send(());
    server
        .await
        .context("health server task panicked")?
        .context("health server error")?;
    info!("shutdown complete");
    Ok(())
}
