use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::container::{ContainerError, ContainerId, ContainerRuntime, ContainerSpec};

/// Number of consecutive teardown failures after which the reaper gives up
/// and drops the entry from tracking.
const MAX_TEARDOWN_FAILURES: u32 = 3;

pub struct ContainerLease {
    pub id: ContainerId,
    pub session_id: String,
    pub created_at: Instant,
    pub ttl: Duration,
    /// Consecutive teardown failures observed by the reaper.
    pub teardown_failures: u32,
}

impl ContainerLease {
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.ttl
    }
}

/// Manages container lifecycle: TTL enforcement, resource tracking, cleanup.
pub struct ContainerLifecycle {
    runtime: Arc<dyn ContainerRuntime>,
    active: Arc<Mutex<HashMap<ContainerId, ContainerLease>>>,
}

impl ContainerLifecycle {
    pub fn new(runtime: Arc<dyn ContainerRuntime>) -> Self {
        Self {
            runtime,
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Acquire a container for a session. If an existing container for the
    /// same session is still alive, returns its ID. Otherwise creates a new
    /// container.
    pub async fn acquire(
        &self,
        session_id: &str,
        spec: &ContainerSpec,
        ttl: Duration,
    ) -> Result<ContainerId, ContainerError> {
        // Check for a reusable container for this session (lock released
        // before the create call so we never hold it across an await).
        {
            let active = self.active.lock().await;
            for lease in active.values() {
                if lease.session_id == session_id && !lease.is_expired() {
                    debug!("reusing container {} for session {}", lease.id, session_id);
                    return Ok(lease.id.clone());
                }
            }
        }

        // Create a new container outside the lock.
        let id = self.runtime.create(spec).await?;

        // Re-lock and re-check: another task may have created a container
        // for this session while we were creating ours.
        let mut active = self.active.lock().await;
        for lease in active.values() {
            if lease.session_id == session_id && !lease.is_expired() {
                let existing = lease.id.clone();
                drop(active);
                warn!(
                    "container {} created concurrently for session {}; tearing down duplicate {}",
                    existing, session_id, id
                );
                if let Err(e) = self.runtime.teardown(&id).await {
                    warn!("failed to tear down duplicate container {id}: {e}");
                }
                return Ok(existing);
            }
        }

        let lease = ContainerLease {
            id: id.clone(),
            session_id: session_id.to_string(),
            created_at: Instant::now(),
            ttl,
            teardown_failures: 0,
        };

        debug!("created container {} for session {}", id, session_id);
        active.insert(id.clone(), lease);
        Ok(id)
    }

    /// Register an externally created container for TTL-based reaping.
    /// Used as a safety net so leaked containers are eventually cleaned up.
    pub async fn register(&self, id: ContainerId, session_id: &str, ttl: Duration) {
        let mut active = self.active.lock().await;
        debug!("registered container {} for session {}", id, session_id);
        active.insert(
            id.clone(),
            ContainerLease {
                id,
                session_id: session_id.to_string(),
                created_at: Instant::now(),
                ttl,
                teardown_failures: 0,
            },
        );
    }

    /// Release a container back (marks for teardown after TTL).
    /// If no TTL is set (duration is zero), tears down immediately.
    pub async fn release(&self, id: &ContainerId) -> Result<(), ContainerError> {
        let active = self.active.lock().await;
        if let Some(lease) = active.get(id) {
            if lease.ttl.is_zero() {
                drop(active);
                return self.teardown_internal(id).await;
            }
            // Otherwise the background reaper will clean it up when TTL expires
            debug!(
                "released container {} (TTL remaining: {:?})",
                id,
                lease.ttl.saturating_sub(lease.created_at.elapsed())
            );
            Ok(())
        } else {
            Err(ContainerError::NotFound(id.to_string()))
        }
    }

    /// Immediately tear down a container and remove it from tracking.
    pub async fn teardown(&self, id: &ContainerId) -> Result<(), ContainerError> {
        let mut active = self.active.lock().await;
        active.remove(id);
        drop(active);
        self.teardown_internal(id).await
    }

    async fn teardown_internal(&self, id: &ContainerId) -> Result<(), ContainerError> {
        self.runtime.teardown(id).await?;
        let mut active = self.active.lock().await;
        active.remove(id);
        info!("tore down container {}", id);
        Ok(())
    }

    /// Reap expired containers (called periodically).
    pub async fn reap_expired(&self) -> Result<usize, ContainerError> {
        let expired_ids: Vec<ContainerId> = {
            let active = self.active.lock().await;
            active
                .values()
                .filter(|l| l.is_expired())
                .map(|l| l.id.clone())
                .collect()
        };

        let count = expired_ids.len();
        for id in &expired_ids {
            warn!("reaping expired container {}", id);
            match self.runtime.teardown(id).await {
                Ok(()) => {
                    let mut active = self.active.lock().await;
                    active.remove(id);
                }
                Err(e) => {
                    let mut active = self.active.lock().await;
                    if let Some(lease) = active.get_mut(id) {
                        lease.teardown_failures += 1;
                        if lease.teardown_failures >= MAX_TEARDOWN_FAILURES {
                            error!(
                                "teardown of container {id} failed {} times; giving up: {e}",
                                lease.teardown_failures
                            );
                            active.remove(id);
                        } else {
                            warn!(
                                "failed to tear down expired container {id} (attempt {}): {e}",
                                lease.teardown_failures
                            );
                        }
                    }
                }
            }
        }

        Ok(count)
    }

    /// Number of active containers.
    pub async fn active_count(&self) -> usize {
        let active = self.active.lock().await;
        active.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{
        ContainerInfo, ContainerRuntime, ExecOutput, ExecSpec, ImageRef, ListFilter, NetworkPolicy,
        RegistryAuth,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct MockRuntime {
        create_count: AtomicUsize,
        teardown_count: AtomicUsize,
    }

    impl MockRuntime {
        fn new() -> Self {
            Self {
                create_count: AtomicUsize::new(0),
                teardown_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ContainerRuntime for MockRuntime {
        async fn pull(
            &self,
            _image: &str,
            _auth: Option<&RegistryAuth>,
        ) -> Result<ImageRef, ContainerError> {
            Ok("test-image".into())
        }

        async fn create(&self, spec: &ContainerSpec) -> Result<ContainerId, ContainerError> {
            self.create_count.fetch_add(1, Ordering::SeqCst);
            Ok(ContainerId(spec.namespace.clone(), spec.name.clone()))
        }

        async fn exec(
            &self,
            _id: &ContainerId,
            _cmd: &ExecSpec,
        ) -> Result<ExecOutput, ContainerError> {
            Ok(ExecOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            })
        }

        async fn teardown(&self, _id: &ContainerId) -> Result<(), ContainerError> {
            self.teardown_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn list(&self, _filter: &ListFilter) -> Result<Vec<ContainerInfo>, ContainerError> {
            Ok(Vec::new())
        }
    }

    fn test_spec(name: &str) -> ContainerSpec {
        ContainerSpec {
            image: "ubuntu:22.04".into(),
            namespace: "default".into(),
            name: name.into(),
            cpu_limit: "1".into(),
            memory_limit: "1Gi".into(),
            network_policy: NetworkPolicy::None,
            timeout_s: 30,
            env: HashMap::new(),
            allowed_registries: vec![],
        }
    }

    #[tokio::test]
    async fn acquire_creates_new_container() {
        let runtime = Arc::new(MockRuntime::new());
        let lifecycle = ContainerLifecycle::new(runtime.clone());

        let id = lifecycle
            .acquire("session-1", &test_spec("test-1"), Duration::from_secs(60))
            .await
            .unwrap();

        assert_eq!(id.1, "test-1");
        assert_eq!(runtime.create_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn acquire_reuses_existing_container() {
        let runtime = Arc::new(MockRuntime::new());
        let lifecycle = ContainerLifecycle::new(runtime.clone());

        let id1 = lifecycle
            .acquire("session-1", &test_spec("test-1"), Duration::from_secs(60))
            .await
            .unwrap();

        let id2 = lifecycle
            .acquire("session-1", &test_spec("test-1"), Duration::from_secs(60))
            .await
            .unwrap();

        assert_eq!(id1, id2);
        assert_eq!(runtime.create_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn acquire_different_sessions_create_separate_containers() {
        let runtime = Arc::new(MockRuntime::new());
        let lifecycle = ContainerLifecycle::new(runtime.clone());

        let id1 = lifecycle
            .acquire("session-1", &test_spec("test-1"), Duration::from_secs(60))
            .await
            .unwrap();
        let id2 = lifecycle
            .acquire("session-2", &test_spec("test-2"), Duration::from_secs(60))
            .await
            .unwrap();

        assert_ne!(id1, id2);
        assert_eq!(runtime.create_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn release_immediate_teardown_when_ttl_zero() {
        let runtime = Arc::new(MockRuntime::new());
        let lifecycle = ContainerLifecycle::new(runtime.clone());

        let id = lifecycle
            .acquire("session-1", &test_spec("test-1"), Duration::from_secs(0))
            .await
            .unwrap();

        lifecycle.release(&id).await.unwrap();
        // TTL=0 means immediate teardown on release
        assert_eq!(runtime.teardown_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn teardown_removes_and_cleans_up() {
        let runtime = Arc::new(MockRuntime::new());
        let lifecycle = ContainerLifecycle::new(runtime.clone());

        let id = lifecycle
            .acquire("session-1", &test_spec("test-1"), Duration::from_secs(60))
            .await
            .unwrap();

        lifecycle.teardown(&id).await.unwrap();
        assert_eq!(runtime.teardown_count.load(Ordering::SeqCst), 1);
        assert_eq!(lifecycle.active_count().await, 0);
    }

    #[tokio::test]
    async fn reap_expired_removes_stale_containers() {
        let runtime = Arc::new(MockRuntime::new());
        let lifecycle = ContainerLifecycle::new(runtime.clone());

        let spec = test_spec("test-1");
        lifecycle
            .acquire("session-1", &spec, Duration::from_millis(1))
            .await
            .unwrap();

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(10)).await;

        let reaped = lifecycle.reap_expired().await.unwrap();
        assert_eq!(reaped, 1);
        assert_eq!(runtime.teardown_count.load(Ordering::SeqCst), 1);
        assert_eq!(lifecycle.active_count().await, 0);
    }

    struct FailingTeardownRuntime {
        teardown_count: AtomicUsize,
    }

    #[async_trait]
    impl ContainerRuntime for FailingTeardownRuntime {
        async fn pull(
            &self,
            _image: &str,
            _auth: Option<&RegistryAuth>,
        ) -> Result<ImageRef, ContainerError> {
            Ok("test-image".into())
        }

        async fn create(&self, spec: &ContainerSpec) -> Result<ContainerId, ContainerError> {
            Ok(ContainerId(spec.namespace.clone(), spec.name.clone()))
        }

        async fn exec(
            &self,
            _id: &ContainerId,
            _cmd: &ExecSpec,
        ) -> Result<ExecOutput, ContainerError> {
            Ok(ExecOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            })
        }

        async fn teardown(&self, _id: &ContainerId) -> Result<(), ContainerError> {
            self.teardown_count.fetch_add(1, Ordering::SeqCst);
            Err(ContainerError::TeardownFailed("simulated failure".into()))
        }

        async fn list(&self, _filter: &ListFilter) -> Result<Vec<ContainerInfo>, ContainerError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn reaper_retries_failed_teardowns_then_gives_up() {
        let runtime = Arc::new(FailingTeardownRuntime {
            teardown_count: AtomicUsize::new(0),
        });
        let lifecycle = ContainerLifecycle::new(runtime.clone());

        lifecycle
            .acquire("session-1", &test_spec("test-1"), Duration::from_millis(1))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        // First two reaps: teardown fails, entry is retained.
        lifecycle.reap_expired().await.unwrap();
        assert_eq!(lifecycle.active_count().await, 1);
        lifecycle.reap_expired().await.unwrap();
        assert_eq!(lifecycle.active_count().await, 1);

        // Third failure: entry is dropped from tracking.
        lifecycle.reap_expired().await.unwrap();
        assert_eq!(lifecycle.active_count().await, 0);
        assert_eq!(runtime.teardown_count.load(Ordering::SeqCst), 3);
    }
}
