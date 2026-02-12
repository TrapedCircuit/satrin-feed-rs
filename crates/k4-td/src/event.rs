//! Event types emitted by TD modules to downstream consumers (strategies).
//!
//! The strategy layer subscribes to a [`TdEventReceiver`] channel and reacts
//! to order updates, position changes, and connection lifecycle events.

use k4_core::enums::AccountType;
use k4_core::trading::{OrderUpdate, Position};

/// A typed event emitted by a [`TdModule`](crate::TdModule) implementation.
#[derive(Debug, Clone)]
pub enum TdEvent {
    /// An order status update (fill, cancel, reject, etc.).
    OrderUpdate(OrderUpdate),

    /// A position snapshot for one or more symbols.
    PositionUpdate(Vec<Position>),

    /// An account's user-data WebSocket stream connected successfully.
    Connected {
        /// Which account connected.
        account: AccountType,
    },

    /// An account's user-data WebSocket stream disconnected.
    Disconnected {
        /// Which account disconnected.
        account: AccountType,
        /// Human-readable reason.
        reason: String,
    },

    /// Listen key was refreshed successfully.
    ListenKeyRefreshed {
        /// Which account's listen key was refreshed.
        account: AccountType,
    },

    /// A non-fatal error occurred in the TD module.
    Error {
        /// Which account encountered the error.
        account: AccountType,
        /// Error description.
        message: String,
    },
}

/// Sender half of the TD event channel.
pub type TdEventSender = tokio::sync::mpsc::UnboundedSender<TdEvent>;

/// Receiver half of the TD event channel.
///
/// Strategy tasks should poll this to receive order updates and connection
/// lifecycle events.
pub type TdEventReceiver = tokio::sync::mpsc::UnboundedReceiver<TdEvent>;
