//! Re-export of UUID-based deduplication for Bybit futures trades.
//!
//! Bybit futures trades use UUID strings as trade IDs, which are not
//! monotonically increasing. This module re-exports [`UuidDedup`] from
//! `k4_core` for use in the Bybit dedup loop.

pub use k4_core::dedup::UuidDedup;
