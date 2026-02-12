//! Binance SBE (Simple Binary Encoding) parser.
//!
//! Binance provides a binary WebSocket stream using SBE encoding for lower
//! latency. The format is little-endian with a fixed message header followed
//! by a variable-length body.
//!
//! # SBE Message Header (8 bytes)
//!
//! | Offset | Size | Field         | Description                        |
//! |--------|------|---------------|------------------------------------|
//! | 0      | 2    | blockLength   | Length of the root block            |
//! | 2      | 2    | templateId    | 10000=trades, 10001=BBA, 10002=depth |
//! | 4      | 2    | schemaId      | Schema identifier                  |
//! | 6      | 2    | version       | Schema version                     |
//!
//! # Decimal128 Encoding
//!
//! Prices and quantities use a mantissa (i64) + exponent (i8) format:
//! `value = mantissa × 10^(exponent + 18)`

use k4_core::time_util;
use k4_core::*;

const SBE_HEADER_SIZE: usize = 8;
const TEMPLATE_TRADES: u16 = 10000;
const TEMPLATE_BEST_BID_ASK: u16 = 10001;
const TEMPLATE_DEPTH: u16 = 10002;

/// Parse an SBE binary message, returning zero or more MarketDataMsg items.
///
/// A single SBE message may contain multiple trades (via group encoding),
/// hence the Vec return type.
pub fn parse_sbe_message(data: &[u8]) -> Vec<MarketDataMsg> {
    if data.len() < SBE_HEADER_SIZE {
        return vec![];
    }

    let _block_length = u16::from_le_bytes([data[0], data[1]]);
    let template_id = u16::from_le_bytes([data[2], data[3]]);
    // schema_id and version at [4..8] are not used for routing

    let body = &data[SBE_HEADER_SIZE..];

    match template_id {
        TEMPLATE_BEST_BID_ASK => parse_best_bid_ask(body).into_iter().collect(),
        TEMPLATE_TRADES => parse_trades(body),
        TEMPLATE_DEPTH => parse_depth(body).into_iter().collect(),
        _ => vec![],
    }
}

/// Decode a Decimal128 value: `mantissa × 10^(exponent + 18)`.
///
/// The exponent is biased by +18 to avoid negative exponents in most cases.
#[inline]
fn decode_decimal128(mantissa: i64, exponent: i8) -> f64 {
    let exp = exponent as i32 + 18;
    mantissa as f64 * 10f64.powi(exp - 18)
}

/// Read a little-endian i64 from a byte slice.
#[inline]
fn read_i64_le(data: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap_or([0; 8]))
}

/// Read a little-endian u64 from a byte slice.
#[inline]
fn read_u64_le(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap_or([0; 8]))
}

/// Read a little-endian u16 from a byte slice.
#[inline]
fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap_or([0; 2]))
}

/// Extract a null-terminated ASCII symbol from the SBE body.
fn read_symbol(data: &[u8], offset: usize, max_len: usize) -> String {
    let end = (offset + max_len).min(data.len());
    let slice = &data[offset..end];
    let null_pos = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..null_pos]).to_uppercase()
}

/// Parse BestBidAsk (templateId=10001).
fn parse_best_bid_ask(body: &[u8]) -> Option<MarketDataMsg> {
    // Minimum body size check
    if body.len() < 72 {
        return None;
    }

    let local_time = time_util::now_us();
    let sym = read_symbol(body, 0, 20);

    let seq_num = read_u64_le(body, 20);
    let transact_time_ns = read_u64_le(body, 28);

    // Bid price: mantissa at 36, exponent at 44
    let bid_mantissa = read_i64_le(body, 36);
    let bid_exponent = body[44] as i8;
    let bid_price = decode_decimal128(bid_mantissa, bid_exponent);

    // Bid qty: mantissa at 45, exponent at 53
    let bid_qty_mantissa = read_i64_le(body, 45);
    let bid_qty_exponent = body[53] as i8;
    let bid_vol = decode_decimal128(bid_qty_mantissa, bid_qty_exponent);

    // Ask price: mantissa at 54, exponent at 62
    let ask_mantissa = read_i64_le(body, 54);
    let ask_exponent = body[62] as i8;
    let ask_price = decode_decimal128(ask_mantissa, ask_exponent);

    // Ask qty: mantissa at 63, exponent at 71
    let ask_qty_mantissa = read_i64_le(body, 63);
    let ask_qty_exponent = body[71] as i8;
    let ask_vol = decode_decimal128(ask_qty_mantissa, ask_qty_exponent);

    Some(MarketDataMsg::Bbo(Bookticker {
        symbol: symbol_to_bytes(&sym),
        product_type: ProductType::Spot,
        event_timestamp_us: transact_time_ns / 1000,
        trade_timestamp_us: transact_time_ns / 1000,
        update_id: seq_num,
        bid_price,
        bid_vol,
        ask_price,
        ask_vol,
        bid_order_count: 0,
        ask_order_count: 0,
        local_time_us: local_time,
    }))
}

/// Parse Trades (templateId=10000) — may contain multiple trades via group encoding.
fn parse_trades(body: &[u8]) -> Vec<MarketDataMsg> {
    let mut results = Vec::new();

    if body.len() < 28 {
        return results;
    }

    let local_time = time_util::now_us();
    let sym = read_symbol(body, 0, 20);

    // After the root block, there's a group header (4 bytes: blockLength u16 + numInGroup u16)
    // The root block length is from the SBE header, but typically 20 bytes for symbol
    let root_block_end = 20; // symbol length for trades

    if body.len() < root_block_end + 4 {
        return results;
    }

    let group_block_length = read_u16_le(body, root_block_end) as usize;
    let num_trades = read_u16_le(body, root_block_end + 2) as usize;

    let mut offset = root_block_end + 4;

    for _ in 0..num_trades {
        if offset + group_block_length > body.len() {
            break;
        }

        // Each trade entry has: price (9 bytes), qty (9 bytes), tradeId (u64),
        // transactTime (u64), isBuyerMaker (u8)
        let price_mantissa = read_i64_le(body, offset);
        let price_exponent = body[offset + 8] as i8;
        let price = decode_decimal128(price_mantissa, price_exponent);

        let qty_mantissa = read_i64_le(body, offset + 9);
        let qty_exponent = body[offset + 17] as i8;
        let vol = decode_decimal128(qty_mantissa, qty_exponent);

        let trade_id = read_u64_le(body, offset + 18);
        let transact_time_ns = read_u64_le(body, offset + 26);
        let is_buyer_maker = body.get(offset + 34).copied().unwrap_or(0) != 0;

        results.push(MarketDataMsg::Trade(Trade {
            symbol: symbol_to_bytes(&sym),
            product_type: ProductType::Spot,
            event_timestamp_us: transact_time_ns / 1000,
            trade_timestamp_us: transact_time_ns / 1000,
            trade_id,
            price,
            vol,
            is_buyer_maker,
            local_time_us: local_time,
        }));

        offset += group_block_length;
    }

    results
}

/// Parse Depth snapshot (templateId=10002).
fn parse_depth(body: &[u8]) -> Option<MarketDataMsg> {
    if body.len() < 28 {
        return None;
    }

    let local_time = time_util::now_us();
    let sym = read_symbol(body, 0, 20);
    let transact_time_ns = read_u64_le(body, 20);

    let mut depth = Depth5 {
        symbol: symbol_to_bytes(&sym),
        product_type: ProductType::Spot,
        event_timestamp_us: transact_time_ns / 1000,
        trade_timestamp_us: transact_time_ns / 1000,
        update_id: 0,
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

    // After root block (28 bytes), parse bid group then ask group
    let root_end = 28;
    if body.len() <= root_end + 4 {
        return Some(MarketDataMsg::Depth5(depth));
    }

    // Bid group header
    let bid_block_len = read_u16_le(body, root_end) as usize;
    let num_bids = read_u16_le(body, root_end + 2) as usize;
    let mut offset = root_end + 4;

    for i in 0..num_bids.min(5) {
        if offset + bid_block_len > body.len() {
            break;
        }
        let p_mantissa = read_i64_le(body, offset);
        let p_exp = body[offset + 8] as i8;
        let q_mantissa = read_i64_le(body, offset + 9);
        let q_exp = body[offset + 17] as i8;

        depth.bid_prices[i] = decode_decimal128(p_mantissa, p_exp);
        depth.bid_vols[i] = decode_decimal128(q_mantissa, q_exp);
        depth.bid_level = (i + 1) as u32;

        offset += bid_block_len;
    }
    // Skip remaining bid levels beyond 5
    for _ in 5..num_bids {
        offset += bid_block_len;
    }

    // Ask group header
    if offset + 4 <= body.len() {
        let ask_block_len = read_u16_le(body, offset) as usize;
        let num_asks = read_u16_le(body, offset + 2) as usize;
        offset += 4;

        for i in 0..num_asks.min(5) {
            if offset + ask_block_len > body.len() {
                break;
            }
            let p_mantissa = read_i64_le(body, offset);
            let p_exp = body[offset + 8] as i8;
            let q_mantissa = read_i64_le(body, offset + 9);
            let q_exp = body[offset + 17] as i8;

            depth.ask_prices[i] = decode_decimal128(p_mantissa, p_exp);
            depth.ask_vols[i] = decode_decimal128(q_mantissa, q_exp);
            depth.ask_level = (i + 1) as u32;

            offset += ask_block_len;
        }
    }

    Some(MarketDataMsg::Depth5(depth))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_short_returns_empty() {
        assert!(parse_sbe_message(&[0; 4]).is_empty());
        assert!(parse_sbe_message(&[]).is_empty());
    }

    #[test]
    fn unknown_template_returns_empty() {
        let mut data = vec![0u8; 16];
        // Set template_id to unknown value 9999
        data[2] = 0x0F;
        data[3] = 0x27; // 9999 LE
        assert!(parse_sbe_message(&data).is_empty());
    }

    #[test]
    fn decimal128_conversion() {
        // mantissa=50000, exponent=0 → 50000 * 10^(0+18-18) = 50000.0
        assert!((decode_decimal128(50000, 0) - 50000.0).abs() < 0.001);
        // mantissa=123456789, exponent=-6 → 123456789 * 10^(-6) = 123.456789
        assert!((decode_decimal128(123456789, -6) - 123.456789).abs() < 1e-6);
    }
}
