//! Asynchronous UDP sender and receiver for market data distribution.
//!
//! Uses `rkyv` for safe zero-copy serialization — no `unsafe` pointer
//! operations needed. The wire format is:
//!
//! ```text
//! ┌────────────┬────────────────────────────────────┐
//! │ msg_type   │ rkyv-serialized payload             │
//! │ i8 (1 byte)│ variable length                     │
//! └────────────┴────────────────────────────────────┘
//! ```

use std::net::SocketAddr;

use tokio::{net::UdpSocket, sync::mpsc};
use tracing::{debug, error, warn};

use crate::types::{AggTrade, Bookticker, Depth5, MarketDataMsg, MessageType, Trade};

/// Maximum UDP payload size.
const MAX_UDP_PAYLOAD: usize = 65507;

// ---------------------------------------------------------------------------
// UdpSender
// ---------------------------------------------------------------------------

/// Asynchronous UDP market data sender.
///
/// Messages are submitted via an MPSC channel and sent from a background tokio
/// task. This decouples the hot path (market data parsing) from network I/O.
pub struct UdpSender {
    tx: mpsc::Sender<MarketDataMsg>,
    _task: tokio::task::JoinHandle<()>,
}

impl UdpSender {
    /// Create and start a new UDP sender targeting `dest_addr`.
    pub async fn new(dest_addr: SocketAddr) -> anyhow::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(dest_addr).await?;
        let (tx, mut rx) = mpsc::channel::<MarketDataMsg>(4096);

        let task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match encode_msg(&msg) {
                    Some(bytes) => {
                        if let Err(e) = socket.send(&bytes).await {
                            warn!("UDP send error: {e}");
                        }
                    }
                    None => {
                        warn!("UDP encode failed, dropping message");
                    }
                }
            }
            debug!("UDP sender task exited");
        });

        Ok(Self { tx, _task: task })
    }

    /// Enqueue a market data message for sending.
    ///
    /// Returns immediately. If the channel is full, the message is dropped.
    #[inline]
    pub fn send(&self, msg: MarketDataMsg) {
        if self.tx.try_send(msg).is_err() {
            warn!("UDP sender channel full, dropping message");
        }
    }
}

/// Encode a `MarketDataMsg` into bytes: `[msg_type: u8] ++ [rkyv payload]`.
fn encode_msg(msg: &MarketDataMsg) -> Option<Vec<u8>> {
    // Helper to prepend msg_type byte to rkyv-serialized payload
    fn with_type(msg_type: MessageType, payload: rkyv::util::AlignedVec) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + payload.len());
        buf.push(msg_type as u8);
        buf.extend_from_slice(&payload);
        buf
    }

    type E = rkyv::rancor::Error;
    match msg {
        MarketDataMsg::Bbo(d) => Some(with_type(MessageType::BookTicker, rkyv::to_bytes::<E>(d).ok()?)),
        MarketDataMsg::Trade(d) => Some(with_type(MessageType::Trade, rkyv::to_bytes::<E>(d).ok()?)),
        MarketDataMsg::AggTrade(d) => Some(with_type(MessageType::AggTrade, rkyv::to_bytes::<E>(d).ok()?)),
        MarketDataMsg::Depth5(d) => Some(with_type(MessageType::Depth5, rkyv::to_bytes::<E>(d).ok()?)),
    }
}

// ---------------------------------------------------------------------------
// UdpReceiver
// ---------------------------------------------------------------------------

/// Callback handler for received UDP market data.
pub struct UdpCallbackHandler {
    pub on_bbo: Option<Box<dyn Fn(Bookticker) + Send>>,
    pub on_trade: Option<Box<dyn Fn(Trade) + Send>>,
    pub on_agg_trade: Option<Box<dyn Fn(AggTrade) + Send>>,
    pub on_depth5: Option<Box<dyn Fn(Depth5) + Send>>,
}

/// Asynchronous UDP receiver.
pub struct UdpReceiver {
    socket: UdpSocket,
}

impl UdpReceiver {
    /// Bind a UDP socket on the given address.
    pub async fn bind(addr: SocketAddr) -> anyhow::Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        Ok(Self { socket })
    }

    /// Run the receive loop, dispatching messages to `handler`.
    pub async fn run(self, handler: UdpCallbackHandler) -> anyhow::Result<()> {
        let mut buf = vec![0u8; MAX_UDP_PAYLOAD];

        loop {
            let n = match self.socket.recv(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    error!("UDP recv error: {e}");
                    continue;
                }
            };

            if n < 2 {
                continue; // Need at least msg_type + 1 byte payload
            }

            let msg_type = buf[0];
            let payload = &buf[1..n];

            dispatch_payload(msg_type, payload, &handler);
        }
    }
}

/// Dispatch a received payload to the appropriate callback.
///
/// Uses `rkyv::from_bytes` for safe, validated deserialization.
/// Copy payload into an aligned buffer and decode with rkyv.
macro_rules! decode_rkyv {
    ($T:ty, $payload:expr) => {{
        let mut a = rkyv::util::AlignedVec::<8>::with_capacity($payload.len());
        a.extend_from_slice($payload);
        rkyv::from_bytes::<$T, rkyv::rancor::Error>(&a).ok()
    }};
}

fn dispatch_payload(msg_type: u8, payload: &[u8], handler: &UdpCallbackHandler) {
    match msg_type {
        t if t == MessageType::BookTicker as u8 => {
            if let Some(cb) = &handler.on_bbo
                && let Some(bbo) = decode_rkyv!(Bookticker, payload)
            {
                cb(bbo);
            }
        }
        t if t == MessageType::Trade as u8 => {
            if let Some(cb) = &handler.on_trade
                && let Some(trade) = decode_rkyv!(Trade, payload)
            {
                cb(trade);
            }
        }
        t if t == MessageType::AggTrade as u8 => {
            if let Some(cb) = &handler.on_agg_trade
                && let Some(agg) = decode_rkyv!(AggTrade, payload)
            {
                cb(agg);
            }
        }
        t if t == MessageType::Depth5 as u8 => {
            if let Some(cb) = &handler.on_depth5
                && let Some(depth) = decode_rkyv!(Depth5, payload)
            {
                cb(depth);
            }
        }
        _ => {
            debug!("Unknown UDP message type: {msg_type}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn encode_decode_bookticker() {
        let bbo = Bookticker {
            symbol: symbol_to_bytes("BTCUSDT"),
            product_type: ProductType::Spot,
            event_timestamp_us: 1672515782136000,
            trade_timestamp_us: 1672515782136000,
            update_id: 123456,
            bid_price: 50000.0,
            bid_vol: 1.5,
            ask_price: 50001.0,
            ask_vol: 2.0,
            bid_order_count: 10,
            ask_order_count: 20,
            local_time_us: 1672515782137000,
        };

        // Encode via encode_msg
        let bytes = encode_msg(&MarketDataMsg::Bbo(bbo)).unwrap();
        assert_eq!(bytes[0], MessageType::BookTicker as u8);

        // Decode
        let payload = &bytes[1..];
        let decoded = decode_rkyv!(Bookticker, payload).expect("rkyv decode failed");
        assert_eq!(decoded.bid_price, bbo.bid_price);
        assert_eq!(decoded.ask_price, bbo.ask_price);
        assert_eq!(decoded.update_id, bbo.update_id);
        assert_eq!(decoded.product_type, ProductType::Spot);
        assert_eq!(symbol_from_bytes(&decoded.symbol), "BTCUSDT");
    }

    #[test]
    fn encode_decode_trade() {
        let trade = Trade {
            symbol: symbol_to_bytes("ETHUSDT"),
            product_type: ProductType::Futures,
            event_timestamp_us: 100000,
            trade_timestamp_us: 100000,
            trade_id: 999,
            price: 3000.5,
            vol: 10.0,
            is_buyer_maker: true,
            local_time_us: 100001,
        };

        let bytes = encode_msg(&MarketDataMsg::Trade(trade)).unwrap();
        let decoded = decode_rkyv!(Trade, &bytes[1..]).unwrap();
        assert_eq!(decoded.price, trade.price);
        assert!(decoded.is_buyer_maker);
        assert_eq!(decoded.product_type, ProductType::Futures);
    }
}
