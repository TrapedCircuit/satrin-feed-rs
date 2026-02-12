//! # k4-core
//!
//! Core crate for the K4 Crypto system, providing:
//!
//! - **Types** (`types`) — enums, market data structs, trading structs, symbol utils
//! - **Configuration** (`config`) — JSON config deserialization
//! - **Error types** (`error`) — domain-specific `K4Error` via thiserror
//! - **Shared memory** (`shm`) — ring-buffer market data store over mmap
//! - **UDP channel** (`udp`) — async UDP sender/receiver with rkyv serialization
//! - **WebSocket** (`ws`) — WS client with auto-reconnect + redundancy
//! - **CPU affinity** (`cpu_affinity`) — thread-to-core pinning for low latency
//! - **Latency** (`latency`) — histogram-based latency statistics
//! - **Deduplication** (`dedup`) — update-ID and UUID-based deduplicators
//! - **Time utilities** (`time_util`) — high-precision timestamps
//! - **Logging** (`logging`) — tracing-based structured logging

pub mod config;
pub mod cpu_affinity;
pub mod dedup;
pub mod error;
pub mod latency;
pub mod logging;
pub mod shm;
pub mod time_util;
pub mod types;
pub mod udp;
pub mod ws;

// Re-export types at crate root for convenience.
pub use types::*;
