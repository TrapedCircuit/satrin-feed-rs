//! Binance SBE (Simple Binary Encoding) parser.
//!
//! Parses binary WebSocket frames from `stream-sbe.binance.com` into
//! [`MarketDataMsg`] variants. Field layouts follow the official SBE schema
//! (`stream_1_0.xml`) and match the production C++ parser in
//! `binance_sbe_parser.h`.
//!
//! # SBE Message Header (8 bytes)
//!
//! | Offset | Size | Field       | Description                            |
//! |--------|------|-------------|----------------------------------------|
//! | 0      | 2    | blockLength | Length of the root block                |
//! | 2      | 2    | templateId  | 10000=trades, 10001=BBA, 10002=depth   |
//! | 4      | 2    | schemaId    | Schema identifier                      |
//! | 6      | 2    | version     | Schema version                         |
//!
//! # Decimal128 Encoding
//!
//! Prices and quantities use a shared-exponent format:
//!   - Root block contains `priceExponent` (i8) and `qtyExponent` (i8)
//!   - Each mantissa is i64
//!   - `value = mantissa × 10^exponent`
//!
//! # VarString8
//!
//! Symbol is always the **last** field in each message, encoded as a 1-byte
//! length prefix followed by the ASCII symbol bytes.

use k4_core::{time_util, *};

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

    let template_id = read_u16_le(data, 2);
    let body = &data[SBE_HEADER_SIZE..];

    match template_id {
        TEMPLATE_BEST_BID_ASK => parse_best_bid_ask(body).into_iter().collect(),
        TEMPLATE_TRADES => parse_trades(body),
        TEMPLATE_DEPTH => parse_depth(body).into_iter().collect(),
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Decimal128 conversion (lookup table, matches C++ to_double_fast)
// ---------------------------------------------------------------------------

const POW10: [f64; 37] = [
    1e-18, 1e-17, 1e-16, 1e-15, 1e-14, 1e-13, 1e-12, 1e-11, 1e-10, 1e-9, 1e-8, 1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2,
    1e-1, 1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18,
];

#[inline]
fn decode_decimal128(mantissa: i64, exponent: i8) -> f64 {
    let idx = (exponent as i32 + 18) as usize;
    mantissa as f64 * POW10[idx]
}

// ---------------------------------------------------------------------------
// Little-endian readers
// ---------------------------------------------------------------------------

#[inline]
fn read_i64_le(data: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap_or([0; 8]))
}

#[inline]
fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap_or([0; 2]))
}

#[inline]
fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]))
}

/// Read a VarString8 field: 1-byte length prefix + ASCII bytes.
#[inline]
fn read_var_string8(data: &[u8], offset: usize) -> &[u8] {
    if offset >= data.len() {
        return &[];
    }
    let len = data[offset] as usize;
    let start = offset + 1;
    let end = (start + len).min(data.len());
    &data[start..end]
}

// ---------------------------------------------------------------------------
// BestBidAsk (templateId=10001)
// ---------------------------------------------------------------------------
//
// Body layout:
//   Offset  0: eventTime        (i64, 8 bytes) — microseconds
//   Offset  8: bookUpdateId     (i64, 8 bytes)
//   Offset 16: priceExponent    (i8,  1 byte)  — shared
//   Offset 17: qtyExponent      (i8,  1 byte)  — shared
//   Offset 18: bidPrice mantissa (i64, 8 bytes)
//   Offset 26: bidQty mantissa   (i64, 8 bytes)
//   Offset 34: askPrice mantissa (i64, 8 bytes)
//   Offset 42: askQty mantissa   (i64, 8 bytes)
//   Offset 50: symbol VarString8

const BBA_MIN_BODY: usize = 51; // 50 bytes root + 1 byte VarString8 length

fn parse_best_bid_ask(body: &[u8]) -> Option<MarketDataMsg> {
    if body.len() < BBA_MIN_BODY {
        return None;
    }

    let local_time = time_util::now_us();

    let event_time_us = read_i64_le(body, 0) as u64;
    let update_id = read_i64_le(body, 8) as u64;
    let price_exp = body[16] as i8;
    let qty_exp = body[17] as i8;

    let bid_price = decode_decimal128(read_i64_le(body, 18), price_exp);
    let bid_vol = decode_decimal128(read_i64_le(body, 26), qty_exp);
    let ask_price = decode_decimal128(read_i64_le(body, 34), price_exp);
    let ask_vol = decode_decimal128(read_i64_le(body, 42), qty_exp);

    let sym_bytes = read_var_string8(body, 50);
    let sym = std::str::from_utf8(sym_bytes).unwrap_or("");

    Some(MarketDataMsg::Bbo(Bookticker {
        symbol: symbol_to_bytes(sym),
        product_type: ProductType::Spot,
        event_timestamp_us: event_time_us,
        trade_timestamp_us: event_time_us,
        update_id,
        bid_price,
        bid_vol,
        ask_price,
        ask_vol,
        bid_order_count: 0,
        ask_order_count: 0,
        local_time_us: local_time,
    }))
}

// ---------------------------------------------------------------------------
// Trades (templateId=10000)
// ---------------------------------------------------------------------------
//
// Body layout:
//   Offset  0: eventTime      (i64, 8 bytes)
//   Offset  8: transactTime   (i64, 8 bytes)
//   Offset 16: priceExponent  (i8,  1 byte) — shared
//   Offset 17: qtyExponent    (i8,  1 byte) — shared
//   Offset 18: groupSizeEncoding (6 bytes: blockLength u16 + numInGroup u32)
//   Per trade entry (blockLength bytes):
//     +0: tradeId         (i64, 8 bytes)
//     +8: price mantissa  (i64, 8 bytes)
//    +16: qty mantissa    (i64, 8 bytes)
//    +24: isBuyerMaker    (u8,  1 byte)
//   After all trades: symbol VarString8

const TRADES_ROOT_SIZE: usize = 18;
const TRADES_GROUP_HEADER_SIZE: usize = 6; // groupSizeEncoding: u16 + u32

fn parse_trades(body: &[u8]) -> Vec<MarketDataMsg> {
    if body.len() < TRADES_ROOT_SIZE + TRADES_GROUP_HEADER_SIZE {
        return vec![];
    }

    let local_time = time_util::now_us();

    let event_time_us = read_i64_le(body, 0) as u64;
    let transact_time_us = read_i64_le(body, 8) as u64;
    let price_exp = body[16] as i8;
    let qty_exp = body[17] as i8;

    let block_length = read_u16_le(body, TRADES_ROOT_SIZE) as usize;
    let num_trades = read_u32_le(body, TRADES_ROOT_SIZE + 2) as usize;

    let trades_start = TRADES_ROOT_SIZE + TRADES_GROUP_HEADER_SIZE;
    let trades_end = trades_start + block_length * num_trades;
    if trades_end > body.len() {
        return vec![];
    }

    let sym_bytes = read_var_string8(body, trades_end);
    let sym = std::str::from_utf8(sym_bytes).unwrap_or("");
    let sym_arr = symbol_to_bytes(sym);

    let mut results = Vec::with_capacity(num_trades);
    let mut offset = trades_start;

    for _ in 0..num_trades {
        if offset + block_length > body.len() {
            break;
        }

        let trade_id = read_i64_le(body, offset) as u64;
        let price = decode_decimal128(read_i64_le(body, offset + 8), price_exp);
        let vol = decode_decimal128(read_i64_le(body, offset + 16), qty_exp);
        let is_buyer_maker = body.get(offset + 24).copied().unwrap_or(0) != 0;

        results.push(MarketDataMsg::Trade(Trade {
            symbol: sym_arr,
            product_type: ProductType::Spot,
            event_timestamp_us: event_time_us,
            trade_timestamp_us: transact_time_us,
            trade_id,
            price,
            vol,
            is_buyer_maker,
            local_time_us: local_time,
        }));

        offset += block_length;
    }

    results
}

// ---------------------------------------------------------------------------
// Depth Snapshot (templateId=10002)
// ---------------------------------------------------------------------------
//
// Body layout:
//   Offset  0: eventTime       (i64, 8 bytes)
//   Offset  8: bookUpdateId    (i64, 8 bytes)
//   Offset 16: priceExponent   (i8,  1 byte) — shared
//   Offset 17: qtyExponent     (i8,  1 byte) — shared
//   Offset 18: bids groupSize16Encoding (4 bytes: blockLength u16 + numInGroup u16)
//     Per bid: price mantissa (i64) + qty mantissa (i64)
//   After bids: asks groupSize16Encoding (4 bytes)
//     Per ask: same layout
//   After asks: symbol VarString8

const DEPTH_ROOT_SIZE: usize = 18;
const DEPTH_GROUP_HEADER_SIZE: usize = 4; // groupSize16Encoding: u16 + u16

fn parse_depth(body: &[u8]) -> Option<MarketDataMsg> {
    if body.len() < DEPTH_ROOT_SIZE + DEPTH_GROUP_HEADER_SIZE {
        return None;
    }

    let local_time = time_util::now_us();

    let event_time_us = read_i64_le(body, 0) as u64;
    let update_id = read_i64_le(body, 8) as u64;
    let price_exp = body[16] as i8;
    let qty_exp = body[17] as i8;

    let mut depth = Depth5 {
        symbol: [0; 32],
        product_type: ProductType::Spot,
        event_timestamp_us: event_time_us,
        trade_timestamp_us: event_time_us,
        update_id,
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

    let mut offset = DEPTH_ROOT_SIZE;

    // --- Bids group ---
    if offset + DEPTH_GROUP_HEADER_SIZE > body.len() {
        return Some(MarketDataMsg::Depth5(depth));
    }
    let bid_block_len = read_u16_le(body, offset) as usize;
    let num_bids = read_u16_le(body, offset + 2) as usize;
    offset += DEPTH_GROUP_HEADER_SIZE;

    let bid_levels = num_bids.min(5);
    for i in 0..bid_levels {
        if offset + bid_block_len > body.len() {
            break;
        }
        depth.bid_prices[i] = decode_decimal128(read_i64_le(body, offset), price_exp);
        depth.bid_vols[i] = decode_decimal128(read_i64_le(body, offset + 8), qty_exp);
        depth.bid_level = (i + 1) as u32;
        offset += bid_block_len;
    }
    // Skip remaining bid levels beyond 5
    for _ in bid_levels..num_bids {
        offset += bid_block_len;
    }

    // --- Asks group ---
    if offset + DEPTH_GROUP_HEADER_SIZE > body.len() {
        return Some(MarketDataMsg::Depth5(depth));
    }
    let ask_block_len = read_u16_le(body, offset) as usize;
    let num_asks = read_u16_le(body, offset + 2) as usize;
    offset += DEPTH_GROUP_HEADER_SIZE;

    let ask_levels = num_asks.min(5);
    for i in 0..ask_levels {
        if offset + ask_block_len > body.len() {
            break;
        }
        depth.ask_prices[i] = decode_decimal128(read_i64_le(body, offset), price_exp);
        depth.ask_vols[i] = decode_decimal128(read_i64_le(body, offset + 8), qty_exp);
        depth.ask_level = (i + 1) as u32;
        offset += ask_block_len;
    }
    for _ in ask_levels..num_asks {
        offset += ask_block_len;
    }

    // --- Symbol (VarString8 at end) ---
    let sym_bytes = read_var_string8(body, offset);
    let sym = std::str::from_utf8(sym_bytes).unwrap_or("");
    depth.symbol = symbol_to_bytes(sym);

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
        data[2] = 0x0F;
        data[3] = 0x27; // templateId = 9999 LE
        assert!(parse_sbe_message(&data).is_empty());
    }

    #[test]
    fn decimal128_conversion() {
        assert!((decode_decimal128(50000, 0) - 50000.0).abs() < 0.001);
        assert!((decode_decimal128(123456789, -6) - 123.456789).abs() < 1e-6);
        assert!((decode_decimal128(1, -8) - 1e-8).abs() < 1e-20);
        assert!((decode_decimal128(-5000, -2) - (-50.0)).abs() < 0.001);
    }

    /// Helper to build a full SBE message with header + body.
    fn make_sbe_msg(template_id: u16, body: &[u8]) -> Vec<u8> {
        let block_length: u16 = 0;
        let schema_id: u16 = 1;
        let version: u16 = 1;
        let mut msg = Vec::with_capacity(SBE_HEADER_SIZE + body.len());
        msg.extend_from_slice(&block_length.to_le_bytes());
        msg.extend_from_slice(&template_id.to_le_bytes());
        msg.extend_from_slice(&schema_id.to_le_bytes());
        msg.extend_from_slice(&version.to_le_bytes());
        msg.extend_from_slice(body);
        msg
    }

    fn append_i64(buf: &mut Vec<u8>, val: i64) {
        buf.extend_from_slice(&val.to_le_bytes());
    }

    fn append_u16(buf: &mut Vec<u8>, val: u16) {
        buf.extend_from_slice(&val.to_le_bytes());
    }

    fn append_u32(buf: &mut Vec<u8>, val: u32) {
        buf.extend_from_slice(&val.to_le_bytes());
    }

    fn append_var_string8(buf: &mut Vec<u8>, s: &str) {
        buf.push(s.len() as u8);
        buf.extend_from_slice(s.as_bytes());
    }

    #[test]
    fn parse_best_bid_ask_msg() {
        // Build body: eventTime(8) + updateId(8) + priceExp(1) + qtyExp(1) +
        //             bidPrice(8) + bidQty(8) + askPrice(8) + askQty(8) +
        //             symbol VarString8
        let mut body = Vec::new();
        let event_time: i64 = 1_700_000_000_000_000; // microseconds
        let update_id: i64 = 42;
        let price_exp: i8 = -2; // prices in cents: mantissa * 10^(-2)
        let qty_exp: i8 = -4; // qty: mantissa * 10^(-4)

        append_i64(&mut body, event_time); // offset 0
        append_i64(&mut body, update_id); // offset 8
        body.push(price_exp as u8); // offset 16
        body.push(qty_exp as u8); // offset 17
        append_i64(&mut body, 3000050); // offset 18: bidPrice = 30000.50
        append_i64(&mut body, 15000); // offset 26: bidQty = 1.5000
        append_i64(&mut body, 3000100); // offset 34: askPrice = 30001.00
        append_i64(&mut body, 20000); // offset 42: askQty = 2.0000
        append_var_string8(&mut body, "BTCUSDT"); // offset 50

        let data = make_sbe_msg(TEMPLATE_BEST_BID_ASK, &body);
        let msgs = parse_sbe_message(&data);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            MarketDataMsg::Bbo(bbo) => {
                assert_eq!(symbol_from_bytes(&bbo.symbol), "BTCUSDT");
                assert_eq!(bbo.event_timestamp_us, event_time as u64);
                assert_eq!(bbo.update_id, 42);
                assert!((bbo.bid_price - 30000.50).abs() < 0.01);
                assert!((bbo.bid_vol - 1.5).abs() < 0.001);
                assert!((bbo.ask_price - 30001.00).abs() < 0.01);
                assert!((bbo.ask_vol - 2.0).abs() < 0.001);
                assert_eq!(bbo.product_type, ProductType::Spot);
            }
            _ => panic!("expected Bbo"),
        }
    }

    #[test]
    fn parse_trades_msg() {
        // Build body: eventTime(8) + transactTime(8) + priceExp(1) + qtyExp(1) +
        //             groupSizeEncoding(6) + N trade entries + symbol VarString8
        let mut body = Vec::new();
        let event_time: i64 = 1_700_000_000_000_000;
        let transact_time: i64 = 1_700_000_000_000_100;
        let price_exp: i8 = -2;
        let qty_exp: i8 = -3;

        append_i64(&mut body, event_time); // offset 0
        append_i64(&mut body, transact_time); // offset 8
        body.push(price_exp as u8); // offset 16
        body.push(qty_exp as u8); // offset 17

        // groupSizeEncoding: blockLength=25 (8+8+8+1), numInGroup=2
        let block_length: u16 = 25;
        append_u16(&mut body, block_length); // offset 18
        append_u32(&mut body, 2); // offset 20: 2 trades

        // Trade 1
        append_i64(&mut body, 100001); // tradeId
        append_i64(&mut body, 3000050); // price mantissa: 30000.50
        append_i64(&mut body, 1500); // qty mantissa: 1.500
        body.push(1); // isBuyerMaker = true

        // Trade 2
        append_i64(&mut body, 100002); // tradeId
        append_i64(&mut body, 3000100); // price mantissa: 30001.00
        append_i64(&mut body, 500); // qty mantissa: 0.500
        body.push(0); // isBuyerMaker = false

        append_var_string8(&mut body, "ETHUSDT");

        let data = make_sbe_msg(TEMPLATE_TRADES, &body);
        let msgs = parse_sbe_message(&data);
        assert_eq!(msgs.len(), 2);

        match &msgs[0] {
            MarketDataMsg::Trade(t) => {
                assert_eq!(symbol_from_bytes(&t.symbol), "ETHUSDT");
                assert_eq!(t.trade_id, 100001);
                assert!((t.price - 30000.50).abs() < 0.01);
                assert!((t.vol - 1.5).abs() < 0.001);
                assert!(t.is_buyer_maker);
                assert_eq!(t.event_timestamp_us, event_time as u64);
                assert_eq!(t.trade_timestamp_us, transact_time as u64);
            }
            _ => panic!("expected Trade"),
        }
        match &msgs[1] {
            MarketDataMsg::Trade(t) => {
                assert_eq!(t.trade_id, 100002);
                assert!((t.price - 30001.00).abs() < 0.01);
                assert!(!t.is_buyer_maker);
            }
            _ => panic!("expected Trade"),
        }
    }

    #[test]
    fn parse_trades_empty_group() {
        let mut body = Vec::new();
        append_i64(&mut body, 1_000_000); // eventTime
        append_i64(&mut body, 1_000_000); // transactTime
        body.push(0); // priceExponent
        body.push(0); // qtyExponent
        append_u16(&mut body, 25); // blockLength
        append_u32(&mut body, 0); // numInGroup = 0
        append_var_string8(&mut body, "BTCUSDT");

        let data = make_sbe_msg(TEMPLATE_TRADES, &body);
        let msgs = parse_sbe_message(&data);
        assert!(msgs.is_empty());
    }

    #[test]
    fn parse_depth_msg() {
        // Build body: eventTime(8) + updateId(8) + priceExp(1) + qtyExp(1) +
        //             bids_header(4) + bid_levels + asks_header(4) + ask_levels +
        //             symbol VarString8
        let mut body = Vec::new();
        let event_time: i64 = 1_700_000_000_000_000;
        let update_id: i64 = 999;
        let price_exp: i8 = -2;
        let qty_exp: i8 = -3;

        append_i64(&mut body, event_time); // offset 0
        append_i64(&mut body, update_id); // offset 8
        body.push(price_exp as u8); // offset 16
        body.push(qty_exp as u8); // offset 17

        // Bids: 3 levels, blockLength=16 (8+8)
        append_u16(&mut body, 16); // bids blockLength
        append_u16(&mut body, 3); // bids count
        // Bid 0: price=30000.00, qty=1.000
        append_i64(&mut body, 3000000);
        append_i64(&mut body, 1000);
        // Bid 1: price=29999.50, qty=2.000
        append_i64(&mut body, 2999950);
        append_i64(&mut body, 2000);
        // Bid 2: price=29999.00, qty=0.500
        append_i64(&mut body, 2999900);
        append_i64(&mut body, 500);

        // Asks: 2 levels
        append_u16(&mut body, 16);
        append_u16(&mut body, 2);
        // Ask 0: price=30000.50, qty=0.800
        append_i64(&mut body, 3000050);
        append_i64(&mut body, 800);
        // Ask 1: price=30001.00, qty=3.000
        append_i64(&mut body, 3000100);
        append_i64(&mut body, 3000);

        append_var_string8(&mut body, "BTCUSDT");

        let data = make_sbe_msg(TEMPLATE_DEPTH, &body);
        let msgs = parse_sbe_message(&data);
        assert_eq!(msgs.len(), 1);

        match &msgs[0] {
            MarketDataMsg::Depth5(d) => {
                assert_eq!(symbol_from_bytes(&d.symbol), "BTCUSDT");
                assert_eq!(d.update_id, 999);
                assert_eq!(d.bid_level, 3);
                assert_eq!(d.ask_level, 2);
                assert!((d.bid_prices[0] - 30000.00).abs() < 0.01);
                assert!((d.bid_vols[0] - 1.0).abs() < 0.001);
                assert!((d.bid_prices[2] - 29999.00).abs() < 0.01);
                assert!((d.ask_prices[0] - 30000.50).abs() < 0.01);
                assert!((d.ask_prices[1] - 30001.00).abs() < 0.01);
                assert!((d.ask_vols[1] - 3.0).abs() < 0.001);
            }
            _ => panic!("expected Depth5"),
        }
    }

    #[test]
    fn parse_depth_more_than_5_levels() {
        let mut body = Vec::new();
        append_i64(&mut body, 1_000_000);
        append_i64(&mut body, 1);
        body.push(-2i8 as u8);
        body.push(-3i8 as u8);

        // 7 bid levels
        append_u16(&mut body, 16);
        append_u16(&mut body, 7);
        for i in 0..7u64 {
            append_i64(&mut body, (3000000 - i * 100) as i64);
            append_i64(&mut body, 1000);
        }

        // 6 ask levels
        append_u16(&mut body, 16);
        append_u16(&mut body, 6);
        for i in 0..6u64 {
            append_i64(&mut body, (3000100 + i * 100) as i64);
            append_i64(&mut body, 500);
        }

        append_var_string8(&mut body, "BTCUSDT");

        let data = make_sbe_msg(TEMPLATE_DEPTH, &body);
        let msgs = parse_sbe_message(&data);
        assert_eq!(msgs.len(), 1);

        match &msgs[0] {
            MarketDataMsg::Depth5(d) => {
                assert_eq!(d.bid_level, 5);
                assert_eq!(d.ask_level, 5);
                assert!((d.bid_prices[4] - 29996.00).abs() < 0.01);
                assert!((d.ask_prices[4] - 30005.00).abs() < 0.01);
            }
            _ => panic!("expected Depth5"),
        }
    }
}
