//! Versioned policy store with hot-reload. Holds the current endpoint and
//! hosted policies, re-merges them on every load, and hands out fresh gates
//! reflecting the latest merged state — no restart required.

use std::path::Path;

use fabric_types::policy::{EffectivePolicy, EndpointPolicy, HostedPolicy};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::eval::PolicyGate;
use crate::merge::merge;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("policy store I/O at {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("policy store serialization: {0}")]
    Serde(#[from] serde_json::Error),
}

/// On-disk form of the store: both policy documents, versioned.
#[derive(Debug, Serialize, Deserialize)]
struct PersistedPolicies {
    endpoint: Option<EndpointPolicy>,
    hosted: Option<HostedPolicy>,
}

/// Holds the latest endpoint + hosted policies and their merged product.
/// Loading a new policy version re-merges immediately; the next [`Self::gate`]
/// call reflects it.
#[derive(Debug, Default)]
pub struct PolicyStore {
    endpoint: Option<EndpointPolicy>,
    hosted: Option<HostedPolicy>,
    effective: EffectivePolicy,
}

impl PolicyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a new endpoint policy version and re-merge.
    pub fn load_endpoint(&mut self, policy: EndpointPolicy) {
        self.endpoint = Some(policy);
        self.remerge();
    }

    /// Read an endpoint policy from a JSON file and load it, re-merging
    /// automatically. The file must be a bare `EndpointPolicy` document
    /// (pbjson serde); MDM wrapper formats are unwrapped by the MDM ingest
    /// layer before they reach the store.
    pub fn load_endpoint_from_file(&mut self, path: impl AsRef<Path>) -> Result<(), StoreError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| StoreError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let policy: EndpointPolicy = serde_json::from_slice(&bytes)?;
        self.load_endpoint(policy);
        Ok(())
    }

    /// Store a new hosted policy version and re-merge.
    pub fn load_hosted(&mut self, policy: HostedPolicy) {
        self.hosted = Some(policy);
        self.remerge();
    }

    /// A gate over the current merged state. DLP patterns from the endpoint
    /// policy are attached to the gate.
    pub fn gate(&self) -> PolicyGate {
        let dlp = self
            .endpoint
            .as_ref()
            .map(|e| e.dlp_patterns.clone())
            .unwrap_or_default();
        PolicyGate::new(self.effective.clone()).with_dlp_patterns(dlp)
    }

    /// Version of the loaded endpoint policy, if any.
    pub fn endpoint_version(&self) -> Option<&str> {
        self.endpoint.as_ref().map(|e| e.version.as_str())
    }

    /// Version of the loaded hosted policy, if any.
    pub fn hosted_version(&self) -> Option<&str> {
        self.hosted.as_ref().map(|h| h.version.as_str())
    }

    /// The current merged policy.
    pub fn effective(&self) -> &EffectivePolicy {
        &self.effective
    }

    fn remerge(&mut self) {
        let endpoint = self.endpoint.clone().unwrap_or_default();
        let hosted = self.hosted.clone().unwrap_or_default();
        self.effective = merge(&endpoint, &hosted);
    }

    /// Persist both policy documents to `path` as JSON.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), StoreError> {
        let path = path.as_ref();
        let persisted = PersistedPolicies {
            endpoint: self.endpoint.clone(),
            hosted: self.hosted.clone(),
        };
        let json = serde_json::to_vec_pretty(&persisted)?;
        std::fs::write(path, json).map_err(|source| StoreError::Io {
            path: path.display().to_string(),
            source,
        })
    }

    /// Load both policy documents from `path`, re-merging automatically.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| StoreError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let persisted: PersistedPolicies = serde_json::from_slice(&bytes)?;
        let mut store = Self {
            endpoint: persisted.endpoint,
            hosted: persisted.hosted,
            effective: EffectivePolicy::default(),
        };
        store.remerge();
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::policy::{ToolAction, ToolRule};

    use crate::eval::Decision;

    fn endpoint(version: &str, tool_rules: Vec<ToolRule>) -> EndpointPolicy {
        EndpointPolicy {
            policy_id: "ep".into(),
            version: version.into(),
            org_id: "org".into(),
            data_rules: vec![],
            tool_rules,
            model_rules: vec![],
            cua: None,
            kill_switch: false,
            max_retention_hours: 0,
            dlp_patterns: vec![],
            safety: None,
        }
    }

    fn hosted(version: &str) -> HostedPolicy {
        HostedPolicy {
            policy_id: "hp".into(),
            version: version.into(),
            org_id: "org".into(),
            inference_rules: vec![],
            background_quota: None,
            tool_restrictions: vec![],
            max_session_duration_hours: 0,
            max_concurrent_sessions: 0,
        }
    }

    fn allow(pattern: &str) -> ToolRule {
        ToolRule {
            tool_pattern: pattern.into(),
            action: ToolAction::Allow as i32,
            condition: String::new(),
        }
    }

    #[test]
    fn load_endpoint_from_file_round_trip() {
        let path = std::env::temp_dir().join(format!(
            "fabric-endpoint-policy-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            serde_json::to_vec(&endpoint("v5", vec![allow("fs.read")])).unwrap(),
        )
        .unwrap();

        let mut store = PolicyStore::new();
        store.load_endpoint_from_file(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(store.endpoint_version(), Some("v5"));
        assert!(store.gate().check_tool("fs.read").is_allowed());
        assert!(matches!(
            store.gate().check_tool("shell.exec"),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn load_endpoint_from_file_missing_errors() {
        let mut store = PolicyStore::new();
        let res = store.load_endpoint_from_file("/nonexistent/fabric-policy.json");
        assert!(res.is_err());
        // Failed load leaves the store empty and fail-closed.
        assert_eq!(store.endpoint_version(), None);
        assert!(matches!(
            store.gate().check_tool("fs.read"),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn load_endpoint_from_file_rejects_corrupt_json() {
        let path = std::env::temp_dir().join(format!(
            "fabric-endpoint-policy-corrupt-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, b"{not json").unwrap();

        let mut store = PolicyStore::new();
        let res = store.load_endpoint_from_file(&path);
        std::fs::remove_file(&path).ok();

        assert!(matches!(res, Err(StoreError::Serde(_))));
        assert_eq!(store.endpoint_version(), None);
    }

    #[test]
    fn empty_store_fails_closed() {
        let store = PolicyStore::new();
        assert_eq!(store.endpoint_version(), None);
        assert_eq!(store.hosted_version(), None);
        assert!(matches!(
            store.gate().check_tool("fs.read"),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn hot_reload_replaces_rules() {
        let mut store = PolicyStore::new();
        store.load_endpoint(endpoint("v1", vec![allow("shell.*")]));
        store.load_hosted(hosted("v1"));
        assert_eq!(store.endpoint_version(), Some("v1"));
        assert!(store.gate().check_tool("shell.exec").is_allowed());

        // v2 is stricter: only fs.read allowed, shell gone.
        store.load_endpoint(endpoint("v2", vec![allow("fs.read")]));
        assert_eq!(store.endpoint_version(), Some("v2"));
        let gate = store.gate();
        assert!(matches!(gate.check_tool("shell.exec"), Decision::Deny(_)));
        assert!(gate.check_tool("fs.read").is_allowed());
    }

    #[test]
    fn hosted_reload_stacks_restrictions() {
        let mut store = PolicyStore::new();
        store.load_endpoint(endpoint("v1", vec![allow("shell.*")]));
        store.load_hosted(hosted("v1"));
        assert!(store.gate().check_tool("shell.exec").is_allowed());

        let mut hp = hosted("v2");
        hp.tool_restrictions = vec![ToolRule {
            tool_pattern: "shell.exec".into(),
            action: ToolAction::Deny as i32,
            condition: String::new(),
        }];
        store.load_hosted(hp);
        assert_eq!(store.hosted_version(), Some("v2"));
        let gate = store.gate();
        assert!(matches!(gate.check_tool("shell.exec"), Decision::Deny(_)));
        assert!(gate.check_tool("shell.list").is_allowed());
    }

    #[test]
    fn save_load_round_trip() {
        let mut store = PolicyStore::new();
        let mut ep = endpoint("v9", vec![allow("shell.*")]);
        ep.dlp_patterns = vec![fabric_types::policy::DlpPattern {
            name: "ssn".into(),
            regex: r"\b\d{3}-\d{2}-\d{4}\b".into(),
            action: fabric_types::policy::DlpAction::Redact as i32,
        }];
        store.load_endpoint(ep);
        let mut hp = hosted("v4");
        hp.tool_restrictions = vec![ToolRule {
            tool_pattern: "shell.exec".into(),
            action: ToolAction::Deny as i32,
            condition: String::new(),
        }];
        hp.max_concurrent_sessions = 7;
        store.load_hosted(hp);

        let path =
            std::env::temp_dir().join(format!("fabric-policy-store-{}.json", std::process::id()));
        store.save(&path).unwrap();
        let loaded = PolicyStore::load(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.endpoint_version(), Some("v9"));
        assert_eq!(loaded.hosted_version(), Some("v4"));
        assert_eq!(loaded.effective(), store.effective());

        let gate = loaded.gate();
        assert!(matches!(gate.check_tool("shell.exec"), Decision::Deny(_)));
        assert!(gate.check_tool("shell.list").is_allowed());
        assert!(matches!(
            gate.check_session_limits(0.0, 7),
            Decision::Deny(_)
        ));
        let out = gate.scan_dlp("ssn 123-45-6789").unwrap();
        assert!(out.redacted_content.contains("[REDACTED:ssn]"));
    }

    #[test]
    fn load_missing_file_errors() {
        let res = PolicyStore::load("/nonexistent/fabric-policy.json");
        assert!(res.is_err());
    }

    #[test]
    fn gate_reflects_endpoint_dlp_patterns() {
        let mut store = PolicyStore::new();
        let mut ep = endpoint("v1", vec![]);
        ep.dlp_patterns = vec![fabric_types::policy::DlpPattern {
            name: "ssn".into(),
            regex: r"\b\d{3}-\d{2}-\d{4}\b".into(),
            action: fabric_types::policy::DlpAction::Redact as i32,
        }];
        store.load_endpoint(ep);
        let out = store.gate().scan_dlp("ssn 123-45-6789").unwrap();
        assert!(out.redacted_content.contains("[REDACTED:ssn]"));
    }
}
