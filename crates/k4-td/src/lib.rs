//! # k4-td
//!
//! Trading (order execution) modules for cryptocurrency exchanges.
//!
//! Each exchange implements the [`TdModule`] trait, which provides a uniform
//! interface for order placement, cancellation, and position queries. The
//! lifecycle is: `login()` → order operations → `stop()`.
//!
//! ## Supported exchanges
//!
//! | Exchange | Module    | Accounts             | Order channel      |
//! |----------|-----------|----------------------|--------------------|
//! | Binance  | `binance` | Spot, UBase, CBase   | REST + WS API      |

pub mod binance;
pub mod event;

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use k4_core::{enums::AccountType, trading::*};

/// Trait implemented by all trading modules.
///
/// # Lifecycle
///
/// 1. Construct via exchange-specific `new(config)`.
/// 2. Call [`login`](TdModule::login) to authenticate, create listen keys, and establish user-data
///    WebSocket streams.
/// 3. Use [`insert_order`](TdModule::insert_order), [`cancel_order`](TdModule::cancel_order), etc.
///    for order management.
/// 4. Call [`stop`](TdModule::stop) to close all connections and clean up.
///
/// All order operations take `&self` so they can be called concurrently from
/// multiple strategy tasks.
#[async_trait]
pub trait TdModule: Send + Sync {
    /// Authenticate with the exchange and establish data streams.
    ///
    /// Returns `true` if login succeeded within the given `timeout`.
    async fn login(&mut self, timeout: Duration) -> Result<bool>;

    /// Submit a new order.
    ///
    /// Returns the exchange-assigned order ID on success.
    async fn insert_order(&self, order: &InputOrder) -> Result<u64>;

    /// Cancel an existing order.
    ///
    /// The `order` must contain at least `symbol`, `account_type`, and either
    /// `client_order_id` or the exchange order ID.
    async fn cancel_order(&self, order: &InputOrder) -> Result<()>;

    /// Cancel all open orders for a given account and symbol.
    async fn cancel_all_orders(&self, account: AccountType, symbol: &str) -> Result<()>;

    /// Query all open orders across enabled accounts.
    async fn query_open_orders(&self) -> Result<Vec<OrderUpdate>>;

    /// Query current positions (futures accounts only).
    async fn query_positions(&self) -> Result<Vec<Position>>;

    /// Gracefully shut down — close WebSockets, delete listen keys, abort tasks.
    async fn stop(&mut self) -> Result<()>;
}
