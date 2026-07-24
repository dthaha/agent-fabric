//! Shared daemon state, handed to every HTTP handler.

use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use fabric_context::ContextStore;
use fabric_policy::PolicyStore;

use crate::config::DaemonConfig;

/// State shared between the main loop and the localhost HTTP server. The
/// context store and policy store sit behind locks; critical sections are
/// short (local SQLite reads) and never held across an await point.
pub struct DaemonState {
    pub cfg: DaemonConfig,
    pub started: Instant,
    pub store: Mutex<ContextStore>,
    pub policy: RwLock<PolicyStore>,
}

impl DaemonState {
    pub fn new(cfg: DaemonConfig, store: ContextStore) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            started: Instant::now(),
            store: Mutex::new(store),
            policy: RwLock::new(PolicyStore::new()),
        })
    }

    /// True when the context store answers a trivial query.
    pub fn store_open(&self) -> bool {
        self.store
            .lock()
            .map(|store| store.ping().is_ok())
            .unwrap_or(false)
    }

    /// True when at least one policy (endpoint or hosted) is loaded. Until
    /// then the daemon is not ready: the fail-closed gate denies everything.
    pub fn policy_loaded(&self) -> bool {
        self.policy
            .read()
            .map(|p| p.endpoint_version().is_some() || p.hosted_version().is_some())
            .unwrap_or(false)
    }
}
