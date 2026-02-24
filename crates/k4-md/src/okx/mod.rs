//! OKX market data — stream definitions.
//!
//! Produces up to 2 [`StreamDef`]s (same URL, different subscriptions):
//! - Spot — bbo-tbt, trades, books5
//! - Swap — bbo-tbt, trades, books5

pub mod config;
pub mod json_parser;

use std::time::Duration;

use anyhow::Result;
use k4_core::{config::ConnectionConfig, ws::PingPayload};

use self::config::OkxConfig;
use crate::pipeline::{PingConfig, ShmNames, StreamDef};

const OKX_WS_URL: &str = "wss://ws.okx.com:8443/ws/v5/public";

/// Build OKX stream definitions from the connection config.
pub fn build(conn_config: &ConnectionConfig) -> Result<Vec<StreamDef>> {
    let cfg = OkxConfig::from_connection(conn_config)?;
    let ping =
        PingConfig { interval: Duration::from_secs(cfg.ping_interval_sec), payload: PingPayload::Text("ping".into()) };
    let mut streams = Vec::new();

    if !cfg.spot_symbols.is_empty() {
        streams.push(StreamDef {
            label: "okx_spot".into(),
            ws_url: OKX_WS_URL.into(),
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
            text_parser: Some(Box::new(|data| json_parser::parse_message(data).into_iter().collect())),
            binary_parser: None,
            custom_trade_dedup: None,
            dedup_cpu_core: None,
        });
    }

    if !cfg.swap_symbols.is_empty() {
        streams.push(StreamDef {
            label: "okx_swap".into(),
            ws_url: OKX_WS_URL.into(),
            subscribe_msg: json_parser::build_swap_subscribe(&cfg.swap_symbols),
            ping: Some(ping.clone()),
            extra_headers: Default::default(),
            shm: ShmNames {
                bbo: cfg.swap_bbo_shm_name.clone(),
                trade: cfg.swap_trade_shm_name.clone(),
                depth5: cfg.swap_depth5_shm_name.clone(),
                ..Default::default()
            },
            symbols: cfg.swap_symbols.clone(),
            md_size: cfg.md_size,
            text_parser: Some(Box::new(|data| json_parser::parse_message(data).into_iter().collect())),
            binary_parser: None,
            custom_trade_dedup: None,
            dedup_cpu_core: None,
        });
    }

    Ok(streams)
}
