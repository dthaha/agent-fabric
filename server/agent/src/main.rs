//! Server-side agent loop: runs turns for sessions whose lease is server-side
//! (long-horizon, background, or weak-endpoint cases) and calls endpoint
//! tools over the authenticated bridge. Leased with the context plane.

use anyhow::{Context, Result};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let telemetry = fabric_telemetry::init_telemetry(fabric_telemetry::TelemetryConfig {
        service_name: "fabric-server".into(),
        service_version: env!("CARGO_PKG_VERSION").into(),
    })
    .context("initializing telemetry")?;

    info!("fabric-server starting (agent loop server lands in a later phase)");

    wait_for_shutdown_signal().await;
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
