//! Endpoint daemon lease behavior against a real control plane (ADR 004:
//! Postgres op-log + Valkey leases). These tests used to live in
//! `endpoint/daemon/src/lease.rs`, which forced the endpoint crate to
//! dev-depend on `fabric-control` (the server) — a layering inversion.
//! They live here instead, in the workspace's designated integration crate.
//!
//! Run the infra-gated tests against the dev compose stack:
//!   docker compose -f deploy/docker-compose.yaml up -d
//!   FABRIC_PG_URL='postgres://fabric:***@localhost:5432/fabric' \
//!   FABRIC_KV_URL='redis://localhost:6379' \
//!   cargo test -p fabric-tests -- --ignored

use std::sync::Arc;
use std::time::{Duration, Instant};

use fabric_context::{ContextStore, LeaseAuthority, SqliteContextStore};
use fabric_control::soul::SoulRegistry;
use fabric_control::{ControlState, PostgresContextStore, ValkeyLeaseAuthority};
use fabric_endpoint::config::DaemonConfig;
use fabric_endpoint::lease::{ensure_lease, LeaseClient, DEFAULT_TTL_MS};
use fabric_endpoint::state::DaemonState;
use fabric_types::context::{ContextEntry, EntryKind, Locus, SessionMeta, SessionState};

fn test_cfg(server_url: &str) -> DaemonConfig {
    DaemonConfig {
        device_id: "endpoint-test".into(),
        server_url: server_url.into(),
        ..Default::default()
    }
}

fn create_local_session(state: &DaemonState, session_id: &str) {
    let store = state.store.lock().unwrap();
    store
        .create_session(&SessionMeta {
            session_id: session_id.into(),
            soul_id: "soul".into(),
            user_id: "user".into(),
            state: SessionState::Active as i32,
            active_lease: String::new(),
            created_at: Some(pbjson_types::Timestamp {
                seconds: 100,
                nanos: 0,
            }),
            last_activity: None,
            labels: Default::default(),
            org_id: String::new(),
        })
        .unwrap();
}

fn commit_local_turn(state: &DaemonState, session_id: &str, entry_id: &str) {
    let store = state.store.lock().unwrap();
    // Offline, the device is its own writer: it acquires a LOCAL lease
    // from its own store and commits real entries. The server lease is
    // irrelevant to this path.
    store
        .acquire_lease(session_id, "endpoint-test", Locus::Endpoint, 30_000)
        .unwrap();
    let mut entry = ContextEntry {
        entry_id: entry_id.into(),
        session_id: session_id.into(),
        seq: 0,
        kind: EntryKind::UserMessage as i32,
        payload: b"offline work".to_vec(),
        lease_holder: "endpoint-test".into(),
        policy_version: String::new(),
        locus: Locus::Endpoint as i32,
        created_at: None,
        received_at: None,
        disposition: String::new(),
    };
    store.append_entry(&mut entry).unwrap();
    store.release_lease(session_id, "endpoint-test").unwrap();
}

/// Bind a control plane on an ephemeral localhost port, backed by the
/// server's Postgres + Valkey stores (ADR 004 — the server no longer has a
/// SQLite fallback). Requires `FABRIC_PG_URL` + `FABRIC_KV_URL`.
async fn spawn_control() -> (String, Arc<ControlState>) {
    let pg = PostgresContextStore::connect(&std::env::var("FABRIC_PG_URL").expect("FABRIC_PG_URL"))
        .await
        .unwrap();
    let souls = SoulRegistry::new(pg.pool().clone());
    let kv = ValkeyLeaseAuthority::connect(&std::env::var("FABRIC_KV_URL").expect("FABRIC_KV_URL"))
        .await
        .unwrap();
    let control = ControlState::from_env(pg, kv, souls);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fabric_control::router(Arc::clone(&control));
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("control plane server failed");
    });
    (format!("http://{addr}"), control)
}

/// A URL nothing is listening on (bind, grab the port, drop).
async fn dead_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

#[tokio::test]
async fn offline_server_never_blocks_local_work() {
    let state = DaemonState::new(
        test_cfg(&dead_url().await),
        SqliteContextStore::open_in_memory().unwrap(),
    );
    create_local_session(&state, "s1");

    // Lease acquisition fails fast and marks the lease wanted...
    ensure_lease(&state, "s1").await;
    {
        let cache = state.leases.lock().unwrap();
        let entry = cache.get("s1").expect("wanted entry cached");
        assert!(entry.wanted);
        assert!(entry.lease.is_none());
    }

    // ...and local turns still commit to the op-log.
    commit_local_turn(&state, "s1", "e1");
    commit_local_turn(&state, "s1", "e2");
    let store = state.store.lock().unwrap();
    assert_eq!(store.head_seq("s1").unwrap(), 2);
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn acquire_renew_release_against_control_plane() {
    let (url, control) = spawn_control().await;
    let state = DaemonState::new(
        test_cfg(&url),
        SqliteContextStore::open_in_memory().unwrap(),
    );

    ensure_lease(&state, "s1").await;
    let (lease_id, first_expiry) = {
        let cache = state.leases.lock().unwrap();
        let entry = cache.get("s1").expect("lease cached");
        assert!(!entry.wanted);
        let lease = entry.lease.as_ref().unwrap();
        assert_eq!(lease.granted_by, "fabric-server-test");
        assert_eq!(lease.holder_id, "endpoint-test");
        (lease.lease_id.clone(), lease.expires_at.unwrap())
    };

    // Force the cached lease into the renewal margin and re-ensure.
    {
        let mut cache = state.leases.lock().unwrap();
        cache.get_mut("s1").unwrap().fetched_at =
            Instant::now() - Duration::from_millis(DEFAULT_TTL_MS as u64);
    }
    ensure_lease(&state, "s1").await;
    {
        let cache = state.leases.lock().unwrap();
        let lease = cache.get("s1").unwrap().lease.as_ref().unwrap();
        assert_eq!(lease.lease_id, lease_id, "renew keeps the same lease");
        assert!(lease.expires_at.unwrap().seconds >= first_expiry.seconds);
    }

    // The server agrees on the active lease.
    let active = LeaseAuthority::active_lease(&control.kv, "s1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.lease_id, lease_id);

    // Release at the end of the turn.
    let client = state.lease_client.clone().unwrap();
    client.release("s1").await.unwrap();
    assert!(LeaseAuthority::active_lease(&control.kv, "s1")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn reconnect_reacquires_and_replays_local_oplog() {
    // Start against a dead server: lease wanted, real turns committed.
    let dead = dead_url().await;
    let mut state = DaemonState::new(
        test_cfg(&dead),
        SqliteContextStore::open_in_memory().unwrap(),
    );
    create_local_session(&state, "s1");
    ensure_lease(&state, "s1").await;
    commit_local_turn(&state, "s1", "e1");
    commit_local_turn(&state, "s1", "e2");
    assert!(state.leases.lock().unwrap().get("s1").unwrap().wanted);

    // Server comes back: point the client at it and re-ensure.
    let (url, control) = spawn_control().await;
    Arc::get_mut(&mut state).unwrap().lease_client =
        Some(LeaseClient::new(&url, "endpoint-test", "local-user", ""));
    ensure_lease(&state, "s1").await;

    // Lease acquired and no longer wanted.
    {
        let cache = state.leases.lock().unwrap();
        let entry = cache.get("s1").unwrap();
        assert!(!entry.wanted);
        assert_eq!(
            entry.lease.as_ref().unwrap().granted_by,
            "fabric-server-test"
        );
        assert_eq!(entry.synced_seq, 2, "sync marker advanced past replay");
    }

    // The offline turns converged on the server, in order.
    let entries = ContextStore::entries_since(&control.pg, "s1", 0)
        .await
        .unwrap();
    let ids: Vec<&str> = entries.iter().map(|e| e.entry_id.as_str()).collect();
    assert_eq!(ids, ["e1", "e2"]);
}
