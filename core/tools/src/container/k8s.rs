use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use k8s_openapi::api::core::v1::{
    Capabilities, Container, EmptyDirVolumeSource, EnvVar, LocalObjectReference, Pod,
    PodSecurityContext, PodSpec, ResourceRequirements, SeccompProfile, Secret, SecurityContext,
    Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Api, AttachParams, DeleteParams, ListParams, PostParams};
use kube::Client;
use tokio::io::{AsyncRead, AsyncReadExt};
use tracing::{debug, info, warn};

use crate::container::{
    validate_image_allowed, ContainerError, ContainerId, ContainerInfo, ContainerRuntime,
    ContainerSpec, ExecOutput, ExecSpec, ImageRef, ListFilter, NetworkPolicy, RegistryAuth,
};

/// Name of the dockerconfigjson pull secret created by `pull()` when
/// registry auth is provided.
const PULL_SECRET_NAME: &str = "fabric-registry-auth";

/// Hard cap on captured exec stdout/stderr (10 MiB). Output beyond the cap
/// is truncated and flagged, never buffered unboundedly.
const MAX_EXEC_OUTPUT_BYTES: u64 = 10_485_760;

/// Kubernetes container runtime backend. Connects via the default kubeconfig
/// or in-cluster config. For dev, minikube sets up kubeconfig automatically.
///
/// Network isolation (NetworkPolicy) is NOT created by this runtime — that
/// requires cluster-admin scope and is the cluster operator's responsibility.
pub struct K8sRuntime {
    client: Client,
    default_namespace: String,
}

impl K8sRuntime {
    pub async fn new() -> Result<Self, ContainerError> {
        let client = Client::try_default()
            .await
            .map_err(|e| ContainerError::Runtime(format!("failed to create kube client: {e}")))?;
        Ok(Self {
            client,
            default_namespace: "default".into(),
        })
    }

    pub async fn with_namespace(namespace: &str) -> Result<Self, ContainerError> {
        let client = Client::try_default()
            .await
            .map_err(|e| ContainerError::Runtime(format!("failed to create kube client: {e}")))?;
        Ok(Self {
            client,
            default_namespace: namespace.into(),
        })
    }

    pub fn from_client(client: Client) -> Self {
        Self {
            client,
            default_namespace: "default".into(),
        }
    }

    fn ns<'a>(&'a self, spec: &'a ContainerSpec) -> &'a str {
        if spec.namespace.is_empty() {
            &self.default_namespace
        } else {
            &spec.namespace
        }
    }

    fn container_name(spec: &ContainerSpec) -> String {
        format!("fabric-{}", spec.name)
    }

    fn pod_labels(name: &str) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("app".into(), "fabric".into());
        labels.insert("fabric/container".into(), name.to_string());
        labels
    }

    /// Create (or update) a dockerconfigjson pull secret in the namespace.
    async fn ensure_pull_secret(
        &self,
        namespace: &str,
        auth: &RegistryAuth,
    ) -> Result<(), ContainerError> {
        let docker_config = serde_json::json!({
            "auths": {
                auth.registry.clone(): {
                    "username": auth.username,
                    "password": auth.password,
                }
            }
        });

        let secret = Secret {
            metadata: ObjectMeta {
                name: Some(PULL_SECRET_NAME.into()),
                namespace: Some(namespace.into()),
                ..Default::default()
            },
            type_: Some("kubernetes.io/dockerconfigjson".into()),
            string_data: Some(BTreeMap::from([(
                ".dockerconfigjson".into(),
                docker_config.to_string(),
            )])),
            ..Default::default()
        };

        let secrets: Api<Secret> = Api::namespaced(self.client.clone(), namespace);
        match secrets.create(&PostParams::default(), &secret).await {
            Ok(_) => Ok(()),
            Err(kube::Error::Api(resp)) if resp.code == 409 => {
                // Already exists — replace it.
                secrets
                    .replace(PULL_SECRET_NAME, &PostParams::default(), &secret)
                    .await
                    .map_err(ContainerError::Kube)?;
                Ok(())
            }
            Err(e) => Err(ContainerError::Kube(e)),
        }
    }

    /// Reference the pull secret on the pod if it exists in the namespace.
    async fn pull_secret_ref(&self, namespace: &str) -> Option<Vec<LocalObjectReference>> {
        let secrets: Api<Secret> = Api::namespaced(self.client.clone(), namespace);
        match secrets.get(PULL_SECRET_NAME).await {
            Ok(_) => Some(vec![LocalObjectReference {
                name: PULL_SECRET_NAME.into(),
            }]),
            Err(_) => None,
        }
    }

    /// Wait until the pod is Running so exec can attach.
    async fn wait_pod_running(pods: &Api<Pod>, name: &str) -> Result<(), ContainerError> {
        for _ in 0..60 {
            let pod = pods.get(name).await.map_err(ContainerError::Kube)?;
            let phase = pod
                .status
                .as_ref()
                .and_then(|s| s.phase.clone())
                .unwrap_or_default();
            match phase.as_str() {
                "Running" => return Ok(()),
                "Failed" | "Succeeded" | "Unknown" => {
                    return Err(ContainerError::ExecFailed(format!(
                        "pod {name} entered terminal phase {phase} before exec"
                    )));
                }
                _ => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
        Err(ContainerError::ExecFailed(format!(
            "pod {name} did not reach Running phase in time"
        )))
    }
}

/// Read an exec stream to EOF, capped at [`MAX_EXEC_OUTPUT_BYTES`]. Returns
/// the (possibly truncated) UTF-8-lossy contents and whether truncation
/// happened.
async fn read_capped(
    reader: impl AsyncRead + Unpin,
    stream: &str,
) -> Result<(String, bool), ContainerError> {
    let mut limited = reader.take(MAX_EXEC_OUTPUT_BYTES + 1);
    let mut buf = Vec::new();
    limited
        .read_to_end(&mut buf)
        .await
        .map_err(|e| ContainerError::ExecFailed(format!("failed to read {stream}: {e}")))?;
    let truncated = buf.len() as u64 > MAX_EXEC_OUTPUT_BYTES;
    if truncated {
        buf.truncate(MAX_EXEC_OUTPUT_BYTES as usize);
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
}

#[async_trait]
impl ContainerRuntime for K8sRuntime {
    async fn pull(
        &self,
        image: &str,
        auth: Option<&RegistryAuth>,
    ) -> Result<ImageRef, ContainerError> {
        if let Some(auth) = auth {
            self.ensure_pull_secret(&self.default_namespace, auth)
                .await?;
            debug!(
                "created pull secret {PULL_SECRET_NAME} for registry {}",
                auth.registry
            );
        }
        // The actual image pull is delegated to the kubelet on pod creation.
        info!("image pull delegated to K8s: {image}");
        Ok(image.to_string())
    }

    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerId, ContainerError> {
        validate_image_allowed(&spec.image, &spec.allowed_registries)?;

        if spec.network_policy == NetworkPolicy::Restricted {
            warn!(
                pod = %spec.name,
                "network_policy=Restricted is not yet enforced; pod will have default network access"
            );
        }

        let ns = self.ns(spec).to_string();
        let pod_name = Self::container_name(spec);
        let labels = Self::pod_labels(&pod_name);

        let env_vars: Vec<EnvVar> = spec
            .env
            .iter()
            .map(|(k, v)| EnvVar {
                name: k.clone(),
                value: Some(v.clone()),
                ..Default::default()
            })
            .collect();

        let container = Container {
            name: "tool".into(),
            image: Some(spec.image.clone()),
            command: Some(vec!["sleep".into(), "infinity".into()]),
            env: if env_vars.is_empty() {
                None
            } else {
                Some(env_vars)
            },
            resources: Some(ResourceRequirements {
                limits: Some(BTreeMap::from([
                    ("cpu".into(), Quantity(spec.cpu_limit.clone())),
                    ("memory".into(), Quantity(spec.memory_limit.clone())),
                ])),
                requests: Some(BTreeMap::from([
                    ("cpu".into(), Quantity(spec.cpu_limit.clone())),
                    ("memory".into(), Quantity(spec.memory_limit.clone())),
                ])),
                ..Default::default()
            }),
            security_context: Some(SecurityContext {
                allow_privilege_escalation: Some(false),
                read_only_root_filesystem: Some(true),
                capabilities: Some(Capabilities {
                    drop: Some(vec!["ALL".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            volume_mounts: Some(vec![VolumeMount {
                name: "tmp".into(),
                mount_path: "/tmp".into(),
                ..Default::default()
            }]),
            ..Default::default()
        };

        let image_pull_secrets = self.pull_secret_ref(&ns).await;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some(pod_name.clone()),
                namespace: Some(ns.clone()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![container],
                restart_policy: Some("Never".into()),
                automount_service_account_token: Some(false),
                active_deadline_seconds: Some(spec.timeout_s as i64),
                security_context: Some(PodSecurityContext {
                    run_as_non_root: Some(true),
                    run_as_user: Some(1000),
                    seccomp_profile: Some(SeccompProfile {
                        type_: "RuntimeDefault".into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                volumes: Some(vec![Volume {
                    name: "tmp".into(),
                    empty_dir: Some(EmptyDirVolumeSource::default()),
                    ..Default::default()
                }]),
                image_pull_secrets,
                ..Default::default()
            }),
            ..Default::default()
        };

        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &ns);
        pods.create(&PostParams::default(), &pod)
            .await
            .map_err(ContainerError::Kube)?;

        debug!("created pod {pod_name} in namespace {ns}");
        Ok(ContainerId(ns, pod_name))
    }

    async fn exec(&self, id: &ContainerId, cmd: &ExecSpec) -> Result<ExecOutput, ContainerError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &id.0);

        Self::wait_pod_running(&pods, &id.1).await?;

        // Build argv. Never join command args into a shell string — pass argv
        // directly to the exec API. Env vars are applied via `env(1)`; a
        // workdir is applied via a fixed shell wrapper that receives the
        // workdir and command as separate positional args (no injection).
        let mut argv: Vec<String> = Vec::new();
        if !cmd.env.is_empty() {
            argv.push("env".into());
            for (k, v) in &cmd.env {
                argv.push(format!("{k}={v}"));
            }
        }
        if let Some(wd) = &cmd.workdir {
            argv.extend([
                "/bin/sh".into(),
                "-c".into(),
                "cd \"$1\" && shift && exec \"$@\"".into(),
                "sh".into(),
                wd.clone(),
            ]);
        }
        argv.extend(cmd.command.iter().cloned());

        let ap = AttachParams {
            stdout: true,
            stderr: true,
            ..Default::default()
        };

        let mut attached = pods
            .exec(&id.1, argv, &ap)
            .await
            .map_err(|e| ContainerError::ExecFailed(format!("failed to exec in pod: {e}")))?;

        let stdout_reader = attached
            .stdout()
            .ok_or_else(|| ContainerError::ExecFailed("stdout stream unavailable".into()))?;
        let stderr_reader = attached
            .stderr()
            .ok_or_else(|| ContainerError::ExecFailed("stderr stream unavailable".into()))?;
        let status_fut = attached.take_status();

        let (out_res, err_res) = tokio::join!(
            read_capped(stdout_reader, "stdout"),
            read_capped(stderr_reader, "stderr"),
        );
        let (mut stdout, out_truncated) = out_res?;
        let (mut stderr, err_truncated) = err_res?;
        if out_truncated {
            stdout.push_str("\n[stdout truncated at 10 MiB]");
        }
        if err_truncated {
            stderr.push_str("\n[stderr truncated at 10 MiB]");
        }

        // Real exit status from the exec stream: "Success" maps to 0,
        // "Failure" maps to the reported status code (1 if absent/zero). A
        // missing status is NOT success: report -1 so callers fail closed.
        let exit_code = match status_fut {
            Some(fut) => match fut.await {
                Some(status) if status.status.as_deref() == Some("Success") => 0,
                Some(status) => match status.code.unwrap_or(1) {
                    0 => 1,
                    code => code,
                },
                None => -1,
            },
            None => -1,
        };
        if exit_code == -1 && !stderr.ends_with("status was not reported") {
            stderr.push_str("\nexec completed but status was not reported");
        }

        Ok(ExecOutput {
            stdout,
            stderr,
            exit_code,
        })
    }

    async fn teardown(&self, id: &ContainerId) -> Result<(), ContainerError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &id.0);
        pods.delete(&id.1, &DeleteParams::default())
            .await
            .map_err(|e| ContainerError::TeardownFailed(format!("delete pod failed: {e}")))?;

        debug!("deleted pod {} in namespace {}", id.1, id.0);
        Ok(())
    }

    async fn list(&self, filter: &ListFilter) -> Result<Vec<ContainerInfo>, ContainerError> {
        let ns = filter
            .namespace
            .clone()
            .unwrap_or_else(|| self.default_namespace.clone());

        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &ns);

        let lp = if filter.labels.is_empty() {
            ListParams::default()
        } else {
            let label_selector = filter
                .labels
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            ListParams::default().labels(&label_selector)
        };

        let pod_list = pods.list(&lp).await.map_err(ContainerError::Kube)?;

        let mut result = Vec::new();
        for item in pod_list.items {
            let name = item.metadata.name.unwrap_or_default();
            let image = item
                .spec
                .as_ref()
                .and_then(|ps| ps.containers.first())
                .and_then(|c| c.image.clone())
                .unwrap_or_default();
            let status = item
                .status
                .as_ref()
                .and_then(|s| s.phase.clone())
                .unwrap_or_else(|| "unknown".into());
            result.push(ContainerInfo {
                id: ContainerId(ns.clone(), name.clone()),
                name,
                namespace: ns.clone(),
                image,
                status,
            });
        }

        Ok(result)
    }
}
