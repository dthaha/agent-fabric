//! Jamf Configuration Profile plist parser (ADR 005). Parses macOS-native
//! plist XML with PascalCase keys into [`RawPolicy`], then maps to the
//! internal `EndpointPolicy` proto struct.
//!
//! Keys without a proto representation (`MaxCallsPerSession`,
//! `CuaMaxScreenArea`, `SafetyEnabled`, `Threshold`) are validated but not
//! mapped; they are reserved for future proto evolution.

use std::fmt;
use std::marker::PhantomData;

use fabric_types::policy::{
    CuaPolicy, DataClassRule, DlpAction, DlpPattern, EndpointPolicy, FailMode, ModelRule,
    SafetyAction, SafetyConfig, SafetyPolicyRule, ToolAction, ToolRule,
};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};

use crate::{MdmError, Result};

/// Rule list container that accepts both wire shapes: a bare array of
/// dicts (plist) or a single-key wrapper element holding the items
/// (OMA-URI XML, e.g. `<ToolRules><ToolRule>..</ToolRule></ToolRules>`).
#[derive(Debug)]
pub(crate) struct RuleList<T>(pub Vec<T>);

impl<T> Default for RuleList<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<'de, T> Deserialize<'de> for RuleList<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RuleListVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for RuleListVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = RuleList<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a rule array or a wrapper element holding rule items")
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(RuleList(items))
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let mut items = Vec::new();
                while map.next_key::<de::IgnoredAny>()?.is_some() {
                    let mut nested: Vec<T> = map.next_value()?;
                    items.append(&mut nested);
                }
                Ok(RuleList(items))
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value.trim().is_empty() {
                    Ok(RuleList(Vec::new()))
                } else {
                    Err(E::custom("expected a rule array, got text"))
                }
            }

            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(RuleList(Vec::new()))
            }
        }

        deserializer.deserialize_any(RuleListVisitor(PhantomData))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename = "FabricPolicy", rename_all = "PascalCase")]
pub(crate) struct RawPolicy {
    #[serde(rename = "PolicyID")]
    policy_id: String,
    version: String,
    #[serde(rename = "OrgID")]
    org_id: String,
    #[serde(default)]
    kill_switch: bool,
    #[serde(default)]
    max_retention_hours: u32,
    #[serde(default)]
    data_rules: RuleList<RawDataRule>,
    #[serde(default)]
    tool_rules: RuleList<RawToolRule>,
    #[serde(default)]
    model_rules: RuleList<RawModelRule>,
    #[serde(default)]
    cua_enabled: bool,
    #[serde(default = "default_max_screen_area")]
    cua_max_screen_area: f64,
    #[serde(default = "default_true")]
    cua_require_confirmation: bool,
    #[serde(default)]
    cua_blocked_apps: RuleList<String>,
    #[serde(default)]
    dlp_patterns: RuleList<RawDlpPattern>,
    #[serde(default = "default_true")]
    safety_enabled: bool,
    #[serde(default = "default_fail_mode")]
    safety_fail_mode: String,
    #[serde(default)]
    safety_rules: RuleList<RawSafetyRule>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RawDataRule {
    data_class: String,
    #[serde(default)]
    may_leave_device: bool,
    #[serde(default)]
    requires_redaction: bool,
    #[serde(default)]
    allowed_destinations: RuleList<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RawToolRule {
    tool_pattern: String,
    action: String,
    #[serde(default)]
    max_calls_per_session: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RawModelRule {
    model_pattern: String,
    action: String,
    #[serde(default)]
    max_tokens_per_call: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RawDlpPattern {
    name: String,
    regex: String,
    action: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RawSafetyRule {
    category: String,
    action: String,
    #[serde(default)]
    threshold: Option<f64>,
}

fn default_max_screen_area() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_fail_mode() -> String {
    "closed".to_string()
}

fn parse_tool_action(action: &str) -> Result<i32> {
    match action {
        "allow" => Ok(ToolAction::Allow as i32),
        "deny" => Ok(ToolAction::Deny as i32),
        "confirm" => Ok(ToolAction::RequireApproval as i32),
        other => Err(MdmError::Validation(format!(
            "unknown tool action: {other}"
        ))),
    }
}

fn parse_dlp_action(action: &str) -> Result<i32> {
    match action {
        "redact" => Ok(DlpAction::Redact as i32),
        "block" => Ok(DlpAction::Block as i32),
        "warn" => Ok(DlpAction::LogOnly as i32),
        other => Err(MdmError::Validation(format!("unknown DLP action: {other}"))),
    }
}

fn parse_safety_action(action: &str) -> Result<i32> {
    match action {
        "allow" => Ok(SafetyAction::Allow as i32),
        "warn" => Ok(SafetyAction::Warn as i32),
        "block" => Ok(SafetyAction::Block as i32),
        other => Err(MdmError::Validation(format!(
            "unknown safety action: {other}"
        ))),
    }
}

fn parse_fail_mode(fail_mode: &str) -> Result<i32> {
    match fail_mode {
        "open" => Ok(FailMode::Open as i32),
        "closed" => Ok(FailMode::Closed as i32),
        other => Err(MdmError::Validation(format!(
            "unknown safety fail mode: {other}"
        ))),
    }
}

impl TryFrom<RawPolicy> for EndpointPolicy {
    type Error = MdmError;

    fn try_from(raw: RawPolicy) -> Result<Self> {
        if !(0.0..=1.0).contains(&raw.cua_max_screen_area) {
            return Err(MdmError::Validation(format!(
                "CuaMaxScreenArea must be within 0.0..=1.0, got {}",
                raw.cua_max_screen_area
            )));
        }
        let tool_rules = raw
            .tool_rules
            .0
            .iter()
            .map(|rule| {
                let _ = rule.max_calls_per_session;
                Ok(ToolRule {
                    tool_pattern: rule.tool_pattern.clone(),
                    action: parse_tool_action(&rule.action)?,
                    condition: String::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let model_rules = raw
            .model_rules
            .0
            .iter()
            .map(|rule| {
                let (allowed_local, allowed_server) = match rule.action.as_str() {
                    "allow" => (true, true),
                    "deny" => (false, false),
                    other => {
                        return Err(MdmError::Validation(format!(
                            "unknown model action: {other}"
                        )))
                    }
                };
                Ok(ModelRule {
                    model_pattern: rule.model_pattern.clone(),
                    allowed_local,
                    allowed_server,
                    max_context_tokens: rule.max_tokens_per_call,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let dlp_patterns = raw
            .dlp_patterns
            .0
            .iter()
            .map(|pattern| {
                Ok(DlpPattern {
                    name: pattern.name.clone(),
                    regex: pattern.regex.clone(),
                    action: parse_dlp_action(&pattern.action)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let safety_rules = raw
            .safety_rules
            .0
            .iter()
            .map(|rule| {
                let _ = rule.threshold;
                Ok(SafetyPolicyRule {
                    category: rule.category.clone(),
                    action: parse_safety_action(&rule.action)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let safety = if raw.safety_enabled || !safety_rules.is_empty() {
            Some(SafetyConfig {
                endpoint_url: String::new(),
                model: String::new(),
                parser: String::new(),
                timeout_ms: 0,
                fail_mode: parse_fail_mode(&raw.safety_fail_mode)?,
                rules: safety_rules,
                default_action: SafetyAction::Unspecified as i32,
                api_key: String::new(),
                extra_body_json: String::new(),
                system_prompt: String::new(),
            })
        } else {
            None
        };
        Ok(EndpointPolicy {
            policy_id: raw.policy_id,
            version: raw.version,
            org_id: raw.org_id,
            data_rules: raw
                .data_rules
                .0
                .into_iter()
                .map(|rule| DataClassRule {
                    data_class: rule.data_class,
                    may_leave_device: rule.may_leave_device,
                    requires_redaction: rule.requires_redaction,
                    allowed_destinations: rule.allowed_destinations.0,
                })
                .collect(),
            tool_rules,
            model_rules,
            cua: Some(CuaPolicy {
                enabled: raw.cua_enabled,
                allowed_apps: vec![],
                denied_apps: raw.cua_blocked_apps.0,
                screenshot_redaction: false,
                require_confirmation_destructive: raw.cua_require_confirmation,
                max_actions_per_minute: 0,
            }),
            kill_switch: raw.kill_switch,
            max_retention_hours: raw.max_retention_hours,
            dlp_patterns,
            safety,
        })
    }
}

pub(crate) fn parse(bytes: &[u8]) -> Result<EndpointPolicy> {
    let raw: RawPolicy = plist::from_bytes(bytes)?;
    raw.try_into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_plist() -> &'static [u8] {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>PolicyID</key>
    <string>ep-mac-1</string>
    <key>Version</key>
    <string>1.2.0</string>
    <key>OrgID</key>
    <string>acme-corp</string>
    <key>KillSwitch</key>
    <false/>
    <key>MaxRetentionHours</key>
    <integer>720</integer>
    <key>DataRules</key>
    <array>
        <dict>
            <key>DataClass</key>
            <string>pii</string>
            <key>MayLeaveDevice</key>
            <false/>
            <key>RequiresRedaction</key>
            <true/>
            <key>AllowedDestinations</key>
            <array>
                <string>redactor.internal</string>
            </array>
        </dict>
    </array>
    <key>ToolRules</key>
    <array>
        <dict>
            <key>ToolPattern</key>
            <string>shell.*</string>
            <key>Action</key>
            <string>confirm</string>
            <key>MaxCallsPerSession</key>
            <integer>50</integer>
        </dict>
        <dict>
            <key>ToolPattern</key>
            <string>file.*</string>
            <key>Action</key>
            <string>allow</string>
        </dict>
    </array>
    <key>ModelRules</key>
    <array>
        <dict>
            <key>ModelPattern</key>
            <string>local/*</string>
            <key>Action</key>
            <string>allow</string>
            <key>MaxTokensPerCall</key>
            <integer>4096</integer>
        </dict>
        <dict>
            <key>ModelPattern</key>
            <string>nvidia/*</string>
            <key>Action</key>
            <string>deny</string>
        </dict>
    </array>
    <key>CuaEnabled</key>
    <true/>
    <key>CuaMaxScreenArea</key>
    <real>0.8</real>
    <key>CuaRequireConfirmation</key>
    <false/>
    <key>CuaBlockedApps</key>
    <array>
        <string>com.apple.Terminal</string>
    </array>
    <key>DlpPatterns</key>
    <array>
        <dict>
            <key>Name</key>
            <string>us-ssn</string>
            <key>Regex</key>
            <string>\b\d{3}-\d{2}-\d{4}\b</string>
            <key>Action</key>
            <string>redact</string>
        </dict>
    </array>
    <key>SafetyEnabled</key>
    <true/>
    <key>SafetyFailMode</key>
    <string>open</string>
    <key>SafetyRules</key>
    <array>
        <dict>
            <key>Category</key>
            <string>violence</string>
            <key>Action</key>
            <string>block</string>
        </dict>
    </array>
</dict>
</plist>
"#
    }

    #[test]
    fn parses_full_plist() {
        let policy = parse(full_plist()).unwrap();
        assert_eq!(policy.policy_id, "ep-mac-1");
        assert_eq!(policy.version, "1.2.0");
        assert_eq!(policy.org_id, "acme-corp");
        assert!(!policy.kill_switch);
        assert_eq!(policy.max_retention_hours, 720);

        assert_eq!(policy.data_rules.len(), 1);
        let data = &policy.data_rules[0];
        assert_eq!(data.data_class, "pii");
        assert!(!data.may_leave_device);
        assert!(data.requires_redaction);
        assert_eq!(data.allowed_destinations, ["redactor.internal"]);

        assert_eq!(policy.tool_rules.len(), 2);
        assert_eq!(policy.tool_rules[0].tool_pattern, "shell.*");
        assert_eq!(
            policy.tool_rules[0].action,
            ToolAction::RequireApproval as i32
        );
        assert_eq!(policy.tool_rules[1].action, ToolAction::Allow as i32);

        assert_eq!(policy.model_rules.len(), 2);
        assert!(policy.model_rules[0].allowed_local);
        assert!(policy.model_rules[0].allowed_server);
        assert_eq!(policy.model_rules[0].max_context_tokens, 4096);
        assert!(!policy.model_rules[1].allowed_local);
        assert!(!policy.model_rules[1].allowed_server);

        let cua = policy.cua.unwrap();
        assert!(cua.enabled);
        assert!(!cua.require_confirmation_destructive);
        assert_eq!(cua.denied_apps, ["com.apple.Terminal"]);

        assert_eq!(policy.dlp_patterns.len(), 1);
        assert_eq!(policy.dlp_patterns[0].name, "us-ssn");
        assert_eq!(policy.dlp_patterns[0].action, DlpAction::Redact as i32);

        let safety = policy.safety.unwrap();
        assert_eq!(safety.fail_mode, FailMode::Open as i32);
        assert_eq!(safety.rules.len(), 1);
        assert_eq!(safety.rules[0].category, "violence");
        assert_eq!(safety.rules[0].action, SafetyAction::Block as i32);
    }

    #[test]
    fn minimal_plist_uses_defaults() {
        let bytes = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>PolicyID</key>
    <string>ep-min</string>
    <key>Version</key>
    <string>v1</string>
    <key>OrgID</key>
    <string>org-1</string>
    <key>ToolRules</key>
    <array></array>
</dict>
</plist>
"#;
        let policy = parse(bytes).unwrap();
        assert_eq!(policy.policy_id, "ep-min");
        assert!(policy.tool_rules.is_empty());
        assert!(policy.data_rules.is_empty());
        assert!(!policy.kill_switch);
        let cua = policy.cua.unwrap();
        assert!(!cua.enabled);
        assert!(cua.require_confirmation_destructive);
        let safety = policy.safety.unwrap();
        assert_eq!(safety.fail_mode, FailMode::Closed as i32);
        assert!(safety.rules.is_empty());
    }

    #[test]
    fn safety_disabled_with_no_rules_yields_none() {
        let bytes = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>PolicyID</key>
    <string>ep-min</string>
    <key>Version</key>
    <string>v1</string>
    <key>OrgID</key>
    <string>org-1</string>
    <key>SafetyEnabled</key>
    <false/>
</dict>
</plist>
"#;
        let policy = parse(bytes).unwrap();
        assert!(policy.safety.is_none());
    }

    #[test]
    fn invalid_tool_action_fails() {
        let bytes = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>PolicyID</key>
    <string>ep-1</string>
    <key>Version</key>
    <string>v1</string>
    <key>OrgID</key>
    <string>org-1</string>
    <key>ToolRules</key>
    <array>
        <dict>
            <key>ToolPattern</key>
            <string>file.*</string>
            <key>Action</key>
            <string>yolo</string>
        </dict>
    </array>
</dict>
</plist>
"#;
        let err = parse(bytes).unwrap_err();
        assert!(matches!(err, MdmError::Validation(ref m) if m.contains("yolo")));
    }

    #[test]
    fn malformed_plist_fails() {
        let err = parse(b"<plist><dict><key>PolicyID</key>").unwrap_err();
        assert!(matches!(err, MdmError::Plist(_)));
    }
}
