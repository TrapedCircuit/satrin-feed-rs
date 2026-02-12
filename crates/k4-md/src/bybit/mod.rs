//! Bybit market data — stream definitions.
//!
//! Produces up to 2 [`StreamDef`]s:
//! - Spot (`/v5/public/spot`) — publicTrade, orderbook.1, orderbook.50
//! - Futures (`/v5/public/linear`) — publicTrade, orderbook.1, orderbook.50
//!
//! Bybit is the most complex exchange due to:
//! - Incremental `orderbook.50` requiring local [`OrderBook`] state
//! - UUID-based trade IDs on futures (vs numeric on spot)
//!
//! Both complexities are handled via **stateful parser closures** that capture
//! the order book and UUID dedup state, producing standard `MarketDataMsg`
//! output compatible with the generic pipeline.

pub mod config;
pub mod json_parser;
pub mod order_book;
pub mod uuid_dedup;

use std::sync::Mutex;
use std::time::Duration;

use ahash::AHashMap;
use anyhow::Result;
use k4_core::config::ConnectionConfig;
use k4_core::dedup::UuidDedup;
use k4_core::types::*;
use k4_core::ws::PingPayload;

use self::config::BybitConfig;
use self::order_book::OrderBook;
use crate::pipeline::{PingConfig, ShmNames, StreamDef};

const BYBIT_SPOT_WS_URL: &str = "wss://stream.bybit.com:443/v5/public/spot";
const BYBIT_LINEAR_WS_URL: &str = "wss://stream.bybit.com:443/v5/public/linear";

/// Build Bybit stream definitions from the connection config.
pub fn build(conn_config: &ConnectionConfig) -> Result<Vec<StreamDef>> {
    let cfg = BybitConfig::from_connection(conn_config)?;
    let ping = PingConfig {
        interval: Duration::from_secs(cfg.ping_interval_sec),
        payload: PingPayload::Json(serde_json::json!({"req_id": "3002", "op": "ping"})),
    };
    let mut streams = Vec::new();

    // --- Spot ---
    if !cfg.spot_symbols.is_empty() {
        let parser = make_bybit_parser(ProductType::Spot);

        streams.push(StreamDef {
            label: "bybit_spot".into(),
            ws_url: BYBIT_SPOT_WS_URL.into(),
            subscribe_msg: json_parser::build_subscribe(&cfg.spot_symbols),
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
            text_parser: Some(parser),
            binary_parser: None,
            custom_trade_dedup: None, // spot uses standard numeric dedup
            dedup_cpu_core: None,
        });
    }

    // --- Futures ---
    if !cfg.futures_symbols.is_empty() {
        let parser = make_bybit_parser(ProductType::Futures);

        // UUID dedup for futures trades (wrapped in Mutex for Fn closure)
        let uuid_dedup = Mutex::new(UuidDedup::new());
        let custom_dedup: Box<dyn FnMut(&str, u64) -> bool + Send> =
            Box::new(move |_sym, trade_id| {
                // trade_id was already hashed from UUID by the parser.
                // We use the raw hash as the dedup key.
                let mut d = uuid_dedup.lock().unwrap();
                d.check_and_insert(&format!("{trade_id}"))
            });

        streams.push(StreamDef {
            label: "bybit_futures".into(),
            ws_url: BYBIT_LINEAR_WS_URL.into(),
            subscribe_msg: json_parser::build_subscribe(&cfg.futures_symbols),
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
            text_parser: Some(parser),
            binary_parser: None,
            custom_trade_dedup: Some(custom_dedup),
            dedup_cpu_core: None,
        });
    }

    Ok(streams)
}

/// Create a stateful Bybit parser closure that manages OrderBook state
/// internally and outputs `Vec<MarketDataMsg>` directly.
///
/// The closure captures a per-symbol `OrderBook<50>` map. When an
/// `orderbook.50` snapshot or delta arrives, the closure updates the book
/// and emits a `Depth5` message.
fn make_bybit_parser(
    product_type: ProductType,
) -> Box<dyn Fn(&str) -> Vec<MarketDataMsg> + Send + Sync> {
    let books: Mutex<AHashMap<String, OrderBook<50>>> = Mutex::new(AHashMap::new());

    Box::new(move |text| parse_to_market_data(text, product_type, &books))
}

/// Parse a Bybit JSON message into `Vec<MarketDataMsg>`, managing OrderBook
/// state for incremental depth updates.
fn parse_to_market_data(
    text: &str,
    product_type: ProductType,
    books: &Mutex<AHashMap<String, OrderBook<50>>>,
) -> Vec<MarketDataMsg> {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let topic = match v.get("topic").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return vec![],
    };

    if topic.starts_with("orderbook.1.") {
        // BBO — pass through directly
        json_parser::parse_bbo(&v, product_type)
            .into_iter()
            .map(MarketDataMsg::Bbo)
            .collect()
    } else if topic.starts_with("publicTrade.") {
        // Trades — convert to MarketDataMsg::Trade
        json_parser::parse_trades_to_md(&v, product_type)
    } else if topic.starts_with("orderbook.50.") {
        // Depth — update OrderBook and emit Depth5
        parse_depth_to_md(&v, product_type, books)
    } else {
        vec![]
    }
}

/// Parse an `orderbook.50` message, update the local OrderBook, and emit Depth5.
fn parse_depth_to_md(
    v: &serde_json::Value,
    product_type: ProductType,
    books: &Mutex<AHashMap<String, OrderBook<50>>>,
) -> Vec<MarketDataMsg> {
    let msg_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("snapshot");
    let data = match v.get("data") {
        Some(d) => d,
        None => return vec![],
    };

    let sym = match data.get("s").and_then(|s| s.as_str()) {
        Some(s) => s,
        None => return vec![],
    };
    let ts = v.get("ts").and_then(|t| t.as_u64()).unwrap_or(0);
    let cts = v.get("cts").and_then(|c| c.as_u64()).unwrap_or(ts);
    let update_id = data.get("u").and_then(|u| u.as_u64()).unwrap_or(0);

    let bids = parse_levels(data.get("b"));
    let asks = parse_levels(data.get("a"));

    let mut books_guard = books.lock().unwrap();
    let book = books_guard
        .entry(sym.to_string())
        .or_default();

    if msg_type == "snapshot" {
        book.set_snapshot(&bids, &asks);
    } else {
        book.update(&bids, &asks);
    }

    let (bid_prices, bid_vols, ask_prices, ask_vols, bid_level, ask_level) = book.get_depth5();

    let depth = Depth5 {
        symbol: symbol_to_bytes(sym),
        product_type,
        event_timestamp_us: ts * 1000,
        trade_timestamp_us: cts * 1000,
        update_id,
        bid_level,
        ask_level,
        last_price: 0.0,
        bid_prices,
        bid_vols,
        ask_prices,
        ask_vols,
        bid_order_counts: [0; 5],
        ask_order_counts: [0; 5],
        local_time_us: k4_core::time_util::now_us(),
    };

    vec![MarketDataMsg::Depth5(depth)]
}

/// Parse `[price_str, vol_str]` arrays from JSON.
fn parse_levels(v: Option<&serde_json::Value>) -> Vec<[f64; 2]> {
    let arr = match v.and_then(|a| a.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    arr.iter()
        .filter_map(|level| {
            let a = level.as_array()?;
            let price = crate::json_util::parse_str_f64(a.first())?;
            let vol = crate::json_util::parse_str_f64(a.get(1))?;
            Some([price, vol])
        })
        .collect()
}
