pub mod containerd;
pub mod k8s;
pub mod lifecycle;

use std::collections::HashMap;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContainerError {
    #[error("container runtime error: {0}")]
    Runtime(String),
    #[error("image pull failed: {0}")]
    ImagePull(String),
    #[error("exec failed: {0}")]
    ExecFailed(String),
    #[error("teardown failed: {0}")]
    TeardownFailed(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
    #[error("Kubernetes error: {0}")]
    Kube(#[from] kube::Error),
    #[error("container not found: {0}")]
    NotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ImageRef = String;

/// Identifies a container: `.0` is the namespace, `.1` is the name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerId(pub String, pub String);

impl std::fmt::Display for ContainerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.0, self.1)
    }
}

#[derive(Debug, Clone)]
pub struct ContainerSpec {
    pub image: String,
    pub namespace: String,
    pub name: String,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub network_policy: NetworkPolicy,
    pub timeout_s: u64,
    pub env: HashMap<String, String>,
    /// Registry allowlist. Empty means all registries are allowed
    /// (backward compatible). When non-empty, the image's registry must
    /// match one of the entries; images without an explicit registry are
    /// treated as `docker.io`.
    pub allowed_registries: Vec<String>,
}

/// Extract the registry host from an image reference. Images without an
/// explicit registry (e.g. `ubuntu:22.04`) default to `docker.io`.
pub fn image_registry(image: &str) -> &str {
    let first = image.split('/').next().unwrap_or("");
    if first.contains('.') || first.contains(':') || first == "localhost" {
        first
    } else {
        "docker.io"
    }
}

/// Validate an image against a registry allowlist. Empty allowlist allows
/// everything.
pub fn validate_image_allowed(
    image: &str,
    allowed_registries: &[String],
) -> Result<(), ContainerError> {
    if allowed_registries.is_empty() {
        return Ok(());
    }
    let registry = image_registry(image);
    if allowed_registries.iter().any(|r| r == registry) {
        Ok(())
    } else {
        Err(ContainerError::ImagePull(format!(
            "image '{image}' registry '{registry}' is not in the allowed registries: {allowed_registries:?}"
        )))
    }
}

#[derive(Debug, Clone)]
pub struct ExecSpec {
    pub command: Vec<String>,
    pub workdir: Option<String>,
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Clone)]
pub struct RegistryAuth {
    pub username: String,
    pub password: String,
    pub registry: String,
}

impl std::fmt::Debug for RegistryAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryAuth")
            .field("username", &self.username)
            .field("registry", &self.registry)
            .field("password", &"***REDACTED***")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkPolicy {
    Unrestricted,
    Restricted,
    None,
}

#[derive(Debug, Clone)]
pub struct ListFilter {
    pub namespace: Option<String>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: ContainerId,
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub status: String,
}

#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    async fn pull(
        &self,
        image: &str,
        auth: Option<&RegistryAuth>,
    ) -> Result<ImageRef, ContainerError>;
    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerId, ContainerError>;
    async fn exec(&self, id: &ContainerId, cmd: &ExecSpec) -> Result<ExecOutput, ContainerError>;
    async fn teardown(&self, id: &ContainerId) -> Result<(), ContainerError>;
    async fn list(&self, filter: &ListFilter) -> Result<Vec<ContainerInfo>, ContainerError>;
}
