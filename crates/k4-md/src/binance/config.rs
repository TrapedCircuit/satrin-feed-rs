//! Binance-specific configuration extraction.

use anyhow::Result;
use k4_core::config::ConnectionConfig;
use std::collections::HashMap;

/// Parsed Binance configuration.
#[derive(Debug, Clone)]
pub struct BinanceConfig {
    pub md_size: u32,
    pub spot_symbols: Vec<String>,
    pub ubase_symbols: Vec<String>,
    pub spot_conn_count: u32,
    pub ubase_conn_count: u32,

    // SHM names
    pub spot_bbo_shm_name: Option<String>,
    pub spot_agg_shm_name: Option<String>,
    pub spot_trade_shm_name: Option<String>,
    pub spot_depth5_shm_name: Option<String>,
    pub ubase_bbo_shm_name: Option<String>,
    pub ubase_agg_shm_name: Option<String>,
    pub ubase_trade_shm_name: Option<String>,
    pub ubase_depth5_shm_name: Option<String>,

    // Extra HTTP headers
    pub spot_extra_headers: HashMap<String, String>,
    pub ubase_extra_headers: HashMap<String, String>,

    // Redundancy
    pub hb_interval_sec: u64,
    pub reset_on_hb: bool,
    pub reset_threshold: u64,
}

impl BinanceConfig {
    pub fn from_connection(conn: &ConnectionConfig) -> Result<Self> {
        let md_size = conn.effective_md_size();

        // Spot config
        let (
            spot_symbols,
            spot_conn_count,
            spot_bbo,
            spot_agg,
            spot_trade,
            spot_depth5,
            spot_headers,
        ) = if let Some(ref spot) = conn.spot {
            (
                spot.symbols.clone().unwrap_or_default(),
                spot.redun_conn_count.unwrap_or(1),
                spot.bbo_shm_name.clone(),
                spot.aggtrade_shm_name.clone(),
                spot.trade_shm_name.clone(),
                spot.depth5_shm_name.clone(),
                spot.extra_headers.clone().unwrap_or_default(),
            )
        } else {
            (vec![], 1, None, None, None, None, HashMap::new())
        };

        // Futures/UBase config
        let (ubase_symbols, ubase_conn_count, ub_bbo, ub_agg, ub_trade, ub_depth5, ub_headers) =
            if let Some(ref fut) = conn.futures {
                (
                    fut.effective_symbols(),
                    fut.effective_conn_count(),
                    fut.bbo_shm_name.clone(),
                    fut.aggtrade_shm_name.clone(),
                    fut.trade_shm_name.clone(),
                    fut.depth5_shm_name.clone(),
                    fut.extra_headers.clone().unwrap_or_default(),
                )
            } else {
                (vec![], 1, None, None, None, None, HashMap::new())
            };

        Ok(Self {
            md_size,
            spot_symbols,
            ubase_symbols,
            spot_conn_count,
            ubase_conn_count,
            spot_bbo_shm_name: spot_bbo,
            spot_agg_shm_name: spot_agg,
            spot_trade_shm_name: spot_trade,
            spot_depth5_shm_name: spot_depth5,
            ubase_bbo_shm_name: ub_bbo,
            ubase_agg_shm_name: ub_agg,
            ubase_trade_shm_name: ub_trade,
            ubase_depth5_shm_name: ub_depth5,
            spot_extra_headers: spot_headers,
            ubase_extra_headers: ub_headers,
            hb_interval_sec: conn.hb_interval_sec.unwrap_or(30),
            reset_on_hb: conn.redun_reset_on_hb.unwrap_or(false),
            reset_threshold: conn.redun_reset_on_threshold.unwrap_or(10_000),
        })
    }
}
