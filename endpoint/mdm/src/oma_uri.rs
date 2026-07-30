//! Intune OMA-URI XML parser (ADR 005). Parses the Windows-native
//! `FabricPolicy` XML document into the same PascalCase [`RawPolicy`]
//! intermediate used by the plist parser, then maps to the internal
//! `EndpointPolicy` proto struct.

use fabric_types::policy::EndpointPolicy;

use crate::plist_parser::RawPolicy;
use crate::Result;

pub(crate) fn parse(bytes: &[u8]) -> Result<EndpointPolicy> {
    let raw: RawPolicy = quick_xml::de::from_reader(bytes)?;
    raw.try_into()
}

#[cfg(test)]
mod tests {
    use fabric_types::policy::{DlpAction, FailMode, ToolAction};

    use super::*;
    use crate::MdmError;

    fn full_xml() -> &'static [u8] {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<FabricPolicy>
  <PolicyID>ep-win-1</PolicyID>
  <Version>2.0.0</Version>
  <OrgID>acme-corp</OrgID>
  <KillSwitch>true</KillSwitch>
  <MaxRetentionHours>168</MaxRetentionHours>
  <DataRules>
    <DataRule>
      <DataClass>financial</DataClass>
      <MayLeaveDevice>true</MayLeaveDevice>
      <RequiresRedaction>true</RequiresRedaction>
      <AllowedDestinations>
        <string>inference.internal</string>
      </AllowedDestinations>
    </DataRule>
  </DataRules>
  <ToolRules>
    <ToolRule>
      <ToolPattern>file.*</ToolPattern>
      <Action>allow</Action>
    </ToolRule>
    <ToolRule>
      <ToolPattern>shell.exec</ToolPattern>
      <Action>deny</Action>
    </ToolRule>
  </ToolRules>
  <ModelRules>
    <ModelRule>
      <ModelPattern>local/*</ModelPattern>
      <Action>allow</Action>
    </ModelRule>
  </ModelRules>
  <CuaEnabled>true</CuaEnabled>
  <CuaBlockedApps>
    <string>Microsoft.WindowsTerminal_8wekyb3d8bbwe</string>
  </CuaBlockedApps>
  <DlpPatterns>
    <DlpPattern>
      <Name>us-ssn</Name>
      <Regex>\b\d{3}-\d{2}-\d{4}\b</Regex>
      <Action>block</Action>
    </DlpPattern>
  </DlpPatterns>
  <SafetyFailMode>closed</SafetyFailMode>
  <SafetyRules>
    <SafetyRule>
      <Category>injection</Category>
      <Action>block</Action>
    </SafetyRule>
  </SafetyRules>
</FabricPolicy>
"#
    }

    #[test]
    fn parses_full_oma_uri_xml() {
        let policy = parse(full_xml()).unwrap();
        assert_eq!(policy.policy_id, "ep-win-1");
        assert_eq!(policy.version, "2.0.0");
        assert_eq!(policy.org_id, "acme-corp");
        assert!(policy.kill_switch);
        assert_eq!(policy.max_retention_hours, 168);

        assert_eq!(policy.data_rules.len(), 1);
        assert_eq!(policy.data_rules[0].data_class, "financial");
        assert!(policy.data_rules[0].may_leave_device);
        assert_eq!(
            policy.data_rules[0].allowed_destinations,
            ["inference.internal"]
        );

        assert_eq!(policy.tool_rules.len(), 2);
        assert_eq!(policy.tool_rules[0].tool_pattern, "file.*");
        assert_eq!(policy.tool_rules[0].action, ToolAction::Allow as i32);
        assert_eq!(policy.tool_rules[1].action, ToolAction::Deny as i32);

        assert_eq!(policy.model_rules.len(), 1);
        assert!(policy.model_rules[0].allowed_local);

        let cua = policy.cua.unwrap();
        assert!(cua.enabled);
        assert!(cua.require_confirmation_destructive);
        assert_eq!(cua.denied_apps, ["Microsoft.WindowsTerminal_8wekyb3d8bbwe"]);

        assert_eq!(policy.dlp_patterns[0].action, DlpAction::Block as i32);

        let safety = policy.safety.unwrap();
        assert_eq!(safety.fail_mode, FailMode::Closed as i32);
        assert_eq!(safety.rules[0].category, "injection");
    }

    #[test]
    fn minimal_xml_with_empty_tool_rules() {
        let bytes = br#"<FabricPolicy>
  <PolicyID>ep-1</PolicyID>
  <Version>v1</Version>
  <OrgID>org-1</OrgID>
  <ToolRules></ToolRules>
</FabricPolicy>"#;
        let policy = parse(bytes).unwrap();
        assert_eq!(policy.policy_id, "ep-1");
        assert!(policy.tool_rules.is_empty());
    }

    #[test]
    fn invalid_action_string_fails() {
        let bytes = br#"<FabricPolicy>
  <PolicyID>ep-1</PolicyID>
  <Version>v1</Version>
  <OrgID>org-1</OrgID>
  <ToolRules>
    <ToolRule>
      <ToolPattern>file.*</ToolPattern>
      <Action>maybe</Action>
    </ToolRule>
  </ToolRules>
</FabricPolicy>"#;
        let err = parse(bytes).unwrap_err();
        assert!(matches!(err, MdmError::Validation(ref m) if m.contains("maybe")));
    }

    #[test]
    fn malformed_xml_fails() {
        let err = parse(b"<FabricPolicy><PolicyID>ep-1").unwrap_err();
        assert!(matches!(err, MdmError::OmaUri(_)));
    }
}
