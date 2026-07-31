//! Integration tests for [`PostgresContextStore`] (ADR 004). All are
//! `#[ignore]`'d — they require a live Postgres reachable via
//! `FABRIC_PG_URL` (run `deploy/docker-compose.yaml` to provision one).
//!
//! Each test uses a unique session id so concurrent runs share the public
//! schema without clashing (the init migration is idempotent).

use fabric_context::store::ContextStore;
use fabric_types::context::{ContextEntry, EntryKind, Locus, SessionMeta, SessionState};

use fabric_control::PostgresContextStore;

fn pg_url() -> String {
    std::env::var("FABRIC_PG_URL").expect("FABRIC_PG_URL must be set for integration tests")
}

async fn fresh_store() -> PostgresContextStore {
    PostgresContextStore::connect(&pg_url())
        .await
        .expect("connect")
}

fn test_session(session_id: &str) -> SessionMeta {
    SessionMeta {
        session_id: session_id.into(),
        soul_id: "soul-1".into(),
        user_id: "user-1".into(),
        org_id: "org-1".into(),
        state: SessionState::Active as i32,
        active_lease: String::new(),
        created_at: None,
        last_activity: None,
        labels: Default::default(),
    }
}

fn test_entry(entry_id: &str, session: &str, holder: &str) -> ContextEntry {
    ContextEntry {
        entry_id: entry_id.into(),
        session_id: session.into(),
        seq: 0,
        kind: EntryKind::UserMessage as i32,
        payload: b"hello".to_vec(),
        lease_holder: holder.into(),
        policy_version: "v1".into(),
        locus: Locus::Endpoint as i32,
        created_at: None,
        received_at: None,
        disposition: String::new(),
    }
}

fn unique_session(tag: &str) -> String {
    format!("{tag}-{}", uuid::Uuid::new_v4().simple())
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn create_session_and_append_assigns_increasing_seq() {
    let pg = fresh_store().await;
    let s = unique_session("append");
    pg.create_session(&test_session(&s)).await.unwrap();
    let meta = ContextStore::session(&pg, &s).await.unwrap();
    assert_eq!(meta.session_id, s);
    assert_eq!(meta.state, SessionState::Active as i32);

    let mut e1 = test_entry(&format!("{s}-e1"), &s, "endpoint-1");
    assert_eq!(pg.append_entry(&mut e1).await.unwrap(), 1);
    let mut e2 = test_entry(&format!("{s}-e2"), &s, "endpoint-1");
    assert_eq!(pg.append_entry(&mut e2).await.unwrap(), 2);
    assert_eq!(ContextStore::head_seq(&pg, &s).await.unwrap(), 2);
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn entries_since_returns_in_order() {
    let pg = fresh_store().await;
    let s = unique_session("since");
    pg.create_session(&test_session(&s)).await.unwrap();
    for suffix in ["a", "b", "c"] {
        let mut e = test_entry(&format!("{s}-{suffix}"), &s, "endpoint-1");
        pg.append_entry(&mut e).await.unwrap();
    }
    let entries = ContextStore::entries_since(&pg, &s, 1).await.unwrap();
    let ids: Vec<_> = entries.iter().map(|e| e.entry_id.as_str()).collect();
    assert_eq!(ids, [format!("{s}-b"), format!("{s}-c")]);
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn concurrent_appends_get_distinct_increasing_seqs() {
    let pg = fresh_store().await;
    let s = unique_session("conc");
    pg.create_session(&test_session(&s)).await.unwrap();
    // Two concurrent appends: the session-row `FOR UPDATE` lock serializes
    // them so each gets a distinct, increasing seq (no collision).
    let pg2 = pg.clone();
    let sid = s.clone();
    let a = tokio::spawn(async move {
        let mut e = test_entry(&format!("{sid}-a"), &sid, "endpoint-1");
        pg2.append_entry(&mut e).await.unwrap()
    });
    let pg2 = pg.clone();
    let sid = s.clone();
    let b = tokio::spawn(async move {
        let mut e = test_entry(&format!("{sid}-b"), &sid, "endpoint-2");
        pg2.append_entry(&mut e).await.unwrap()
    });
    let (sa, sb) = tokio::join!(a, b);
    let (sa, sb) = (sa.unwrap(), sb.unwrap());
    assert_ne!(sa, sb, "concurrent appends must get distinct seqs");
    assert_eq!(ContextStore::head_seq(&pg, &s).await.unwrap(), 2);
    let n = ContextStore::entries_since(&pg, &s, 0).await.unwrap().len();
    assert_eq!(n, 2);
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn set_disposition_stamps_entry() {
    let pg = fresh_store().await;
    let s = unique_session("disp");
    pg.create_session(&test_session(&s)).await.unwrap();
    let id = format!("{s}-e1");
    let mut e = test_entry(&id, &s, "endpoint-1");
    pg.append_entry(&mut e).await.unwrap();
    pg.set_disposition(&id, "QUARANTINE").await.unwrap();
    let stored = ContextStore::entry_by_id(&pg, &id).await.unwrap().unwrap();
    assert_eq!(stored.disposition, "QUARANTINE");
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn insert_entry_raw_is_idempotent_replay() {
    let pg = fresh_store().await;
    let s = unique_session("raw");
    pg.create_session(&test_session(&s)).await.unwrap();
    let id = format!("{s}-e1");
    let mut e = test_entry(&id, &s, "endpoint-1");
    e.seq = 1;
    pg.insert_entry_raw(&e).await.unwrap();
    // Replaying the same (session_id, seq) is a no-op (ON CONFLICT DO NOTHING).
    pg.insert_entry_raw(&e).await.unwrap();
    assert_eq!(ContextStore::head_seq(&pg, &s).await.unwrap(), 1);
}
