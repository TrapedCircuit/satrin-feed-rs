//! Shared WebSocket connection helpers for market data modules.
//!
//! Provides a generic `run_ws_md_stream` that handles connection setup,
//! subscription, message parsing, and forwarding to a dedup channel. This
//! replaces the per-exchange `start_ws_text_connection` functions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Sender;
use k4_core::types::MarketDataMsg;
use k4_core::ws::client::{
    OnBinaryCallback, OnMessageCallback, PingPayload, WsConnConfig, WsConnection,
};
use tracing::warn;

/// Start a text-mode WebSocket connection that parses messages and sends them
/// to the dedup channel.
///
/// The `parser` function converts a raw JSON string into zero or more
/// `MarketDataMsg` items. This is the main abstraction point — each exchange
/// provides its own parser.
///
/// This function blocks until the task is aborted (via `tokio::signal::ctrl_c`
/// or task cancellation).
pub async fn run_ws_text_stream<F>(
    url: String,
    subscribe_msg: String,
    extra_headers: HashMap<String, String>,
    ping_interval: Option<Duration>,
    ping_payload: Option<PingPayload>,
    tx: Sender<MarketDataMsg>,
    parser: F,
    label: String,
) where
    F: Fn(&str) -> Vec<MarketDataMsg> + Send + Sync + 'static,
{
    let on_msg: OnMessageCallback = Arc::new(move |_conn_id, text| {
        for msg in parser(text) {
            if tx.try_send(msg).is_err() {
                warn!("[{label}] dedup channel full");
            }
        }
    });

    let config = WsConnConfig {
        url,
        subscribe_msg: Some(subscribe_msg),
        extra_headers,
        ping_interval,
        ping_payload,
        id: 0,
    };

    let mut conn = WsConnection::new(config);
    conn.start(on_msg, None);

    // Keep alive until cancelled
    std::future::pending::<()>().await;
    conn.stop().await;
}

/// Start a binary-mode WebSocket connection (e.g. Binance SBE).
///
/// `binary_parser` handles binary frames; `text_parser` handles text frames
/// (typically subscription acks, which can be ignored).
pub async fn run_ws_binary_stream<F>(
    url: String,
    subscribe_msg: String,
    extra_headers: HashMap<String, String>,
    tx: Sender<MarketDataMsg>,
    binary_parser: F,
    label: String,
) where
    F: Fn(&[u8]) -> Vec<MarketDataMsg> + Send + Sync + 'static,
{
    let tx_clone = tx.clone();
    let label_clone = label.clone();
    let on_binary: OnBinaryCallback = Arc::new(move |_conn_id, data| {
        for msg in binary_parser(data) {
            if tx_clone.try_send(msg).is_err() {
                warn!("[{label_clone}] SBE dedup channel full");
            }
        }
    });

    let on_text: OnMessageCallback = Arc::new(|_conn_id, _text| {
        // Subscription ack — no action needed
    });

    let config = WsConnConfig {
        url,
        subscribe_msg: Some(subscribe_msg),
        extra_headers,
        ping_interval: None,
        ping_payload: None,
        id: 0,
    };

    let mut conn = WsConnection::new(config);
    conn.start(on_text, Some(on_binary));

    std::future::pending::<()>().await;
    conn.stop().await;
}
