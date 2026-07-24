//! Endpoint daemon: the long-running agent service shipped to managed
//! laptops via MDM. Single static binary, no runtime dependencies. Owns the
//! local context store, the offline classifier, seeded models, the tool
//! bridge, and the CUA actuator.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

/// Daemon configuration. Loaded from a JSON file (MDM-delivered) with
/// environment overrides; every field has a safe default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    /// Stable device identifier (MDM enrollment id).
    pub device_id: String,
    /// Path to the local context store (SQLite, WAL mode).
    pub context_db: PathBuf,
    /// Hosted control-plane base URL. Empty means offline-only.
    pub hosted_url: String,
    /// Port for the local tool bridge server.
    pub tool_bridge_port: u16,
    /// Disk budget for seeded models, in MiB.
    pub model_disk_budget_mib: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            device_id: format!("device-{}", uuid::Uuid::now_v7()),
            context_db: PathBuf::from("fabric-context.db"),
            hosted_url: String::new(),
            tool_bridge_port: 47771,
            model_disk_budget_mib: 8192,
        }
    }
}

impl DaemonConfig {
    /// Load config from `FABRIC_CONFIG` (path to JSON), falling back to
    /// `./fabric-endpoint.json` if present, then to defaults.
    pub fn load() -> Result<Self> {
        let path = std::env::var("FABRIC_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("fabric-endpoint.json"));
        if !path.exists() {
            info!(?path, "no config file found, using defaults");
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Self = serde_json::from_str(&raw)
            .with_context(|| format!("parsing config {}", path.display()))?;
        info!(?path, "loaded daemon config");
        Ok(cfg)
    }
}

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
        tool_bridge_port = cfg.tool_bridge_port,
        "fabric-endpoint starting"
    );

    let store = fabric_context::ContextStore::open(&cfg.context_db)
        .with_context(|| format!("opening context store {}", cfg.context_db.display()))?;
    info!("context store ready (WAL mode)");

    #[cfg(feature = "enterprise")]
    info!("enterprise features compiled in (mdm, audit-siem, ha, private-registry)");

    if cfg.hosted_url.is_empty() {
        warn!("no hosted URL configured — running offline-only");
    }

    // The daemon's long-running services (classifier, tool bridge, CUA,
    // seeding) attach here in later phases. Phase 0 keeps the process alive
    // so supervisors can health-check it.
    drop(store);
    tokio::signal::ctrl_c().await?;
    info!("shutdown signal received, exiting");
    Ok(())
}
