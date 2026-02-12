//! Typed error definitions for the K4 Crypto system.
//!
//! Provides [`K4Error`] for domain-specific errors that are more informative
//! than plain `anyhow::Error` strings. All variants implement `std::error::Error`
//! via `thiserror`, so they integrate seamlessly with `anyhow::Result`.

use thiserror::Error;

/// Domain-specific errors for the K4 Crypto system.
#[derive(Debug, Error)]
pub enum K4Error {
    /// Configuration parsing or validation error.
    #[error("config error: {0}")]
    Config(String),

    /// Shared memory creation, mapping, or access error.
    #[error("shm error: {0}")]
    Shm(String),

    /// WebSocket connection, handshake, or communication error.
    #[error("websocket error: {0}")]
    WebSocket(String),

    /// Market data or order response parsing error.
    #[error("parse error: {0}")]
    Parse(String),

    /// UDP socket or serialization error.
    #[error("udp error: {0}")]
    Udp(String),

    /// Trading operation error (order placement, cancellation, etc.).
    #[error("trading error: {0}")]
    Trading(String),
}
