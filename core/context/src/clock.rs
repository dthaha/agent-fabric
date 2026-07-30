//! Wall-clock time for the context plane, pinned to UTC. All timestamps
//! stored in the op-log are milliseconds since the Unix epoch (UTC), so
//! replicas in different timezones resolve conflicts identically. No local
//! timezone conversion ever happens in this crate.

use std::sync::Mutex;

/// Milliseconds since the Unix epoch (UTC) at 2024-01-01T00:00:00Z.
const MIN_SANE_MS: i64 = 1_704_067_200_000;

/// Milliseconds since the Unix epoch (UTC) at 2020-01-01T00:00:00Z. Entry
/// `created_at` timestamps older than this are insane (the fabric did not
/// exist); used by the replay path to flag garbage endpoint clocks.
const MIN_SANE_TIMESTAMP_MS: i64 = 1_577_836_800_000;

/// How far into the future an entry `created_at` may be before it counts as
/// insane (24h of clock skew tolerance).
const MAX_FUTURE_SKEW_MS: i64 = 86_400_000;

/// Current time in milliseconds since the Unix epoch, UTC. This is a wall
/// clock reading: it may go backwards across calls (NTP adjustments), so
/// conflict-resolution timestamps should come from [`MonotonicClock`]
/// instead.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Sanity check that the system clock is not wildly wrong (e.g. reset to
/// the epoch on a factory-reset device). This is NOT clock synchronization —
/// the fabric never trusts wall time across machines — it only guards
/// against garbage timestamps polluting the op-log.
pub fn is_clock_sane() -> bool {
    now_ms() >= MIN_SANE_MS
}

/// Sanity check on an entry `created_at` timestamp (epoch milliseconds):
/// sane means not before 2020 and not more than 24h in the future. Per
/// ADR 006 ("accept everything") an insane timestamp is flagged, never
/// rejected — `created_at` is an untrusted endpoint claim and merge
/// ordering does not depend on it.
pub fn is_timestamp_sane(ms: i64) -> bool {
    ms >= MIN_SANE_TIMESTAMP_MS && ms <= now_ms().saturating_add(MAX_FUTURE_SKEW_MS)
}

/// Per-writer monotonic timestamp source. Each tick returns
/// `max(wall_clock_ms, last_tick + 1)`, so timestamps handed out by one
/// writer are strictly increasing even when entries are appended within the
/// same wall-clock millisecond (or the wall clock ticks backwards). This
/// keeps (created_at, entry_id) conflict resolution deterministic without
/// relying on sub-millisecond wall-clock resolution.
#[derive(Debug, Default)]
pub struct MonotonicClock {
    last_ms: Mutex<i64>,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next strictly-increasing timestamp, in UTC epoch milliseconds.
    pub fn tick(&self) -> i64 {
        // Recover from a poisoned mutex rather than panicking: the counter
        // is just an i64, never left in a corrupt state worth dying over.
        let mut last = self.last_ms.lock().unwrap_or_else(|p| p.into_inner());
        let now = now_ms();
        let tick = if now > *last { now } else { *last + 1 };
        *last = tick;
        tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_is_sane() {
        assert!(is_clock_sane());
    }

    #[test]
    fn timestamp_sanity_bounds() {
        // 2026-ish now is sane.
        assert!(is_timestamp_sane(now_ms()));
        // 1970 and 2019 are insane (before 2020).
        assert!(!is_timestamp_sane(0));
        assert!(!is_timestamp_sane(1_000_000_000_000));
        // 2099 is insane (far future).
        assert!(!is_timestamp_sane(4_070_889_600_000));
    }

    #[test]
    fn tick_is_strictly_increasing() {
        let clock = MonotonicClock::new();
        let mut prev = clock.tick();
        // Far faster than 1ms per iteration: wall time alone would collide.
        for _ in 0..100_000 {
            let tick = clock.tick();
            assert!(tick > prev, "tick {tick} not after {prev}");
            prev = tick;
        }
    }

    #[test]
    fn tick_never_returns_zero_when_wall_clock_is_epoch() {
        let clock = MonotonicClock::new();
        assert!(clock.tick() > 0);
    }
}
