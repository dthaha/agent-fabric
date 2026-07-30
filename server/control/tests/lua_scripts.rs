//! Unit tests for the Valkey Lua-script contract (ADR 004). These run without
//! a Valkey server: they assert the `const` scripts shipped in
//! [`ValkeyLeaseAuthority`] are well-formed and atomic-shaped — the right
//! Redis primitives, the `!`-prefixed error convention, and the lease/reverse-
//! index bookkeeping. Behavioral coverage (acquire/conflict/renew/release/
//! preempt/TTL) lives in the ignored integration tests against a live Valkey.

#![cfg(feature = "server-store")]

use fabric_control::valkey_lease::{
    ACQUIRE, PREEMPT, RELEASE, RENEW, SET_GRANTED_BY, SET_GRANTED_SEQ, TRANSFER,
};

/// A mutating script must run its writes under a single atomic block and
/// must report errors via the `!`-prefix sentinel convention. `!NOTHOLDER` is
/// emitted as `!NOTHOLDER:<holder>` so a substring match covers it.
fn assert_error_convention(title: &str, script: &str, sentinel: &str) {
    assert!(
        script.contains(sentinel),
        "{title}: script must return the `{sentinel}` sentinel"
    );
}

#[test]
fn acquire_is_atomic_setnx_plus_reverse_index() {
    assert_error_convention("acquire", ACQUIRE, "!CONFLICT");
    assert!(ACQUIRE.contains("'NX'"), "acquire must use SET NX (atomic)");
    assert!(ACQUIRE.contains("'PX'"), "acquire must stamp a TTL");
    // The reverse-index key is written with the SAME ttl as the lease key.
    assert!(ACQUIRE.contains("KEYS[2]"));
}

#[test]
fn release_is_verify_then_del_and_idempotent() {
    // `nil` return = already gone (idempotent release); mismatch = !NOTHOLDER.
    assert!(
        RELEASE.contains("if not v then return nil end"),
        "release must be idempotent"
    );
    assert_error_convention("release", RELEASE, "!NOTHOLDER");
    assert!(RELEASE.contains("'DEL'"), "release must delete the lease");
    assert!(
        RELEASE.contains("leaseid:"),
        "release must drop the reverse index"
    );
}

#[test]
fn renew_verifies_holder_and_refreshes_ttl() {
    assert_error_convention("renew", RENEW, "!NOLEASE");
    assert_error_convention("renew", RENEW, "!NOTHOLDER");
    assert!(RENEW.contains("'PX'"), "renew must refresh the TTL");
    assert!(
        RENEW.contains("'PEXPIRE'"),
        "renew must refresh the reverse index TTL"
    );
    assert!(
        RENEW.contains("expires_at_ms"),
        "renew must rewrite the expiry in JSON"
    );
}

#[test]
fn preempt_is_same_holder_noop_otherwise_revokes_and_regrants() {
    // Same holder → return the existing lease unchanged (no-op).
    assert!(
        PREEMPT.contains("j.holder_id == ARGV[4] then return v end"),
        "preempt from the current holder is a no-op"
    );
    // Otherwise revoke the old reverse index and grant fresh.
    assert!(
        PREEMPT.contains("DEL"),
        "preempt must revoke the old reverse index"
    );
    assert!(PREEMPT.contains("'PX'"), "preempt must grant with a TTL");
}

#[test]
fn transfer_lease_is_atomic_release_plus_grant() {
    assert_error_convention("transfer", TRANSFER, "!NOLEASE");
    assert_error_convention("transfer", TRANSFER, "!NOTHOLDER");
    // Both the old reverse-index DEL and the new SET happen in one script
    // (atomic under Valkey's single-threaded model) — H4 fix.
    assert!(
        TRANSFER.contains("'DEL'"),
        "transfer must drop the old reverse index"
    );
    assert!(
        TRANSFER.matches("'SET'").count() == 2,
        "transfer set lease + reverse index"
    );
}

#[test]
fn set_granted_seq_and_granted_by_preserve_ttl() {
    assert!(
        SET_GRANTED_SEQ.contains("'KEEPTTL'"),
        "set_granted_seq must preserve TTL"
    );
    assert!(
        SET_GRANTED_BY.contains("'KEEPTTL'"),
        "set_granted_by must preserve TTL"
    );
}

#[test]
fn scripts_reference_keys_and_argv_only() {
    // No hard-coded key names beyond the `leaseid:` reverse-index prefix;
    // everything else flows through KEYS/ARGV so the authority is per-session.
    for (name, script) in [
        ("acquire", ACQUIRE),
        ("release", RELEASE),
        ("renew", RENEW),
        ("preempt", PREEMPT),
        ("transfer", TRANSFER),
        ("set_granted_seq", SET_GRANTED_SEQ),
        ("set_granted_by", SET_GRANTED_BY),
    ] {
        assert!(
            !script.contains("lease:") && !script.contains("leaseid:{"),
            "{name}: must not hard-code session/lease keys (use KEYS[])"
        );
    }
}
