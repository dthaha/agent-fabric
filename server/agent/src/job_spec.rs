//! Builds the K8s Job spec for a delegated agent task (ADR 008 §3). Same
//! pattern as the terminal tool's ephemeral pods in
//! `core/tools/src/container/k8s.rs`, with a different entrypoint: the Job
//! runs the org's agent image with `@fabric/pi-session-backend` headless,
//! pointed at the spine (Postgres) and the lease authority (Valkey).

use std::collections::BTreeMap;

use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{
    Capabilities, Container, EnvVar, PodSecurityContext, PodSpec, PodTemplateSpec,
    ResourceRequirements, SeccompProfile, SecurityContext,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use uuid::Uuid;

use crate::orchestrator::AgentTaskRequest;

/// Default namespace for agent Jobs (`FABRIC_K8S_NAMESPACE` overrides).
pub const DEFAULT_NAMESPACE: &str = "fabric-agents";

/// Jobs self-clean 5 minutes after finishing; the spine is the record.
pub const TTL_SECONDS_AFTER_FINISHED: i32 = 300;

/// No retries: the spine handles resume, a retried Job would double-append.
pub const BACKOFF_LIMIT: i32 = 0;

const JOB_NAME_PREFIX: &str = "fabric-agent-";
/// `fabric-agent-` (13) + session short (38) + `-` (1) + uuid short (8) = 60,
/// under the 63-char DNS label cap.
const SESSION_SHORT_MAX: usize = 38;
const UUID_SHORT_LEN: usize = 8;

/// Server-side config baked into the Job as env vars.
#[derive(Clone, Debug)]
pub struct JobSpecConfig {
    pub namespace: String,
    pub pg_url: String,
    pub kv_url: String,
}

/// Lowercase DNS-1123-label-safe rendering: alnum kept, anything else
/// collapses to single dashes, no leading/trailing dashes. Input is ASCII-
/// sanitized, so `truncate` on the result is always on a char boundary.
fn dns_safe(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_dash = false;
    for c in raw.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !out.is_empty() && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Kubernetes label value: max 63 chars, `[A-Za-z0-9-_.]` with alnum ends.
fn label_value(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .take(63)
        .collect();
    while out.starts_with(|c: char| !c.is_ascii_alphanumeric()) {
        out.remove(0);
    }
    while out.ends_with(|c: char| !c.is_ascii_alphanumeric()) {
        out.pop();
    }
    out
}

/// `fabric-agent-{session_short}-{uuid_short}`, DNS-safe, max 63 chars.
pub fn job_name(session_id: &str, task_uuid: &Uuid) -> String {
    let mut session_short = dns_safe(session_id);
    if session_short.is_empty() {
        session_short.push_str("session");
    }
    session_short.truncate(SESSION_SHORT_MAX);
    while session_short.ends_with('-') {
        session_short.pop();
    }
    let uuid_short = &task_uuid.simple().to_string()[..UUID_SHORT_LEN];
    format!("{JOB_NAME_PREFIX}{session_short}-{uuid_short}")
}

fn env(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: Some(value.to_string()),
        ..Default::default()
    }
}

/// Labels identifying the task on the Job and its pod template.
fn task_labels(task_id: &str, session_id: &str, soul_id: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app".into(), "fabric".into()),
        ("fabric.dev/task-id".into(), label_value(task_id)),
        ("fabric.dev/session-id".into(), label_value(session_id)),
        ("fabric.dev/soul-id".into(), label_value(soul_id)),
    ])
}

/// Build the Job for a delegated agent task. Pure: no API calls, so the
/// shape is fully unit-testable.
pub fn build_job(
    req: &AgentTaskRequest,
    task_id: &str,
    job_name: &str,
    lease_id: &str,
    cfg: &JobSpecConfig,
) -> Job {
    let labels = task_labels(task_id, &req.session_id, &req.soul_id);
    let limits = &req.resource_limits;
    let resources = BTreeMap::from([
        (
            "cpu".into(),
            Quantity(format!("{}m", limits.cpu_millicores)),
        ),
        (
            "memory".into(),
            Quantity(format!("{}Mi", limits.memory_mib)),
        ),
    ]);

    let container = Container {
        name: "agent".into(),
        image: Some(req.image.clone()),
        command: Some(vec![
            "node".into(),
            "-e".into(),
            "require('@fabric/pi-session-backend/headless').run()".into(),
        ]),
        env: Some(vec![
            env("FABRIC_PG_URL", &cfg.pg_url),
            env("FABRIC_KV_URL", &cfg.kv_url),
            env("FABRIC_SESSION_ID", &req.session_id),
            env("FABRIC_SOUL_ID", &req.soul_id),
            env("FABRIC_ORG_ID", &req.org_id),
            env("FABRIC_TASK_ID", task_id),
            env("FABRIC_LEASE_ID", lease_id),
            env("FABRIC_TASK_PROMPT", &req.prompt),
            env("FABRIC_LOCUS", "SERVER"),
        ]),
        resources: Some(ResourceRequirements {
            limits: Some(resources.clone()),
            requests: Some(resources),
            ..Default::default()
        }),
        // Same locked-down posture as the terminal tool's pods.
        security_context: Some(SecurityContext {
            allow_privilege_escalation: Some(false),
            read_only_root_filesystem: Some(true),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".into()]),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    Job {
        metadata: ObjectMeta {
            name: Some(job_name.to_string()),
            namespace: Some(cfg.namespace.clone()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(JobSpec {
            backoff_limit: Some(BACKOFF_LIMIT),
            active_deadline_seconds: Some(limits.timeout_secs as i64),
            ttl_seconds_after_finished: Some(TTL_SECONDS_AFTER_FINISHED),
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![container],
                    restart_policy: Some("Never".into()),
                    automount_service_account_token: Some(false),
                    security_context: Some(PodSecurityContext {
                        run_as_non_root: Some(true),
                        run_as_user: Some(1000),
                        seccomp_profile: Some(SeccompProfile {
                            type_: "RuntimeDefault".into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use fabric_types::context::Locus;

    use super::*;
    use crate::orchestrator::ResourceLimits;

    fn cfg() -> JobSpecConfig {
        JobSpecConfig {
            namespace: DEFAULT_NAMESPACE.into(),
            pg_url: "postgres://fabric:fabric@postgres:5432/fabric".into(),
            kv_url: "redis://valkey:6379".into(),
        }
    }

    fn req() -> AgentTaskRequest {
        AgentTaskRequest {
            session_id: "sess-abc123".into(),
            soul_id: "soul-xyz".into(),
            org_id: "org-1".into(),
            prompt: "summarize the quarter".into(),
            locus: Locus::Server,
            ttl_ms: None,
            image: "registry.example.com/agents/pi:1.2.3".into(),
            resource_limits: ResourceLimits {
                cpu_millicores: 2000,
                memory_mib: 4096,
                timeout_secs: 1800,
            },
        }
    }

    fn env_map(job: &Job) -> BTreeMap<String, String> {
        job.spec
            .as_ref()
            .unwrap()
            .template
            .spec
            .as_ref()
            .unwrap()
            .containers[0]
            .env
            .as_ref()
            .unwrap()
            .iter()
            .map(|e| (e.name.clone(), e.value.clone().unwrap()))
            .collect()
    }

    #[test]
    fn job_name_is_dns_safe_and_bounded() {
        let uuid = Uuid::now_v7();
        let name = job_name("Sess_ABC 123/with weird chars", &uuid);
        assert!(name.len() <= 63, "{name} ({} chars)", name.len());
        assert!(name.starts_with("fabric-agent-"), "{name}");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{name}"
        );
        assert!(!name.ends_with('-'), "{name}");
        assert!(name.contains(&uuid.simple().to_string()[..UUID_SHORT_LEN]));

        // Long session ids are truncated, still bounded.
        let long = job_name(&"s".repeat(500), &uuid);
        assert!(long.len() <= 63, "{long}");

        // A session id with no alnum at all falls back to a valid name.
        let fallback = job_name("@@@", &uuid);
        assert!(fallback.starts_with("fabric-agent-session-"), "{fallback}");
    }

    #[test]
    fn dns_safe_sanitization() {
        assert_eq!(dns_safe("Sess_ABC 123"), "sess-abc-123");
        assert_eq!(dns_safe("--lead--trail--"), "lead-trail");
        assert_eq!(dns_safe("a//b///c"), "a-b-c");
        assert_eq!(dns_safe(""), "");
    }

    #[test]
    fn label_value_sanitization() {
        assert_eq!(label_value("sess-abc_1.2"), "sess-abc_1.2");
        assert_eq!(label_value("@@sess@@"), "sess");
        assert_eq!(label_value(&"x".repeat(100)), "x".repeat(63));
    }

    #[test]
    fn job_metadata_and_labels() {
        let uuid = Uuid::now_v7();
        let name = job_name("sess-abc123", &uuid);
        let job = build_job(&req(), "task-1", &name, "lease-1", &cfg());

        let meta = &job.metadata;
        assert_eq!(meta.name.as_deref(), Some(name.as_str()));
        assert_eq!(meta.namespace.as_deref(), Some(DEFAULT_NAMESPACE));

        let labels = meta.labels.as_ref().unwrap();
        assert_eq!(labels.get("fabric.dev/task-id").unwrap(), "task-1");
        assert_eq!(labels.get("fabric.dev/session-id").unwrap(), "sess-abc123");
        assert_eq!(labels.get("fabric.dev/soul-id").unwrap(), "soul-xyz");

        // Pod template carries the same labels so selectors find the pod.
        let pod_labels = job
            .spec
            .as_ref()
            .unwrap()
            .template
            .metadata
            .as_ref()
            .unwrap()
            .labels
            .as_ref()
            .unwrap();
        assert_eq!(pod_labels, labels);
    }

    #[test]
    fn job_lifecycle_fields() {
        let job = build_job(&req(), "task-1", "fabric-agent-x-y", "lease-1", &cfg());
        let spec = job.spec.as_ref().unwrap();
        assert_eq!(spec.backoff_limit, Some(0));
        assert_eq!(spec.active_deadline_seconds, Some(1800));
        assert_eq!(
            spec.ttl_seconds_after_finished,
            Some(TTL_SECONDS_AFTER_FINISHED)
        );

        let pod = spec.template.spec.as_ref().unwrap();
        assert_eq!(pod.restart_policy.as_deref(), Some("Never"));
        assert_eq!(pod.automount_service_account_token, Some(false));
    }

    #[test]
    fn container_shape() {
        let job = build_job(&req(), "task-1", "fabric-agent-x-y", "lease-1", &cfg());
        let pod = job.spec.as_ref().unwrap().template.spec.as_ref().unwrap();
        let container = &pod.containers[0];

        assert_eq!(
            container.image.as_deref(),
            Some("registry.example.com/agents/pi:1.2.3")
        );
        assert_eq!(
            container.command.as_ref().unwrap(),
            &vec![
                "node".to_string(),
                "-e".to_string(),
                "require('@fabric/pi-session-backend/headless').run()".to_string()
            ]
        );

        let resources = container.resources.as_ref().unwrap();
        let limits = resources.limits.as_ref().unwrap();
        assert_eq!(limits.get("cpu").unwrap(), &Quantity("2000m".into()));
        assert_eq!(limits.get("memory").unwrap(), &Quantity("4096Mi".into()));
        assert_eq!(resources.requests.as_ref().unwrap(), limits);

        let env = env_map(&job);
        let expected = [
            (
                "FABRIC_PG_URL",
                "postgres://fabric:fabric@postgres:5432/fabric",
            ),
            ("FABRIC_KV_URL", "redis://valkey:6379"),
            ("FABRIC_SESSION_ID", "sess-abc123"),
            ("FABRIC_SOUL_ID", "soul-xyz"),
            ("FABRIC_ORG_ID", "org-1"),
            ("FABRIC_TASK_ID", "task-1"),
            ("FABRIC_LEASE_ID", "lease-1"),
            ("FABRIC_TASK_PROMPT", "summarize the quarter"),
            ("FABRIC_LOCUS", "SERVER"),
        ];
        for (key, value) in expected {
            assert_eq!(env.get(key).map(String::as_str), Some(value), "env {key}");
        }
    }

    #[test]
    fn container_is_locked_down() {
        let job = build_job(&req(), "task-1", "fabric-agent-x-y", "lease-1", &cfg());
        let pod = job.spec.as_ref().unwrap().template.spec.as_ref().unwrap();

        let sc = pod.containers[0].security_context.as_ref().unwrap();
        assert_eq!(sc.allow_privilege_escalation, Some(false));
        assert_eq!(sc.read_only_root_filesystem, Some(true));
        assert_eq!(
            sc.capabilities.as_ref().unwrap().drop.as_ref().unwrap(),
            &vec!["ALL".to_string()]
        );

        let psc = pod.security_context.as_ref().unwrap();
        assert_eq!(psc.run_as_non_root, Some(true));
        assert_eq!(psc.run_as_user, Some(1000));
        assert_eq!(
            psc.seccomp_profile.as_ref().unwrap().type_,
            "RuntimeDefault"
        );
    }
}
