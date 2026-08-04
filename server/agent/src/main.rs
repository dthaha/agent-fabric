//! Server-side agent loop: runs turns for sessions whose lease is server-side
//! (long-horizon, background, or weak-endpoint cases) and calls endpoint
//! tools over the authenticated bridge. Leased with the context plane.

use anyhow::{Context, Result};
use fabric_control::ValkeyLeaseAuthority;
use fabric_server::job_spec::{self, JobSpecConfig};
use fabric_server::AgentTaskOrchestrator;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    let telemetry = fabric_telemetry::init_telemetry(fabric_telemetry::TelemetryConfig {
        service_name: "fabric-server".into(),
        service_version: env!("CARGO_PKG_VERSION").into(),
    })
    .context("initializing telemetry")?;

    match init_orchestrator().await {
        Ok(Some(_orchestrator)) => info!("agent task orchestrator ready"),
        Ok(None) => info!("FABRIC_PG_URL/FABRIC_KV_URL unset; orchestrator disabled (dev mode)"),
        Err(e) => warn!(error = %e, "orchestrator init failed; running without it"),
    }

    wait_for_shutdown_signal().await;
    info!("shutdown complete");
    telemetry.shutdown().context("shutting down telemetry")?;
    Ok(())
}

/// Build the task orchestrator from env config. `Ok(None)` when the
/// Postgres/Valkey URLs are not configured (local dev without the server
/// backends).
async fn init_orchestrator() -> Result<Option<AgentTaskOrchestrator<ValkeyLeaseAuthority>>> {
    let (Ok(pg_url), Ok(kv_url)) = (
        std::env::var("FABRIC_PG_URL"),
        std::env::var("FABRIC_KV_URL"),
    ) else {
        return Ok(None);
    };

    let pool = sqlx::PgPool::connect(&pg_url)
        .await
        .context("connecting to Postgres")?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("running agent task migrations")?;
    let leases = ValkeyLeaseAuthority::connect(&kv_url)
        .await
        .context("connecting to Valkey")?;
    let kube = kube::Client::try_default()
        .await
        .context("creating kube client")?;

    let namespace = std::env::var("FABRIC_K8S_NAMESPACE")
        .unwrap_or_else(|_| job_spec::DEFAULT_NAMESPACE.into());
    let creds_secret_name = std::env::var("FABRIC_K8S_CREDS_SECRET")
        .unwrap_or_else(|_| job_spec::CREDS_SECRET_NAME.into());
    let spec = JobSpecConfig {
        namespace,
        pg_url,
        kv_url,
        creds_secret_name,
    };
    Ok(Some(AgentTaskOrchestrator::new(kube, leases, pool, spec)))
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
