//! Binance trading module configuration.
//!
//! Holds API credentials, endpoint URLs, and per-account enable flags.
//! All URL fields have sensible production defaults so only `api_key`,
//! `secret_key`, and the desired account flags need to be specified.

use serde::Deserialize;

/// Configuration for the Binance trading module.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceTdConfig {
    /// Binance API key.
    pub api_key: String,

    /// Binance API secret (HMAC-SHA256 signing).
    pub secret_key: String,

    /// Optional path to a PEM-encoded Ed25519 private key for Ed25519 signing.
    #[serde(default)]
    pub ed25519_key_path: Option<String>,

    // -- Account toggles --
    /// Enable spot account trading.
    #[serde(default)]
    pub spot_enabled: bool,

    /// Enable USDT-margined futures (UBase) trading.
    #[serde(default)]
    pub ubase_enabled: bool,

    /// Enable coin-margined futures (CBase) trading.
    #[serde(default)]
    pub cbase_enabled: bool,

    // -- REST base URLs --
    /// Spot REST API base URL.
    #[serde(default = "default_spot_rest_url")]
    pub spot_rest_url: String,

    /// UBase (USDT-margined futures) REST API base URL.
    #[serde(default = "default_ubase_rest_url")]
    pub ubase_rest_url: String,

    /// CBase (coin-margined futures) REST API base URL.
    #[serde(default = "default_cbase_rest_url")]
    pub cbase_rest_url: String,

    // -- WebSocket user-data stream URLs --
    /// Spot user-data stream WebSocket URL.
    #[serde(default = "default_spot_ws_url")]
    pub spot_ws_url: String,

    /// UBase user-data stream WebSocket URL.
    #[serde(default = "default_ubase_ws_url")]
    pub ubase_ws_url: String,

    /// CBase user-data stream WebSocket URL.
    #[serde(default = "default_cbase_ws_url")]
    pub cbase_ws_url: String,

    // -- WebSocket API URL (for order placement) --
    /// Spot WebSocket API URL for order operations.
    #[serde(default = "default_spot_ws_api_url")]
    pub spot_ws_api_url: String,

    // -- Timing --
    /// `recvWindow` for signed requests (milliseconds, 0 = Binance default).
    #[serde(default = "default_recv_window")]
    pub recv_window: u64,

    /// Listen-key keepalive interval in seconds.
    #[serde(default = "default_listen_key_interval")]
    pub listen_key_refresh_secs: u64,
}

impl Default for BinanceTdConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            secret_key: String::new(),
            ed25519_key_path: None,
            spot_enabled: false,
            ubase_enabled: false,
            cbase_enabled: false,
            spot_rest_url: default_spot_rest_url(),
            ubase_rest_url: default_ubase_rest_url(),
            cbase_rest_url: default_cbase_rest_url(),
            spot_ws_url: default_spot_ws_url(),
            ubase_ws_url: default_ubase_ws_url(),
            cbase_ws_url: default_cbase_ws_url(),
            spot_ws_api_url: default_spot_ws_api_url(),
            recv_window: default_recv_window(),
            listen_key_refresh_secs: default_listen_key_interval(),
        }
    }
}

// ---------------------------------------------------------------------------
// Default URL helpers (used by serde)
// ---------------------------------------------------------------------------

fn default_spot_rest_url() -> String {
    "https://api.binance.com".into()
}

fn default_ubase_rest_url() -> String {
    "https://fapi.binance.com".into()
}

fn default_cbase_rest_url() -> String {
    "https://dapi.binance.com".into()
}

fn default_spot_ws_url() -> String {
    "wss://stream.binance.com:9443/ws".into()
}

fn default_ubase_ws_url() -> String {
    "wss://fstream.binance.com/ws".into()
}

fn default_cbase_ws_url() -> String {
    "wss://dstream.binance.com/ws".into()
}

fn default_spot_ws_api_url() -> String {
    "wss://ws-api.binance.com:443/ws-api/v3".into()
}

fn default_recv_window() -> u64 {
    5000
}

fn default_listen_key_interval() -> u64 {
    1800 // 30 minutes (Binance recommends refresh every 30 min, key expires at 60 min)
}
