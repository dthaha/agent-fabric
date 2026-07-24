//! The evaluation gate. Every tool call, model selection, inference request,
//! data egress, and CUA action passes through here. DENY WINS: across all
//! matching rules the strictest outcome applies, and unknown requests fail
//! closed.

use regex::Regex;
use thiserror::Error;

use fabric_types::policy::{DlpAction, DlpPattern, EffectivePolicy, ToolAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
    RequireApproval(String),
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

#[derive(Debug, Error)]
pub enum EvalError {
    #[error("invalid DLP pattern '{name}': {source}")]
    InvalidDlpPattern { name: String, source: regex::Error },
}

/// Where a model is being asked to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLocus {
    Local,
    Hosted,
}

/// The policy gate. Constructed from an EffectivePolicy (the merged product
/// of endpoint + hosted policy) and consulted before every privileged act.
pub struct PolicyGate {
    effective: EffectivePolicy,
    dlp_patterns: Vec<DlpPattern>,
}

impl PolicyGate {
    pub fn new(effective: EffectivePolicy) -> Self {
        Self {
            effective,
            dlp_patterns: Vec::new(),
        }
    }

    pub fn effective(&self) -> &EffectivePolicy {
        &self.effective
    }

    /// Kill switch: when set, everything halts.
    pub fn is_killed(&self) -> bool {
        self.effective.kill_switch
    }

    /// Gate a tool invocation. All matching rules are considered; the
    /// strictest action wins (DENY > REQUIRE_APPROVAL > ALLOW). No matching
    /// rule means deny (fail closed). Rule conditions are ignored.
    pub fn check_tool(&self, tool_name: &str) -> Decision {
        self.eval_tool(tool_name, |_| true)
    }

    /// Gate a tool invocation with execution context, evaluating rule
    /// conditions. A rule whose condition does not match the current locus /
    /// local hour is skipped. Empty conditions always match.
    pub fn check_tool_with_context(&self, tool_name: &str, locus: &str, hour: u32) -> Decision {
        self.eval_tool(tool_name, |rule| {
            condition_matches(&rule.condition, locus, hour)
        })
    }

    fn eval_tool(
        &self,
        tool_name: &str,
        condition_ok: impl Fn(&fabric_types::policy::ToolRule) -> bool,
    ) -> Decision {
        if self.is_killed() {
            return Decision::Deny("kill switch engaged".into());
        }
        let mut saw_allow = false;
        let mut approval: Option<String> = None;
        for rule in &self.effective.tool_rules {
            if !glob_matches(&rule.tool_pattern, tool_name) || !condition_ok(rule) {
                continue;
            }
            match ToolAction::try_from(rule.action) {
                Ok(ToolAction::Deny) => {
                    return Decision::Deny(format!(
                        "tool '{tool_name}' denied by rule '{}'",
                        rule.tool_pattern
                    ));
                }
                Ok(ToolAction::RequireApproval) => {
                    approval = Some(format!(
                        "tool '{tool_name}' requires approval by rule '{}'",
                        rule.tool_pattern
                    ));
                }
                Ok(ToolAction::Allow) => saw_allow = true,
                _ => {}
            }
        }
        if let Some(reason) = approval {
            return Decision::RequireApproval(reason);
        }
        if saw_allow {
            return Decision::Allow;
        }
        Decision::Deny(format!("tool '{tool_name}' has no allowing rule"))
    }

    /// Gate a model selection for a given locus.
    pub fn check_model(&self, model_id: &str, locus: ModelLocus) -> Decision {
        if self.is_killed() {
            return Decision::Deny("kill switch engaged".into());
        }
        for rule in &self.effective.model_rules {
            if !glob_matches(&rule.model_pattern, model_id) {
                continue;
            }
            let allowed = match locus {
                ModelLocus::Local => rule.allowed_local,
                ModelLocus::Hosted => rule.allowed_hosted,
            };
            if !allowed {
                return Decision::Deny(format!(
                    "model '{model_id}' not allowed {:?} by rule '{}'",
                    locus, rule.model_pattern
                ));
            }
        }
        Decision::Allow
    }

    /// Gate a request to run the loop in the background (hosted) against the
    /// hosted background quota. No quota configured means no restriction.
    pub fn check_background_quota(
        &self,
        active_background: u32,
        daily_turns_used: u32,
        user_consented: bool,
    ) -> Decision {
        if self.is_killed() {
            return Decision::Deny("kill switch engaged".into());
        }
        let Some(quota) = &self.effective.background_quota else {
            return Decision::Allow;
        };
        if quota.max_concurrent_background > 0
            && active_background >= quota.max_concurrent_background
        {
            return Decision::Deny(format!(
                "background concurrency limit {} reached",
                quota.max_concurrent_background
            ));
        }
        if quota.max_daily_hosted_turns > 0 && daily_turns_used >= quota.max_daily_hosted_turns {
            return Decision::Deny(format!(
                "daily hosted turn budget {} exhausted",
                quota.max_daily_hosted_turns
            ));
        }
        if quota.require_user_consent && !user_consented {
            return Decision::Deny("background execution requires user consent".into());
        }
        Decision::Allow
    }

    /// Gate session lifecycle against the hosted session limits. Zero on
    /// either limit means unlimited.
    pub fn check_session_limits(&self, session_age_hours: f64, active_sessions: u32) -> Decision {
        if self.is_killed() {
            return Decision::Deny("kill switch engaged".into());
        }
        let max_hours = self.effective.max_session_duration_hours;
        if max_hours > 0 && session_age_hours >= f64::from(max_hours) {
            return Decision::Deny(format!(
                "session age {session_age_hours:.1}h exceeds limit of {max_hours}h"
            ));
        }
        let max_sessions = self.effective.max_concurrent_sessions;
        if max_sessions > 0 && active_sessions >= max_sessions {
            return Decision::Deny(format!("concurrent session limit {max_sessions} reached"));
        }
        Decision::Allow
    }

    /// The context-token cap for a model, if a rule matches.
    pub fn max_context_tokens(&self, model_id: &str) -> Option<u32> {
        self.effective
            .model_rules
            .iter()
            .filter(|r| glob_matches(&r.model_pattern, model_id) && r.max_context_tokens > 0)
            .map(|r| r.max_context_tokens)
            .min()
    }

    /// Gate a hosted inference request against the hosted inference rules.
    pub fn check_inference(&self, provider: &str, model: &str, tokens: u32) -> Decision {
        if self.is_killed() {
            return Decision::Deny("kill switch engaged".into());
        }
        let rule = self
            .effective
            .inference_rules
            .iter()
            .find(|r| r.provider == provider);
        let Some(rule) = rule else {
            return Decision::Deny(format!("provider '{provider}' has no inference rule"));
        };
        if !rule.allowed_models.is_empty()
            && !rule.allowed_models.iter().any(|p| glob_matches(p, model))
        {
            return Decision::Deny(format!(
                "model '{model}' not allowed for provider '{provider}'"
            ));
        }
        if rule.max_tokens_per_request > 0 && tokens > rule.max_tokens_per_request {
            return Decision::Deny(format!(
                "request of {tokens} tokens exceeds per-request cap of {}",
                rule.max_tokens_per_request
            ));
        }
        Decision::Allow
    }

    /// Gate data leaving the device for a data class and destination.
    pub fn check_data_egress(&self, data_class: &str, destination: &str) -> Decision {
        if self.is_killed() {
            return Decision::Deny("kill switch engaged".into());
        }
        let rule = self
            .effective
            .data_rules
            .iter()
            .find(|r| r.data_class == data_class);
        let Some(rule) = rule else {
            return Decision::Deny(format!("data class '{data_class}' has no rule"));
        };
        if !rule.may_leave_device && destination != "local" {
            return Decision::Deny(format!(
                "data class '{data_class}' may not leave the device"
            ));
        }
        if !rule.allowed_destinations.is_empty()
            && !rule
                .allowed_destinations
                .iter()
                .any(|d| glob_matches(d, destination))
        {
            return Decision::Deny(format!(
                "destination '{destination}' not allowed for data class '{data_class}'"
            ));
        }
        if rule.requires_redaction {
            return Decision::RequireApproval(format!(
                "data class '{data_class}' requires redaction before egress"
            ));
        }
        Decision::Allow
    }

    /// Gate a CUA (computer-use) action against an application.
    pub fn check_cua_app(&self, app: &str) -> Decision {
        if self.is_killed() {
            return Decision::Deny("kill switch engaged".into());
        }
        let Some(cua) = &self.effective.cua else {
            return Decision::Deny("no CUA policy configured".into());
        };
        if !cua.enabled {
            return Decision::Deny("CUA disabled by policy".into());
        }
        if cua.denied_apps.iter().any(|p| glob_matches(p, app)) {
            return Decision::Deny(format!("app '{app}' denied by CUA policy"));
        }
        if !cua.allowed_apps.is_empty() && !cua.allowed_apps.iter().any(|p| glob_matches(p, app)) {
            return Decision::Deny(format!("app '{app}' not in CUA allowlist"));
        }
        Decision::Allow
    }

    /// Whether a destructive CUA action needs user confirmation.
    pub fn cua_requires_confirmation(&self) -> bool {
        self.effective
            .cua
            .as_ref()
            .map(|c| c.require_confirmation_destructive)
            .unwrap_or(true)
    }

    /// Scan content against DLP patterns. Returns the redacted content and
    /// the strictest action found across all matches.
    pub fn scan_dlp(&self, content: &str) -> std::result::Result<DlpOutcome, EvalError> {
        let mut redacted = content.to_string();
        let mut strictest: Option<DlpAction> = None;
        let mut hits = Vec::new();

        for pattern in self.effective_dlp_patterns() {
            let re = Regex::new(&pattern.regex).map_err(|source| EvalError::InvalidDlpPattern {
                name: pattern.name.clone(),
                source,
            })?;
            if !re.is_match(&redacted) {
                continue;
            }
            let action = DlpAction::try_from(pattern.action).unwrap_or(DlpAction::LogOnly);
            hits.push(pattern.name.clone());
            strictest = Some(match (strictest, action) {
                (Some(DlpAction::Block), _) | (_, DlpAction::Block) => DlpAction::Block,
                (Some(DlpAction::Redact), _) | (_, DlpAction::Redact) => DlpAction::Redact,
                _ => DlpAction::LogOnly,
            });
            if action == DlpAction::Redact {
                redacted = re
                    .replace_all(&redacted, format!("[REDACTED:{}]", pattern.name))
                    .into_owned();
            }
        }

        Ok(DlpOutcome {
            action: strictest,
            matched_patterns: hits,
            redacted_content: redacted,
        })
    }

    fn effective_dlp_patterns(&self) -> impl Iterator<Item = &DlpPattern> {
        // DLP patterns live on the endpoint policy; they are attached to the
        // gate via `with_dlp_patterns` rather than carried in the merged
        // EffectivePolicy message.
        self.dlp_patterns.iter()
    }

    /// Attach endpoint DLP patterns to the gate (they are endpoint-side and
    /// not part of the merged EffectivePolicy message).
    pub fn with_dlp_patterns(mut self, patterns: Vec<DlpPattern>) -> Self {
        self.dlp_patterns = patterns;
        self
    }
}

/// Result of a DLP scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DlpOutcome {
    pub action: Option<DlpAction>,
    pub matched_patterns: Vec<String>,
    pub redacted_content: String,
}

/// Evaluate a rule condition against the execution context. Conditions are
/// simple `key=value` expressions AND-joined with `&&`. Supported keys:
/// `locus` (endpoint|hosted) and `time` (day = 06:00–18:00, night otherwise).
/// An empty condition always matches; unknown keys or values never match
/// (fail closed).
pub fn condition_matches(condition: &str, locus: &str, hour: u32) -> bool {
    let condition = condition.trim();
    if condition.is_empty() {
        return true;
    }
    condition.split("&&").all(|clause| {
        let clause = clause.trim();
        let Some((key, value)) = clause.split_once('=') else {
            return false;
        };
        match (key.trim(), value.trim()) {
            ("locus", v) => v == locus,
            ("time", "day") => (6..18).contains(&hour),
            ("time", "night") => !(6..18).contains(&hour),
            _ => false,
        }
    })
}

/// Glob matcher supporting `*` (any sequence) and exact matches. Patterns
/// are matched case-sensitively against the whole value.
pub fn glob_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = value;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match rest.find(part) {
            Some(idx) if i == 0 && !pattern.starts_with('*') && idx != 0 => return false,
            Some(idx) => {
                rest = &rest[idx + part.len()..];
            }
            None => return false,
        }
    }
    // If the pattern does not end with '*', the last literal must anchor the
    // end of the value.
    if !pattern.ends_with('*') {
        if let Some(last) = parts.iter().rev().find(|p| !p.is_empty()) {
            return value.ends_with(last);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::policy::{
        CuaPolicy, DataClassRule, DlpPattern, InferenceRule, ModelRule, ToolRule,
    };

    fn gate(tool_rules: Vec<ToolRule>) -> PolicyGate {
        PolicyGate::new(EffectivePolicy {
            endpoint_version: "1".into(),
            hosted_version: "1".into(),
            data_rules: vec![],
            tool_rules,
            model_rules: vec![],
            cua: None,
            inference_rules: vec![],
            kill_switch: false,
            max_retention_hours: 0,
            background_quota: None,
            max_session_duration_hours: 0,
            max_concurrent_sessions: 0,
        })
    }

    fn rule(pattern: &str, action: ToolAction) -> ToolRule {
        ToolRule {
            tool_pattern: pattern.into(),
            action: action as i32,
            condition: String::new(),
        }
    }

    #[test]
    fn deny_wins_over_allow() {
        let g = gate(vec![
            rule("shell.*", ToolAction::Allow),
            rule("shell.exec", ToolAction::Deny),
        ]);
        assert!(matches!(g.check_tool("shell.exec"), Decision::Deny(_)));
        assert!(g.check_tool("shell.list").is_allowed());
    }

    #[test]
    fn approval_wins_over_allow_but_not_deny() {
        let g = gate(vec![
            rule("fs.*", ToolAction::Allow),
            rule("fs.write", ToolAction::RequireApproval),
        ]);
        assert!(matches!(
            g.check_tool("fs.write"),
            Decision::RequireApproval(_)
        ));
        assert!(g.check_tool("fs.read").is_allowed());

        let g2 = gate(vec![
            rule("fs.write", ToolAction::RequireApproval),
            rule("fs.*", ToolAction::Deny),
        ]);
        assert!(matches!(g2.check_tool("fs.write"), Decision::Deny(_)));
    }

    #[test]
    fn unknown_tool_fails_closed() {
        let g = gate(vec![]);
        assert!(matches!(g.check_tool("anything"), Decision::Deny(_)));
    }

    #[test]
    fn kill_switch_denies_everything() {
        let mut g = gate(vec![rule("*", ToolAction::Allow)]);
        g.effective.kill_switch = true;
        assert!(matches!(g.check_tool("fs.read"), Decision::Deny(_)));
        assert!(matches!(
            g.check_model("any", ModelLocus::Local),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn model_locus_enforced() {
        let mut eff = gate(vec![]).effective().clone();
        eff.model_rules = vec![ModelRule {
            model_pattern: "qwen3-*".into(),
            allowed_local: true,
            allowed_hosted: false,
            max_context_tokens: 32768,
        }];
        let g = PolicyGate::new(eff);
        assert!(g.check_model("qwen3-8b", ModelLocus::Local).is_allowed());
        assert!(matches!(
            g.check_model("qwen3-8b", ModelLocus::Hosted),
            Decision::Deny(_)
        ));
        assert_eq!(g.max_context_tokens("qwen3-8b"), Some(32768));
    }

    #[test]
    fn inference_rules_enforced() {
        let mut eff = gate(vec![]).effective().clone();
        eff.inference_rules = vec![InferenceRule {
            provider: "bedrock".into(),
            allowed_models: vec!["claude-*".into()],
            allowed_regions: vec![],
            max_tokens_per_request: 8192,
            daily_token_budget: 0,
        }];
        let g = PolicyGate::new(eff);
        assert!(g
            .check_inference("bedrock", "claude-sonnet", 4096)
            .is_allowed());
        assert!(matches!(
            g.check_inference("bedrock", "gpt-4o", 100),
            Decision::Deny(_)
        ));
        assert!(matches!(
            g.check_inference("bedrock", "claude-sonnet", 9000),
            Decision::Deny(_)
        ));
        assert!(matches!(
            g.check_inference("openai", "gpt-4o", 100),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn data_egress_rules() {
        let mut eff = gate(vec![]).effective().clone();
        eff.data_rules = vec![
            DataClassRule {
                data_class: "secret".into(),
                may_leave_device: false,
                requires_redaction: false,
                allowed_destinations: vec![],
            },
            DataClassRule {
                data_class: "internal".into(),
                may_leave_device: true,
                requires_redaction: true,
                allowed_destinations: vec!["hosted".into()],
            },
        ];
        let g = PolicyGate::new(eff);
        assert!(matches!(
            g.check_data_egress("secret", "hosted"),
            Decision::Deny(_)
        ));
        assert!(g.check_data_egress("secret", "local").is_allowed());
        assert!(matches!(
            g.check_data_egress("internal", "hosted"),
            Decision::RequireApproval(_)
        ));
        assert!(matches!(
            g.check_data_egress("internal", "internet"),
            Decision::Deny(_)
        ));
        assert!(matches!(
            g.check_data_egress("unknown-class", "local"),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn cua_app_gating() {
        let mut eff = gate(vec![]).effective().clone();
        eff.cua = Some(CuaPolicy {
            enabled: true,
            allowed_apps: vec!["com.apple.*".into()],
            denied_apps: vec!["com.apple.finder".into()],
            screenshot_redaction: true,
            require_confirmation_destructive: true,
            max_actions_per_minute: 30,
        });
        let g = PolicyGate::new(eff);
        assert!(g.check_cua_app("com.apple.safari").is_allowed());
        assert!(matches!(
            g.check_cua_app("com.apple.finder"),
            Decision::Deny(_)
        ));
        assert!(matches!(
            g.check_cua_app("com.malware.evil"),
            Decision::Deny(_)
        ));
        assert!(g.cua_requires_confirmation());
    }

    #[test]
    fn dlp_scan_redacts_and_blocks() {
        let g = gate(vec![]).with_dlp_patterns(vec![
            DlpPattern {
                name: "ssn".into(),
                regex: r"\b\d{3}-\d{2}-\d{4}\b".into(),
                action: DlpAction::Redact as i32,
            },
            DlpPattern {
                name: "private-key".into(),
                regex: r"BEGIN [A-Z ]*PRIVATE KEY".into(),
                action: DlpAction::Block as i32,
            },
        ]);
        let out = g.scan_dlp("my ssn is 123-45-6789 ok").unwrap();
        assert_eq!(out.action, Some(DlpAction::Redact));
        assert!(out.redacted_content.contains("[REDACTED:ssn]"));
        assert!(!out.redacted_content.contains("123-45-6789"));

        let out = g.scan_dlp("-----BEGIN RSA PRIVATE KEY-----").unwrap();
        assert_eq!(out.action, Some(DlpAction::Block));

        let out = g.scan_dlp("nothing sensitive").unwrap();
        assert_eq!(out.action, None);
    }

    #[test]
    fn background_quota_enforced() {
        use fabric_types::policy::BackgroundQuota;

        let mut eff = gate(vec![]).effective().clone();
        eff.background_quota = Some(BackgroundQuota {
            max_concurrent_background: 2,
            max_daily_hosted_turns: 100,
            require_user_consent: true,
        });
        let g = PolicyGate::new(eff);

        // Over concurrent limit.
        assert!(matches!(
            g.check_background_quota(2, 0, true),
            Decision::Deny(_)
        ));
        // Over daily turns.
        assert!(matches!(
            g.check_background_quota(0, 100, true),
            Decision::Deny(_)
        ));
        // Consent required but not given.
        assert!(matches!(
            g.check_background_quota(0, 0, false),
            Decision::Deny(_)
        ));
        // Within limits with consent.
        assert!(g.check_background_quota(1, 99, true).is_allowed());
    }

    #[test]
    fn no_background_quota_allows() {
        let g = gate(vec![]);
        assert!(g.check_background_quota(1000, 1000, false).is_allowed());
    }

    #[test]
    fn session_limits_enforced() {
        let mut eff = gate(vec![]).effective().clone();
        eff.max_session_duration_hours = 24;
        eff.max_concurrent_sessions = 4;
        let g = PolicyGate::new(eff);

        // Over duration.
        assert!(matches!(g.check_session_limits(24.0, 0), Decision::Deny(_)));
        assert!(matches!(g.check_session_limits(25.5, 0), Decision::Deny(_)));
        // Over concurrent sessions.
        assert!(matches!(g.check_session_limits(1.0, 4), Decision::Deny(_)));
        // Within limits.
        assert!(g.check_session_limits(23.9, 3).is_allowed());
    }

    #[test]
    fn zero_session_limits_are_unlimited() {
        let g = gate(vec![]);
        assert!(g.check_session_limits(9999.0, 9999).is_allowed());
    }

    #[test]
    fn tool_conditions_gate_by_locus_and_time() {
        let cond_rule = |pattern: &str, action: ToolAction, condition: &str| ToolRule {
            tool_pattern: pattern.into(),
            action: action as i32,
            condition: condition.into(),
        };

        // locus=endpoint: allows on endpoint, skipped (deny, fail closed) on hosted.
        let g = gate(vec![cond_rule(
            "shell.*",
            ToolAction::Allow,
            "locus=endpoint",
        )]);
        assert!(g
            .check_tool_with_context("shell.exec", "endpoint", 12)
            .is_allowed());
        assert!(matches!(
            g.check_tool_with_context("shell.exec", "hosted", 12),
            Decision::Deny(_)
        ));

        // time=night allow-rule does not match at 3pm.
        let g = gate(vec![cond_rule("shell.*", ToolAction::Allow, "time=night")]);
        assert!(matches!(
            g.check_tool_with_context("shell.exec", "endpoint", 15),
            Decision::Deny(_)
        ));
        assert!(g
            .check_tool_with_context("shell.exec", "endpoint", 23)
            .is_allowed());

        // Empty condition matches everywhere.
        let g = gate(vec![cond_rule("shell.*", ToolAction::Allow, "")]);
        for locus in ["endpoint", "hosted"] {
            for hour in [0, 12, 23] {
                assert!(g
                    .check_tool_with_context("shell.exec", locus, hour)
                    .is_allowed());
            }
        }

        // AND-joined conditions.
        let g = gate(vec![cond_rule(
            "shell.*",
            ToolAction::Allow,
            "locus=endpoint && time=day",
        )]);
        assert!(g
            .check_tool_with_context("shell.exec", "endpoint", 10)
            .is_allowed());
        assert!(matches!(
            g.check_tool_with_context("shell.exec", "endpoint", 22),
            Decision::Deny(_)
        ));
        assert!(matches!(
            g.check_tool_with_context("shell.exec", "hosted", 10),
            Decision::Deny(_)
        ));

        // check_tool ignores conditions (backward compat).
        let g = gate(vec![cond_rule(
            "shell.*",
            ToolAction::Allow,
            "locus=hosted",
        )]);
        assert!(g.check_tool("shell.exec").is_allowed());

        // Unknown condition keys never match.
        let g = gate(vec![cond_rule(
            "shell.*",
            ToolAction::Allow,
            "weather=sunny",
        )]);
        assert!(matches!(
            g.check_tool_with_context("shell.exec", "endpoint", 12),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn glob_semantics() {
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("shell.*", "shell.exec"));
        assert!(!glob_matches("shell.*", "fs.read"));
        assert!(glob_matches("*.prod", "db.prod"));
        assert!(glob_matches("claude-*", "claude-sonnet"));
        assert!(!glob_matches("claude-*", "gpt-4o"));
        assert!(glob_matches("a*b*c", "aXbYc"));
        assert!(!glob_matches("a*b*c", "aXbYcZ"));
        assert!(glob_matches("exact", "exact"));
        assert!(!glob_matches("exact", "exactly"));
    }
}
