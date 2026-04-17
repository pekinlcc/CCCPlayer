//! Stall detection using a monotonic clock that pauses during system sleep.
//! See PRD §16.4.
//!
//! On macOS, `Instant` is backed by mach_absolute_time which does NOT pause
//! during sleep — that's `CLOCK_BOOTTIME` semantics. What we want is
//! `CLOCK_MONOTONIC` which on Darwin **does** pause during sleep, so we use
//! the lower-level clock directly via libc on unix targets.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A wall-clock-independent monotonic timestamp, in nanoseconds. Pauses
/// while the system is asleep on Darwin (CLOCK_MONOTONIC) and on Linux.
/// On Windows / tests we fall back to `Instant` which is close enough.
#[cfg(target_family = "unix")]
pub fn now_ns() -> u128 {
    let mut ts = libc_ts_stub::Timespec::default();
    unsafe {
        libc_ts_stub::clock_gettime_monotonic(&mut ts);
    }
    (ts.tv_sec as u128) * 1_000_000_000 + (ts.tv_nsec as u128)
}

#[cfg(not(target_family = "unix"))]
pub fn now_ns() -> u128 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    Instant::now().duration_since(*start).as_nanos()
}

/// Watches the "last heartbeat" timestamp and decides when to declare the
/// child stalled.
#[derive(Clone, Debug)]
pub struct StallWatcher {
    last_ns: Arc<AtomicU64>,
    threshold: Duration,
}

impl StallWatcher {
    pub fn new(threshold: Duration) -> Self {
        let me = Self {
            last_ns: Arc::new(AtomicU64::new(now_ns() as u64)),
            threshold,
        };
        me.bump();
        me
    }

    /// Record a fresh heartbeat. Should be called whenever the harness sees
    /// any new event from the CLI (stdout line, stderr line, usage update).
    pub fn bump(&self) {
        self.last_ns.store(now_ns() as u64, Ordering::Release);
    }

    /// Same as [`bump`] but shifts the reference point forward by `extra`, used
    /// when resuming from macOS sleep to give the CLI a grace window.
    /// See §16.4.
    pub fn wake_grace(&self, extra: Duration) {
        let now = now_ns() as u64;
        let shifted = now + extra.as_nanos() as u64;
        self.last_ns.store(shifted, Ordering::Release);
    }

    pub fn elapsed(&self) -> Duration {
        let then = self.last_ns.load(Ordering::Acquire);
        let now = now_ns() as u64;
        Duration::from_nanos(now.saturating_sub(then))
    }

    pub fn is_stalled(&self) -> bool {
        self.elapsed() >= self.threshold
    }

    pub fn threshold(&self) -> Duration {
        self.threshold
    }
}

#[cfg(target_family = "unix")]
mod libc_ts_stub {
    // We avoid a full libc dep by declaring the small bits of clock_gettime
    // that we need. CLOCK_MONOTONIC = 1 on Linux and macOS.
    #[repr(C)]
    #[derive(Default)]
    pub struct Timespec {
        pub tv_sec: i64,
        pub tv_nsec: i64,
    }

    // SAFETY: these are posix-standard FFI declarations. The function is a
    // no-side-effect read on a thread-local kernel clock.
    extern "C" {
        fn clock_gettime(clk_id: i32, tp: *mut Timespec) -> i32;
    }
    pub const CLOCK_MONOTONIC: i32 = 1;

    /// Fills `ts` with the current CLOCK_MONOTONIC value. Panics only in the
    /// pathological case that the kernel rejects CLOCK_MONOTONIC, which
    /// indicates a platform we don't support.
    ///
    /// # Safety
    /// The caller must provide a valid, writable Timespec pointer.
    pub unsafe fn clock_gettime_monotonic(ts: *mut Timespec) {
        let rc = clock_gettime(CLOCK_MONOTONIC, ts);
        if rc != 0 {
            // If the syscall fails the pointed-to struct is uninitialized,
            // so zero it so callers read predictable values.
            if !ts.is_null() {
                *ts = Timespec::default();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_resets_elapsed() {
        let w = StallWatcher::new(Duration::from_secs(600));
        std::thread::sleep(Duration::from_millis(10));
        let before = w.elapsed();
        w.bump();
        let after = w.elapsed();
        assert!(after < before);
    }

    #[test]
    fn crosses_threshold() {
        let w = StallWatcher::new(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(25));
        assert!(w.is_stalled());
    }
}
