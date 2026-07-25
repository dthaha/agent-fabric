use std::collections::BTreeMap;

use async_trait::async_trait;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{Container, EnvVar, PodSpec, ResourceRequirements};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use kube::Client;
use tracing::{debug, info};

use crate::container::{
    ContainerError, ContainerId, ContainerInfo, ContainerRuntime, ContainerSpec, ExecOutput,
    ExecSpec, ImageRef, ListFilter, RegistryAuth,
};

/// Kubernetes container runtime backend. Connects via the default kubeconfig
/// or in-cluster config. For dev, minikube sets up kubeconfig automatically.
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

    fn pod_labels(id: &ContainerId) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("app".into(), "fabric".into());
        labels.insert("fabric/container".into(), id.0.clone());
        labels
    }
}

#[async_trait]
impl ContainerRuntime for K8sRuntime {
    async fn pull(
        &self,
        image: &str,
        _auth: Option<&RegistryAuth>,
    ) -> Result<ImageRef, ContainerError> {
        info!("image pull delegated to K8s: {image}");
        Ok(image.to_string())
    }

    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerId, ContainerError> {
        let ns = self.ns(spec).to_string();
        let container_name = Self::container_name(spec);
        let id = ContainerId(container_name.clone());
        let labels = Self::pod_labels(&id);

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
            ..Default::default()
        };

        let job = Job {
            metadata: ObjectMeta {
                name: Some(container_name.clone()),
                namespace: Some(ns.clone()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::batch::v1::JobSpec {
                template: k8s_openapi::api::core::v1::PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(labels),
                        ..Default::default()
                    }),
                    spec: Some(PodSpec {
                        containers: vec![container],
                        restart_policy: Some("Never".into()),
                        ..Default::default()
                    }),
                },
                active_deadline_seconds: Some(spec.timeout_s as i64),
                backoff_limit: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        };

        let jobs: Api<Job> = Api::namespaced(self.client.clone(), &ns);
        jobs.create(&PostParams::default(), &job)
            .await
            .map_err(ContainerError::Kube)?;

        debug!("created job {container_name} in namespace {ns}");
        Ok(ContainerId(container_name))
    }

    async fn exec(&self, id: &ContainerId, cmd: &ExecSpec) -> Result<ExecOutput, ContainerError> {
        let ns = "default".to_string();
        let exec_job_name = format!(
            "{}-exec-{}",
            id.0,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let labels = Self::pod_labels(id);

        let env_vars: Vec<EnvVar> = cmd
            .env
            .iter()
            .map(|(k, v)| EnvVar {
                name: k.clone(),
                value: Some(v.clone()),
                ..Default::default()
            })
            .collect();

        let mut shell_args = vec!["/bin/sh".into(), "-c".into()];
        let cmd_str = cmd.command.join(" ");
        if let Some(wd) = &cmd.workdir {
            shell_args.push(format!("cd {} && {} && echo __EXIT__=$?", wd, cmd_str));
        } else {
            shell_args.push(format!("{} && echo __EXIT__=$?", cmd_str));
        }

        let pod = Container {
            name: "exec".into(),
            image: Some("ubuntu:22.04".into()),
            command: Some(shell_args),
            env: if env_vars.is_empty() {
                None
            } else {
                Some(env_vars)
            },
            ..Default::default()
        };

        let job = Job {
            metadata: ObjectMeta {
                name: Some(exec_job_name.clone()),
                namespace: Some(ns.clone()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::batch::v1::JobSpec {
                template: k8s_openapi::api::core::v1::PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        name: Some(exec_job_name.clone()),
                        ..Default::default()
                    }),
                    spec: Some(PodSpec {
                        containers: vec![pod],
                        restart_policy: Some("Never".into()),
                        ..Default::default()
                    }),
                },
                active_deadline_seconds: Some(30),
                backoff_limit: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        };

        let jobs: Api<Job> = Api::namespaced(self.client.clone(), &ns);
        jobs.create(&PostParams::default(), &job)
            .await
            .map_err(|e| ContainerError::ExecFailed(format!("failed to create exec job: {e}")))?;

        // Wait for the job to complete
        let job_api: Api<Job> = Api::namespaced(self.client.clone(), &ns);
        let mut succeeded = false;
        for _ in 0..60 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            if let Ok(j) = job_api.get(&exec_job_name).await {
                if let Some(status) = &j.status {
                    if status.succeeded == Some(1) {
                        succeeded = true;
                        break;
                    }
                    if status.failed == Some(1) {
                        break;
                    }
                }
            }
        }

        // Read pod logs by fetching the pod's log through the K8s API directly
        let logs_url = format!(
            "/api/v1/namespaces/{}/pods/{}/log?container=exec",
            ns, exec_job_name
        );

        let request = http::Request::get(&logs_url)
            .body(Vec::<u8>::new())
            .map_err(|e| ContainerError::ExecFailed(format!("failed to build request: {e}")))?;

        let log_text: String = self
            .client
            .request(request)
            .await
            .map_err(|e| ContainerError::ExecFailed(format!("K8s API request failed: {e}")))?;

        let mut stdout = String::new();
        let stderr = String::new();
        let mut exit_code = 1;

        for line in log_text.lines() {
            if let Some(code_str) = line.strip_prefix("__EXIT__=") {
                if let Ok(code) = code_str.trim().parse::<i32>() {
                    exit_code = code;
                }
            } else {
                stdout.push_str(line);
                stdout.push('\n');
            }
        }

        if succeeded && exit_code == 1 {
            exit_code = 0;
        }

        // Cleanup the exec job
        jobs.delete(&exec_job_name, &DeleteParams::default())
            .await
            .ok();

        Ok(ExecOutput {
            stdout,
            stderr,
            exit_code,
        })
    }

    async fn teardown(&self, id: &ContainerId) -> Result<(), ContainerError> {
        let ns = "default".to_string();
        let jobs: Api<Job> = Api::namespaced(self.client.clone(), &ns);
        jobs.delete(&id.0, &DeleteParams::default())
            .await
            .map_err(|e| ContainerError::TeardownFailed(format!("delete job failed: {e}")))?;

        debug!("deleted job {}", id.0);
        Ok(())
    }

    async fn list(&self, filter: &ListFilter) -> Result<Vec<ContainerInfo>, ContainerError> {
        let ns = filter
            .namespace
            .clone()
            .unwrap_or_else(|| self.default_namespace.clone());

        let jobs: Api<Job> = Api::namespaced(self.client.clone(), &ns);

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

        let job_list = jobs.list(&lp).await.map_err(ContainerError::Kube)?;

        let mut result = Vec::new();
        for item in job_list.items {
            let name = item.metadata.name.unwrap_or_default();
            let image = item
                .spec
                .as_ref()
                .and_then(|s| s.template.spec.as_ref())
                .and_then(|ps| ps.containers.first())
                .and_then(|c| c.image.clone())
                .unwrap_or_default();
            result.push(ContainerInfo {
                id: ContainerId(name.clone()),
                name,
                namespace: ns.clone(),
                image,
                status: "active".into(),
            });
        }

        Ok(result)
    }
}
