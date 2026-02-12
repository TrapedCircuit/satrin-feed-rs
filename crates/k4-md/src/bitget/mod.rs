//! Bitget market data — stream definitions.
//!
//! Produces up to 2 [`StreamDef`]s (same URL, different subscriptions):
//! - Spot (`instType: "SPOT"`) — books1, trade, books5
//! - Futures (`instType: "USDT-FUTURES"`) — books1, trade, books5

pub mod config;
pub mod json_parser;

use std::time::Duration;

use anyhow::Result;
use k4_core::config::ConnectionConfig;
use k4_core::ws::PingPayload;

use self::config::BitgetConfig;
use crate::pipeline::{PingConfig, ShmNames, StreamDef};

const BITGET_WS_URL: &str = "wss://ws.bitget.com:443/v2/ws/public";

/// Build Bitget stream definitions from the connection config.
pub fn build(conn_config: &ConnectionConfig) -> Result<Vec<StreamDef>> {
    let cfg = BitgetConfig::from_connection(conn_config)?;
    let ping = PingConfig {
        interval: Duration::from_secs(cfg.ping_interval_sec),
        payload: PingPayload::Text("ping".into()),
    };
    let mut streams = Vec::new();

    if !cfg.spot_symbols.is_empty() {
        streams.push(StreamDef {
            label: "bitget_spot".into(),
            ws_url: BITGET_WS_URL.into(),
            subscribe_msg: json_parser::build_spot_subscribe(&cfg.spot_symbols),
            ping: Some(ping.clone()),
            extra_headers: Default::default(),
            shm: ShmNames {
                bbo: cfg.spot_bbo_shm_name.clone(),
                trade: cfg.spot_trade_shm_name.clone(),
                depth5: cfg.spot_depth5_shm_name.clone(),
                ..Default::default()
            },
            symbols: cfg.spot_symbols.clone(),
            md_size: cfg.md_size,
            text_parser: Some(Box::new(json_parser::parse_message)),
            binary_parser: None,
            custom_trade_dedup: None,
            dedup_cpu_core: None,
        });
    }

    if !cfg.futures_symbols.is_empty() {
        streams.push(StreamDef {
            label: "bitget_futures".into(),
            ws_url: BITGET_WS_URL.into(),
            subscribe_msg: json_parser::build_futures_subscribe(&cfg.futures_symbols),
            ping: Some(ping.clone()),
            extra_headers: Default::default(),
            shm: ShmNames {
                bbo: cfg.futures_bbo_shm_name.clone(),
                trade: cfg.futures_trade_shm_name.clone(),
                depth5: cfg.futures_depth5_shm_name.clone(),
                ..Default::default()
            },
            symbols: cfg.futures_symbols.clone(),
            md_size: cfg.md_size,
            text_parser: Some(Box::new(json_parser::parse_message)),
            binary_parser: None,
            custom_trade_dedup: None,
            dedup_cpu_core: None,
        });
    }

    Ok(streams)
}
