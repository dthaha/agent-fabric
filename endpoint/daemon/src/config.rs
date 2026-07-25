//! Daemon configuration: JSON file (MDM-delivered) with safe defaults.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Daemon configuration. Loaded from a JSON file (MDM-delivered) with
/// environment overrides; every field has a safe default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    /// Stable device identifier (MDM enrollment id).
    pub device_id: String,
    /// Path to the local context store (SQLite, WAL mode).
    pub context_db: PathBuf,
    /// Path to the endpoint policy document (JSON). Missing file means the
    /// daemon starts fail-closed with an empty policy store.
    pub policy_path: PathBuf,
    /// Server-side control-plane base URL. Empty means offline-only.
    pub server_url: String,
    /// Port for the localhost health/status HTTP server.
    pub health_port: u16,
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
            policy_path: PathBuf::from("fabric-policy.json"),
            server_url: String::new(),
            health_port: 47770,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.health_port, 47770);
        assert_eq!(cfg.tool_bridge_port, 47771);
        assert!(cfg.server_url.is_empty());
        assert!(!cfg.device_id.is_empty());
    }

    #[test]
    fn partial_json_uses_defaults() {
        let cfg: DaemonConfig = serde_json::from_str(r#"{"device_id":"dev-1"}"#).unwrap();
        assert_eq!(cfg.device_id, "dev-1");
        assert_eq!(cfg.health_port, 47770);
        assert_eq!(cfg.policy_path, PathBuf::from("fabric-policy.json"));
    }
}
