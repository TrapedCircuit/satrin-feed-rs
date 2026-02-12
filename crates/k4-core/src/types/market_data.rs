//! Market data structures — the core data types flowing through the system.
//!
//! These structs are `#[repr(C)]` and `Copy` so they can be stored directly in
//! shared memory ring buffers without serialization overhead. For UDP
//! serialization, they derive `rkyv::Archive` for safe zero-copy ser/deser.
//!
//! # Timestamp convention
//!
//! All timestamps are in **microseconds since Unix epoch** (us), matching the
//! C++ convention of `E * 1000` (exchange sends milliseconds, we multiply by 1000).

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

use super::enums::ProductType;
use super::symbol::SYMBOL_LEN;

// ---------------------------------------------------------------------------
// Bookticker (Best Bid / Offer)
// ---------------------------------------------------------------------------

/// Best bid and offer quote — the tightest spread on the order book.
#[derive(Debug, Clone, Copy, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[repr(C)]
pub struct Bookticker {
    pub symbol: [u8; SYMBOL_LEN],
    pub product_type: ProductType,
    pub event_timestamp_us: u64,
    pub trade_timestamp_us: u64,
    pub update_id: u64,
    pub bid_price: f64,
    pub bid_vol: f64,
    pub ask_price: f64,
    pub ask_vol: f64,
    pub bid_order_count: i32,
    pub ask_order_count: i32,
    pub local_time_us: u64,
}

// ---------------------------------------------------------------------------
// Trade
// ---------------------------------------------------------------------------

/// A single trade execution.
#[derive(Debug, Clone, Copy, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[repr(C)]
pub struct Trade {
    pub symbol: [u8; SYMBOL_LEN],
    pub product_type: ProductType,
    pub event_timestamp_us: u64,
    pub trade_timestamp_us: u64,
    pub trade_id: u64,
    pub price: f64,
    pub vol: f64,
    pub is_buyer_maker: bool,
    pub local_time_us: u64,
}

// ---------------------------------------------------------------------------
// AggTrade (Aggregated Trade)
// ---------------------------------------------------------------------------

/// Aggregated trade — multiple fills at the same price grouped together.
///
/// Only Binance provides native aggTrades; other exchanges produce individual
/// trades instead.
#[derive(Debug, Clone, Copy, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[repr(C)]
pub struct AggTrade {
    pub symbol: [u8; SYMBOL_LEN],
    pub product_type: ProductType,
    pub event_timestamp_us: u64,
    pub trade_timestamp_us: u64,
    pub first_trade_id: u64,
    pub last_trade_id: u64,
    pub agg_trade_id: u64,
    pub price: f64,
    pub vol: f64,
    pub trade_count: i32,
    pub is_buyer_maker: bool,
    pub local_time_us: u64,
}

// ---------------------------------------------------------------------------
// Depth5 (5-level order book snapshot)
// ---------------------------------------------------------------------------

/// Top-5-level order book snapshot.
///
/// `bid_prices[0]` is the best (highest) bid, `ask_prices[0]` is the best
/// (lowest) ask.
#[derive(Debug, Clone, Copy, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[repr(C)]
pub struct Depth5 {
    pub symbol: [u8; SYMBOL_LEN],
    pub product_type: ProductType,
    pub event_timestamp_us: u64,
    pub trade_timestamp_us: u64,
    pub update_id: u64,
    pub bid_level: u32,
    pub ask_level: u32,
    pub last_price: f64,
    pub bid_prices: [f64; 5],
    pub bid_vols: [f64; 5],
    pub ask_prices: [f64; 5],
    pub ask_vols: [f64; 5],
    pub bid_order_counts: [i32; 5],
    pub ask_order_counts: [i32; 5],
    pub local_time_us: u64,
}

// ---------------------------------------------------------------------------
// MarketDataMsg — tagged union for channel passing
// ---------------------------------------------------------------------------

/// A tagged union of all market-data message types.
#[derive(Debug, Clone)]
pub enum MarketDataMsg {
    Bbo(Bookticker),
    Trade(Trade),
    AggTrade(AggTrade),
    Depth5(Depth5),
}

// ---------------------------------------------------------------------------
// Default impls
// ---------------------------------------------------------------------------

impl Default for Bookticker {
    fn default() -> Self {
        Self {
            symbol: [0; SYMBOL_LEN],
            product_type: ProductType::default(),
            event_timestamp_us: 0,
            trade_timestamp_us: 0,
            update_id: 0,
            bid_price: 0.0,
            bid_vol: 0.0,
            ask_price: 0.0,
            ask_vol: 0.0,
            bid_order_count: 0,
            ask_order_count: 0,
            local_time_us: 0,
        }
    }
}

impl Default for Trade {
    fn default() -> Self {
        Self {
            symbol: [0; SYMBOL_LEN],
            product_type: ProductType::default(),
            event_timestamp_us: 0,
            trade_timestamp_us: 0,
            trade_id: 0,
            price: 0.0,
            vol: 0.0,
            is_buyer_maker: false,
            local_time_us: 0,
        }
    }
}

impl Default for AggTrade {
    fn default() -> Self {
        Self {
            symbol: [0; SYMBOL_LEN],
            product_type: ProductType::default(),
            event_timestamp_us: 0,
            trade_timestamp_us: 0,
            first_trade_id: 0,
            last_trade_id: 0,
            agg_trade_id: 0,
            price: 0.0,
            vol: 0.0,
            trade_count: 0,
            is_buyer_maker: false,
            local_time_us: 0,
        }
    }
}

impl Default for Depth5 {
    fn default() -> Self {
        Self {
            symbol: [0; SYMBOL_LEN],
            product_type: ProductType::default(),
            event_timestamp_us: 0,
            trade_timestamp_us: 0,
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
            local_time_us: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Display impls
// ---------------------------------------------------------------------------

impl std::fmt::Display for Bookticker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sym = super::symbol::symbol_from_bytes(&self.symbol);
        write!(
            f,
            "BBO({sym} bid={:.8}x{:.4} ask={:.8}x{:.4} uid={})",
            self.bid_price, self.bid_vol, self.ask_price, self.ask_vol, self.update_id
        )
    }
}

impl std::fmt::Display for Trade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sym = super::symbol::symbol_from_bytes(&self.symbol);
        let side = if self.is_buyer_maker { "SELL" } else { "BUY" };
        write!(
            f,
            "Trade({sym} {side} {:.8}x{:.4} id={})",
            self.price, self.vol, self.trade_id
        )
    }
}

impl std::fmt::Display for AggTrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sym = super::symbol::symbol_from_bytes(&self.symbol);
        let side = if self.is_buyer_maker { "SELL" } else { "BUY" };
        write!(
            f,
            "AggTrade({sym} {side} {:.8}x{:.4} id={})",
            self.price, self.vol, self.agg_trade_id
        )
    }
}

impl std::fmt::Display for Depth5 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sym = super::symbol::symbol_from_bytes(&self.symbol);
        write!(
            f,
            "Depth5({sym} bid[0]={:.8} ask[0]={:.8} uid={})",
            self.bid_prices[0], self.ask_prices[0], self.update_id
        )
    }
}
