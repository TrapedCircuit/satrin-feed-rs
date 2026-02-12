//! OKX JSON message parser.
//!
//! Parses WebSocket JSON messages from OKX spot and swap streams into
//! [`MarketDataMsg`] variants. Routes by the `arg.channel` field:
//!
//! - `bbo-tbt` → [`Bookticker`]
//! - `trades` → [`Trade`]
//! - `books5` → [`Depth5`]

use k4_core::time_util;
use k4_core::*;

use crate::json_util::{fill_depth5_levels, parse_str_f64, parse_str_i32, parse_str_u64};

/// Parse an OKX JSON WebSocket message into a [`MarketDataMsg`].
///
/// Returns `None` for non-data messages (subscription acks, pong, etc.).
pub fn parse_message(text: &str) -> Option<MarketDataMsg> {
    // OKX echoes "pong" in response to our "ping".
    if text == "pong" {
        return None;
    }

    let v: serde_json::Value = serde_json::from_str(text).ok()?;

    let arg = v.get("arg")?;
    let channel = arg.get("channel")?.as_str()?;
    let inst_id = arg.get("instId")?.as_str()?;

    match channel {
        "bbo-tbt" => parse_book_ticker(&v, inst_id),
        "trades" => parse_trade(&v, inst_id),
        "books5" => parse_depth5(&v, inst_id),
        _ => None,
    }
}

/// Build subscription message for OKX spot symbols.
///
/// Subscribes to `bbo-tbt`, `trades`, and `books5` for each symbol.
pub fn build_spot_subscribe(symbols: &[String]) -> String {
    let args: Vec<serde_json::Value> = symbols
        .iter()
        .flat_map(|s| {
            vec![
                serde_json::json!({"channel": "bbo-tbt", "instId": s}),
                serde_json::json!({"channel": "trades", "instId": s}),
                serde_json::json!({"channel": "books5", "instId": s}),
            ]
        })
        .collect();

    serde_json::json!({
        "id": "3000",
        "op": "subscribe",
        "args": args
    })
    .to_string()
}

/// Build subscription message for OKX swap symbols.
///
/// Same channels as spot but with swap instIds (e.g. `BTC-USDT-SWAP`).
pub fn build_swap_subscribe(symbols: &[String]) -> String {
    let args: Vec<serde_json::Value> = symbols
        .iter()
        .flat_map(|s| {
            vec![
                serde_json::json!({"channel": "bbo-tbt", "instId": s}),
                serde_json::json!({"channel": "trades", "instId": s}),
                serde_json::json!({"channel": "books5", "instId": s}),
            ]
        })
        .collect();

    serde_json::json!({
        "id": "3001",
        "op": "subscribe",
        "args": args
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Individual parsers
// ---------------------------------------------------------------------------

fn parse_book_ticker(v: &serde_json::Value, inst_id: &str) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let data = v.get("data")?.as_array()?.first()?;

    let product_type = product_type_from_inst_id(inst_id);

    let asks = data.get("asks")?.as_array()?;
    let bids = data.get("bids")?.as_array()?;
    let ask0 = asks.first()?.as_array()?;
    let bid0 = bids.first()?.as_array()?;

    let ts_ms = parse_str_u64(data.get("ts"))?;
    let seq_id = parse_str_u64(data.get("seqId"))?;

    let bbo = Bookticker {
        symbol: symbol_to_bytes(inst_id),
        product_type,
        event_timestamp_us: ts_ms * 1000,
        trade_timestamp_us: ts_ms * 1000,
        update_id: seq_id,
        ask_price: parse_str_f64(ask0.first())?,
        ask_vol: parse_str_f64(ask0.get(1))?,
        bid_price: parse_str_f64(bid0.first())?,
        bid_vol: parse_str_f64(bid0.get(1))?,
        // OKX provides order count at index 3 of each level.
        ask_order_count: parse_str_i32(ask0.get(3)).unwrap_or(0),
        bid_order_count: parse_str_i32(bid0.get(3)).unwrap_or(0),
        local_time_us: local_time,
    };

    Some(MarketDataMsg::Bbo(bbo))
}

fn parse_trade(v: &serde_json::Value, inst_id: &str) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let data = v.get("data")?.as_array()?.first()?;

    let product_type = product_type_from_inst_id(inst_id);
    let ts_ms = parse_str_u64(data.get("ts"))?;
    let side = data.get("side")?.as_str()?;

    let trade = Trade {
        symbol: symbol_to_bytes(inst_id),
        product_type,
        event_timestamp_us: ts_ms * 1000,
        trade_timestamp_us: ts_ms * 1000,
        trade_id: parse_str_u64(data.get("tradeId"))?,
        price: parse_str_f64(data.get("px"))?,
        vol: parse_str_f64(data.get("sz"))?,
        is_buyer_maker: side == "sell",
        local_time_us: local_time,
    };

    Some(MarketDataMsg::Trade(trade))
}

fn parse_depth5(v: &serde_json::Value, inst_id: &str) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let data = v.get("data")?.as_array()?.first()?;

    let product_type = product_type_from_inst_id(inst_id);
    let ts_ms = parse_str_u64(data.get("ts"))?;
    let seq_id = parse_str_u64(data.get("seqId"))?;

    let asks = data.get("asks")?.as_array()?;
    let bids = data.get("bids")?.as_array()?;

    let mut depth = Depth5 {
        symbol: symbol_to_bytes(inst_id),
        product_type,
        event_timestamp_us: ts_ms * 1000,
        trade_timestamp_us: ts_ms * 1000,
        update_id: seq_id,
        bid_level: 0,
        ask_level: 0,
        last_price: 0.0,
        bid_prices: [0.0; 5],
        bid_vols: [0.0; 5],
        ask_prices: [0.0; 5],
        ask_vols: [0.0; 5],
        bid_order_counts: [0; 5],
        ask_order_counts: [0; 5],
        local_time_us: local_time,
    };

    fill_depth5_levels(&mut depth, bids, asks);

    Some(MarketDataMsg::Depth5(depth))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Determine product type from OKX instId.
///
/// Symbols ending in `-SWAP` are swap/futures, otherwise spot.
fn product_type_from_inst_id(inst_id: &str) -> ProductType {
    if inst_id.ends_with("-SWAP") {
        ProductType::Futures
    } else {
        ProductType::Spot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bbo_tbt() {
        let json = r#"{
            "arg": {"channel": "bbo-tbt", "instId": "BTC-USDT"},
            "data": [{
                "asks": [["30000.1", "0.5", "0", "3"]],
                "bids": [["29999.9", "0.3", "0", "2"]],
                "ts": "1672515782136",
                "seqId": "123456789"
            }]
        }"#;
        let msg = parse_message(json).unwrap();
        match msg {
            MarketDataMsg::Bbo(bbo) => {
                assert_eq!(symbol_from_bytes(&bbo.symbol), "BTC-USDT");
                assert!((bbo.ask_price - 30000.1).abs() < 0.01);
                assert!((bbo.bid_price - 29999.9).abs() < 0.01);
                assert_eq!(bbo.update_id, 123456789);
                assert_eq!(bbo.ask_order_count, 3);
                assert_eq!(bbo.bid_order_count, 2);
                assert_eq!(bbo.product_type, ProductType::Spot);
            }
            _ => panic!("expected Bbo"),
        }
    }

    #[test]
    fn parse_trade_msg() {
        let json = r#"{
            "arg": {"channel": "trades", "instId": "BTC-USDT-SWAP"},
            "data": [{
                "tradeId": "987654321",
                "px": "30001.5",
                "sz": "0.01",
                "side": "sell",
                "ts": "1672515782200"
            }]
        }"#;
        let msg = parse_message(json).unwrap();
        match msg {
            MarketDataMsg::Trade(trade) => {
                assert_eq!(symbol_from_bytes(&trade.symbol), "BTC-USDT-SWAP");
                assert!((trade.price - 30001.5).abs() < 0.01);
                assert!(trade.is_buyer_maker);
                assert_eq!(trade.product_type, ProductType::Futures);
            }
            _ => panic!("expected Trade"),
        }
    }

    #[test]
    fn pong_returns_none() {
        assert!(parse_message("pong").is_none());
    }
}
