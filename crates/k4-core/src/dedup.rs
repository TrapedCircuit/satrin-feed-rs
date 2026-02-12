//! Market data deduplication utilities.
//!
//! When running multiple redundant WebSocket connections to the same exchange,
//! duplicate messages arrive with the same `update_id`. The deduplicators in
//! this module filter out stale / duplicate data so that only the *first*
//! occurrence of each update is forwarded to shared memory and UDP.
//!
//! Two strategies are provided:
//!
//! 1. [`UpdateIdDedup`] — for exchanges that provide a monotonically increasing
//!    sequence number per symbol (all exchanges except Bybit futures trades).
//! 2. [`UuidDedup`] — for Bybit futures trades that use UUID trade IDs which
//!    must be hashed and checked in a Bloom-filter-like table.

use ahash::AHashMap;

// ---------------------------------------------------------------------------
// UpdateIdDedup — monotonic sequence-based
// ---------------------------------------------------------------------------

/// Deduplicator based on a per-symbol monotonically increasing update ID.
///
/// For each symbol, the last seen update ID is stored. A new message is
/// accepted only if its update ID is strictly greater than the stored value.
///
/// # Thread safety
///
/// Not thread-safe. Each dedup thread should own its own instance.
pub struct UpdateIdDedup {
    last_ids: AHashMap<String, u64>,
}

impl UpdateIdDedup {
    pub fn new() -> Self {
        Self {
            last_ids: AHashMap::new(),
        }
    }

    /// Check whether `update_id` is new for the given `symbol`.
    ///
    /// Returns `true` if this is a new (non-duplicate) update, `false` if it
    /// has already been seen or is older than the last seen ID.
    ///
    /// If `true`, the internal state is updated to record this ID.
    #[inline]
    pub fn check_and_update(&mut self, symbol: &str, update_id: u64) -> bool {
        let entry = self.last_ids.entry(symbol.to_string()).or_insert(0);
        if update_id > *entry {
            *entry = update_id;
            true
        } else {
            false
        }
    }

    /// Returns the last seen update ID for a symbol, or `None`.
    pub fn last_id(&self, symbol: &str) -> Option<u64> {
        self.last_ids.get(symbol).copied()
    }

    /// Clear all state.
    pub fn clear(&mut self) {
        self.last_ids.clear();
    }
}

impl Default for UpdateIdDedup {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// UuidDedup — hash-table based (for Bybit futures UUID trade IDs)
// ---------------------------------------------------------------------------

/// Number of slots in the UUID dedup hash table.
///
/// Must be a power of 2. 8192 slots × 8 bytes = 64 KB, which fits in L1 cache.
const UUID_TABLE_SIZE: usize = 8192;

/// Deduplicator for UUID-based trade IDs (Bybit futures).
///
/// Bybit futures trades use UUID strings as trade IDs, which are not
/// monotonically increasing. This deduplicator hashes the UUID and stores
/// the hash in a fixed-size table. Collisions cause silent replacement (false
/// negatives are possible but rare given the table size vs. throughput).
///
/// Uses xxHash64 for fast hashing.
pub struct UuidDedup {
    table: Vec<u64>,
}

impl UuidDedup {
    pub fn new() -> Self {
        Self {
            table: vec![0u64; UUID_TABLE_SIZE],
        }
    }

    /// Hash a UUID string using xxHash64.
    #[inline]
    fn hash_uuid(uuid: &str) -> u64 {
        xxhash_rust::xxh64::xxh64(uuid.as_bytes(), 0)
    }

    /// Check whether a UUID has been seen before.
    ///
    /// Returns `true` if the UUID is new, `false` if it was already recorded
    /// (or a hash collision occurred with a previously seen UUID).
    #[inline]
    pub fn check_and_insert(&mut self, uuid: &str) -> bool {
        let hash = Self::hash_uuid(uuid);
        let idx = (hash as usize) & (UUID_TABLE_SIZE - 1);

        if self.table[idx] == hash {
            false // duplicate (or very unlikely hash collision)
        } else {
            self.table[idx] = hash;
            true
        }
    }

    /// Clear all state.
    pub fn clear(&mut self) {
        self.table.fill(0);
    }
}

impl Default for UuidDedup {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_id_dedup_basic() {
        let mut d = UpdateIdDedup::new();
        assert!(d.check_and_update("BTCUSDT", 1));
        assert!(d.check_and_update("BTCUSDT", 2));
        assert!(!d.check_and_update("BTCUSDT", 2)); // duplicate
        assert!(!d.check_and_update("BTCUSDT", 1)); // stale
        assert!(d.check_and_update("BTCUSDT", 3));
    }

    #[test]
    fn update_id_dedup_multi_symbol() {
        let mut d = UpdateIdDedup::new();
        assert!(d.check_and_update("BTCUSDT", 1));
        assert!(d.check_and_update("ETHUSDT", 1)); // different symbol, same id
        assert!(!d.check_and_update("BTCUSDT", 1));
    }

    #[test]
    fn uuid_dedup_basic() {
        let mut d = UuidDedup::new();
        assert!(d.check_and_insert("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!d.check_and_insert("550e8400-e29b-41d4-a716-446655440000")); // dup
        assert!(d.check_and_insert("550e8400-e29b-41d4-a716-446655440001")); // new
    }
}
