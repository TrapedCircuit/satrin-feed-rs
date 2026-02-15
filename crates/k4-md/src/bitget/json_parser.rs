//! Bitget JSON message parser.
//!
//! Parses WebSocket JSON messages from Bitget spot and futures streams into
//! [`MarketDataMsg`] variants. Routes by the `arg.channel` field:
//!
//! - `books1` → [`Bookticker`]
//! - `trade` → [`Trade`] (batch — may return multiple trades per message)
//! - `books5` → [`Depth5`]

use k4_core::{time_util, *};

use crate::json_util::{fill_depth5_levels, parse_str_f64, parse_str_u64};

/// Parse a Bitget JSON WebSocket message into zero or more [`MarketDataMsg`].
///
/// Returns an empty `Vec` for non-data messages (subscription acks, pong, etc.).
/// Trade messages may produce multiple results since Bitget batches trades.
pub fn parse_message(text: &str) -> Vec<MarketDataMsg> {
    // Bitget echoes "pong" in response to our "ping".
    if text == "pong" {
        return vec![];
    }

    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let arg = match v.get("arg") {
        Some(a) => a,
        None => return vec![],
    };

    let channel = match arg.get("channel").and_then(|c| c.as_str()) {
        Some(c) => c,
        None => return vec![],
    };

    let inst_id = match arg.get("instId").and_then(|i| i.as_str()) {
        Some(i) => i,
        None => return vec![],
    };

    let product_type = product_type_from_inst_type(arg);

    match channel {
        "books1" => parse_book_ticker(&v, inst_id, product_type).into_iter().collect(),
        "trade" => parse_trades(&v, inst_id, product_type),
        "books5" => parse_depth5(&v, inst_id, product_type).into_iter().collect(),
        _ => vec![],
    }
}

/// Build subscription message for Bitget spot symbols.
///
/// Subscribes to `books1` (BBO), `trade`, and `books5` for each symbol.
pub fn build_spot_subscribe(symbols: &[String]) -> String {
    let args: Vec<serde_json::Value> = symbols
        .iter()
        .flat_map(|s| {
            vec![
                serde_json::json!({"instType": "SPOT", "channel": "books1", "instId": s}),
                serde_json::json!({"instType": "SPOT", "channel": "trade", "instId": s}),
                serde_json::json!({"instType": "SPOT", "channel": "books5", "instId": s}),
            ]
        })
        .collect();

    serde_json::json!({
        "op": "subscribe",
        "args": args
    })
    .to_string()
}

/// Build subscription message for Bitget futures symbols.
///
/// Uses `USDT-FUTURES` as the `instType`.
pub fn build_futures_subscribe(symbols: &[String]) -> String {
    let args: Vec<serde_json::Value> = symbols
        .iter()
        .flat_map(|s| {
            vec![
                serde_json::json!({"instType": "USDT-FUTURES", "channel": "books1", "instId": s}),
                serde_json::json!({"instType": "USDT-FUTURES", "channel": "trade", "instId": s}),
                serde_json::json!({"instType": "USDT-FUTURES", "channel": "books5", "instId": s}),
            ]
        })
        .collect();

    serde_json::json!({
        "op": "subscribe",
        "args": args
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Individual parsers
// ---------------------------------------------------------------------------

fn parse_book_ticker(v: &serde_json::Value, inst_id: &str, product_type: ProductType) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let data = v.get("data")?.as_array()?.first()?;

    // Root-level ts is event timestamp, data-level ts is trade timestamp.
    let event_ts_ms = parse_str_u64(v.get("ts"))?;
    let trade_ts_ms = parse_str_u64(data.get("ts"))?;
    let seq = parse_str_u64(data.get("seq"))?;

    let asks = data.get("asks")?.as_array()?;
    let bids = data.get("bids")?.as_array()?;
    let ask0 = asks.first()?.as_array()?;
    let bid0 = bids.first()?.as_array()?;

    let bbo = Bookticker {
        symbol: symbol_to_bytes(inst_id),
        product_type,
        event_timestamp_us: event_ts_ms * 1000,
        trade_timestamp_us: trade_ts_ms * 1000,
        update_id: seq,
        ask_price: parse_str_f64(ask0.first())?,
        ask_vol: parse_str_f64(ask0.get(1))?,
        bid_price: parse_str_f64(bid0.first())?,
        bid_vol: parse_str_f64(bid0.get(1))?,
        ask_order_count: 0,
        bid_order_count: 0,
        local_time_us: local_time,
    };

    Some(MarketDataMsg::Bbo(bbo))
}

fn parse_trades(v: &serde_json::Value, inst_id: &str, product_type: ProductType) -> Vec<MarketDataMsg> {
    let local_time = time_util::now_us();
    let data = match v.get("data").and_then(|d| d.as_array()) {
        Some(arr) => arr,
        None => return vec![],
    };

    // Bitget sends trades in reverse order (newest first), iterate backwards
    // to process oldest first.
    let mut result = Vec::with_capacity(data.len());
    for item in data.iter().rev() {
        if let Some(trade) = parse_single_trade(item, inst_id, product_type, local_time) {
            result.push(MarketDataMsg::Trade(trade));
        }
    }
    result
}

fn parse_single_trade(
    item: &serde_json::Value,
    inst_id: &str,
    product_type: ProductType,
    local_time: u64,
) -> Option<Trade> {
    let ts_ms = parse_str_u64(item.get("ts"))?;
    let side = item.get("side")?.as_str()?;

    Some(Trade {
        symbol: symbol_to_bytes(inst_id),
        product_type,
        event_timestamp_us: ts_ms * 1000,
        trade_timestamp_us: ts_ms * 1000,
        trade_id: parse_str_u64(item.get("tradeId"))?,
        price: parse_str_f64(item.get("price"))?,
        vol: parse_str_f64(item.get("size"))?,
        is_buyer_maker: side == "sell",
        local_time_us: local_time,
    })
}

fn parse_depth5(v: &serde_json::Value, inst_id: &str, product_type: ProductType) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let data = v.get("data")?.as_array()?.first()?;

    let event_ts_ms = parse_str_u64(v.get("ts"))?;
    let trade_ts_ms = parse_str_u64(data.get("ts"))?;
    let seq = parse_str_u64(data.get("seq"))?;

    let asks = data.get("asks")?.as_array()?;
    let bids = data.get("bids")?.as_array()?;

    let mut depth = Depth5 {
        symbol: symbol_to_bytes(inst_id),
        product_type,
        event_timestamp_us: event_ts_ms * 1000,
        trade_timestamp_us: trade_ts_ms * 1000,
        update_id: seq,
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

/// Determine product type from Bitget `arg.instType` field.
fn product_type_from_inst_type(arg: &serde_json::Value) -> ProductType {
    match arg.get("instType").and_then(|t| t.as_str()) {
        Some("USDT-FUTURES") => ProductType::Futures,
        Some("COIN-FUTURES") => ProductType::CoinMargin,
        _ => ProductType::Spot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_books1_bbo() {
        let json = r#"{
            "arg": {"instType": "SPOT", "channel": "books1", "instId": "BTCUSDT"},
            "ts": "1672515782136",
            "data": [{
                "asks": [["30000.1", "0.5"]],
                "bids": [["29999.9", "0.3"]],
                "ts": "1672515782135",
                "seq": "123456789"
            }]
        }"#;
        let msgs = parse_message(json);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            MarketDataMsg::Bbo(bbo) => {
                assert_eq!(symbol_from_bytes(&bbo.symbol), "BTCUSDT");
                assert!((bbo.ask_price - 30000.1).abs() < 0.01);
                assert_eq!(bbo.update_id, 123456789);
                assert_eq!(bbo.product_type, ProductType::Spot);
            }
            _ => panic!("expected Bbo"),
        }
    }

    #[test]
    fn parse_trade_batch() {
        let json = r#"{
            "arg": {"instType": "SPOT", "channel": "trade", "instId": "BTCUSDT"},
            "data": [
                {"tradeId": "3", "price": "30002", "size": "0.1", "side": "buy", "ts": "1672515782138"},
                {"tradeId": "2", "price": "30001", "size": "0.2", "side": "sell", "ts": "1672515782137"},
                {"tradeId": "1", "price": "30000", "size": "0.3", "side": "buy", "ts": "1672515782136"}
            ]
        }"#;
        let msgs = parse_message(json);
        assert_eq!(msgs.len(), 3);
        // Should be reversed: oldest first
        match &msgs[0] {
            MarketDataMsg::Trade(t) => assert_eq!(t.trade_id, 1),
            _ => panic!("expected Trade"),
        }
        match &msgs[2] {
            MarketDataMsg::Trade(t) => assert_eq!(t.trade_id, 3),
            _ => panic!("expected Trade"),
        }
    }

    #[test]
    fn pong_returns_empty() {
        assert!(parse_message("pong").is_empty());
    }
}
