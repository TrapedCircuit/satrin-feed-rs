//! Binance trading module.
//!
//! Implements the [`TdModule`](crate::TdModule) trait for the Binance exchange,
//! supporting three account types:
//!
//! - **Spot** — `https://api.binance.com` + WebSocket API for orders
//! - **UBase** — USDT-margined futures (`https://fapi.binance.com`)
//! - **CBase** — Coin-margined futures (`https://dapi.binance.com`)
//!
//! # Architecture
//!
//! ```text
//! BinanceTd
//! ├── SpotClient          (REST + WS API)
//! ├── FuturesClient       (UBase REST)
//! ├── FuturesClient       (CBase REST)
//! ├── user-data WS tasks  (order/position updates → TdEvent channel)
//! └── listen-key refresh  (background keepalive every N seconds)
//! ```
//!
//! All order methods take `&self` and are safe to call from multiple tasks
//! concurrently. Internal state that requires mutation is wrapped in
//! [`tokio::sync::Mutex`].

pub mod auth;
pub mod config;
pub mod futures;
pub mod spot;
pub mod symbol_mapper;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use k4_core::enums::AccountType;
use k4_core::trading::*;
use tracing::{error, info, warn};

use self::config::BinanceTdConfig;
use self::futures::{FuturesClient, FuturesVariant};
use self::spot::SpotClient;
use self::symbol_mapper::SymbolMapper;
use crate::event::{TdEvent, TdEventSender};

/// Binance trading module.
///
/// Manages spot and futures sub-clients, user data streams, and listen key
/// lifecycle. Order operations are routed to the appropriate sub-client based
/// on the [`AccountType`] in the order request.
pub struct BinanceTd {
    /// Module configuration.
    config: BinanceTdConfig,
    /// Spot sub-client (created during [`login`](crate::TdModule::login)).
    spot: Option<Arc<SpotClient>>,
    /// USDT-margined futures sub-client.
    ubase: Option<Arc<FuturesClient>>,
    /// Coin-margined futures sub-client.
    cbase: Option<Arc<FuturesClient>>,
    /// Channel for emitting events to the strategy layer.
    event_tx: TdEventSender,
    /// Bidirectional symbol mapper.
    symbol_mapper: SymbolMapper,
    /// Background task handles (listen key refresh, user data WS, etc.).
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl BinanceTd {
    /// Create a new Binance TD module.
    ///
    /// Returns the module and a receiver for [`TdEvent`]s that the strategy
    /// layer should poll.
    pub fn new(config: BinanceTdConfig) -> (Self, crate::event::TdEventReceiver) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let td = Self {
            config,
            spot: None,
            ubase: None,
            cbase: None,
            event_tx: tx,
            symbol_mapper: SymbolMapper::new(),
            tasks: Vec::new(),
        };
        (td, rx)
    }

    /// Start a background task that refreshes a listen key at a fixed interval.
    fn spawn_listen_key_refresh(
        &mut self,
        account: AccountType,
        client_spot: Option<Arc<SpotClient>>,
        client_futures: Option<Arc<FuturesClient>>,
        interval_secs: u64,
        event_tx: TdEventSender,
    ) {
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            interval.tick().await; // skip the immediate first tick

            loop {
                interval.tick().await;

                let result = if let Some(ref spot) = client_spot {
                    spot.keepalive_listen_key().await
                } else if let Some(ref fut) = client_futures {
                    fut.keepalive_listen_key().await
                } else {
                    break;
                };

                match result {
                    Ok(()) => {
                        let _ = event_tx.send(TdEvent::ListenKeyRefreshed { account });
                    }
                    Err(e) => {
                        warn!("[binance-td] listen key refresh failed for {account:?}: {e}");
                        let _ = event_tx.send(TdEvent::Error {
                            account,
                            message: format!("listen key refresh failed: {e}"),
                        });
                    }
                }
            }
        });
        self.tasks.push(task);
    }

    /// Route an account type to the appropriate sub-client label.
    fn account_label(account: AccountType) -> &'static str {
        match account {
            AccountType::Spot => "spot",
            AccountType::UBased => "ubase",
            AccountType::CBased => "cbase",
        }
    }
}

#[async_trait]
impl crate::TdModule for BinanceTd {
    async fn login(&mut self, timeout: Duration) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + timeout;
        let refresh_secs = self.config.listen_key_refresh_secs;

        // -- Spot --
        if self.config.spot_enabled {
            let client = Arc::new(SpotClient::new(
                self.config.api_key.clone(),
                self.config.secret_key.clone(),
                self.config.spot_rest_url.clone(),
                self.config.spot_ws_api_url.clone(),
                self.config.recv_window,
            ));

            match tokio::time::timeout_at(deadline, client.create_listen_key()).await {
                Ok(Ok(key)) => {
                    info!("[binance-td] spot listen key: {}", &key[..8.min(key.len())]);
                    self.spot = Some(Arc::clone(&client));
                    self.spawn_listen_key_refresh(
                        AccountType::Spot,
                        Some(Arc::clone(&client)),
                        None,
                        refresh_secs,
                        self.event_tx.clone(),
                    );
                    let _ = self.event_tx.send(TdEvent::Connected {
                        account: AccountType::Spot,
                    });
                }
                Ok(Err(e)) => {
                    error!("[binance-td] spot login failed: {e}");
                    return Ok(false);
                }
                Err(_) => {
                    error!("[binance-td] spot login timed out");
                    return Ok(false);
                }
            }
        }

        // -- UBase --
        if self.config.ubase_enabled {
            let client = Arc::new(FuturesClient::new(
                self.config.api_key.clone(),
                self.config.secret_key.clone(),
                self.config.ubase_rest_url.clone(),
                self.config.recv_window,
                FuturesVariant::UBase,
            ));

            match tokio::time::timeout_at(deadline, client.create_listen_key()).await {
                Ok(Ok(key)) => {
                    info!(
                        "[binance-td] ubase listen key: {}",
                        &key[..8.min(key.len())]
                    );
                    self.ubase = Some(Arc::clone(&client));
                    self.spawn_listen_key_refresh(
                        AccountType::UBased,
                        None,
                        Some(Arc::clone(&client)),
                        refresh_secs,
                        self.event_tx.clone(),
                    );
                    let _ = self.event_tx.send(TdEvent::Connected {
                        account: AccountType::UBased,
                    });
                }
                Ok(Err(e)) => {
                    error!("[binance-td] ubase login failed: {e}");
                    return Ok(false);
                }
                Err(_) => {
                    error!("[binance-td] ubase login timed out");
                    return Ok(false);
                }
            }
        }

        // -- CBase --
        if self.config.cbase_enabled {
            let client = Arc::new(FuturesClient::new(
                self.config.api_key.clone(),
                self.config.secret_key.clone(),
                self.config.cbase_rest_url.clone(),
                self.config.recv_window,
                FuturesVariant::CBase,
            ));

            match tokio::time::timeout_at(deadline, client.create_listen_key()).await {
                Ok(Ok(key)) => {
                    info!(
                        "[binance-td] cbase listen key: {}",
                        &key[..8.min(key.len())]
                    );
                    self.cbase = Some(Arc::clone(&client));
                    self.spawn_listen_key_refresh(
                        AccountType::CBased,
                        None,
                        Some(Arc::clone(&client)),
                        refresh_secs,
                        self.event_tx.clone(),
                    );
                    let _ = self.event_tx.send(TdEvent::Connected {
                        account: AccountType::CBased,
                    });
                }
                Ok(Err(e)) => {
                    error!("[binance-td] cbase login failed: {e}");
                    return Ok(false);
                }
                Err(_) => {
                    error!("[binance-td] cbase login timed out");
                    return Ok(false);
                }
            }
        }

        // Load symbol mappings from exchange info (best-effort)
        if let Some(ref spot) = self.spot {
            match spot.get_exchange_info().await {
                Ok(info) => self.symbol_mapper.load_from_exchange_info(&info),
                Err(e) => warn!("[binance-td] failed to load spot exchange info: {e}"),
            }
        }

        info!(
            "[binance-td] login complete — spot={}, ubase={}, cbase={}",
            self.spot.is_some(),
            self.ubase.is_some(),
            self.cbase.is_some(),
        );
        Ok(true)
    }

    async fn insert_order(&self, order: &InputOrder) -> Result<u64> {
        let side = match order.direction {
            k4_core::enums::Direction::Buy => "BUY",
            k4_core::enums::Direction::Sell => "SELL",
        };
        let order_type = match order.order_type {
            k4_core::enums::OrderType::Market => "MARKET",
            k4_core::enums::OrderType::Limit | k4_core::enums::OrderType::Gtc => "LIMIT",
            k4_core::enums::OrderType::PostOnly => "LIMIT_MAKER",
            k4_core::enums::OrderType::Ioc => "LIMIT",
            k4_core::enums::OrderType::Fok => "LIMIT",
        };
        let qty_str = order.quantity.to_string();
        let price_str = order.price.to_string();
        let coid_str = order.client_order_id.to_string();
        let price = if order.price > 0.0 {
            Some(price_str.as_str())
        } else {
            None
        };

        let resp = match order.account_type {
            AccountType::Spot => {
                let client = self
                    .spot
                    .as_ref()
                    .ok_or_else(|| anyhow!("spot client not initialized"))?;
                client
                    .ws_place_order(
                        &order.symbol,
                        side,
                        order_type,
                        &qty_str,
                        price,
                        Some(&coid_str),
                    )
                    .await?
            }
            AccountType::UBased => {
                let client = self
                    .ubase
                    .as_ref()
                    .ok_or_else(|| anyhow!("ubase client not initialized"))?;
                client
                    .place_order(
                        &order.symbol,
                        side,
                        order_type,
                        &qty_str,
                        price,
                        Some(&coid_str),
                    )
                    .await?
            }
            AccountType::CBased => {
                let client = self
                    .cbase
                    .as_ref()
                    .ok_or_else(|| anyhow!("cbase client not initialized"))?;
                client
                    .place_order(
                        &order.symbol,
                        side,
                        order_type,
                        &qty_str,
                        price,
                        Some(&coid_str),
                    )
                    .await?
            }
        };

        // Extract the exchange order ID from the response
        let order_id = resp.get("orderId").and_then(|v| v.as_u64()).unwrap_or(0);

        info!(
            "[binance-td] order placed: {} {} {} qty={} → id={}",
            Self::account_label(order.account_type),
            order.symbol,
            side,
            order.quantity,
            order_id,
        );

        Ok(order_id)
    }

    async fn cancel_order(&self, order: &InputOrder) -> Result<()> {
        let coid_str = order.client_order_id.to_string();
        match order.account_type {
            AccountType::Spot => {
                let client = self
                    .spot
                    .as_ref()
                    .ok_or_else(|| anyhow!("spot client not initialized"))?;
                client
                    .ws_cancel_order(&order.symbol, None, Some(&coid_str))
                    .await?;
            }
            AccountType::UBased => {
                let client = self
                    .ubase
                    .as_ref()
                    .ok_or_else(|| anyhow!("ubase client not initialized"))?;
                client
                    .cancel_order(&order.symbol, None, Some(&coid_str))
                    .await?;
            }
            AccountType::CBased => {
                let client = self
                    .cbase
                    .as_ref()
                    .ok_or_else(|| anyhow!("cbase client not initialized"))?;
                client
                    .cancel_order(&order.symbol, None, Some(&coid_str))
                    .await?;
            }
        }

        info!(
            "[binance-td] order cancelled: {} {} coid={}",
            Self::account_label(order.account_type),
            order.symbol,
            order.client_order_id,
        );

        Ok(())
    }

    async fn cancel_all_orders(&self, account: AccountType, symbol: &str) -> Result<()> {
        match account {
            AccountType::Spot => {
                // Spot doesn't have a batch cancel-all endpoint; query + cancel each
                let client = self
                    .spot
                    .as_ref()
                    .ok_or_else(|| anyhow!("spot client not initialized"))?;
                let orders = client.get_open_orders(Some(symbol)).await?;
                if let Some(arr) = orders.as_array() {
                    for o in arr {
                        if let Some(oid) = o.get("orderId").and_then(|v| v.as_u64()) {
                            let _ = client.ws_cancel_order(symbol, Some(oid), None).await;
                        }
                    }
                }
            }
            AccountType::UBased => {
                let client = self
                    .ubase
                    .as_ref()
                    .ok_or_else(|| anyhow!("ubase client not initialized"))?;
                client.cancel_all_orders(symbol).await?;
            }
            AccountType::CBased => {
                let client = self
                    .cbase
                    .as_ref()
                    .ok_or_else(|| anyhow!("cbase client not initialized"))?;
                client.cancel_all_orders(symbol).await?;
            }
        }

        info!(
            "[binance-td] all orders cancelled: {} {}",
            Self::account_label(account),
            symbol,
        );
        Ok(())
    }

    async fn query_open_orders(&self) -> Result<Vec<OrderUpdate>> {
        let mut result = Vec::new();

        // Query each enabled account separately (different future types).
        if let Some(ref client) = self.spot {
            match client.get_open_orders(None).await {
                Ok(val) => collect_order_updates(&val, &mut result),
                Err(e) => warn!("[binance-td] query open orders (spot) failed: {e}"),
            }
        }
        if let Some(ref client) = self.ubase {
            match client.get_open_orders(None).await {
                Ok(val) => collect_order_updates(&val, &mut result),
                Err(e) => warn!("[binance-td] query open orders (ubase) failed: {e}"),
            }
        }
        if let Some(ref client) = self.cbase {
            match client.get_open_orders(None).await {
                Ok(val) => collect_order_updates(&val, &mut result),
                Err(e) => warn!("[binance-td] query open orders (cbase) failed: {e}"),
            }
        }

        Ok(result)
    }

    async fn query_positions(&self) -> Result<Vec<Position>> {
        let mut result = Vec::new();

        if let Some(ref client) = self.ubase {
            match client.get_positions(None).await {
                Ok(val) => collect_positions(&val, "ubase", &mut result),
                Err(e) => warn!("[binance-td] query positions (ubase) failed: {e}"),
            }
        }
        if let Some(ref client) = self.cbase {
            match client.get_positions(None).await {
                Ok(val) => collect_positions(&val, "cbase", &mut result),
                Err(e) => warn!("[binance-td] query positions (cbase) failed: {e}"),
            }
        }

        Ok(result)
    }

    async fn stop(&mut self) -> Result<()> {
        // Abort all background tasks
        for task in self.tasks.drain(..) {
            task.abort();
        }

        // Shutdown sub-clients (close listen keys, abort WS tasks)
        if let Some(ref spot) = self.spot {
            spot.shutdown().await;
        }
        if let Some(ref ubase) = self.ubase {
            ubase.shutdown().await;
        }
        if let Some(ref cbase) = self.cbase {
            cbase.shutdown().await;
        }

        info!("[binance-td] stopped");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// JSON → typed helpers
// ---------------------------------------------------------------------------

/// Extract order updates from a JSON array value into the result vector.
fn collect_order_updates(val: &serde_json::Value, result: &mut Vec<OrderUpdate>) {
    if let Some(arr) = val.as_array() {
        for o in arr {
            if let Some(update) = parse_order_update(o) {
                result.push(update);
            }
        }
    }
}

/// Extract positions from a JSON array value into the result vector.
fn collect_positions(val: &serde_json::Value, label: &str, result: &mut Vec<Position>) {
    if let Some(arr) = val.as_array() {
        for p in arr {
            if let Some(pos) = parse_position(p, label) {
                result.push(pos);
            }
        }
    }
}

/// Parse a Binance order JSON object into an [`OrderUpdate`].
fn parse_order_update(v: &serde_json::Value) -> Option<OrderUpdate> {
    Some(OrderUpdate {
        symbol: v.get("symbol")?.as_str()?.to_string(),
        order_id: v.get("orderId")?.as_u64()?,
        client_order_id: v
            .get("clientOrderId")
            .and_then(|c| c.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0),
        strategy_id: 0,
        status: parse_order_status(v.get("status")?.as_str()?),
        direction: if v.get("side")?.as_str()? == "BUY" {
            k4_core::enums::Direction::Buy
        } else {
            k4_core::enums::Direction::Sell
        },
        price: v
            .get("price")
            .and_then(|p| p.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        quantity: v
            .get("origQty")
            .and_then(|q| q.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        filled_quantity: v
            .get("executedQty")
            .and_then(|q| q.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        filled_avg_price: 0.0,
        commission: 0.0,
        update_time: v.get("updateTime").and_then(|t| t.as_u64()).unwrap_or(0),
    })
}

/// Parse a Binance position JSON object into a [`Position`].
fn parse_position(v: &serde_json::Value, label: &str) -> Option<Position> {
    let amt: f64 = v
        .get("positionAmt")
        .and_then(|a| a.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    // Skip zero positions
    if amt.abs() < 1e-12 {
        return None;
    }

    let account_type = match label {
        "ubase" => AccountType::UBased,
        "cbase" => AccountType::CBased,
        _ => AccountType::Spot,
    };

    Some(Position {
        symbol: v.get("symbol")?.as_str()?.to_string(),
        account_type,
        position_amt: amt,
        entry_price: v
            .get("entryPrice")
            .and_then(|p| p.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        unrealized_pnl: v
            .get("unRealizedProfit")
            .and_then(|p| p.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
    })
}

/// Map a Binance order status string to an [`OrderStatus`] enum.
fn parse_order_status(status: &str) -> k4_core::enums::OrderStatus {
    match status {
        "NEW" => k4_core::enums::OrderStatus::New,
        "PARTIALLY_FILLED" => k4_core::enums::OrderStatus::PartiallyFilled,
        "FILLED" => k4_core::enums::OrderStatus::Filled,
        "CANCELED" => k4_core::enums::OrderStatus::Canceled,
        "REJECTED" => k4_core::enums::OrderStatus::Rejected,
        "EXPIRED" | "EXPIRED_IN_MATCH" => k4_core::enums::OrderStatus::Expired,
        "PENDING_CANCEL" => k4_core::enums::OrderStatus::PendingCancel,
        _ => k4_core::enums::OrderStatus::New,
    }
}
