//! fabric-control: the control-plane binary. Serves the lease authority and
//! admin API from the library's axum router.
//!
//! Config via env vars:
//! - `FABRIC_CONTROL_ADDR` — bind address (default `127.0.0.1:47800`;
//!   loopback-only by default so an unauthenticated control plane is never
//!   exposed to the network by accident — put a proxy with auth in front
//!   before binding wider)
//! - `FABRIC_CONTROL_DB` — SQLite context store path (default `fabric-control.db`)
//! - `FABRIC_IDENTITY_DB` — SQLite SOUL/device registry path
//!   (default `fabric-identity.db`)
//! - `FABRIC_SERVER_IDENTITY` — identity stamped into `granted_by`
//!   (default `fabric-server`)
//! - `FABRIC_ORG_ID` — org fallback for single-org deployments
//!   (default `default`)

use anyhow::{Context, Result};
use fabric_context::SqliteContextStore;
use fabric_control::soul::SoulRegistry;
use fabric_control::ControlState;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let telemetry = fabric_telemetry::init_telemetry(fabric_telemetry::TelemetryConfig {
        service_name: "fabric-control".into(),
        service_version: env!("CARGO_PKG_VERSION").into(),
    })
    .context("initializing telemetry")?;

    let addr: std::net::SocketAddr = std::env::var("FABRIC_CONTROL_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:47800".into())
        .parse()
        .context("parsing FABRIC_CONTROL_ADDR")?;
    let db_path = std::env::var("FABRIC_CONTROL_DB").unwrap_or_else(|_| "fabric-control.db".into());
    let identity_db_path =
        std::env::var("FABRIC_IDENTITY_DB").unwrap_or_else(|_| "fabric-identity.db".into());

    let store = SqliteContextStore::open(&db_path)
        .with_context(|| format!("opening context store {db_path}"))?;
    let souls = SoulRegistry::open(&identity_db_path)
        .with_context(|| format!("opening identity registry {identity_db_path}"))?;
    let state = ControlState::from_env(store, souls);

    info!(%addr, "fabric-control listening");
    fabric_control::serve(state, addr, wait_for_shutdown_signal()).await?;
    info!("shutdown complete");
    telemetry.shutdown().context("shutting down telemetry")?;
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
