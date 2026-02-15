//! Trading-related data structures — orders, positions, and events.
//!
//! These types are used by the TD (trading) modules and flow between the
//! strategy layer and the exchange API via message queues.

use serde::{Deserialize, Serialize};

use super::enums::{AccountType, Direction, OrderStatus, OrderType};

// ---------------------------------------------------------------------------
// Input order (strategy → TD module)
// ---------------------------------------------------------------------------

/// An order request sent from a strategy to the TD module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputOrder {
    /// Unified symbol (e.g. `"BTCUSDT"`).
    pub symbol: String,
    /// Which account to place the order on.
    pub account_type: AccountType,
    /// Buy or sell.
    pub direction: Direction,
    /// Order type / time-in-force.
    pub order_type: OrderType,
    /// Limit price (ignored for market orders).
    pub price: f64,
    /// Order quantity.
    pub quantity: f64,
    /// Client-assigned order ID (unique per strategy).
    pub client_order_id: u64,
    /// Strategy ID for attribution.
    pub strategy_id: u32,
    /// Recv window for Binance signature (ms, 0 = default).
    pub recv_window: u64,
}

// ---------------------------------------------------------------------------
// Order update (TD module → strategy)
// ---------------------------------------------------------------------------

/// An order status update received from the exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderUpdate {
    /// Unified symbol.
    pub symbol: String,
    /// Exchange-assigned order ID.
    pub order_id: u64,
    /// Client-assigned order ID.
    pub client_order_id: u64,
    /// Strategy ID extracted from the client order ID.
    pub strategy_id: u32,
    /// Current order status.
    pub status: OrderStatus,
    /// Buy or sell.
    pub direction: Direction,
    /// Order price.
    pub price: f64,
    /// Original order quantity.
    pub quantity: f64,
    /// Cumulative filled quantity.
    pub filled_quantity: f64,
    /// Average fill price.
    pub filled_avg_price: f64,
    /// Cumulative commission paid.
    pub commission: f64,
    /// Timestamp of this update (ms since epoch).
    pub update_time: u64,
}

// ---------------------------------------------------------------------------
// Position
// ---------------------------------------------------------------------------

/// A position snapshot from the exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Unified symbol.
    pub symbol: String,
    /// Account type.
    pub account_type: AccountType,
    /// Net position amount (positive = long, negative = short).
    pub position_amt: f64,
    /// Average entry price.
    pub entry_price: f64,
    /// Unrealized PnL.
    pub unrealized_pnl: f64,
}
