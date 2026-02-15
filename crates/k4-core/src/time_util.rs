//! High-precision time utilities.
//!
//! Provides microsecond- and nanosecond-resolution timestamps using
//! `clock_gettime(CLOCK_REALTIME)` on Unix and `SystemTime` as fallback.

use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Linux: use clock_gettime for maximum precision
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[inline]
fn clock_realtime() -> (u64, u64) {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: CLOCK_REALTIME is always valid. Failure returns -1 but the
    // zeroed ts is a safe fallback (epoch).
    unsafe {
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts);
    }
    (ts.tv_sec as u64, ts.tv_nsec as u64)
}

#[cfg(target_os = "linux")]
#[inline]
fn clock_monotonic() -> (u64, u64) {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC_RAW, &mut ts);
    }
    (ts.tv_sec as u64, ts.tv_nsec as u64)
}

// ---------------------------------------------------------------------------
// Non-Linux: SystemTime / Instant fallback
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
#[inline]
fn clock_realtime() -> (u64, u64) {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (d.as_secs(), d.subsec_nanos() as u64)
}

#[cfg(not(target_os = "linux"))]
#[inline]
fn clock_monotonic() -> (u64, u64) {
    use std::{sync::LazyLock, time::Instant};
    static ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);
    let d = ORIGIN.elapsed();
    (d.as_secs(), d.subsec_nanos() as u64)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Current time as **microseconds** since Unix epoch.
#[inline]
pub fn now_us() -> u64 {
    let (sec, nsec) = clock_realtime();
    sec * 1_000_000 + nsec / 1_000
}

/// Current time as **nanoseconds** since Unix epoch.
#[inline]
pub fn now_ns() -> u64 {
    let (sec, nsec) = clock_realtime();
    sec * 1_000_000_000 + nsec
}

/// Current time as **milliseconds** since Unix epoch.
#[inline]
pub fn now_ms() -> u64 {
    let (sec, nsec) = clock_realtime();
    sec * 1_000 + nsec / 1_000_000
}

/// Monotonic clock in **microseconds** — for elapsed-time measurements
/// without wall-clock jumps.
#[inline]
pub fn monotonic_us() -> u64 {
    let (sec, nsec) = clock_monotonic();
    sec * 1_000_000 + nsec / 1_000
}
