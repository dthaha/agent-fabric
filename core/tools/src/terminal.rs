use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use fabric_types::tools::{ToolDescriptor, ToolLocality, ToolRequest, ToolResponse};
use pbjson_types::value::Kind;
use pbjson_types::{Struct, Value};
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::container::{ContainerRuntime, ContainerSpec, ExecSpec, NetworkPolicy, RegistryAuth};
use crate::dispatch::{Tool, ToolError};

#[derive(Debug, Clone)]
pub struct TerminalConfig {
    pub image: String,
    pub registry_auth: Option<RegistryAuth>,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub network_policy: NetworkPolicy,
    pub timeout_s: u64,
    pub namespace: String,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            image: "ubuntu:22.04".into(),
            registry_auth: None,
            cpu_limit: "1".into(),
            memory_limit: "1Gi".into(),
            network_policy: NetworkPolicy::None,
            timeout_s: 30,
            namespace: "default".into(),
        }
    }
}

/// The catch-all tool. Runs commands in an ephemeral OCI container.
pub struct TerminalTool {
    runtime: Arc<dyn ContainerRuntime>,
    config: TerminalConfig,
}

impl TerminalTool {
    pub fn new(runtime: Arc<dyn ContainerRuntime>, config: TerminalConfig) -> Self {
        Self { runtime, config }
    }

    async fn execute_command(
        &self,
        request: &ToolRequest,
        command: Vec<String>,
        workdir: Option<String>,
        env: HashMap<String, String>,
    ) -> Result<ToolResponse, ToolError> {
        let container_name = format!("term-{}", Uuid::now_v7());
        let spec = ContainerSpec {
            image: self.config.image.clone(),
            namespace: self.config.namespace.clone(),
            name: container_name.clone(),
            cpu_limit: self.config.cpu_limit.clone(),
            memory_limit: self.config.memory_limit.clone(),
            network_policy: self.config.network_policy.clone(),
            timeout_s: self.config.timeout_s,
            env: env.clone(),
        };

        // 1. Create ephemeral container
        debug!("creating ephemeral container {container_name}");
        let container_id = self
            .runtime
            .create(&spec)
            .await
            .map_err(|e| ToolError::Execution(format!("failed to create container: {e}")))?;

        // 2. Exec the command
        let exec_spec = ExecSpec {
            command,
            workdir,
            env,
        };

        debug!("executing command in container {container_id}");
        let exec_result = self
            .runtime
            .exec(&container_id, &exec_spec)
            .await
            .map_err(|e| ToolError::Execution(format!("exec failed: {e}")))?;

        // 3. Capture output
        let success = exec_result.exit_code == 0;
        let mut fields = HashMap::new();
        fields.insert(
            "stdout".into(),
            Value {
                kind: Some(Kind::StringValue(exec_result.stdout)),
            },
        );
        fields.insert(
            "stderr".into(),
            Value {
                kind: Some(Kind::StringValue(exec_result.stderr)),
            },
        );
        fields.insert(
            "exit_code".into(),
            Value {
                kind: Some(Kind::NumberValue(exec_result.exit_code as f64)),
            },
        );

        let error = if !success {
            format!("command exited with code {}", exec_result.exit_code)
        } else {
            String::new()
        };

        // 4. Teardown container
        if let Err(e) = self.runtime.teardown(&container_id).await {
            error!("failed to teardown container {container_id}: {e}");
        }

        info!(
            "terminal command completed (exit_code={}, success={})",
            exec_result.exit_code, success
        );

        // 5. Return ToolResponse
        Ok(ToolResponse {
            request_id: request.request_id.clone(),
            success,
            result: Some(Struct { fields }),
            error,
            screenshot: vec![],
            screenshot_mime: String::new(),
            completed_at: Some(pbjson_types::Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: 0,
            }),
            executed_on: String::new(),
        })
    }
}

#[async_trait]
impl Tool for TerminalTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn descriptor(&self) -> ToolDescriptor {
        let mut fields = HashMap::new();
        fields.insert(
            "command".into(),
            Value {
                kind: Some(Kind::StringValue(
                    "string — shell command to execute".into(),
                )),
            },
        );
        fields.insert(
            "workdir".into(),
            Value {
                kind: Some(Kind::StringValue(
                    "string — optional working directory".into(),
                )),
            },
        );
        fields.insert(
            "env".into(),
            Value {
                kind: Some(Kind::StringValue(
                    "object — optional environment variables".into(),
                )),
            },
        );

        ToolDescriptor {
            tool_name: "terminal".into(),
            description: "Execute shell commands in an ephemeral OCI container".into(),
            input_schema: Some(Struct { fields }),
            locality: ToolLocality::Either as i32,
            required_permissions: vec!["terminal.exec".into()],
        }
    }

    async fn execute(&self, request: &ToolRequest) -> Result<ToolResponse, ToolError> {
        let params = request
            .params
            .as_ref()
            .ok_or_else(|| ToolError::Execution("missing params".into()))?;

        let command_field = params
            .fields
            .get("command")
            .or_else(|| params.fields.get("Command"))
            .and_then(|v| v.kind.as_ref())
            .ok_or_else(|| ToolError::Execution("missing 'command' in params".into()))?;

        let command = if let Kind::StringValue(cmd) = command_field {
            vec!["/bin/sh".into(), "-c".into(), cmd.clone()]
        } else {
            return Err(ToolError::Execution("'command' must be a string".into()));
        };

        let workdir = params
            .fields
            .get("workdir")
            .or_else(|| params.fields.get("Workdir"))
            .and_then(|v| v.kind.as_ref())
            .and_then(|k| {
                if let Kind::StringValue(wd) = k {
                    Some(wd.clone())
                } else {
                    None
                }
            });

        let mut env = HashMap::new();
        if let Some(env_field) = params
            .fields
            .get("env")
            .or_else(|| params.fields.get("Env"))
        {
            if let Some(Kind::StructValue(struct_val)) = &env_field.kind {
                for (k, v) in &struct_val.fields {
                    if let Some(Kind::StringValue(s)) = &v.kind {
                        env.insert(k.clone(), s.clone());
                    }
                }
            }
        }

        self.execute_command(request, command, workdir, env).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{
        ContainerError, ContainerId, ContainerInfo, ContainerRuntime, ExecOutput, ExecSpec,
        ImageRef, ListFilter, RegistryAuth,
    };
    use async_trait::async_trait;
    use std::sync::Arc;

    struct MockTerminalRuntime {
        _private: std::marker::PhantomData<()>,
    }

    impl MockTerminalRuntime {
        fn new() -> Self {
            Self {
                _private: std::marker::PhantomData,
            }
        }
    }

    #[async_trait]
    impl ContainerRuntime for MockTerminalRuntime {
        async fn pull(
            &self,
            _image: &str,
            _auth: Option<&RegistryAuth>,
        ) -> Result<ImageRef, ContainerError> {
            Ok("test-image".into())
        }

        async fn create(&self, spec: &ContainerSpec) -> Result<ContainerId, ContainerError> {
            Ok(ContainerId(spec.name.clone()))
        }

        async fn exec(
            &self,
            _id: &ContainerId,
            _cmd: &ExecSpec,
        ) -> Result<ExecOutput, ContainerError> {
            Ok(ExecOutput {
                stdout: "hello world".into(),
                stderr: String::new(),
                exit_code: 0,
            })
        }

        async fn teardown(&self, _id: &ContainerId) -> Result<(), ContainerError> {
            Ok(())
        }

        async fn list(&self, _filter: &ListFilter) -> Result<Vec<ContainerInfo>, ContainerError> {
            Ok(Vec::new())
        }
    }

    fn make_terminal_request(cmd: &str, workdir: Option<&str>) -> ToolRequest {
        let mut fields = HashMap::new();
        fields.insert(
            "command".into(),
            Value {
                kind: Some(Kind::StringValue(cmd.into())),
            },
        );
        if let Some(wd) = workdir {
            fields.insert(
                "workdir".into(),
                Value {
                    kind: Some(Kind::StringValue(wd.into())),
                },
            );
        }

        ToolRequest {
            request_id: "term-req-1".into(),
            session_id: "session-1".into(),
            lease_id: "lease-1".into(),
            tool_name: "terminal".into(),
            params: Some(Struct { fields }),
            policy_version: "1".into(),
            requested_at: None,
        }
    }

    #[tokio::test]
    async fn terminal_tool_returns_success_response() {
        let runtime: Arc<dyn ContainerRuntime> = Arc::new(MockTerminalRuntime::new());
        let tool = TerminalTool::new(runtime, TerminalConfig::default());

        let request = make_terminal_request("echo hello", None);
        let response = tool.execute(&request).await.unwrap();

        assert!(response.success);
        assert_eq!(response.request_id, "term-req-1");
    }

    #[tokio::test]
    async fn terminal_tool_missing_command_returns_error() {
        let runtime: Arc<dyn ContainerRuntime> = Arc::new(MockTerminalRuntime::new());
        let tool = TerminalTool::new(runtime, TerminalConfig::default());

        let request = ToolRequest {
            request_id: "err-req".into(),
            session_id: "session-1".into(),
            lease_id: "lease-1".into(),
            tool_name: "terminal".into(),
            params: Some(Struct {
                fields: HashMap::new(),
            }),
            policy_version: "1".into(),
            requested_at: None,
        };

        let err = tool.execute(&request).await.unwrap_err();
        assert!(matches!(err, ToolError::Execution(_)));
    }

    #[tokio::test]
    async fn terminal_tool_executed_on_field_set_by_dispatcher() {
        let runtime: Arc<dyn ContainerRuntime> = Arc::new(MockTerminalRuntime::new());
        let tool = TerminalTool::new(runtime, TerminalConfig::default());

        let request = make_terminal_request("echo hello", None);
        let mut response = tool.execute(&request).await.unwrap();

        assert_eq!(response.executed_on, "");
        let locality = tool.descriptor().locality;
        response.executed_on = ToolLocality::try_from(locality)
            .unwrap()
            .as_str_name()
            .to_string();
        assert_eq!(response.executed_on, "TOOL_LOCALITY_EITHER");
    }

    #[test]
    fn terminal_tool_descriptor() {
        let runtime: Arc<dyn ContainerRuntime> = Arc::new(MockTerminalRuntime::new());
        let tool = TerminalTool::new(runtime, TerminalConfig::default());

        let desc = tool.descriptor();
        assert_eq!(desc.tool_name, "terminal");
        assert_eq!(desc.locality, ToolLocality::Either as i32);
        assert!(desc.required_permissions.contains(&"terminal.exec".into()));
    }
}
