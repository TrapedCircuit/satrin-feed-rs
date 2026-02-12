//! Binance market data — stream definitions.
//!
//! Produces up to 3 [`StreamDef`]s:
//! - Spot JSON (`stream.binance.com`) — aggTrade
//! - Spot SBE (`stream-sbe.binance.com`) — bookTicker, trade, depth (binary)
//! - UBase JSON (`fstream.binance.com`) — aggTrade, bookTicker, trade, depth5

pub mod config;
pub mod json_parser;
pub mod sbe_parser;

use anyhow::Result;
use k4_core::config::ConnectionConfig;

use self::config::BinanceConfig;
use crate::pipeline::{ShmNames, StreamDef};

/// Build Binance stream definitions from the connection config.
pub fn build(conn_config: &ConnectionConfig) -> Result<Vec<StreamDef>> {
    let cfg = BinanceConfig::from_connection(conn_config)?;
    let mut streams = Vec::new();

    // --- Spot streams ---
    if !cfg.spot_symbols.is_empty() {
        // Stream 1: Spot JSON (aggTrade only)
        streams.push(StreamDef {
            label: "binance_spot_json".into(),
            ws_url: "wss://stream.binance.com:443/ws".into(),
            subscribe_msg: json_parser::build_spot_json_subscribe(&cfg.spot_symbols),
            ping: None,
            extra_headers: cfg.spot_extra_headers.clone(),
            shm: ShmNames {
                agg: cfg.spot_agg_shm_name.clone(),
                ..Default::default()
            },
            symbols: cfg.spot_symbols.clone(),
            md_size: cfg.md_size,
            text_parser: Some(Box::new(|text| {
                json_parser::parse_message(text).into_iter().collect()
            })),
            binary_parser: None,
            custom_trade_dedup: None,
            dedup_cpu_core: None,
        });

        // Stream 2: Spot SBE (bbo, trade, depth — binary protocol)
        streams.push(StreamDef {
            label: "binance_spot_sbe".into(),
            ws_url: "wss://stream-sbe.binance.com:9443/stream".into(),
            subscribe_msg: json_parser::build_spot_sbe_subscribe(&cfg.spot_symbols),
            ping: None,
            extra_headers: cfg.spot_extra_headers.clone(),
            shm: ShmNames {
                bbo: cfg.spot_bbo_shm_name.clone(),
                trade: cfg.spot_trade_shm_name.clone(),
                depth5: cfg.spot_depth5_shm_name.clone(),
                ..Default::default()
            },
            symbols: cfg.spot_symbols.clone(),
            md_size: cfg.md_size,
            text_parser: None,
            binary_parser: Some(Box::new(sbe_parser::parse_sbe_message)),
            custom_trade_dedup: None,
            dedup_cpu_core: None,
        });
    }

    // --- UBase stream ---
    if !cfg.ubase_symbols.is_empty() {
        streams.push(StreamDef {
            label: "binance_ubase".into(),
            ws_url: "wss://fstream.binance.com:443/ws".into(),
            subscribe_msg: json_parser::build_ubase_subscribe(&cfg.ubase_symbols),
            ping: None,
            extra_headers: cfg.ubase_extra_headers.clone(),
            shm: ShmNames {
                bbo: cfg.ubase_bbo_shm_name.clone(),
                agg: cfg.ubase_agg_shm_name.clone(),
                trade: cfg.ubase_trade_shm_name.clone(),
                depth5: cfg.ubase_depth5_shm_name.clone(),
            },
            symbols: cfg.ubase_symbols.clone(),
            md_size: cfg.md_size,
            text_parser: Some(Box::new(|text| {
                json_parser::parse_message(text).into_iter().collect()
            })),
            binary_parser: None,
            custom_trade_dedup: None,
            dedup_cpu_core: None,
        });
    }

    Ok(streams)
}
