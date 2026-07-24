//! Versioned policy store with hot-reload. Holds the current endpoint and
//! hosted policies, re-merges them on every load, and hands out fresh gates
//! reflecting the latest merged state — no restart required.

use fabric_types::policy::{EffectivePolicy, EndpointPolicy, HostedPolicy};

use crate::eval::PolicyGate;
use crate::merge::merge;

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
