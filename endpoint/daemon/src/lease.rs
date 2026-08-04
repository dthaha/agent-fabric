//! Endpoint-side client of the server lease authority. The server grants,
//! renews, and preempts leases, stamping every timestamp with its own clock;
//! this module requests those leases, caches them, renews them before
//! expiry, and re-requests them on reconnect.
//!
//! OFFLINE-FIRST INVARIANT: nothing here ever gates local work. When the
//! server is unreachable the daemon keeps committing real turns to its local
//! op-log (the local store is the writer's own authority offline). A failed
//! acquisition only marks the lease `wanted` in the cache; the maintenance
//! task retries on the next tick and replays the local op-log once the
//! server answers again.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use fabric_types::context::{ContextEntry, Locus};
use fabric_types::lease::{
    AcquireLeaseRequest, Lease, ReleaseLeaseRequest, RenewLeaseRequest, ReplayRequest,
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::state::DaemonState;

/// TTL requested from the server. Matches the core store's turn-scoped
/// safety-net posture. Public so integration tests can reason about
/// renewal margins.
pub const DEFAULT_TTL_MS: i64 = 3_600_000;
/// Renew when the cached lease has less than this much of its TTL left.
const RENEW_MARGIN: Duration = Duration::from_secs(60);
/// How often the maintenance task scans the cache.
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);

/// HTTP client of the control-plane lease API. Cheap to clone. Every
/// request carries the `x-fabric-*` identity headers: the control plane
/// resolves the caller's identity (and thus the lease holder) from them and
/// rejects requests without them.
#[derive(Clone)]
pub struct LeaseClient {
    http: reqwest::Client,
    base_url: String,
    holder_id: String,
    user_id: String,
    org_id: String,
}

impl LeaseClient {
    pub fn new(base_url: &str, holder_id: &str, user_id: &str, org_id: &str) -> Self {
        if base_url.starts_with("http://") {
            // Dev mode needs plaintext http (localhost control planes); in
            // production the lease API carries identity headers and must be
            // TLS-terminated.
            warn!(
                server_url = base_url,
                "lease traffic is plaintext HTTP — set FABRIC_SERVER_URL to https:// for production"
            );
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            holder_id: holder_id.to_string(),
            user_id: user_id.to_string(),
            org_id: org_id.to_string(),
        }
    }

    /// Attach the identity headers the control plane requires (ADR 007).
    fn with_identity(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let req = req
            .header("x-fabric-user-sub", &self.user_id)
            .header("x-fabric-device-sub", &self.holder_id);
        if self.org_id.is_empty() {
            req
        } else {
            req.header("x-fabric-org-id", &self.org_id)
        }
    }

    /// Request a fresh lease from the server. The returned lease is stamped
    /// with the SERVER clock and carries the server's identity in
    /// `granted_by`. The holder is derived server-side from the identity
    /// headers, not from the body.
    pub async fn acquire(&self, session_id: &str, ttl_ms: i64) -> Result<Lease> {
        let lease = self
            .with_identity(self.http.post(format!("{}/lease/acquire", self.base_url)))
            .json(&AcquireLeaseRequest {
                session_id: session_id.to_string(),
                holder_id: String::new(),
                locus: Some(Locus::Endpoint as i32),
                ttl_ms: Some(ttl_ms),
            })
            .send()
            .await
            .context("lease acquire request failed")?
            .error_for_status()
            .context("lease acquire rejected")?
            .json()
            .await
            .context("decoding granted lease")?;
        Ok(lease)
    }

    /// Extend a held lease. The server re-stamps `expires_at` with its own
    /// clock; the holder is matched against the identity headers.
    pub async fn renew(&self, lease_id: &str, ttl_ms: i64) -> Result<Lease> {
        let lease = self
            .with_identity(self.http.post(format!("{}/lease/renew", self.base_url)))
            .json(&RenewLeaseRequest {
                lease_id: lease_id.to_string(),
                holder_id: String::new(),
                ttl_ms: Some(ttl_ms),
            })
            .send()
            .await
            .context("lease renew request failed")?
            .error_for_status()
            .context("lease renew rejected")?
            .json()
            .await
            .context("decoding renewed lease")?;
        Ok(lease)
    }

    /// Release the session lease at the end of a turn. Not yet called from
    /// the daemon loop (the runtime plane wires it to turn completion); kept
    /// as part of the client API.
    #[allow(dead_code)]
    pub async fn release(&self, session_id: &str) -> Result<()> {
        self.with_identity(self.http.delete(format!("{}/lease/release", self.base_url)))
            .json(&ReleaseLeaseRequest {
                session_id: session_id.to_string(),
                holder_id: String::new(),
            })
            .send()
            .await
            .context("lease release request failed")?
            .error_for_status()
            .context("lease release rejected")?;
        Ok(())
    }

    /// Replay local op-log entries to the server after a reconnect. Returns
    /// the reconcile report (applied/duplicates/conflicts).
    pub async fn replay(
        &self,
        session_id: &str,
        entries: &[ContextEntry],
    ) -> Result<fabric_context::ReconcileReport> {
        let report = self
            .with_identity(self.http.post(format!("{}/context/replay", self.base_url)))
            .json(&ReplayRequest {
                session_id: session_id.to_string(),
                entries: entries.to_vec(),
            })
            .send()
            .await
            .context("op-log replay request failed")?
            .error_for_status()
            .context("op-log replay rejected")?
            .json()
            .await
            .context("decoding reconcile report")?;
        Ok(report)
    }
}

/// Per-session lease cache entry. Advisory metadata only — the local op-log
/// never waits on it.
pub struct CachedLease {
    /// The last server-granted lease, if one is held.
    pub lease: Option<Lease>,
    /// When `lease` was fetched, on the local monotonic clock. Expiry math
    /// uses elapsed time against the lease's TTL, never the local wall clock
    /// against server timestamps (device clocks drift).
    pub fetched_at: Instant,
    /// True when the server was unreachable at last attempt: the lease is
    /// wanted and must be re-requested on reconnect.
    pub wanted: bool,
    /// Local op-log seq already replayed to the server.
    pub synced_seq: u64,
}

impl CachedLease {
    fn ttl(&self) -> Duration {
        self.lease
            .as_ref()
            .and_then(|l| {
                let granted = l.granted_at.as_ref()?;
                let expires = l.expires_at.as_ref()?;
                let ms = (expires.seconds - granted.seconds) * 1000
                    + i64::from(expires.nanos - granted.nanos) / 1_000_000;
                u64::try_from(ms).ok()
            })
            .map(Duration::from_millis)
            .unwrap_or(Duration::ZERO)
    }

    /// True when the held lease is still comfortably valid.
    fn fresh(&self) -> bool {
        !self.wanted
            && self.lease.is_some()
            && self.fetched_at.elapsed() + RENEW_MARGIN < self.ttl()
    }

    /// True when a held lease should be renewed (still valid, inside the
    /// renewal margin).
    fn due_for_renewal(&self) -> bool {
        !self.wanted
            && self.lease.is_some()
            && self.fetched_at.elapsed() < self.ttl()
            && !self.fresh()
    }
}

/// Called on user activity / session start. Ensures a server lease is held
/// (or at least requested) for the session. NEVER blocks local work: on any
/// failure the lease is marked wanted and the daemon keeps writing to its
/// local op-log.
pub async fn ensure_lease(state: &Arc<DaemonState>, session_id: &str) {
    let Some(client) = state.lease_client.clone() else {
        return;
    };

    enum Action {
        None,
        Renew(String),
        Acquire { replay_after: bool },
    }
    let action = {
        let cache = state.leases.lock().unwrap_or_else(|e| e.into_inner());
        match cache.get(session_id) {
            Some(c) if c.fresh() => Action::None,
            Some(c) if c.due_for_renewal() => Action::Renew(
                c.lease
                    .as_ref()
                    .expect("fresh implies lease")
                    .lease_id
                    .clone(),
            ),
            Some(c) => Action::Acquire {
                replay_after: c.wanted,
            },
            None => Action::Acquire {
                replay_after: false,
            },
        }
    };

    match action {
        Action::None => {}
        Action::Renew(lease_id) => match client.renew(&lease_id, DEFAULT_TTL_MS).await {
            Ok(lease) => {
                info!(session = session_id, lease = %lease.lease_id, "lease renewed");
                let mut cache = state.leases.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(c) = cache.get_mut(session_id) {
                    c.lease = Some(lease);
                    c.fetched_at = Instant::now();
                }
            }
            Err(e) => {
                warn!(session = session_id, error = %e, "lease renew failed; re-acquiring");
                acquire_and_cache(state, &client, session_id, false).await;
            }
        },
        Action::Acquire { replay_after } => {
            acquire_and_cache(state, &client, session_id, replay_after).await;
        }
    }
}

async fn acquire_and_cache(
    state: &Arc<DaemonState>,
    client: &LeaseClient,
    session_id: &str,
    replay_after: bool,
) {
    match client.acquire(session_id, DEFAULT_TTL_MS).await {
        Ok(lease) => {
            info!(
                session = session_id,
                lease = %lease.lease_id,
                granted_by = %lease.granted_by,
                "server lease acquired"
            );
            {
                let mut cache = state.leases.lock().unwrap_or_else(|e| e.into_inner());
                let synced_seq = cache.get(session_id).map(|c| c.synced_seq).unwrap_or(0);
                cache.insert(
                    session_id.to_string(),
                    CachedLease {
                        lease: Some(lease),
                        fetched_at: Instant::now(),
                        wanted: false,
                        synced_seq,
                    },
                );
            }
            if replay_after {
                replay_local_log(state, client, session_id).await;
            }
        }
        Err(e) => {
            // Offline path: mark the lease wanted and keep going. Local
            // op-log commits are unaffected.
            warn!(session = session_id, error = %e, "server unreachable; lease marked wanted");
            let mut cache = state.leases.lock().unwrap_or_else(|e| e.into_inner());
            cache
                .entry(session_id.to_string())
                .or_insert(CachedLease {
                    lease: None,
                    fetched_at: Instant::now(),
                    wanted: true,
                    synced_seq: 0,
                })
                .wanted = true;
        }
    }
}

/// Reconnect catch-up: replay everything past the last synced seq to the
/// server, then advance the sync marker. Runs the deterministic reconcile
/// merge server-side; conflicts are reported, never fatal here.
async fn replay_local_log(state: &Arc<DaemonState>, client: &LeaseClient, session_id: &str) {
    let (entries, head) = {
        let cache = state.leases.lock().unwrap_or_else(|e| e.into_inner());
        let synced = cache.get(session_id).map(|c| c.synced_seq).unwrap_or(0);
        drop(cache);
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        match store.entries_since(session_id, synced) {
            Ok(entries) => match store.head_seq(session_id) {
                Ok(head) => (entries, head),
                Err(e) => {
                    warn!(session = session_id, error = %e, "reading head seq for replay");
                    return;
                }
            },
            Err(e) => {
                warn!(session = session_id, error = %e, "reading op-log for replay");
                return;
            }
        }
    };
    if entries.is_empty() {
        return;
    }
    match client.replay(session_id, &entries).await {
        Ok(report) => {
            info!(
                session = session_id,
                applied = report.applied,
                duplicates = report.duplicates,
                conflicts = report.conflicts.len(),
                "local op-log replayed to server"
            );
            let mut cache = state.leases.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(c) = cache.get_mut(session_id) {
                c.synced_seq = head;
            }
        }
        Err(e) => {
            warn!(session = session_id, error = %e, "op-log replay failed; will retry");
            let mut cache = state.leases.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(c) = cache.get_mut(session_id) {
                c.wanted = true;
            }
        }
    }
}

/// Background task: renew leases nearing expiry and retry wanted
/// (offline-failed) acquisitions. Runs until `shutdown` is cancelled.
pub async fn lease_maintenance(state: Arc<DaemonState>, shutdown: CancellationToken) {
    if state.lease_client.is_none() {
        return;
    }
    let mut tick = tokio::time::interval(MAINTENANCE_INTERVAL);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = tick.tick() => {}
        }
        let sessions: Vec<String> = {
            let cache = state.leases.lock().unwrap_or_else(|e| e.into_inner());
            cache
                .iter()
                .filter(|(_, c)| c.wanted || c.due_for_renewal())
                .map(|(s, _)| s.clone())
                .collect()
        };
        for session_id in sessions {
            ensure_lease(&state, &session_id).await;
        }
    }
}
