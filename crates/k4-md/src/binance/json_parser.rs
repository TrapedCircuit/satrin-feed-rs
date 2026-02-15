//! Binance JSON message parser.
//!
//! Parses WebSocket JSON messages from Binance Spot and UBase streams into
//! `MarketDataMsg` variants. Uses `serde_json` for parsing and `fast-float`
//! for high-performance string-to-f64 conversion.

use k4_core::{time_util, *};

use crate::json_util::{fill_depth5_levels, parse_f64_field};

/// Parse a Binance JSON WebSocket message into a MarketDataMsg.
///
/// Returns `None` for messages that are not market data (e.g. subscription acks).
pub fn parse_message(text: &str) -> Option<MarketDataMsg> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;

    let event_type = v.get("e")?.as_str()?;
    match event_type {
        "aggTrade" => parse_agg_trade(&v),
        "bookTicker" => parse_book_ticker(&v),
        "trade" => parse_trade(&v),
        "depthUpdate" => parse_depth_update(&v),
        _ => None,
    }
}

/// Build subscription message for Spot JSON (aggTrade only).
pub fn build_spot_json_subscribe(symbols: &[String]) -> String {
    let params: Vec<String> = symbols.iter().map(|s| format!("{}@aggTrade", s.to_lowercase())).collect();
    serde_json::json!({
        "method": "SUBSCRIBE",
        "params": params,
        "id": 1
    })
    .to_string()
}

/// Build subscription message for Spot SBE (bookTicker, trade, depth).
pub fn build_spot_sbe_subscribe(symbols: &[String]) -> String {
    let mut params = Vec::new();
    for s in symbols {
        let lower = s.to_lowercase();
        params.push(format!("{lower}@bestBidAsk"));
        params.push(format!("{lower}@trade"));
        params.push(format!("{lower}@depth20"));
    }
    serde_json::json!({
        "method": "SUBSCRIBE",
        "params": params,
        "id": 1
    })
    .to_string()
}

/// Build subscription message for UBase JSON.
pub fn build_ubase_subscribe(symbols: &[String]) -> String {
    let mut params = Vec::new();
    for s in symbols {
        let lower = s.to_lowercase();
        params.push(format!("{lower}@aggTrade"));
        params.push(format!("{lower}@bookTicker"));
        params.push(format!("{lower}@trade"));
        params.push(format!("{lower}@depth5@100ms"));
    }
    serde_json::json!({
        "method": "SUBSCRIBE",
        "params": params,
        "id": 1
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Individual parsers
// ---------------------------------------------------------------------------

fn parse_agg_trade(v: &serde_json::Value) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let sym = v.get("s")?.as_str()?;
    let product_type = if v.get("ps").is_some() { ProductType::Futures } else { ProductType::Spot };

    let agg = AggTrade {
        symbol: symbol_to_bytes(sym),
        product_type,
        event_timestamp_us: v.get("E")?.as_u64()? * 1000,
        trade_timestamp_us: v.get("T")?.as_u64()? * 1000,
        first_trade_id: v.get("f")?.as_u64()?,
        last_trade_id: v.get("l")?.as_u64()?,
        agg_trade_id: v.get("a")?.as_u64()?,
        price: parse_f64_field(v, "p")?,
        vol: parse_f64_field(v, "q")?,
        trade_count: 0, // Not provided in the message
        is_buyer_maker: v.get("m")?.as_bool()?,
        local_time_us: local_time,
    };

    Some(MarketDataMsg::AggTrade(agg))
}

fn parse_book_ticker(v: &serde_json::Value) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let sym = v.get("s")?.as_str()?;
    let product_type = if v.get("ps").is_some() { ProductType::Futures } else { ProductType::Spot };

    let bbo = Bookticker {
        symbol: symbol_to_bytes(sym),
        product_type,
        event_timestamp_us: v.get("E").and_then(|e| e.as_u64()).unwrap_or(0) * 1000,
        trade_timestamp_us: v.get("T").and_then(|t| t.as_u64()).unwrap_or(0) * 1000,
        update_id: v.get("u")?.as_u64()?,
        bid_price: parse_f64_field(v, "b")?,
        bid_vol: parse_f64_field(v, "B")?,
        ask_price: parse_f64_field(v, "a")?,
        ask_vol: parse_f64_field(v, "A")?,
        bid_order_count: 0,
        ask_order_count: 0,
        local_time_us: local_time,
    };

    Some(MarketDataMsg::Bbo(bbo))
}

fn parse_trade(v: &serde_json::Value) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let sym = v.get("s")?.as_str()?;
    let product_type = if v.get("ps").is_some() { ProductType::Futures } else { ProductType::Spot };

    let trade = Trade {
        symbol: symbol_to_bytes(sym),
        product_type,
        event_timestamp_us: v.get("E").and_then(|e| e.as_u64()).unwrap_or(0) * 1000,
        trade_timestamp_us: v.get("T")?.as_u64()? * 1000,
        trade_id: v.get("t")?.as_u64()?,
        price: parse_f64_field(v, "p")?,
        vol: parse_f64_field(v, "q")?,
        is_buyer_maker: v.get("m")?.as_bool()?,
        local_time_us: local_time,
    };

    Some(MarketDataMsg::Trade(trade))
}

fn parse_depth_update(v: &serde_json::Value) -> Option<MarketDataMsg> {
    let local_time = time_util::now_us();
    let sym = v.get("s")?.as_str()?;

    let bids = v.get("b")?.as_array()?;
    let asks = v.get("a")?.as_array()?;

    let mut depth = Depth5 {
        symbol: symbol_to_bytes(sym),
        product_type: ProductType::Futures,
        event_timestamp_us: v.get("E").and_then(|e| e.as_u64()).unwrap_or(0) * 1000,
        trade_timestamp_us: v.get("T").and_then(|t| t.as_u64()).unwrap_or(0) * 1000,
        update_id: v.get("u")?.as_u64()?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agg_trade_msg() {
        let json = r#"{"e":"aggTrade","E":1672515782136,"s":"BTCUSDT","a":123456789,"p":"16500.50","q":"0.001","f":100,"l":105,"T":1672515782136,"m":true}"#;
        let msg = parse_message(json).unwrap();
        match msg {
            MarketDataMsg::AggTrade(agg) => {
                assert_eq!(symbol_from_bytes(&agg.symbol), "BTCUSDT");
                assert!((agg.price - 16500.50).abs() < 0.01);
                assert_eq!(agg.agg_trade_id, 123456789);
                assert!(agg.is_buyer_maker);
            }
            _ => panic!("expected AggTrade"),
        }
    }

    #[test]
    fn parse_book_ticker_msg() {
        let json = r#"{"e":"bookTicker","u":400900217,"s":"BTCUSDT","b":"25.35190000","B":"31.21000000","a":"25.36520000","A":"40.66000000","E":1672515782136,"T":1672515782136}"#;
        let msg = parse_message(json).unwrap();
        match msg {
            MarketDataMsg::Bbo(bbo) => {
                assert_eq!(symbol_from_bytes(&bbo.symbol), "BTCUSDT");
                assert!((bbo.bid_price - 25.3519).abs() < 0.0001);
                assert_eq!(bbo.update_id, 400900217);
            }
            _ => panic!("expected Bbo"),
        }
    }
}
