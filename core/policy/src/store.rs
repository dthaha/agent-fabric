//! Versioned policy store with hot-reload. Holds the current endpoint and
//! server policies, re-merges them on every load, and hands out fresh gates
//! reflecting the latest merged state — no restart required.

use std::path::Path;

use fabric_types::policy::{EffectivePolicy, EndpointPolicy, ServerPolicy};
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
    #[error(
        "policy downgrade rejected: current version '{current}', attempted '{attempted}' \
         (pass force = true to override)"
    )]
    Downgrade { current: String, attempted: String },
    #[error("policy org mismatch: existing policy is org '{existing}', new policy is org '{new}'")]
    OrgMismatch { existing: String, new: String },
}

/// Compare policy versions: numeric when both sides parse (a leading `v` is
/// tolerated), lexicographic otherwise. Missing/empty versions sort lowest.
fn version_lt(a: &str, b: &str) -> bool {
    let num = |s: &str| s.trim().trim_start_matches(['v', 'V']).parse::<u64>().ok();
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x < y,
        _ => a < b,
    }
}

/// On-disk form of the store: both policy documents, versioned.
#[derive(Debug, Serialize, Deserialize)]
struct PersistedPolicies {
    endpoint: Option<EndpointPolicy>,
    server: Option<ServerPolicy>,
}

/// Reject cross-org policy merges. An empty `org_id` on either side is
/// treated as unspecified and skipped (pre-org deployments); when both are
/// set they must match exactly.
fn check_org(existing: &str, new: &str) -> Result<(), StoreError> {
    if !existing.is_empty() && !new.is_empty() && existing != new {
        return Err(StoreError::OrgMismatch {
            existing: existing.to_string(),
            new: new.to_string(),
        });
    }
    Ok(())
}

/// Holds the latest endpoint + server policies and their merged product.
/// Loading a new policy version re-merges immediately; the next [`Self::gate`]
/// call reflects it.
#[derive(Debug, Default)]
pub struct PolicyStore {
    endpoint: Option<EndpointPolicy>,
    server: Option<ServerPolicy>,
    effective: EffectivePolicy,
}

impl PolicyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a new endpoint policy version and re-merge. Rejects version
    /// downgrades and org changes.
    pub fn load_endpoint(&mut self, policy: EndpointPolicy) -> Result<(), StoreError> {
        self.load_endpoint_with_force(policy, false)
    }

    /// Store a new endpoint policy version and re-merge. A policy pack with
    /// a lower version than the current one, or an `org_id` that does not
    /// match the incumbent policies, is rejected unless `force` is set
    /// (downgrades only — org changes are never forced silently: the org
    /// check still applies).
    pub fn load_endpoint_with_force(
        &mut self,
        policy: EndpointPolicy,
        force: bool,
    ) -> Result<(), StoreError> {
        if let Some(current) = &self.endpoint {
            check_org(&current.org_id, &policy.org_id)?;
            if !force && version_lt(&policy.version, &current.version) {
                return Err(StoreError::Downgrade {
                    current: current.version.clone(),
                    attempted: policy.version.clone(),
                });
            }
        }
        if let Some(server) = &self.server {
            check_org(&server.org_id, &policy.org_id)?;
        }
        self.endpoint = Some(policy);
        self.remerge();
        Ok(())
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
        self.load_endpoint(policy)
    }

    /// Store a new server policy version and re-merge. Rejects version
    /// downgrades and org changes.
    pub fn load_server(&mut self, policy: ServerPolicy) -> Result<(), StoreError> {
        self.load_server_with_force(policy, false)
    }

    /// Store a new server policy version and re-merge. Same downgrade and
    /// org checks as [`PolicyStore::load_endpoint_with_force`].
    pub fn load_server_with_force(
        &mut self,
        policy: ServerPolicy,
        force: bool,
    ) -> Result<(), StoreError> {
        if let Some(current) = &self.server {
            check_org(&current.org_id, &policy.org_id)?;
            if !force && version_lt(&policy.version, &current.version) {
                return Err(StoreError::Downgrade {
                    current: current.version.clone(),
                    attempted: policy.version.clone(),
                });
            }
        }
        if let Some(endpoint) = &self.endpoint {
            check_org(&endpoint.org_id, &policy.org_id)?;
        }
        self.server = Some(policy);
        self.remerge();
        Ok(())
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

    /// Version of the loaded server policy, if any.
    pub fn server_version(&self) -> Option<&str> {
        self.server.as_ref().map(|h| h.version.as_str())
    }

    /// The current merged policy.
    pub fn effective(&self) -> &EffectivePolicy {
        &self.effective
    }

    fn remerge(&mut self) {
        let endpoint = self.endpoint.clone().unwrap_or_default();
        let server = self.server.clone().unwrap_or_default();
        self.effective = merge(&endpoint, &server);
    }

    /// Persist both policy documents to `path` as JSON.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), StoreError> {
        let path = path.as_ref();
        let persisted = PersistedPolicies {
            endpoint: self.endpoint.clone(),
            server: self.server.clone(),
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
            server: persisted.server,
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

    fn server(version: &str) -> ServerPolicy {
        ServerPolicy {
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
        assert_eq!(store.server_version(), None);
        assert!(matches!(
            store.gate().check_tool("fs.read"),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn hot_reload_replaces_rules() {
        let mut store = PolicyStore::new();
        store
            .load_endpoint(endpoint("v1", vec![allow("shell.*")]))
            .unwrap();
        store.load_server(server("v1")).unwrap();
        assert_eq!(store.endpoint_version(), Some("v1"));
        assert!(store.gate().check_tool("shell.exec").is_allowed());

        // v2 is stricter: only fs.read allowed, shell gone.
        store
            .load_endpoint(endpoint("v2", vec![allow("fs.read")]))
            .unwrap();
        assert_eq!(store.endpoint_version(), Some("v2"));
        let gate = store.gate();
        assert!(matches!(gate.check_tool("shell.exec"), Decision::Deny(_)));
        assert!(gate.check_tool("fs.read").is_allowed());
    }

    #[test]
    fn server_reload_stacks_restrictions() {
        let mut store = PolicyStore::new();
        store
            .load_endpoint(endpoint("v1", vec![allow("shell.*")]))
            .unwrap();
        store.load_server(server("v1")).unwrap();
        assert!(store.gate().check_tool("shell.exec").is_allowed());

        let mut hp = server("v2");
        hp.tool_restrictions = vec![ToolRule {
            tool_pattern: "shell.exec".into(),
            action: ToolAction::Deny as i32,
            condition: String::new(),
        }];
        store.load_server(hp).unwrap();
        assert_eq!(store.server_version(), Some("v2"));
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
        store.load_endpoint(ep).unwrap();
        let mut hp = server("v4");
        hp.tool_restrictions = vec![ToolRule {
            tool_pattern: "shell.exec".into(),
            action: ToolAction::Deny as i32,
            condition: String::new(),
        }];
        hp.max_concurrent_sessions = 7;
        store.load_server(hp).unwrap();

        let path =
            std::env::temp_dir().join(format!("fabric-policy-store-{}.json", std::process::id()));
        store.save(&path).unwrap();
        let loaded = PolicyStore::load(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.endpoint_version(), Some("v9"));
        assert_eq!(loaded.server_version(), Some("v4"));
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
    fn downgrade_rejected_unless_forced() {
        let mut store = PolicyStore::new();
        store.load_endpoint(endpoint("v3", vec![])).unwrap();

        // Older endpoint policy: rejected, current policy untouched.
        let err = store.load_endpoint(endpoint("v2", vec![allow("shell.*")]));
        assert!(matches!(err, Err(StoreError::Downgrade { .. })), "{err:?}");
        assert_eq!(store.endpoint_version(), Some("v3"));
        assert!(matches!(
            store.gate().check_tool("shell.exec"),
            Decision::Deny(_)
        ));

        // Same version is a reload, not a downgrade.
        store.load_endpoint(endpoint("v3", vec![])).unwrap();

        // Force overrides the downgrade check.
        store
            .load_endpoint_with_force(endpoint("v2", vec![allow("shell.*")]), true)
            .unwrap();
        assert_eq!(store.endpoint_version(), Some("v2"));

        // Server policy: same rule.
        store.load_server(server("v5")).unwrap();
        assert!(matches!(
            store.load_server(server("v4")),
            Err(StoreError::Downgrade { .. })
        ));
        assert_eq!(store.server_version(), Some("v5"));
        store.load_server_with_force(server("v4"), true).unwrap();
        assert_eq!(store.server_version(), Some("v4"));
    }

    #[test]
    fn cross_org_policies_rejected() {
        let mut store = PolicyStore::new();
        store.load_endpoint(endpoint("v1", vec![])).unwrap();

        // A new endpoint policy for a different org is rejected.
        let mut other = endpoint("v2", vec![]);
        other.org_id = "org-2".into();
        assert!(matches!(
            store.load_endpoint(other),
            Err(StoreError::OrgMismatch { .. })
        ));
        assert_eq!(store.endpoint_version(), Some("v1"));

        // A server policy for a different org than the endpoint policy is
        // rejected (cross-org merges never happen).
        let mut hp = server("v1");
        hp.org_id = "org-2".into();
        assert!(matches!(
            store.load_server(hp),
            Err(StoreError::OrgMismatch { .. })
        ));
        assert_eq!(store.server_version(), None);

        // Matching orgs merge fine.
        store.load_server(server("v1")).unwrap();
        assert_eq!(store.server_version(), Some("v1"));
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
        store.load_endpoint(ep).unwrap();
        let out = store.gate().scan_dlp("ssn 123-45-6789").unwrap();
        assert!(out.redacted_content.contains("[REDACTED:ssn]"));
    }
}
