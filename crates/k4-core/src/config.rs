//! Configuration parsing for the crypto-gateway system.
//!
//! All modules read their settings from a single JSON config file. The top-level
//! structure contains logging metadata and a `connections` array where each entry
//! describes one exchange module instance.
//!
//! # Example config (Binance)
//!
//! ```json
//! {
//!   "RazorTrade": { "module_name": "binance_md", "log_path": "/tmp/log" },
//!   "connections": [{
//!     "lib_path": "...",
//!     "exchange": "binance",
//!     "md_size": 100000,
//!     "spot": { "symbols": ["BTCUSDT"], "conn_count": 2, ... },
//!     "futures": { "ubase_symbols": ["BTCUSDT"], ... }
//!   }]
//! }
//! ```

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level application config, deserialized from a JSON file.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// Module metadata (name, log path).
    #[serde(rename = "RazorTrade")]
    pub razor_trade: Option<ModuleMeta>,

    /// Array of connection configs — one per exchange module instance.
    pub connections: Vec<ConnectionConfig>,
}

/// Module metadata block.
#[derive(Debug, Clone, Deserialize)]
pub struct ModuleMeta {
    pub module_name: Option<String>,
    pub log_path: Option<String>,
}

/// A single connection/module configuration.
///
/// Each connection maps to one exchange and can contain spot, futures, and/or
/// swap product configs.
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionConfig {
    /// Module metadata override (per-connection).
    #[serde(rename = "RazorTrade")]
    pub razor_trade: Option<ModuleMeta>,

    /// Path to the dynamic library (unused in Rust — kept for config compat).
    pub lib_path: Option<String>,

    /// Exchange identifier: `"binance"`, `"okx"`, `"bitget"`, `"bybit"`, `"udp"`.
    pub exchange: String,

    /// Shared memory buffer size per symbol (default: 100_000).
    pub md_size: Option<u32>,

    /// Alternative name for md_size used by some exchange configs.
    pub shm_block_num: Option<u32>,

    /// Optional prefix for SHM names.
    pub shm_prefix: Option<String>,

    /// Heartbeat interval in seconds (triggers redundancy evaluation).
    pub hb_interval_sec: Option<u64>,

    /// Ping interval in seconds (exchange-level keep-alive).
    pub ping_interval_sec: Option<u64>,

    /// Whether to reset the slowest connection on each heartbeat.
    pub redun_reset_on_hb: Option<bool>,

    /// After this many data points, evaluate and reset slowest connection.
    pub redun_reset_on_threshold: Option<u64>,

    /// Latency print interval in milliseconds.
    pub latency_print_interval_ms: Option<u64>,

    /// Spot product configuration.
    pub spot: Option<ProductConfig>,

    /// Futures product configuration (Binance UBase/CBase, Bitget/Bybit).
    pub futures: Option<FuturesConfig>,

    /// Swap product configuration (OKX uses "swap" instead of "futures").
    pub swap: Option<ProductConfig>,

    /// UDP sender configuration (optional forwarding).
    pub udp_sender: Option<UdpSenderConfig>,

    /// UDP receiver configuration (for `udp` exchange type).
    pub udp_receiver: Option<UdpReceiverConfig>,
}

impl ConnectionConfig {
    /// Returns the effective SHM buffer size, checking both `md_size` and
    /// `shm_block_num`, defaulting to 100_000.
    pub fn effective_md_size(&self) -> u32 {
        self.md_size.or(self.shm_block_num).unwrap_or(100_000)
    }

    /// Returns the module name from the per-connection or top-level config.
    pub fn module_name(&self) -> String {
        self.razor_trade.as_ref().and_then(|m| m.module_name.clone()).unwrap_or_else(|| self.exchange.clone())
    }

    /// Returns the log path.
    pub fn log_path(&self) -> Option<String> {
        self.razor_trade.as_ref().and_then(|m| m.log_path.clone())
    }
}

/// Product configuration for a single product type (spot or swap).
#[derive(Debug, Clone, Deserialize)]
pub struct ProductConfig {
    /// Symbols to subscribe (e.g. `["BTCUSDT", "ETHUSDT"]`).
    pub symbols: Option<Vec<String>>,

    /// Number of redundant connections (default: 1).
    #[serde(alias = "conn_count")]
    pub redun_conn_count: Option<u32>,

    /// CPU cores for connection threads.
    #[serde(alias = "cpu_affinity")]
    pub cpu_affinity_conn: Option<Vec<i32>>,

    /// CPU cores for SBE connection threads (Binance only).
    pub cpu_affinity_sbe: Option<Vec<i32>>,

    /// CPU core for the dedup thread.
    pub cpu_affinity_dedup: Option<i32>,

    /// CPU core for SBE dedup thread (Binance only).
    pub cpu_affinity_dedup_sbe: Option<i32>,

    /// SHM name for BookTicker data.
    pub bbo_shm_name: Option<String>,

    /// SHM name for Trade data.
    pub trade_shm_name: Option<String>,

    /// SHM name for AggTrade data.
    pub aggtrade_shm_name: Option<String>,

    /// SHM name for Depth5 data.
    pub depth5_shm_name: Option<String>,

    /// Extra HTTP headers for the WebSocket handshake (e.g. API key).
    pub extra_headers: Option<HashMap<String, String>>,
}

/// Futures configuration — handles both Binance-style (ubase/cbase) and
/// generic (symbols) configs.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesConfig {
    // --- Generic (used by Bitget, Bybit) ---
    pub symbols: Option<Vec<String>>,
    pub redun_conn_count: Option<u32>,
    pub cpu_affinity_conn: Option<Vec<i32>>,
    pub cpu_affinity_dedup: Option<i32>,

    // --- Binance-specific ---
    pub ubase_symbols: Option<Vec<String>>,
    pub ubase_conn_count: Option<u32>,
    pub cbase_symbols: Option<Vec<String>>,
    pub cbase_conn_count: Option<u32>,

    // --- SHM names ---
    pub bbo_shm_name: Option<String>,
    pub trade_shm_name: Option<String>,
    pub aggtrade_shm_name: Option<String>,
    pub depth5_shm_name: Option<String>,

    pub extra_headers: Option<HashMap<String, String>>,
}

impl FuturesConfig {
    /// Returns the effective symbol list, checking both generic and Binance fields.
    pub fn effective_symbols(&self) -> Vec<String> {
        self.symbols.clone().or_else(|| self.ubase_symbols.clone()).unwrap_or_default()
    }

    /// Returns the effective redundant connection count.
    pub fn effective_conn_count(&self) -> u32 {
        self.redun_conn_count.or(self.ubase_conn_count).unwrap_or(1)
    }
}

/// UDP sender configuration for optional market data forwarding.
#[derive(Debug, Clone, Deserialize)]
pub struct UdpSenderConfig {
    pub ip: String,
    pub port: u16,
    pub cpu_affinity: Option<i32>,
    pub enabled: Option<bool>,
}

impl UdpSenderConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }
}

/// UDP receiver configuration (for the `udp` exchange module).
#[derive(Debug, Clone, Deserialize)]
pub struct UdpReceiverConfig {
    pub ip: String,
    pub port: u16,
    pub recv_cpu_affinity: Option<i32>,

    pub spot_symbols: Option<Vec<String>>,
    pub ubase_symbols: Option<Vec<String>>,

    pub spot_bbo_shm_name: Option<String>,
    pub spot_agg_shm_name: Option<String>,
    pub spot_trade_shm_name: Option<String>,
    pub spot_depth5_shm_name: Option<String>,

    pub ubase_bbo_shm_name: Option<String>,
    pub ubase_agg_shm_name: Option<String>,
    pub ubase_trade_shm_name: Option<String>,
    pub ubase_depth5_shm_name: Option<String>,
}

/// Load and parse a JSON config file.
pub fn load_config(path: &std::path::Path) -> anyhow::Result<AppConfig> {
    let content = std::fs::read_to_string(path)?;
    let config: AppConfig = serde_json::from_str(&content)?;
    Ok(config)
}
