//! Server-side agent task orchestration (ADR 008 §3). The orchestrator is
//! control-plane logic only: it never runs the agent loop itself. It
//! acquires the session's write lease, creates an ephemeral K8s Job that
//! runs pi's `agentLoop` headless against the spine, monitors the Job, and
//! releases the lease when the task finishes, is preempted, or dies.

use std::time::{Duration, Instant};

use chrono::Utc;
use fabric_context::{LeaseAuthority, StoreError, DEFAULT_LEASE_TTL_MS, MAX_LEASE_TTL_MS};
use fabric_types::context::Locus;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::Secret;
use kube::api::{Api, DeleteParams, PostParams};
use sqlx::{PgPool, Row};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::error::AgentTaskError;
use crate::job_spec::{self, JobSpecConfig};
use crate::task_state::{TaskRecord, TaskState};

/// Extra time the monitor waits past `active_deadline_seconds` for K8s to
/// reap the Job before declaring it failed.
const MONITOR_GRACE: Duration = Duration::from_secs(30);

/// A delegated agent task request.
#[derive(Clone, Debug)]
pub struct AgentTaskRequest {
    pub session_id: String,
    pub soul_id: String,
    pub org_id: String,
    /// The task instruction, passed to the agent as `FABRIC_TASK_PROMPT`.
    pub prompt: String,
    /// Always `LOCUS_SERVER` for now; `LOCUS_UNSPECIFIED` is accepted and
    /// normalized, anything else is rejected.
    pub locus: Locus,
    /// Max task duration; also the write-lease TTL. Defaults to
    /// [`DEFAULT_LEASE_TTL_MS`], clamped to [`MAX_LEASE_TTL_MS`].
    pub ttl_ms: Option<i64>,
    /// The org's agent container image (policy-pack bound, ADR 008 §3).
    pub image: String,
    pub resource_limits: ResourceLimits,
}

/// Compute/time envelope for the agent Job.
#[derive(Clone, Copy, Debug)]
pub struct ResourceLimits {
    /// e.g. 2000 = 2 cores.
    pub cpu_millicores: u32,
    /// e.g. 4096.
    pub memory_mib: u32,
    /// Hard kill after this (`active_deadline_seconds` on the Job).
    pub timeout_secs: u64,
}

/// Control-plane orchestrator for delegated agent tasks. Generic over the
/// lease authority so unit tests (and the endpoint, if ever needed) can
/// substitute a stub; production wires [`fabric_control::ValkeyLeaseAuthority`].
#[derive(Clone)]
pub struct AgentTaskOrchestrator<L> {
    kube: kube::Client,
    leases: L,
    pool: PgPool,
    spec: JobSpecConfig,
    poll_interval: Duration,
}

impl<L: LeaseAuthority + Clone + Send + Sync + 'static> AgentTaskOrchestrator<L> {
    pub fn new(kube: kube::Client, leases: L, pool: PgPool, spec: JobSpecConfig) -> Self {
        Self {
            kube,
            leases,
            pool,
            spec,
            poll_interval: Duration::from_secs(5),
        }
    }

    /// Override the monitor poll interval (tests).
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    fn jobs(&self) -> Api<Job> {
        Api::namespaced(self.kube.clone(), &self.spec.namespace)
    }

    fn secrets(&self) -> Api<Secret> {
        Api::namespaced(self.kube.clone(), &self.spec.namespace)
    }

    /// Ensure the credentials Secret exists before creating any Job: the
    /// Job's DSN env vars are `secretKeyRef`s into it, so a missing Secret
    /// would leave every agent pod stuck in CreateContainerConfigError.
    /// Idempotent: create-or-patch (409 on the create is folded into a
    /// full replace), so rotated DSNs propagate too.
    async fn ensure_creds_secret(&self) -> Result<(), AgentTaskError> {
        let secret = job_spec::build_creds_secret(
            &self.spec.creds_secret_name,
            &self.spec.namespace,
            &self.spec.pg_url,
            &self.spec.kv_url,
        );
        let api = self.secrets();
        match api.create(&PostParams::default(), &secret).await {
            Ok(_) => Ok(()),
            Err(kube::Error::Api(resp)) if resp.code == 409 => {
                api.replace(
                    &self.spec.creds_secret_name,
                    &PostParams::default(),
                    &secret,
                )
                .await?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Full lifecycle: acquire lease → create Job → monitor → release lease.
    /// Returns once the Job is created; the monitor task finishes the
    /// lifecycle in the background.
    #[instrument(skip(self, req), fields(session = %req.session_id))]
    pub async fn delegate(&self, req: AgentTaskRequest) -> Result<TaskRecord, AgentTaskError> {
        Self::validate(&req)?;

        let task_uuid = Uuid::now_v7();
        let task_id = format!("task-{task_uuid}");
        let job_name = job_spec::job_name(&req.session_id, &task_uuid);
        let ttl_ms = req
            .ttl_ms
            .unwrap_or(DEFAULT_LEASE_TTL_MS)
            .clamp(1, MAX_LEASE_TTL_MS);

        let mut record = TaskRecord {
            task_id: task_id.clone(),
            session_id: req.session_id.clone(),
            soul_id: req.soul_id.clone(),
            state: TaskState::Pending,
            job_name: job_name.clone(),
            lease_id: String::new(),
            created_at: Utc::now(),
            finished_at: None,
        };

        // 1. Lease first: if the session already has a writer this fails
        //    before any record or Job exists.
        let lease = self
            .leases
            .acquire_lease(&req.session_id, &record.holder_id(), Locus::Server, ttl_ms)
            .await?;
        record.lease_id = lease.lease_id.clone();

        // From here on, any error must release the lease we just took.
        if let Err(e) = self.insert_record(&record).await {
            self.release_lease_best_effort(&record).await;
            return Err(e);
        }

        let job = job_spec::build_job(&req, &task_id, &job_name, &lease.lease_id, &self.spec);
        if let Err(e) = self.ensure_creds_secret().await {
            self.set_terminal_quiet(&record.task_id, TaskState::Failed)
                .await;
            self.release_lease_best_effort(&record).await;
            return Err(e);
        }
        if let Err(e) = self.jobs().create(&PostParams::default(), &job).await {
            self.set_terminal_quiet(&record.task_id, TaskState::Failed)
                .await;
            self.release_lease_best_effort(&record).await;
            return Err(e.into());
        }

        // Pending -> Running is always legal.
        record.transition(TaskState::Running)?;
        self.update_record(&record).await?;

        info!(
            task = %record.task_id,
            job = %record.job_name,
            lease = %record.lease_id,
            "agent task delegated"
        );
        self.spawn_monitor(
            record.clone(),
            Duration::from_secs(req.resource_limits.timeout_secs),
        );
        Ok(record)
    }

    /// Cancel a running task (lease preemption, user reclaim, admin kill).
    /// Idempotent: cancelling a terminal task is a no-op.
    #[instrument(skip(self))]
    pub async fn cancel(&self, task_id: &str) -> Result<(), AgentTaskError> {
        let mut record = self.load_record(task_id).await?;
        if record.state.is_terminal() {
            return Ok(());
        }
        self.delete_job_best_effort(&record.job_name).await;
        record.transition(TaskState::Cancelled)?;
        self.update_record(&record).await?;
        self.release_lease_best_effort(&record).await;
        info!(task = %record.task_id, "agent task cancelled");
        Ok(())
    }

    /// Check task status, polling the K8s Job for non-terminal tasks and
    /// folding a terminal Job state back into the record.
    #[instrument(skip(self))]
    pub async fn status(&self, task_id: &str) -> Result<TaskState, AgentTaskError> {
        let mut record = self.load_record(task_id).await?;
        if record.state.is_terminal() {
            return Ok(record.state);
        }
        match self.jobs().get(&record.job_name).await {
            Ok(job) => {
                if let Some(state) = job_terminal_state(&job) {
                    record.transition(state)?;
                    self.update_record(&record).await?;
                    self.release_lease_best_effort(&record).await;
                }
            }
            // The TTL reaper may take the Job before we observe completion;
            // that is not an orchestrator error.
            Err(kube::Error::Api(resp)) if resp.code == 404 => {}
            Err(e) => return Err(e.into()),
        }
        Ok(record.state)
    }

    fn validate(req: &AgentTaskRequest) -> Result<(), AgentTaskError> {
        let bad = |msg: &str| AgentTaskError::InvalidRequest(msg.to_string());
        if req.session_id.is_empty() {
            return Err(bad("session_id must not be empty"));
        }
        if req.prompt.is_empty() {
            return Err(bad("prompt must not be empty"));
        }
        if req.image.is_empty() {
            return Err(bad("image must not be empty"));
        }
        if !matches!(req.locus, Locus::Server | Locus::Unspecified) {
            return Err(bad("only LOCUS_SERVER is supported for delegated tasks"));
        }
        let limits = &req.resource_limits;
        if limits.cpu_millicores == 0 {
            return Err(bad("cpu_millicores must be > 0"));
        }
        if limits.memory_mib == 0 {
            return Err(bad("memory_mib must be > 0"));
        }
        if limits.timeout_secs == 0 {
            return Err(bad("timeout_secs must be > 0"));
        }
        Ok(())
    }

    /// Poll the Job, the lease, and the deadline until the task goes
    /// terminal, then release the lease. Best-effort throughout: the
    /// monitor never panics and never leaves the record non-terminal
    /// without a reason logged.
    fn spawn_monitor(&self, record: TaskRecord, timeout: Duration) {
        let this = self.clone();
        let holder_id = record.holder_id();
        tokio::spawn(async move {
            let deadline = Instant::now() + timeout + MONITOR_GRACE;
            loop {
                tokio::time::sleep(this.poll_interval).await;

                // Preemption / expiry: the lease is gone or moved to
                // another holder. The Job's writer can no longer append,
                // so the task is dead even if the pod is still running.
                match this.leases.active_lease(&record.session_id).await {
                    Ok(Some(lease)) if lease.holder_id == holder_id => {}
                    Ok(Some(_)) | Ok(None) => {
                        warn!(task = %record.task_id, "lease lost; cancelling agent task");
                        this.delete_job_best_effort(&record.job_name).await;
                        this.set_terminal_quiet(&record.task_id, TaskState::Cancelled)
                            .await;
                        return;
                    }
                    Err(e) => {
                        warn!(task = %record.task_id, error = %e, "lease check failed; retrying")
                    }
                }

                match this.jobs().get(&record.job_name).await {
                    Ok(job) => {
                        if let Some(state) = job_terminal_state(&job) {
                            this.set_terminal_quiet(&record.task_id, state).await;
                            this.release_lease_best_effort(&record).await;
                            info!(task = %record.task_id, %state, "agent task finished");
                            return;
                        }
                    }
                    Err(kube::Error::Api(resp)) if resp.code == 404 => {
                        warn!(task = %record.task_id, "job vanished before completion was observed");
                        this.set_terminal_quiet(&record.task_id, TaskState::Failed)
                            .await;
                        this.release_lease_best_effort(&record).await;
                        return;
                    }
                    Err(e) => {
                        warn!(task = %record.task_id, error = %e, "job poll failed; retrying")
                    }
                }

                if Instant::now() >= deadline {
                    warn!(task = %record.task_id, "agent task exceeded deadline");
                    this.delete_job_best_effort(&record.job_name).await;
                    this.set_terminal_quiet(&record.task_id, TaskState::Failed)
                        .await;
                    this.release_lease_best_effort(&record).await;
                    return;
                }
            }
        });
    }

    async fn delete_job_best_effort(&self, job_name: &str) {
        // Foreground propagation so the pod dies with the Job.
        match self
            .jobs()
            .delete(job_name, &DeleteParams::foreground())
            .await
        {
            Ok(_) => {}
            Err(kube::Error::Api(resp)) if resp.code == 404 => {}
            Err(e) => warn!(job = %job_name, error = %e, "failed to delete agent job"),
        }
    }

    /// Lease release at end-of-task. Losing the release race (preempted or
    /// expired lease) is expected and not an error.
    async fn release_lease_best_effort(&self, record: &TaskRecord) {
        match self
            .leases
            .release_lease(&record.session_id, &record.holder_id())
            .await
        {
            Ok(()) | Err(StoreError::NotLeaseHolder { .. }) | Err(StoreError::NoActiveLease(_)) => {
            }
            Err(e) => warn!(task = %record.task_id, error = %e, "failed to release lease"),
        }
    }

    /// Move a task to a terminal state, ignoring races with `cancel` /
    /// `status` (whoever gets there first wins).
    async fn set_terminal_quiet(&self, task_id: &str, state: TaskState) {
        match self.load_record(task_id).await {
            Ok(mut record) if !record.state.is_terminal() => {
                if record.transition(state).is_ok() {
                    if let Err(e) = self.update_record(&record).await {
                        warn!(task = %task_id, error = %e, "failed to persist terminal state");
                    }
                }
            }
            Ok(_) => {}
            Err(e) => warn!(task = %task_id, error = %e, "failed to load task record"),
        }
    }

    // ---- task record persistence (agent_tasks table) ----

    async fn insert_record(&self, record: &TaskRecord) -> Result<(), AgentTaskError> {
        sqlx::query(
            "INSERT INTO agent_tasks (task_id, session_id, soul_id, state, job_name, lease_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&record.task_id)
        .bind(&record.session_id)
        .bind(&record.soul_id)
        .bind(record.state.as_str())
        .bind(&record.job_name)
        .bind(&record.lease_id)
        .bind(record.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_record(&self, record: &TaskRecord) -> Result<(), AgentTaskError> {
        sqlx::query("UPDATE agent_tasks SET state = $2, finished_at = $3 WHERE task_id = $1")
            .bind(&record.task_id)
            .bind(record.state.as_str())
            .bind(record.finished_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn load_record(&self, task_id: &str) -> Result<TaskRecord, AgentTaskError> {
        let row = sqlx::query(
            "SELECT task_id, session_id, soul_id, state, job_name, lease_id, created_at, finished_at \
             FROM agent_tasks WHERE task_id = $1",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AgentTaskError::TaskNotFound(task_id.to_string()))?;
        Ok(TaskRecord {
            task_id: row.try_get("task_id")?,
            session_id: row.try_get("session_id")?,
            soul_id: row.try_get("soul_id")?,
            state: TaskState::parse(row.try_get::<&str, _>("state")?)?,
            job_name: row.try_get("job_name")?,
            lease_id: row.try_get("lease_id")?,
            created_at: row.try_get("created_at")?,
            finished_at: row.try_get("finished_at")?,
        })
    }
}

/// Map a Job status onto a terminal task state. With `backoffLimit: 0` the
/// first pod failure is terminal; `succeeded` means the agent exited 0.
fn job_terminal_state(job: &Job) -> Option<TaskState> {
    let status = job.status.as_ref()?;
    if status.succeeded.unwrap_or(0) > 0 {
        Some(TaskState::Completed)
    } else if status.failed.unwrap_or(0) > 0 {
        Some(TaskState::Failed)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use k8s_openapi::api::batch::v1::JobStatus;

    use super::*;

    fn job_with(succeeded: Option<i32>, failed: Option<i32>, active: Option<i32>) -> Job {
        Job {
            status: Some(JobStatus {
                succeeded,
                failed,
                active,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn job_status_mapping() {
        assert_eq!(
            job_terminal_state(&job_with(Some(1), None, None)),
            Some(TaskState::Completed)
        );
        assert_eq!(
            job_terminal_state(&job_with(None, Some(1), None)),
            Some(TaskState::Failed)
        );
        assert_eq!(job_terminal_state(&job_with(None, None, Some(1))), None);
        assert_eq!(job_terminal_state(&Job::default()), None);
        // Success wins if both are somehow set.
        assert_eq!(
            job_terminal_state(&job_with(Some(1), Some(1), None)),
            Some(TaskState::Completed)
        );
    }

    #[test]
    fn request_validation() {
        let good = AgentTaskRequest {
            session_id: "sess-1".into(),
            soul_id: "soul-1".into(),
            org_id: "org-1".into(),
            prompt: "do the thing".into(),
            locus: Locus::Server,
            ttl_ms: None,
            image: "agent:1".into(),
            resource_limits: ResourceLimits {
                cpu_millicores: 500,
                memory_mib: 512,
                timeout_secs: 60,
            },
        };
        assert!(AgentTaskOrchestrator::<StubLease>::validate(&good).is_ok());

        let cases: Vec<(AgentTaskRequest, &str)> = vec![
            (
                AgentTaskRequest {
                    session_id: String::new(),
                    ..good.clone()
                },
                "session_id",
            ),
            (
                AgentTaskRequest {
                    prompt: String::new(),
                    ..good.clone()
                },
                "prompt",
            ),
            (
                AgentTaskRequest {
                    image: String::new(),
                    ..good.clone()
                },
                "image",
            ),
            (
                AgentTaskRequest {
                    locus: Locus::Endpoint,
                    ..good.clone()
                },
                "LOCUS_SERVER",
            ),
            (
                AgentTaskRequest {
                    resource_limits: ResourceLimits {
                        cpu_millicores: 0,
                        ..good.resource_limits
                    },
                    ..good.clone()
                },
                "cpu_millicores",
            ),
            (
                AgentTaskRequest {
                    resource_limits: ResourceLimits {
                        memory_mib: 0,
                        ..good.resource_limits
                    },
                    ..good.clone()
                },
                "memory_mib",
            ),
            (
                AgentTaskRequest {
                    resource_limits: ResourceLimits {
                        timeout_secs: 0,
                        ..good.resource_limits
                    },
                    ..good.clone()
                },
                "timeout_secs",
            ),
        ];
        for (req, msg) in cases {
            match AgentTaskOrchestrator::<StubLease>::validate(&req) {
                Err(AgentTaskError::InvalidRequest(m)) => assert!(m.contains(msg), "{m} !~ {msg}"),
                other => panic!("expected InvalidRequest({msg}), got {other:?}"),
            }
        }
    }

    /// Minimal LeaseAuthority for generic-parameter checks in unit tests.
    #[derive(Clone)]
    struct StubLease;

    #[async_trait::async_trait]
    impl LeaseAuthority for StubLease {
        async fn acquire_lease(
            &self,
            _s: &str,
            _h: &str,
            _l: Locus,
            _t: i64,
        ) -> fabric_context::db::Result<fabric_types::lease::Lease> {
            unimplemented!()
        }
        async fn release_lease(&self, _s: &str, _h: &str) -> fabric_context::db::Result<()> {
            unimplemented!()
        }
        async fn lease(&self, _id: &str) -> fabric_context::db::Result<fabric_types::lease::Lease> {
            unimplemented!()
        }
        async fn active_lease(
            &self,
            _s: &str,
        ) -> fabric_context::db::Result<Option<fabric_types::lease::Lease>> {
            unimplemented!()
        }
        async fn verify_writer(
            &self,
            _s: &str,
            _w: &str,
        ) -> fabric_context::db::Result<fabric_types::lease::Lease> {
            unimplemented!()
        }
        async fn set_granted_seq(&self, _id: &str, _seq: u64) -> fabric_context::db::Result<()> {
            unimplemented!()
        }
    }
}
