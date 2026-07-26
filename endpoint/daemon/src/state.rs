//! Shared daemon state, handed to every HTTP handler.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use fabric_context::SqliteContextStore;
use fabric_policy::PolicyStore;

use crate::config::DaemonConfig;
use crate::lease::{CachedLease, LeaseClient};

/// State shared between the main loop and the localhost HTTP server. The
/// context store and policy store sit behind locks; critical sections are
/// short (local SQLite reads) and never held across an await point.
pub struct DaemonState {
    pub cfg: DaemonConfig,
    pub started: Instant,
    pub store: Mutex<SqliteContextStore>,
    pub policy: RwLock<PolicyStore>,
    /// Client of the server-side lease authority. `None` when no server URL
    /// is configured (offline-only). Local op-log work never depends on it.
    pub lease_client: Option<LeaseClient>,
    /// Server-granted lease cache, keyed by session id. Advisory only: the
    /// local op-log commits real turns with or without a server lease.
    pub leases: Mutex<HashMap<String, CachedLease>>,
}

impl DaemonState {
    pub fn new(cfg: DaemonConfig, store: SqliteContextStore) -> Arc<Self> {
        let lease_client = if cfg.server_url.is_empty() {
            None
        } else {
            Some(LeaseClient::new(&cfg.server_url, &cfg.device_id))
        };
        Arc::new(Self {
            cfg,
            started: Instant::now(),
            store: Mutex::new(store),
            policy: RwLock::new(PolicyStore::new()),
            lease_client,
            leases: Mutex::new(HashMap::new()),
        })
    }

    /// True when the context store answers a trivial query.
    pub fn store_open(&self) -> bool {
        self.store
            .lock()
            .map(|store| store.ping().is_ok())
            .unwrap_or(false)
    }

    /// True when at least one policy (endpoint or server) is loaded. Until
    /// then the daemon is not ready: the fail-closed gate denies everything.
    pub fn policy_loaded(&self) -> bool {
        self.policy
            .read()
            .map(|p| p.endpoint_version().is_some() || p.server_version().is_some())
            .unwrap_or(false)
    }
}
