//! fabric-control: the control-plane binary. Serves the lease authority and
//! admin API from the library's axum router.
//!
//! Config via env vars:
//! - `FABRIC_CONTROL_ADDR` — bind address (default `127.0.0.1:47800`;
//!   loopback-only by default so an unauthenticated control plane is never
//!   exposed to the network by accident — put a proxy with auth in front
//!   before binding wider)
//! - `FABRIC_PG_URL` — **required** Postgres op-log DSN (e.g.
//!   `postgres://fabric:fabric@localhost:5432/fabric`). ADR 004: no SQLite
//!   fallback for the server.
//! - `FABRIC_KV_URL` — **required** RESP lease-authority URL (e.g.
//!   `redis://localhost:6379`; Valkey recommended).
//! - `FABRIC_SERVER_IDENTITY` — identity stamped into `granted_by`
//!   (default `fabric-server`)
//! - `FABRIC_ORG_ID` — org fallback for single-org deployments (default
//!   `default`)

use anyhow::{Context, Result};
use fabric_control::soul::SoulRegistry;
use fabric_control::{ControlState, PostgresContextStore, ValkeyLeaseAuthority};
use tracing::info;

fn required_env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("{name} is required (ADR 004: no SQLite fallback)"))
}

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

    // ADR 004: Postgres + Valkey are the only server stores. Both are
    // REQUIRED — there is no SQLite fallback path, not even for dev.
    let pg_url = required_env("FABRIC_PG_URL")?;
    let kv_url = required_env("FABRIC_KV_URL")?;

    // The op-log runs the embedded migration on connect; the SOUL registry
    // shares the resulting pool (same Postgres, same init migration).
    let pg = PostgresContextStore::connect(&pg_url)
        .await
        .with_context(|| format!("connecting context store {pg_url}"))?;
    let souls = SoulRegistry::new(pg.pool().clone());
    let kv = ValkeyLeaseAuthority::connect(&kv_url)
        .await
        .with_context(|| format!("connecting lease authority {kv_url}"))?;
    let state = ControlState::from_env(pg, kv, souls);

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
