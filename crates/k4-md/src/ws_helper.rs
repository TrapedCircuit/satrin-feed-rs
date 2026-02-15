//! Shared WebSocket connection helpers for market data modules.

use std::{collections::HashMap, sync::Arc};

use crossbeam_channel::Sender;
use k4_core::{
    types::MarketDataMsg,
    ws::client::{OnBinaryCallback, OnMessageCallback, WsConnConfig, WsConnection},
};
use tracing::warn;

use crate::pipeline::PingConfig;

/// Parameters for a text-mode WebSocket MD stream.
pub struct TextStreamParams<F> {
    pub url: String,
    pub subscribe_msg: String,
    pub extra_headers: HashMap<String, String>,
    pub ping: Option<PingConfig>,
    pub tx: Sender<MarketDataMsg>,
    pub parser: F,
    pub label: String,
}

/// Start a text-mode WebSocket connection that parses messages and sends
/// them to the dedup channel. Blocks until cancelled.
pub async fn run_ws_text_stream<F>(params: TextStreamParams<F>)
where
    F: Fn(&str) -> Vec<MarketDataMsg> + Send + Sync + 'static,
{
    let TextStreamParams { url, subscribe_msg, extra_headers, ping, tx, parser, label } = params;

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
        ping_interval: ping.as_ref().map(|p| p.interval),
        ping_payload: ping.map(|p| p.payload),
        id: 0,
    };

    let mut conn = WsConnection::new(config);
    conn.start(on_msg, None);
    std::future::pending::<()>().await;
    conn.stop().await;
}

/// Parameters for a binary-mode WebSocket MD stream.
pub struct BinaryStreamParams<F> {
    pub url: String,
    pub subscribe_msg: String,
    pub extra_headers: HashMap<String, String>,
    pub tx: Sender<MarketDataMsg>,
    pub parser: F,
    pub label: String,
}

/// Start a binary-mode WebSocket connection (e.g. Binance SBE). Blocks
/// until cancelled.
pub async fn run_ws_binary_stream<F>(params: BinaryStreamParams<F>)
where
    F: Fn(&[u8]) -> Vec<MarketDataMsg> + Send + Sync + 'static,
{
    let BinaryStreamParams { url, subscribe_msg, extra_headers, tx, parser, label } = params;

    let tx_clone = tx.clone();
    let label_clone = label.clone();
    let on_binary: OnBinaryCallback = Arc::new(move |_conn_id, data| {
        for msg in parser(data) {
            if tx_clone.try_send(msg).is_err() {
                warn!("[{label_clone}] SBE dedup channel full");
            }
        }
    });

    let on_text: OnMessageCallback = Arc::new(|_conn_id, _text| {});

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
