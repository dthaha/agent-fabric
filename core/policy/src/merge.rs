//! Dual policy merge. The endpoint (MDM-shipped) policy is the ceiling; the
//! server policy is additive. Merge semantics are DENY WINS: nothing in the
//! server policy can loosen an endpoint restriction, and server restrictions
//! stack on top. Evaluation precedence is applied in [`crate::eval`]; the
//! merge preserves every rule so the gate can apply strictest-match.

use fabric_types::policy::{CuaPolicy, EffectivePolicy, EndpointPolicy, ServerPolicy};

/// Merge an endpoint policy with a server policy into the EffectivePolicy
/// consumed by the evaluation gate.
pub fn merge(endpoint: &EndpointPolicy, server: &ServerPolicy) -> EffectivePolicy {
    // Tool rules: endpoint rules plus server restrictions. Deny-wins
    // precedence is resolved at evaluation time across this combined set.
    let mut tool_rules = endpoint.tool_rules.clone();
    tool_rules.extend(server.tool_restrictions.iter().cloned());

    EffectivePolicy {
        endpoint_version: endpoint.version.clone(),
        server_version: server.version.clone(),
        data_rules: endpoint.data_rules.clone(),
        tool_rules,
        model_rules: endpoint.model_rules.clone(),
        cua: endpoint.cua.clone(),
        inference_rules: server.inference_rules.clone(),
        kill_switch: endpoint.kill_switch,
        max_retention_hours: endpoint.max_retention_hours,
        background_quota: server.background_quota,
        max_session_duration_hours: server.max_session_duration_hours,
        max_concurrent_sessions: server.max_concurrent_sessions,
    }
}

/// Default CUA posture when no policy has loaded yet: fail closed.
pub fn restrictive_cua_default() -> CuaPolicy {
    CuaPolicy {
        enabled: false,
        allowed_apps: vec![],
        denied_apps: vec![],
        screenshot_redaction: true,
        require_confirmation_destructive: true,
        max_actions_per_minute: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::policy::{InferenceRule, ToolAction, ToolRule};

    fn endpoint_policy() -> EndpointPolicy {
        EndpointPolicy {
            policy_id: "ep-1".into(),
            version: "3".into(),
            org_id: "org-1".into(),
            data_rules: vec![],
            tool_rules: vec![ToolRule {
                tool_pattern: "shell.*".into(),
                action: ToolAction::Allow as i32,
                condition: String::new(),
            }],
            model_rules: vec![],
            cua: None,
            kill_switch: false,
            max_retention_hours: 720,
            dlp_patterns: vec![],
            safety: None,
        }
    }

    fn server_policy() -> ServerPolicy {
        ServerPolicy {
            policy_id: "hp-1".into(),
            version: "7".into(),
            org_id: "org-1".into(),
            inference_rules: vec![InferenceRule {
                provider: "bedrock".into(),
                allowed_models: vec!["claude-*".into()],
                allowed_regions: vec!["us-east-1".into()],
                max_tokens_per_request: 8192,
                daily_token_budget: 1_000_000,
            }],
            background_quota: None,
            tool_restrictions: vec![ToolRule {
                tool_pattern: "shell.exec".into(),
                action: ToolAction::Deny as i32,
                condition: String::new(),
            }],
            max_session_duration_hours: 24,
            max_concurrent_sessions: 4,
        }
    }

    #[test]
    fn merge_stacks_tool_rules_from_both_sides() {
        let eff = merge(&endpoint_policy(), &server_policy());
        assert_eq!(eff.tool_rules.len(), 2);
        assert_eq!(eff.endpoint_version, "3");
        assert_eq!(eff.server_version, "7");
        assert_eq!(eff.inference_rules.len(), 1);
        assert_eq!(eff.max_retention_hours, 720);
        assert!(!eff.kill_switch);
    }

    #[test]
    fn merge_preserves_kill_switch_from_endpoint() {
        let mut ep = endpoint_policy();
        ep.kill_switch = true;
        let eff = merge(&ep, &server_policy());
        assert!(eff.kill_switch);
    }
}
