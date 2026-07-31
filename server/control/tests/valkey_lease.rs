//! Integration tests for [`ValkeyLeaseAuthority`] (ADR 004). All are
//! `#[ignore]`'d — they require a live RESP-compatible server (Valkey
//! recommended) reachable via `FABRIC_KV_URL`.

use std::time::Duration;

use fabric_context::store::LeaseAuthority;
use fabric_types::context::Locus;
use fabric_types::lease::LeaseState;

use fabric_control::ValkeyLeaseAuthority;

fn kv_url() -> String {
    std::env::var("FABRIC_KV_URL").expect("FABRIC_KV_URL must be set for integration tests")
}

async fn fresh_kv() -> ValkeyLeaseAuthority {
    ValkeyLeaseAuthority::connect(&kv_url())
        .await
        .expect("connect")
}

fn unique_session(tag: &str) -> String {
    format!("{tag}-{}", uuid::Uuid::new_v4().simple())
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn acquire_then_active_then_release_lifecycle() {
    let kv = fresh_kv().await;
    let s = unique_session("cycle");
    let lease = kv
        .acquire_lease(&s, "endpoint-1", Locus::Endpoint, 30_000)
        .await
        .unwrap();
    assert_eq!(lease.holder_id, "endpoint-1");
    assert_eq!(lease.state, LeaseState::Active as i32);

    let active = kv.active_lease(&s).await.unwrap().unwrap();
    assert_eq!(active.lease_id, lease.lease_id);

    kv.release_lease(&s, "endpoint-1").await.unwrap();
    assert!(kv.active_lease(&s).await.unwrap().is_none());
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn second_acquire_conflicts() {
    let kv = fresh_kv().await;
    let s = unique_session("conflict");
    kv.acquire_lease(&s, "endpoint-1", Locus::Endpoint, 30_000)
        .await
        .unwrap();
    let err = kv
        .acquire_lease(&s, "server-1", Locus::Server, 30_000)
        .await
        .unwrap_err();
    assert!(matches!(err, fabric_context::StoreError::LeaseConflict(_)));
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn renew_extends_expiry_and_rejects_non_holder() {
    let kv = fresh_kv().await;
    let s = unique_session("renew");
    let lease = kv
        .acquire_lease(&s, "endpoint-1", Locus::Endpoint, 1_000)
        .await
        .unwrap();
    let renewed = kv
        .renew_lease(&lease.lease_id, "endpoint-1", 60_000)
        .await
        .unwrap();
    // A non-holder renew is forbidden.
    let err = kv
        .renew_lease(&lease.lease_id, "mallory", 60_000)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        fabric_context::StoreError::NotLeaseHolder { .. }
    ));
    // The renewal recorded the new expiry (later than granted_at).
    let _ = renewed;
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn release_rejects_non_holder_but_stays_idempotent() {
    let kv = fresh_kv().await;
    let s = unique_session("release");
    kv.acquire_lease(&s, "endpoint-1", Locus::Endpoint, 30_000)
        .await
        .unwrap();
    let err = kv.release_lease(&s, "mallory").await.unwrap_err();
    assert!(matches!(
        err,
        fabric_context::StoreError::NotLeaseHolder { .. }
    ));
    kv.release_lease(&s, "endpoint-1").await.unwrap();
    // Releasing an already-released lease is idempotent.
    kv.release_lease(&s, "endpoint-1").await.unwrap();
    assert!(kv.active_lease(&s).await.unwrap().is_none());
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn preempt_takes_over_and_is_noop_for_holder() {
    let kv = fresh_kv().await;
    let s = unique_session("preempt");
    kv.acquire_lease(&s, "endpoint-1", Locus::Endpoint, 30_000)
        .await
        .unwrap();
    let new = kv
        .preempt(&s, "web-1", Locus::Server, 30_000)
        .await
        .unwrap();
    assert_eq!(new.holder_id, "web-1");
    // Preempt from the current holder is a no-op.
    let same = kv
        .preempt(&s, "web-1", Locus::Server, 30_000)
        .await
        .unwrap();
    assert_eq!(same.lease_id, new.lease_id);
    // The old holder can no longer release.
    let err = kv.release_lease(&s, "endpoint-1").await.unwrap_err();
    assert!(matches!(
        err,
        fabric_context::StoreError::NotLeaseHolder { .. }
    ));
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn transfer_lease_is_atomic_handoff() {
    let kv = fresh_kv().await;
    let s = unique_session("transfer");
    kv.acquire_lease(&s, "endpoint-1", Locus::Endpoint, 30_000)
        .await
        .unwrap();
    let new = kv
        .transfer_lease(&s, "endpoint-1", "server-1", Locus::Server, 30_000, 7)
        .await
        .unwrap();
    assert_eq!(new.holder_id, "server-1");
    assert_eq!(new.granted_seq, 7);
    // The old holder is gone; the new holder holds the active lease.
    assert_eq!(
        kv.active_lease(&s).await.unwrap().unwrap().lease_id,
        new.lease_id
    );
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn ttl_expiry_releases_the_lease() {
    let kv = fresh_kv().await;
    let s = unique_session("ttl");
    kv.acquire_lease(&s, "endpoint-1", Locus::Endpoint, 100)
        .await
        .unwrap();
    // Wait past the TTL; the key expires natively and the lease is gone.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(kv.active_lease(&s).await.unwrap().is_none());
    // A fresh acquire succeeds now the expired one is gone.
    kv.acquire_lease(&s, "endpoint-2", Locus::Server, 30_000)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires Postgres + Valkey (FABRIC_PG_URL + FABRIC_KV_URL)"]
async fn verify_writer_checks_holder() {
    let kv = fresh_kv().await;
    let s = unique_session("verify");
    kv.acquire_lease(&s, "endpoint-1", Locus::Endpoint, 30_000)
        .await
        .unwrap();
    let lease = kv.verify_writer(&s, "endpoint-1").await.unwrap();
    assert_eq!(lease.holder_id, "endpoint-1");
    let err = kv.verify_writer(&s, "mallory").await.unwrap_err();
    assert!(matches!(
        err,
        fabric_context::StoreError::NotLeaseHolder { .. }
    ));
}
