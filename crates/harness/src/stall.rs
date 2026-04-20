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
///
/// v1.7.1 fix: clock-id value differs across unix-likes. On Linux,
/// `CLOCK_MONOTONIC` is **1**; on macOS (Darwin) it is **6**. v1.0–v1.7.0
/// hard-coded the Linux value, so `clock_gettime` returned EINVAL on
/// macOS, the caller silently fell back to a zeroed timespec, and every
/// `now_ns()` returned 0 — causing `elapsed()` to always report zero and
/// the stall watcher to never fire. See the Hermes Linux session that
/// exposed it (169-minute no-event hang, round 17 REFINING).
#[cfg(target_family = "unix")]
pub fn now_ns() -> u128 {
    let mut ts = libc_ts_stub::Timespec::default();
    let ok = unsafe { libc_ts_stub::clock_gettime_monotonic(&mut ts) };
    if !ok {
        // Defense in depth: if the kernel rejects our clock id anyway
        // (future platform divergence), fall back to `Instant`. Wrong
        // semantics on macOS (doesn't pause during sleep) is a much
        // cheaper failure than a permanently-zero clock that disables
        // the stall watcher entirely.
        return instant_fallback_ns();
    }
    (ts.tv_sec as u128) * 1_000_000_000 + (ts.tv_nsec as u128)
}

#[cfg(not(target_family = "unix"))]
pub fn now_ns() -> u128 {
    instant_fallback_ns()
}

fn instant_fallback_ns() -> u128 {
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
    // that we need.
    //
    // Clock-id values differ across unix flavours — this was the v1.7.1
    // bug. Linux `<bits/time.h>` has CLOCK_MONOTONIC = 1, while macOS
    // `<sys/_types/_clockid_t.h>` (and the public `<time.h>`) has
    // CLOCK_MONOTONIC = 6. Using the Linux value on macOS returns EINVAL
    // and the watcher silently disables itself.
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

    #[cfg(target_os = "macos")]
    pub const CLOCK_MONOTONIC: i32 = 6;

    #[cfg(all(target_family = "unix", not(target_os = "macos")))]
    pub const CLOCK_MONOTONIC: i32 = 1;

    /// Fills `ts` with the current CLOCK_MONOTONIC value. Returns `true`
    /// on success, `false` on kernel rejection so the caller can fall back
    /// to a monotonic-but-wrong-semantics source instead of silently
    /// reading a zeroed timespec.
    ///
    /// # Safety
    /// The caller must provide a valid, writable Timespec pointer.
    pub unsafe fn clock_gettime_monotonic(ts: *mut Timespec) -> bool {
        let rc = clock_gettime(CLOCK_MONOTONIC, ts);
        if rc != 0 {
            if !ts.is_null() {
                *ts = Timespec::default();
            }
            return false;
        }
        true
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

    /// v1.7.1 regression: `now_ns()` must return a non-zero, monotonically
    /// increasing value on every supported platform. Prior to v1.7.1 this
    /// was silently zero on macOS (wrong CLOCK_MONOTONIC constant), and
    /// the two tests above caught it only obliquely as "after < before
    /// failed". This one says the quiet part out loud so the root cause
    /// can't regress hidden.
    #[test]
    fn now_ns_is_nonzero_and_monotonic() {
        let a = now_ns();
        assert!(a > 0, "now_ns must be > 0 (clock-id wiring broken?); got {a}");
        std::thread::sleep(Duration::from_millis(2));
        let b = now_ns();
        assert!(b > a, "now_ns must be monotonic; got a={a}, b={b}");
    }
}
