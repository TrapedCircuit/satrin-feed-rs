//! CPU affinity utilities for binding threads to specific cores.
//!
//! Low-latency trading systems benefit from pinning hot-path threads (dedup,
//! WebSocket I/O) to dedicated CPU cores, avoiding scheduler jitter and cache
//! thrashing. This module wraps the `core_affinity` crate with a simple API.

use tracing::{info, warn};

/// Bind the current thread to the specified CPU core.
///
/// Returns `true` if the binding succeeded, `false` if the core ID is invalid
/// or the OS rejected the request.
///
/// # Example
///
/// ```ignore
/// std::thread::spawn(move || {
///     k4_core::cpu_affinity::bind_to_core(2);
///     // This thread now runs exclusively on core 2
///     hot_loop();
/// });
/// ```
pub fn bind_to_core(core_id: usize) -> bool {
    let core_ids = core_affinity::get_core_ids().unwrap_or_default();
    if let Some(core) = core_ids.get(core_id) {
        let ok = core_affinity::set_for_current(*core);
        if ok {
            info!("bound thread to CPU core {core_id}");
        } else {
            warn!("failed to bind thread to CPU core {core_id}");
        }
        ok
    } else {
        warn!(
            "CPU core {core_id} not available (system has {} cores)",
            core_ids.len()
        );
        false
    }
}

/// Bind the current thread to the specified core, if `core_id` is `Some`.
///
/// Convenience wrapper that does nothing for `None` (no affinity configured).
pub fn maybe_bind(core_id: Option<i32>) {
    if let Some(id) = core_id
        && id >= 0 {
            bind_to_core(id as usize);
        }
}
