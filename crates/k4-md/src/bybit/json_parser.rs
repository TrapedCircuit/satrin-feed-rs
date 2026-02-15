//! Bybit JSON message parser.
//!
//! Provides parsing functions called by the Bybit `build()` module. BBO and
//! trade parsing produce `MarketDataMsg` directly. Depth parsing is handled
//! in `mod.rs` via the stateful OrderBook closure.

use k4_core::{time_util, *};

use crate::json_util::parse_str_f64;

/// Build subscription message for Bybit symbols.
pub fn build_subscribe(symbols: &[String]) -> String {
    let args: Vec<String> = symbols
        .iter()
        .flat_map(|s| vec![format!("publicTrade.{s}"), format!("orderbook.1.{s}"), format!("orderbook.50.{s}")])
        .collect();

    serde_json::json!({
        "req_id": "3000",
        "op": "subscribe",
        "args": args
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// BBO parsing
// ---------------------------------------------------------------------------

/// Parse an `orderbook.1` message into a Bookticker.
pub fn parse_bbo(v: &serde_json::Value, product_type: ProductType) -> Option<Bookticker> {
    let data = v.get("data")?;
    let sym = data.get("s")?.as_str()?;
    let ts = v.get("ts")?.as_u64()?;
    let cts = v.get("cts").and_then(|c| c.as_u64()).unwrap_or(ts);
    let update_id = data.get("u")?.as_u64()?;

    let bids = data.get("b")?.as_array()?;
    let asks = data.get("a")?.as_array()?;

    let (bid_price, bid_vol) = parse_level(bids.first())?;
    let (ask_price, ask_vol) = parse_level(asks.first())?;

    Some(Bookticker {
        symbol: symbol_to_bytes(sym),
        product_type,
        event_timestamp_us: ts * 1000,
        trade_timestamp_us: cts * 1000,
        update_id,
        bid_price,
        bid_vol,
        ask_price,
        ask_vol,
        bid_order_count: 0,
        ask_order_count: 0,
        local_time_us: time_util::now_us(),
    })
}

// ---------------------------------------------------------------------------
// Trade parsing
// ---------------------------------------------------------------------------

/// Parse a `publicTrade` message into `Vec<MarketDataMsg::Trade>`.
pub fn parse_trades_to_md(v: &serde_json::Value, product_type: ProductType) -> Vec<MarketDataMsg> {
    let local_time = time_util::now_us();
    let ts = v.get("ts").and_then(|t| t.as_u64()).unwrap_or(0);

    let data = match v.get("data").and_then(|d| d.as_array()) {
        Some(arr) => arr,
        None => return vec![],
    };

    data.iter()
        .filter_map(|item| parse_single_trade(item, product_type, ts, local_time))
        .map(MarketDataMsg::Trade)
        .collect()
}

/// Parse a single trade from the `data` array.
fn parse_single_trade(
    item: &serde_json::Value,
    product_type: ProductType,
    event_ts: u64,
    local_time: u64,
) -> Option<Trade> {
    let raw_id = item.get("i")?.as_str()?;
    let sym = item.get("s")?.as_str()?;
    let trade_ts = item.get("T")?.as_u64()?;
    let side = item.get("S")?.as_str()?;

    // Spot: numeric trade ID. Futures: UUID -> xxhash64.
    let trade_id: u64 = raw_id.parse().unwrap_or_else(|_| xxhash_rust::xxh64::xxh64(raw_id.as_bytes(), 0));

    Some(Trade {
        symbol: symbol_to_bytes(sym),
        product_type,
        event_timestamp_us: event_ts * 1000,
        trade_timestamp_us: trade_ts * 1000,
        trade_id,
        price: parse_str_f64(item.get("p"))?,
        vol: parse_str_f64(item.get("v"))?,
        is_buyer_maker: side == "Sell",
        local_time_us: local_time,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_level(v: Option<&serde_json::Value>) -> Option<(f64, f64)> {
    let arr = v?.as_array()?;
    let price = parse_str_f64(arr.first())?;
    let vol = parse_str_f64(arr.get(1))?;
    Some((price, vol))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_orderbook_1_bbo() {
        let json = r#"{
            "topic": "orderbook.1.BTCUSDT",
            "type": "snapshot",
            "ts": 1672515782136,
            "data": {
                "s": "BTCUSDT",
                "b": [["29999.9", "0.3"]],
                "a": [["30000.1", "0.5"]],
                "u": 123456789
            },
            "cts": 1672515782135
        }"#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let bbo = parse_bbo(&v, ProductType::Spot).unwrap();
        assert_eq!(symbol_from_bytes(&bbo.symbol), "BTCUSDT");
        assert!((bbo.bid_price - 29999.9).abs() < 0.01);
        assert!((bbo.ask_price - 30000.1).abs() < 0.01);
        assert_eq!(bbo.update_id, 123456789);
    }

    #[test]
    fn parse_public_trade_spot() {
        let json = r#"{
            "topic": "publicTrade.BTCUSDT",
            "type": "snapshot",
            "ts": 1672515782136,
            "data": [{
                "i": "2100000000007542696",
                "T": 1672515782135,
                "p": "16578.50",
                "v": "0.001",
                "S": "Buy",
                "s": "BTCUSDT"
            }]
        }"#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let trades = parse_trades_to_md(&v, ProductType::Spot);
        assert_eq!(trades.len(), 1);
        match &trades[0] {
            MarketDataMsg::Trade(trade) => {
                assert_eq!(symbol_from_bytes(&trade.symbol), "BTCUSDT");
                assert!(!trade.is_buyer_maker);
                assert_eq!(trade.trade_id, 2100000000007542696);
            }
            _ => panic!("expected Trade"),
        }
    }

    #[test]
    fn parse_public_trade_futures_uuid() {
        let json = r#"{
            "topic": "publicTrade.BTCUSDT",
            "type": "snapshot",
            "ts": 1672515782136,
            "data": [{
                "i": "550e8400-e29b-41d4-a716-446655440000",
                "T": 1672515782135,
                "p": "30000.00",
                "v": "0.01",
                "S": "Sell",
                "s": "BTCUSDT"
            }]
        }"#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let trades = parse_trades_to_md(&v, ProductType::Futures);
        assert_eq!(trades.len(), 1);
        match &trades[0] {
            MarketDataMsg::Trade(trade) => {
                assert!(trade.is_buyer_maker);
                assert_ne!(trade.trade_id, 0);
            }
            _ => panic!("expected Trade"),
        }
    }
}
