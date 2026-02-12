//! OKX-specific configuration extraction.
//!
//! Handles conversion from the generic [`ConnectionConfig`] to OKX-specific
//! settings, including symbol format conversion (`BTCUSDT` → `BTC-USDT`).

use anyhow::Result;
use k4_core::config::ConnectionConfig;

/// Parsed OKX configuration.
#[derive(Debug, Clone)]
pub struct OkxConfig {
    /// SHM buffer size per symbol.
    pub md_size: u32,
    /// Spot symbols in OKX format (e.g. `"BTC-USDT"`).
    pub spot_symbols: Vec<String>,
    /// Swap symbols in OKX format (e.g. `"BTC-USDT-SWAP"`).
    pub swap_symbols: Vec<String>,
    /// Number of redundant spot WebSocket connections.
    pub spot_conn_count: u32,
    /// Number of redundant swap WebSocket connections.
    pub swap_conn_count: u32,

    // SHM names
    /// SHM name for spot BookTicker data.
    pub spot_bbo_shm_name: Option<String>,
    /// SHM name for spot Trade data.
    pub spot_trade_shm_name: Option<String>,
    /// SHM name for spot Depth5 data.
    pub spot_depth5_shm_name: Option<String>,
    /// SHM name for swap BookTicker data.
    pub swap_bbo_shm_name: Option<String>,
    /// SHM name for swap Trade data.
    pub swap_trade_shm_name: Option<String>,
    /// SHM name for swap Depth5 data.
    pub swap_depth5_shm_name: Option<String>,

    /// Ping interval in seconds (default: 25).
    pub ping_interval_sec: u64,
}

impl OkxConfig {
    /// Extract OKX config from a [`ConnectionConfig`].
    ///
    /// Symbols are converted from standard format (`BTCUSDT`) to OKX format
    /// (`BTC-USDT` for spot, `BTC-USDT-SWAP` for swap).
    pub fn from_connection(conn: &ConnectionConfig) -> Result<Self> {
        let md_size = conn.effective_md_size();
        let ping_interval_sec = conn.ping_interval_sec.unwrap_or(25);

        // Spot config
        let (spot_symbols, spot_conn_count, spot_bbo, spot_trade, spot_depth5) =
            if let Some(ref spot) = conn.spot {
                let raw = spot.symbols.clone().unwrap_or_default();
                let converted: Vec<String> = raw.iter().map(|s| to_okx_inst_id(s)).collect();
                (
                    converted,
                    spot.redun_conn_count.unwrap_or(1),
                    spot.bbo_shm_name.clone(),
                    spot.trade_shm_name.clone(),
                    spot.depth5_shm_name.clone(),
                )
            } else {
                (vec![], 1, None, None, None)
            };

        // Swap config (OKX uses "swap" section instead of "futures")
        let (swap_symbols, swap_conn_count, swap_bbo, swap_trade, swap_depth5) =
            if let Some(ref swap) = conn.swap {
                let raw = swap.symbols.clone().unwrap_or_default();
                let converted: Vec<String> = raw.iter().map(|s| to_okx_swap_inst_id(s)).collect();
                (
                    converted,
                    swap.redun_conn_count.unwrap_or(1),
                    swap.bbo_shm_name.clone(),
                    swap.trade_shm_name.clone(),
                    swap.depth5_shm_name.clone(),
                )
            } else {
                (vec![], 1, None, None, None)
            };

        Ok(Self {
            md_size,
            spot_symbols,
            swap_symbols,
            spot_conn_count,
            swap_conn_count,
            spot_bbo_shm_name: spot_bbo,
            spot_trade_shm_name: spot_trade,
            spot_depth5_shm_name: spot_depth5,
            swap_bbo_shm_name: swap_bbo,
            swap_trade_shm_name: swap_trade,
            swap_depth5_shm_name: swap_depth5,
            ping_interval_sec,
        })
    }
}

/// Convert a standard symbol (e.g. `BTCUSDT`) to OKX spot instId (`BTC-USDT`).
///
/// Tries common quote currencies in order. If no match is found, returns the
/// input unchanged (assumes it is already in OKX format).
pub fn to_okx_inst_id(symbol: &str) -> String {
    // If it already contains a hyphen, assume it's already in OKX format.
    if symbol.contains('-') {
        return symbol.to_string();
    }
    const QUOTES: &[&str] = &["USDT", "USDC", "BTC", "ETH", "BUSD", "DAI"];
    for q in QUOTES {
        if let Some(base) = symbol.strip_suffix(q) {
            if !base.is_empty() {
                return format!("{base}-{q}");
            }
        }
    }
    symbol.to_string()
}

/// Convert a standard symbol to OKX swap instId (`BTC-USDT-SWAP`).
pub fn to_okx_swap_inst_id(symbol: &str) -> String {
    format!("{}-SWAP", to_okx_inst_id(symbol))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_conversion_spot() {
        assert_eq!(to_okx_inst_id("BTCUSDT"), "BTC-USDT");
        assert_eq!(to_okx_inst_id("ETHUSDC"), "ETH-USDC");
        assert_eq!(to_okx_inst_id("BTC-USDT"), "BTC-USDT"); // already OKX format
    }

    #[test]
    fn symbol_conversion_swap() {
        assert_eq!(to_okx_swap_inst_id("BTCUSDT"), "BTC-USDT-SWAP");
        assert_eq!(to_okx_swap_inst_id("ETHUSDT"), "ETH-USDT-SWAP");
    }
}
