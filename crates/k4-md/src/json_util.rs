//! Shared JSON parsing helpers used by all exchange modules.
//!
//! These utilities are extracted from the per-exchange parsers to eliminate
//! duplication of common patterns like string-to-f64 conversion and
//! depth-level filling.

use k4_core::types::Depth5;

/// Parse a JSON value (string or number) as `f64`.
///
/// Handles the common exchange pattern where numeric values may be encoded
/// as either JSON strings (`"30000.5"`) or native numbers (`30000.5`).
#[inline]
pub fn parse_str_f64(v: Option<&serde_json::Value>) -> Option<f64> {
    let v = v?;
    if let Some(s) = v.as_str() {
        fast_float2::parse(s).ok()
    } else {
        v.as_f64()
    }
}

/// Parse a JSON value (string or number) as `u64`.
#[inline]
pub fn parse_str_u64(v: Option<&serde_json::Value>) -> Option<u64> {
    let v = v?;
    if let Some(s) = v.as_str() {
        s.parse().ok()
    } else {
        v.as_u64()
    }
}

/// Parse a JSON value (string or number) as `i32`.
#[inline]
pub fn parse_str_i32(v: Option<&serde_json::Value>) -> Option<i32> {
    let v = v?;
    if let Some(s) = v.as_str() {
        s.parse().ok()
    } else {
        v.as_i64().map(|n| n as i32)
    }
}

/// Parse a named field on a JSON object as `f64` (string or number).
#[inline]
pub fn parse_f64_field(v: &serde_json::Value, key: &str) -> Option<f64> {
    parse_str_f64(v.get(key))
}

/// Fill the bid/ask arrays of a [`Depth5`] from JSON level arrays.
///
/// Each level is expected to be `["price", "vol"]` or `["price", "vol", "extra", "count"]`.
/// The optional 4th element is treated as an order count.
pub fn fill_depth5_levels(
    depth: &mut Depth5,
    bids: &[serde_json::Value],
    asks: &[serde_json::Value],
) {
    depth.bid_level = bids.len().min(5) as u32;
    depth.ask_level = asks.len().min(5) as u32;

    for (i, level) in bids.iter().take(5).enumerate() {
        if let Some(arr) = level.as_array() {
            depth.bid_prices[i] = parse_str_f64(arr.first()).unwrap_or(0.0);
            depth.bid_vols[i] = parse_str_f64(arr.get(1)).unwrap_or(0.0);
            if let Some(count) = parse_str_i32(arr.get(3)) {
                depth.bid_order_counts[i] = count;
            }
        }
    }
    for (i, level) in asks.iter().take(5).enumerate() {
        if let Some(arr) = level.as_array() {
            depth.ask_prices[i] = parse_str_f64(arr.first()).unwrap_or(0.0);
            depth.ask_vols[i] = parse_str_f64(arr.get(1)).unwrap_or(0.0);
            if let Some(count) = parse_str_i32(arr.get(3)) {
                depth.ask_order_counts[i] = count;
            }
        }
    }
}
