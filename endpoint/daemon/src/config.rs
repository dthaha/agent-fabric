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
    /// `./fabric-endpoint.json` if present. On first boot (no config file)
    /// the defaults — including a freshly generated device id — are
    /// generated AND persisted, so the device id is stable across restarts.
    /// If the config file cannot be written the load fails closed: starting
    /// with an ephemeral device id would fork the device's identity on
    /// every boot.
    /// `FABRIC_SERVER_URL` overrides the configured server base URL.
    pub fn load() -> Result<Self> {
        let path = std::env::var("FABRIC_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("fabric-endpoint.json"));
        let mut cfg = if !path.exists() {
            let cfg = Self::default();
            let raw = serde_json::to_string_pretty(&cfg).context("serializing default config")?;
            std::fs::write(&path, raw).with_context(|| {
                format!(
                    "writing initial config {} (device id must persist across boots)",
                    path.display()
                )
            })?;
            info!(?path, "first boot: generated and persisted daemon config");
            cfg
        } else {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading config {}", path.display()))?;
            let cfg: Self = serde_json::from_str(&raw)
                .with_context(|| format!("parsing config {}", path.display()))?;
            info!(?path, "loaded daemon config");
            cfg
        };
        if let Ok(url) = std::env::var("FABRIC_SERVER_URL") {
            cfg.server_url = url;
        }
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

    #[test]
    fn first_boot_persists_device_id_and_reloads_it() {
        let dir = std::env::temp_dir().join(format!("fabric-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fabric-endpoint.json");
        std::env::set_var("FABRIC_CONFIG", &path);

        // First boot: no file — defaults are generated and persisted.
        let first = DaemonConfig::load().unwrap();
        assert!(path.exists(), "config must be persisted on first boot");
        let device_id = first.device_id.clone();
        assert!(!device_id.is_empty());

        // Second boot: the same device id comes back from disk.
        let second = DaemonConfig::load().unwrap();
        assert_eq!(second.device_id, device_id);

        std::env::remove_var("FABRIC_CONFIG");
        std::fs::remove_dir_all(&dir).ok();
    }
}
