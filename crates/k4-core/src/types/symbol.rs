//! Fixed-size symbol utilities for shared-memory compatibility.
//!
//! Market data structs use `[u8; 32]` for symbols so they can live in SHM
//! without heap allocation. This module provides helpers to convert between
//! `&str` and the fixed-size representation.

/// Length of the fixed symbol buffer used in all SHM-compatible structs.
pub const SYMBOL_LEN: usize = 32;

/// Write a UTF-8 symbol string into a fixed `[u8; SYMBOL_LEN]` buffer.
///
/// The string is copied byte-for-byte and the remaining bytes are zero-filled.
/// If `s` is longer than `SYMBOL_LEN`, it is silently truncated.
#[inline]
pub fn symbol_to_bytes(s: &str) -> [u8; SYMBOL_LEN] {
    let mut buf = [0u8; SYMBOL_LEN];
    let len = s.len().min(SYMBOL_LEN);
    buf[..len].copy_from_slice(&s.as_bytes()[..len]);
    buf
}

/// Read a symbol from a fixed `[u8; SYMBOL_LEN]` buffer.
///
/// Returns the string up to the first null byte (or the full buffer if no null
/// is found). Returns `""` if the buffer starts with a null byte.
#[inline]
pub fn symbol_from_bytes(buf: &[u8; SYMBOL_LEN]) -> &str {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(SYMBOL_LEN);
    // SAFETY: we only store valid UTF-8 via `symbol_to_bytes`.
    // For robustness, fall back to lossy conversion in debug builds.
    std::str::from_utf8(&buf[..end]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let sym = "BTCUSDT";
        let buf = symbol_to_bytes(sym);
        assert_eq!(symbol_from_bytes(&buf), sym);
    }

    #[test]
    fn empty_symbol() {
        let buf = symbol_to_bytes("");
        assert_eq!(symbol_from_bytes(&buf), "");
    }

    #[test]
    fn max_length_symbol() {
        let sym = "A".repeat(SYMBOL_LEN);
        let buf = symbol_to_bytes(&sym);
        assert_eq!(symbol_from_bytes(&buf), sym);
    }

    #[test]
    fn truncation() {
        let sym = "A".repeat(SYMBOL_LEN + 10);
        let buf = symbol_to_bytes(&sym);
        assert_eq!(symbol_from_bytes(&buf).len(), SYMBOL_LEN);
    }
}
