use std::path::PathBuf;

use async_trait::async_trait;

use crate::container::{
    ContainerError, ContainerId, ContainerInfo, ContainerRuntime, ContainerSpec, ExecOutput,
    ExecSpec, ImageRef, ListFilter, RegistryAuth,
};

/// Direct containerd gRPC backend for endpoint-side execution.
/// Talks to /run/containerd/containerd.sock — no scheduler, single device.
/// Implementation deferred to endpoint-side phase.
pub struct ContainerdRuntime {
    pub socket_path: PathBuf,
}

impl Default for ContainerdRuntime {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from("/run/containerd/containerd.sock"),
        }
    }
}

#[async_trait]
impl ContainerRuntime for ContainerdRuntime {
    async fn pull(
        &self,
        _image: &str,
        _auth: Option<&RegistryAuth>,
    ) -> Result<ImageRef, ContainerError> {
        Err(ContainerError::NotImplemented(
            "containerd runtime not yet implemented".into(),
        ))
    }

    async fn create(&self, _spec: &ContainerSpec) -> Result<ContainerId, ContainerError> {
        Err(ContainerError::NotImplemented(
            "containerd runtime not yet implemented".into(),
        ))
    }

    async fn exec(&self, _id: &ContainerId, _cmd: &ExecSpec) -> Result<ExecOutput, ContainerError> {
        Err(ContainerError::NotImplemented(
            "containerd runtime not yet implemented".into(),
        ))
    }

    async fn teardown(&self, _id: &ContainerId) -> Result<(), ContainerError> {
        Err(ContainerError::NotImplemented(
            "containerd runtime not yet implemented".into(),
        ))
    }

    async fn list(&self, _filter: &ListFilter) -> Result<Vec<ContainerInfo>, ContainerError> {
        Err(ContainerError::NotImplemented(
            "containerd runtime not yet implemented".into(),
        ))
    }
}
