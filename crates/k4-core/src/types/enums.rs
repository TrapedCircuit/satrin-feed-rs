//! Enumerations used throughout the crypto-gateway system.
//!
//! These enums map 1:1 to the C++ definitions in `K4_CryptoDatatype.h` and
//! `K4_Struct.h`, providing a type-safe Rust equivalent.
//!
//! All enums used in market data structs derive `rkyv::Archive` for zero-copy
//! UDP serialization.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Exchange identifiers
// ---------------------------------------------------------------------------

/// Supported cryptocurrency exchanges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Exchange {
    Binance,
    Okx,
    Bitget,
    Bybit,
}

impl std::fmt::Display for Exchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Binance => write!(f, "binance"),
            Self::Okx => write!(f, "okx"),
            Self::Bitget => write!(f, "bitget"),
            Self::Bybit => write!(f, "bybit"),
        }
    }
}

// ---------------------------------------------------------------------------
// Product types
// ---------------------------------------------------------------------------

/// Product (instrument) category.
///
/// Maps to the C++ `ProductType` enum. The discriminant values are preserved
/// for wire-format compatibility with the UDP schema.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[repr(u8)]
#[derive(Default)]
pub enum ProductType {
    #[default]
    Spot = 0,
    Futures = 1,
    UMargin = 2,
    CoinMargin = 3,
    Options = 4,
    UsdtFutures = 5,
    UsdcFutures = 6,
    BtcMargin = 7,
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// Discriminant for the kind of market-data or user-data message.
///
/// Used as the `msg_type` field in the UDP wire format header.
/// Note: kept as `repr(i8)` for wire compatibility but NOT Archive-derived
/// because rkyv's CheckBytes conflicts with negative and gapped discriminants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(i8)]
pub enum MessageType {
    DataError = -1,
    BookTicker = 0,
    Trade = 1,
    AggTrade = 2,
    Depth5 = 3,
    OrderUpdate = 4,
    TradeUpdate = 5,
    QueryOrderResponse = 6,
    QueryInternalResponse = 7,
    DataUnknown = 100,
    Heartbeat = 101,
}

// ---------------------------------------------------------------------------
// Trade metadata
// ---------------------------------------------------------------------------

/// Indicates whether a trade was a market trade, ADL, or insurance fund.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum TradeType {
    #[default]
    Market = 0,
    Adl = 1,
    InsuranceFund = 2,
}

// ---------------------------------------------------------------------------
// Order / trading enums
// ---------------------------------------------------------------------------

/// Order status — unified across all exchanges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
    PendingCancel,
}

/// Buy or sell direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Buy,
    Sell,
}

/// Order type (time-in-force variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
    PostOnly,
    Gtc,
    Fok,
    Ioc,
}

/// Account type for multi-account exchanges (e.g. Binance Spot vs UBase).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccountType {
    Spot,
    UBased,
    CBased,
}

/// Action type for order requests routed through message queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionType {
    Create,
    Cancel,
    Query,
    CancelAll,
    CancelByStrategy,
    Transfer,
}

/// Price type for order placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PriceType {
    Any,
    Limit,
    LimitMaker,
    Market,
}
