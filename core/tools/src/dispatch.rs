use std::collections::HashMap;

use async_trait::async_trait;
use fabric_policy::eval::{Decision, PolicyGate};
use fabric_types::tools::{ToolDescriptor, ToolLocality, ToolRequest, ToolResponse};
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool '{0}' not found in registry")]
    NotFound(String),
    #[error("tool execution failed: {0}")]
    Execution(String),
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("policy requires approval: {0}")]
    RequiresApproval(String),
    #[error("duplicate tool registration: '{0}'")]
    DuplicateRegistration(String),
}

/// A tool that can be invoked via ToolRequest.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name used in ToolRequest.tool_name
    fn name(&self) -> &str;
    /// Tool descriptor for discovery
    fn descriptor(&self) -> ToolDescriptor;
    /// Execute the tool
    async fn execute(&self, request: &ToolRequest) -> Result<ToolResponse, ToolError>;
}

/// Registry of available tools. Maps tool names to implementations.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Returns an error if a tool with the same name already
    /// exists.
    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<(), ToolError> {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            return Err(ToolError::DuplicateRegistration(name));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|b| b.as_ref())
    }

    /// List all registered tool descriptors.
    pub fn list_descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|t| t.descriptor()).collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Central tool dispatcher. Every tool call goes through the policy gate,
/// then routes to the appropriate tool implementation.
pub struct ToolDispatcher {
    registry: ToolRegistry,
    policy_gate: PolicyGate,
}

impl ToolDispatcher {
    pub fn new(registry: ToolRegistry, policy_gate: PolicyGate) -> Self {
        Self {
            registry,
            policy_gate,
        }
    }

    /// Dispatch a tool request through the policy gate to the tool.
    /// 1. Check policy gate (tool_rules, kill switch)
    /// 2. Look up tool in registry
    /// 3. Execute
    /// 4. Return ToolResponse with executed_on field
    #[tracing::instrument(
        name = "tool.dispatch",
        skip_all,
        fields(
            request_id = %request.request_id,
            session_id = %request.session_id,
            lease_id = %request.lease_id,
            tool_name = %request.tool_name,
        )
    )]
    pub async fn dispatch(&self, request: ToolRequest) -> Result<ToolResponse, ToolError> {
        debug!("dispatching tool request: {}", request.tool_name);

        // 0. Lease validation: every tool call must carry a valid context
        // lease ID. Validation is minimal (non-empty) — full lease
        // verification against the context plane happens at the session
        // layer; the dispatcher fails closed on a missing lease.
        if request.lease_id.is_empty() {
            warn!("tool '{}' rejected: missing lease_id", request.tool_name);
            return Err(ToolError::PolicyDenied("missing lease_id".into()));
        }

        // 1. Policy gate check
        let decision = self.policy_gate.check_tool(&request.tool_name);
        match &decision {
            Decision::Deny(reason) => {
                warn!("tool '{}' denied by policy: {reason}", request.tool_name);
                return Err(ToolError::PolicyDenied(reason.clone()));
            }
            Decision::RequireApproval(reason) => {
                warn!("tool '{}' requires approval: {reason}", request.tool_name);
                return Err(ToolError::RequiresApproval(reason.clone()));
            }
            Decision::Allow => {}
        }

        // 2. Look up tool
        let tool = self
            .registry
            .get(&request.tool_name)
            .ok_or_else(|| ToolError::NotFound(request.tool_name.clone()))?;

        // 3. Execute
        let mut response = tool.execute(&request).await?;

        // 4. Set executed_on to the locality string
        let locality = tool.descriptor().locality;
        let executed_on = ToolLocality::try_from(locality)
            .map(|l| l.as_str_name().to_string())
            .unwrap_or_else(|_| "UNKNOWN".into());
        response.executed_on = executed_on;

        debug!(
            "tool '{}' executed successfully (success: {})",
            request.tool_name, response.success
        );

        Ok(response)
    }

    /// Access the underlying registry (immutable).
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Access the policy gate.
    pub fn policy_gate(&self) -> &PolicyGate {
        &self.policy_gate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fabric_policy::eval::PolicyGate;
    use fabric_types::policy::{EffectivePolicy, ToolAction, ToolRule};
    use fabric_types::tools::ToolLocality;
    use pbjson_types::Struct;

    struct MockTool {
        name: String,
        fail: bool,
    }

    impl MockTool {
        fn new(name: &str) -> Self {
            Self {
                name: name.into(),
                fail: false,
            }
        }

        fn new_failing(name: &str) -> Self {
            Self {
                name: name.into(),
                fail: true,
            }
        }
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                tool_name: self.name.clone(),
                description: format!("mock tool: {}", self.name),
                input_schema: None,
                locality: ToolLocality::Either as i32,
                required_permissions: vec![],
            }
        }

        async fn execute(&self, request: &ToolRequest) -> Result<ToolResponse, ToolError> {
            if self.fail {
                return Err(ToolError::Execution("mock failure".into()));
            }
            Ok(ToolResponse {
                request_id: request.request_id.clone(),
                success: true,
                result: None,
                error: String::new(),
                screenshot: vec![],
                screenshot_mime: String::new(),
                completed_at: None,
                executed_on: String::new(),
            })
        }
    }

    fn allow_all_gate() -> PolicyGate {
        PolicyGate::new(EffectivePolicy {
            endpoint_version: "test".into(),
            server_version: "test".into(),
            data_rules: vec![],
            tool_rules: vec![ToolRule {
                tool_pattern: "*".into(),
                action: ToolAction::Allow as i32,
                condition: String::new(),
            }],
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

    fn deny_all_gate() -> PolicyGate {
        PolicyGate::new(EffectivePolicy {
            endpoint_version: "test".into(),
            server_version: "test".into(),
            data_rules: vec![],
            tool_rules: vec![],
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

    fn make_request(tool_name: &str) -> ToolRequest {
        ToolRequest {
            request_id: "req-1".into(),
            session_id: "session-1".into(),
            lease_id: "lease-1".into(),
            tool_name: tool_name.into(),
            params: Some(Struct {
                fields: Default::default(),
            }),
            policy_version: "1".into(),
            requested_at: None,
        }
    }

    #[tokio::test]
    async fn dispatches_to_correct_tool() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(MockTool::new("shell.exec")))
            .unwrap();

        let dispatcher = ToolDispatcher::new(registry, allow_all_gate());
        let resp = dispatcher
            .dispatch(make_request("shell.exec"))
            .await
            .unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn policy_deny_blocks_tool() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(MockTool::new("shell.exec")))
            .unwrap();

        let dispatcher = ToolDispatcher::new(registry, deny_all_gate());
        let err = dispatcher
            .dispatch(make_request("shell.exec"))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PolicyDenied(_)));
    }

    #[tokio::test]
    async fn unknown_tool_returns_not_found() {
        let registry = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(registry, allow_all_gate());
        let err = dispatcher
            .dispatch(make_request("nonexistent"))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn tool_execution_error_propagates() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(MockTool::new_failing("shell.exec")))
            .unwrap();

        let dispatcher = ToolDispatcher::new(registry, allow_all_gate());
        let err = dispatcher
            .dispatch(make_request("shell.exec"))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Execution(_)));
    }

    fn kill_switch_gate() -> PolicyGate {
        PolicyGate::new(EffectivePolicy {
            endpoint_version: "test".into(),
            server_version: "test".into(),
            data_rules: vec![],
            tool_rules: vec![ToolRule {
                tool_pattern: "*".into(),
                action: ToolAction::Allow as i32,
                condition: String::new(),
            }],
            model_rules: vec![],
            cua: None,
            inference_rules: vec![],
            kill_switch: true,
            max_retention_hours: 0,
            background_quota: None,
            max_session_duration_hours: 0,
            max_concurrent_sessions: 0,
        })
    }

    #[tokio::test]
    async fn kill_switch_engages() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(MockTool::new("shell.exec")))
            .unwrap();

        let dispatcher = ToolDispatcher::new(registry, kill_switch_gate());
        let err = dispatcher
            .dispatch(make_request("shell.exec"))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PolicyDenied(_)));
    }

    #[tokio::test]
    async fn empty_lease_id_denied() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(MockTool::new("shell.exec")))
            .unwrap();

        let dispatcher = ToolDispatcher::new(registry, allow_all_gate());
        let mut request = make_request("shell.exec");
        request.lease_id = String::new();
        let err = dispatcher.dispatch(request).await.unwrap_err();
        assert!(matches!(err, ToolError::PolicyDenied(_)));
    }

    #[tokio::test]
    async fn executed_on_set_from_locality() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(MockTool::new("shell.exec")))
            .unwrap();

        let dispatcher = ToolDispatcher::new(registry, allow_all_gate());
        let resp = dispatcher
            .dispatch(make_request("shell.exec"))
            .await
            .unwrap();
        assert_eq!(resp.executed_on, "TOOL_LOCALITY_EITHER");
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    struct TestTool {
        name: String,
    }

    impl TestTool {
        fn new(name: &str) -> Self {
            Self { name: name.into() }
        }
    }

    #[async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                tool_name: self.name.clone(),
                description: String::new(),
                input_schema: None,
                locality: ToolLocality::Either as i32,
                required_permissions: vec![],
            }
        }

        async fn execute(&self, _request: &ToolRequest) -> Result<ToolResponse, ToolError> {
            Ok(ToolResponse {
                request_id: String::new(),
                success: true,
                result: None,
                error: String::new(),
                screenshot: vec![],
                screenshot_mime: String::new(),
                completed_at: None,
                executed_on: String::new(),
            })
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(TestTool::new("fs.read")))
            .unwrap();
        let tool = registry.get("fs.read");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name(), "fs.read");
    }

    #[test]
    fn duplicate_registration_rejected() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(TestTool::new("fs.read")))
            .unwrap();
        let err = registry
            .register(Box::new(TestTool::new("fs.read")))
            .unwrap_err();
        assert!(matches!(err, ToolError::DuplicateRegistration(_)));
    }

    #[test]
    fn lookup_nonexistent_returns_none() {
        let registry = ToolRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn list_descriptors() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(TestTool::new("fs.read")))
            .unwrap();
        registry
            .register(Box::new(TestTool::new("shell.exec")))
            .unwrap();

        let descriptors = registry.list_descriptors();
        assert_eq!(descriptors.len(), 2);

        let names: Vec<&str> = descriptors.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"fs.read"));
        assert!(names.contains(&"shell.exec"));
    }

    #[test]
    fn empty_registry() {
        let registry = ToolRegistry::new();
        assert!(registry.is_empty());
    }
}
