//! Full policy lifecycle: dual load, merge, gate evaluation, kill switch,
//! and hot-reload — through the PolicyStore, as a deployment would drive it.

use fabric_policy::{Decision, ModelLocus, PolicyStore};
use fabric_types::policy::{
    DlpAction, DlpPattern, EndpointPolicy, InferenceRule, ServerPolicy, ToolAction, ToolRule,
};

fn tool_rule(pattern: &str, action: ToolAction) -> ToolRule {
    ToolRule {
        tool_pattern: pattern.into(),
        action: action as i32,
        condition: String::new(),
    }
}

fn endpoint_policy(version: &str, kill_switch: bool, cua_allowed: bool) -> EndpointPolicy {
    let mut tool_rules = vec![tool_rule("shell.*", ToolAction::Allow)];
    if !cua_allowed {
        tool_rules.push(tool_rule("cua.*", ToolAction::Deny));
    } else {
        tool_rules.push(tool_rule("cua.*", ToolAction::Allow));
    }
    EndpointPolicy {
        policy_id: "ep-1".into(),
        version: version.into(),
        org_id: "org-1".into(),
        data_rules: vec![],
        tool_rules,
        model_rules: vec![],
        cua: None,
        kill_switch,
        max_retention_hours: 720,
        dlp_patterns: vec![DlpPattern {
            name: "ssn".into(),
            regex: r"\b\d{3}-\d{2}-\d{4}\b".into(),
            action: DlpAction::Redact as i32,
        }],
        safety: None,
    }
}

fn server_policy(version: &str) -> ServerPolicy {
    ServerPolicy {
        policy_id: "hp-1".into(),
        version: version.into(),
        org_id: "org-1".into(),
        inference_rules: vec![InferenceRule {
            provider: "bedrock".into(),
            allowed_models: vec!["claude-*".into()],
            allowed_regions: vec![],
            max_tokens_per_request: 8192,
            daily_token_budget: 0,
        }],
        background_quota: None,
        tool_restrictions: vec![tool_rule("shell.exec", ToolAction::Deny)],
        max_session_duration_hours: 24,
        max_concurrent_sessions: 4,
    }
}

#[test]
fn full_policy_lifecycle() {
    let mut store = PolicyStore::new();
    store.load_endpoint(endpoint_policy("v1", false, false));
    store.load_server(server_policy("v1"));

    let gate = store.gate();
    assert_eq!(store.endpoint_version(), Some("v1"));
    assert_eq!(store.server_version(), Some("v1"));

    // Endpoint allow minus server restriction.
    assert!(gate.check_tool("shell.list").is_allowed());
    assert!(matches!(gate.check_tool("shell.exec"), Decision::Deny(_)));

    // Endpoint deny.
    assert!(matches!(gate.check_tool("cua.click"), Decision::Deny(_)));

    // Server-side inference rules.
    assert!(gate
        .check_inference("bedrock", "claude-sonnet", 4096)
        .is_allowed());
    assert!(matches!(
        gate.check_inference("bedrock", "gpt-4o", 100),
        Decision::Deny(_)
    ));
    assert!(matches!(
        gate.check_model("bedrock/claude-sonnet", ModelLocus::Server),
        Decision::Allow
    ));

    // DLP scan redacts SSNs.
    let out = gate.scan_dlp("employee ssn: 123-45-6789").unwrap();
    assert_eq!(out.action, Some(DlpAction::Redact));
    assert!(out.redacted_content.contains("[REDACTED:ssn]"));
    assert!(!out.redacted_content.contains("123-45-6789"));

    // Session limits from server policy flow through the merge.
    assert!(gate.check_session_limits(1.0, 2).is_allowed());
    assert!(matches!(
        gate.check_session_limits(25.0, 2),
        Decision::Deny(_)
    ));
    assert!(matches!(
        gate.check_session_limits(1.0, 4),
        Decision::Deny(_)
    ));

    // Kill switch engages: everything denies.
    store.load_endpoint(endpoint_policy("v2", true, false));
    assert_eq!(store.endpoint_version(), Some("v2"));
    let gate = store.gate();
    assert!(matches!(gate.check_tool("shell.list"), Decision::Deny(_)));
    assert!(matches!(gate.check_tool("cua.click"), Decision::Deny(_)));
    assert!(matches!(
        gate.check_inference("bedrock", "claude-sonnet", 100),
        Decision::Deny(_)
    ));

    // Hot-reload: kill switch off, CUA now allowed — no restart.
    store.load_endpoint(endpoint_policy("v3", false, true));
    assert_eq!(store.endpoint_version(), Some("v3"));
    let gate = store.gate();
    assert!(gate.check_tool("cua.click").is_allowed());
    assert!(gate.check_tool("shell.list").is_allowed());
    assert!(matches!(gate.check_tool("shell.exec"), Decision::Deny(_)));
    assert!(gate
        .check_inference("bedrock", "claude-sonnet", 4096)
        .is_allowed());
}
