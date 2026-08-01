//! Integration test for the agent task orchestrator against a real cluster
//! and Postgres. Ignored by default: CI has neither. Run manually with:
//!
//! ```sh
//! FABRIC_TEST_PG_URL=postgres://fabric:fabric@localhost:5432/fabric \
//!   cargo test -p fabric-server --test orchestrator_k8s -- --ignored
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use fabric_context::db::{Result as StoreResult, StoreError};
use fabric_context::LeaseAuthority;
use fabric_server::job_spec::JobSpecConfig;
use fabric_server::{AgentTaskOrchestrator, AgentTaskRequest, ResourceLimits, TaskState};
use fabric_types::context::Locus;
use fabric_types::lease::Lease;

/// In-memory lease authority: enough of `LeaseAuthority` for the
/// orchestrator's acquire / active / release path, no Valkey needed.
#[derive(Clone, Default)]
struct MemLeases {
    inner: std::sync::Arc<Mutex<HashMap<String, Lease>>>,
}

#[async_trait::async_trait]
impl LeaseAuthority for MemLeases {
    async fn acquire_lease(
        &self,
        session_id: &str,
        holder_id: &str,
        locus: Locus,
        _ttl_ms: i64,
    ) -> StoreResult<Lease> {
        let mut guard = self.inner.lock().expect("lease lock poisoned");
        if guard.contains_key(session_id) {
            return Err(StoreError::LeaseConflict(session_id.into()));
        }
        let lease = Lease {
            lease_id: format!("lease-{}", uuid::Uuid::now_v7()),
            session_id: session_id.into(),
            holder_id: holder_id.into(),
            locus: locus as i32,
            ..Default::default()
        };
        guard.insert(session_id.into(), lease.clone());
        Ok(lease)
    }

    async fn release_lease(&self, session_id: &str, holder_id: &str) -> StoreResult<()> {
        let mut guard = self.inner.lock().expect("lease lock poisoned");
        match guard.get(session_id) {
            None => Ok(()),
            Some(l) if l.holder_id == holder_id => {
                guard.remove(session_id);
                Ok(())
            }
            Some(l) => Err(StoreError::NotLeaseHolder {
                writer: holder_id.into(),
                holder: l.holder_id.clone(),
            }),
        }
    }

    async fn lease(&self, lease_id: &str) -> StoreResult<Lease> {
        self.inner
            .lock()
            .expect("lease lock poisoned")
            .values()
            .find(|l| l.lease_id == lease_id)
            .cloned()
            .ok_or_else(|| StoreError::LeaseNotFound(lease_id.into()))
    }

    async fn active_lease(&self, session_id: &str) -> StoreResult<Option<Lease>> {
        Ok(self
            .inner
            .lock()
            .expect("lease lock poisoned")
            .get(session_id)
            .cloned())
    }

    async fn verify_writer(&self, session_id: &str, writer: &str) -> StoreResult<Lease> {
        match self.active_lease(session_id).await? {
            Some(l) if l.holder_id == writer => Ok(l),
            Some(l) => Err(StoreError::NotLeaseHolder {
                writer: writer.into(),
                holder: l.holder_id,
            }),
            None => Err(StoreError::NoActiveLease(session_id.into())),
        }
    }

    async fn set_granted_seq(&self, _lease_id: &str, _granted_seq: u64) -> StoreResult<()> {
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires a Kubernetes cluster and Postgres (FABRIC_TEST_PG_URL)"]
async fn delegate_creates_job_then_cancel_reaps_it() {
    let pg_url =
        std::env::var("FABRIC_TEST_PG_URL").expect("set FABRIC_TEST_PG_URL to run this test");
    let pool = sqlx::PgPool::connect(&pg_url).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let kube = kube::Client::try_default().await.unwrap();

    let orchestrator = AgentTaskOrchestrator::new(
        kube,
        MemLeases::default(),
        pool,
        JobSpecConfig {
            namespace: "default".into(),
            pg_url: "postgres://unused".into(),
            kv_url: "redis://unused".into(),
        },
    );

    let req = AgentTaskRequest {
        session_id: format!("sess-it-{}", uuid::Uuid::now_v7()),
        soul_id: "soul-it".into(),
        org_id: "org-it".into(),
        prompt: "echo hello".into(),
        locus: Locus::Server,
        ttl_ms: None,
        image: "busybox:1.36".into(),
        resource_limits: ResourceLimits {
            cpu_millicores: 100,
            memory_mib: 64,
            timeout_secs: 120,
        },
    };

    let record = orchestrator.delegate(req).await.unwrap();
    assert_eq!(record.state, TaskState::Running);
    assert!(record.job_name.starts_with("fabric-agent-"));
    assert!(!record.lease_id.is_empty());

    // A second delegate on the same session must lose the lease race.
    let conflict = orchestrator
        .delegate(AgentTaskRequest {
            prompt: "echo again".into(),
            ..record_req(&record)
        })
        .await;
    assert!(
        conflict.is_err(),
        "expected lease conflict, got {conflict:?}"
    );

    orchestrator.cancel(&record.task_id).await.unwrap();
    assert_eq!(
        orchestrator.status(&record.task_id).await.unwrap(),
        TaskState::Cancelled
    );
}

fn record_req(record: &fabric_server::TaskRecord) -> AgentTaskRequest {
    AgentTaskRequest {
        session_id: record.session_id.clone(),
        soul_id: record.soul_id.clone(),
        org_id: "org-it".into(),
        prompt: String::new(),
        locus: Locus::Server,
        ttl_ms: None,
        image: "busybox:1.36".into(),
        resource_limits: ResourceLimits {
            cpu_millicores: 100,
            memory_mib: 64,
            timeout_secs: 120,
        },
    }
}
