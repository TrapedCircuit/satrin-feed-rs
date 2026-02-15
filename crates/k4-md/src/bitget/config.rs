//! Bitget-specific configuration extraction.
//!
//! Handles conversion from the generic [`ConnectionConfig`] to Bitget-specific
//! settings. Bitget uses standard symbol format (`BTCUSDT`) natively.

use anyhow::Result;
use k4_core::config::ConnectionConfig;

/// Parsed Bitget configuration.
#[derive(Debug, Clone)]
pub struct BitgetConfig {
    /// SHM buffer size per symbol.
    pub md_size: u32,
    /// Spot symbols (e.g. `"BTCUSDT"`).
    pub spot_symbols: Vec<String>,
    /// Futures symbols (e.g. `"BTCUSDT"`).
    pub futures_symbols: Vec<String>,
    /// Number of redundant spot WebSocket connections.
    pub spot_conn_count: u32,
    /// Number of redundant futures WebSocket connections.
    pub futures_conn_count: u32,

    // SHM names
    /// SHM name for spot BookTicker data.
    pub spot_bbo_shm_name: Option<String>,
    /// SHM name for spot Trade data.
    pub spot_trade_shm_name: Option<String>,
    /// SHM name for spot Depth5 data.
    pub spot_depth5_shm_name: Option<String>,
    /// SHM name for futures BookTicker data.
    pub futures_bbo_shm_name: Option<String>,
    /// SHM name for futures Trade data.
    pub futures_trade_shm_name: Option<String>,
    /// SHM name for futures Depth5 data.
    pub futures_depth5_shm_name: Option<String>,

    /// Ping interval in seconds (default: 25).
    pub ping_interval_sec: u64,
}

impl BitgetConfig {
    /// Extract Bitget config from a [`ConnectionConfig`].
    pub fn from_connection(conn: &ConnectionConfig) -> Result<Self> {
        let md_size = conn.effective_md_size();
        let ping_interval_sec = conn.ping_interval_sec.unwrap_or(25);

        // Spot config
        let (spot_symbols, spot_conn_count, spot_bbo, spot_trade, spot_depth5) = if let Some(ref spot) = conn.spot {
            (
                spot.symbols.clone().unwrap_or_default(),
                spot.redun_conn_count.unwrap_or(1),
                spot.bbo_shm_name.clone(),
                spot.trade_shm_name.clone(),
                spot.depth5_shm_name.clone(),
            )
        } else {
            (vec![], 1, None, None, None)
        };

        // Futures config
        let (futures_symbols, futures_conn_count, fut_bbo, fut_trade, fut_depth5) = if let Some(ref fut) = conn.futures
        {
            (
                fut.effective_symbols(),
                fut.effective_conn_count(),
                fut.bbo_shm_name.clone(),
                fut.trade_shm_name.clone(),
                fut.depth5_shm_name.clone(),
            )
        } else {
            (vec![], 1, None, None, None)
        };

        Ok(Self {
            md_size,
            spot_symbols,
            futures_symbols,
            spot_conn_count,
            futures_conn_count,
            spot_bbo_shm_name: spot_bbo,
            spot_trade_shm_name: spot_trade,
            spot_depth5_shm_name: spot_depth5,
            futures_bbo_shm_name: fut_bbo,
            futures_trade_shm_name: fut_trade,
            futures_depth5_shm_name: fut_depth5,
            ping_interval_sec,
        })
    }
}
