//! Single WebSocket connection with auto-reconnect and ping keep-alive.
//!
//! Each `WsConnection` runs as a tokio task that:
//! 1. Connects to the exchange WebSocket endpoint (TLS).
//! 2. Sends the subscription message.
//! 3. Reads messages and forwards them to a callback.
//! 4. Sends periodic ping messages (exchange-specific format).
//! 5. Automatically reconnects on disconnection with exponential backoff.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

/// Callback invoked for each received text message.
///
/// Parameters: `(connection_id, message_text)`
pub type OnMessageCallback = Arc<dyn Fn(usize, &str) + Send + Sync>;

/// Callback invoked for each received binary message.
///
/// Parameters: `(connection_id, message_bytes)`
pub type OnBinaryCallback = Arc<dyn Fn(usize, &[u8]) + Send + Sync>;

/// Ping payload format — varies by exchange.
#[derive(Debug, Clone)]
pub enum PingPayload {
    /// Send a text frame (e.g. OKX/Bitget send `"ping"`).
    Text(String),
    /// Send a JSON object as text (e.g. Bybit `{"op":"ping"}`).
    Json(serde_json::Value),
    /// Use the standard WebSocket ping frame.
    WebSocketPing,
}

/// Configuration for a single WebSocket connection.
#[derive(Debug, Clone)]
pub struct WsConnConfig {
    /// Full WebSocket URL (e.g. `wss://stream.binance.com:443/ws`).
    pub url: String,
    /// Message to send immediately after connection (subscription request).
    pub subscribe_msg: Option<String>,
    /// Extra HTTP headers for the handshake.
    pub extra_headers: HashMap<String, String>,
    /// Interval between ping messages.
    pub ping_interval: Option<Duration>,
    /// Ping message format.
    pub ping_payload: Option<PingPayload>,
    /// Connection identifier (unique within a RedundantWsClient).
    pub id: usize,
}

/// A single WebSocket connection managed by a background tokio task.
pub struct WsConnection {
    /// Connection configuration.
    pub config: WsConnConfig,
    /// Channel to send outbound messages.
    outbound_tx: Option<mpsc::Sender<String>>,
    /// Shutdown signal sender.
    shutdown_tx: Option<watch::Sender<bool>>,
    /// Task join handle.
    task: Option<tokio::task::JoinHandle<()>>,
}

impl WsConnection {
    /// Create a new (not yet started) connection.
    pub fn new(config: WsConnConfig) -> Self {
        Self {
            config,
            outbound_tx: None,
            shutdown_tx: None,
            task: None,
        }
    }

    /// Start the connection task.
    ///
    /// Messages are forwarded to `on_text` (for text frames) and optionally
    /// `on_binary` (for binary frames, used by Binance SBE).
    pub fn start(&mut self, on_text: OnMessageCallback, on_binary: Option<OnBinaryCallback>) {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (outbound_tx, outbound_rx) = mpsc::channel::<String>(64);
        let config = self.config.clone();

        let task = tokio::spawn(async move {
            connection_loop(config, on_text, on_binary, outbound_rx, shutdown_rx).await;
        });

        self.shutdown_tx = Some(shutdown_tx);
        self.outbound_tx = Some(outbound_tx);
        self.task = Some(task);
    }

    /// Send a text message on this connection.
    pub async fn send(&self, msg: String) -> anyhow::Result<()> {
        if let Some(tx) = &self.outbound_tx {
            tx.send(msg).await?;
        }
        Ok(())
    }

    /// Stop the connection and wait for the task to finish.
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

/// Main connection loop — connects, subscribes, reads, pings, reconnects.
async fn connection_loop(
    config: WsConnConfig,
    on_text: OnMessageCallback,
    on_binary: Option<OnBinaryCallback>,
    mut outbound_rx: mpsc::Receiver<String>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut backoff = Duration::from_millis(100);
    let max_backoff = Duration::from_secs(30);
    let conn_id = config.id;

    loop {
        // Check shutdown before connecting
        if *shutdown_rx.borrow() {
            info!("[ws-{conn_id}] shutdown requested");
            return;
        }

        info!("[ws-{conn_id}] connecting to {}", config.url);

        let ws_stream = match connect_ws(&config).await {
            Ok(s) => {
                backoff = Duration::from_millis(100); // reset backoff on success
                info!("[ws-{conn_id}] connected");
                s
            }
            Err(e) => {
                error!("[ws-{conn_id}] connection failed: {e}, retrying in {backoff:?}");
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {},
                    _ = shutdown_rx.changed() => return,
                }
                backoff = (backoff * 2).min(max_backoff);
                continue;
            }
        };

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // Send subscription message
        if let Some(ref sub_msg) = config.subscribe_msg {
            debug!("[ws-{conn_id}] subscribing: {sub_msg}");
            if let Err(e) = ws_write.send(Message::Text(sub_msg.clone().into())).await {
                error!("[ws-{conn_id}] subscribe send failed: {e}");
                continue;
            }
        }

        // Set up ping timer
        let ping_interval = config.ping_interval.map(|d| tokio::time::interval(d));

        // Pin the interval for use in select!
        tokio::pin! {
            let ping_tick = async {
                if let Some(mut interval) = ping_interval {
                    loop {
                        interval.tick().await;
                    }
                } else {
                    // No pinging — wait forever
                    std::future::pending::<()>().await
                }
            };
        }

        // Main read/write loop
        loop {
            tokio::select! {
                // Shutdown signal
                _ = shutdown_rx.changed() => {
                    info!("[ws-{conn_id}] shutdown signal received");
                    let _ = ws_write.close().await;
                    return;
                }

                // Incoming message
                msg = ws_read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            on_text(conn_id, &text);
                        }
                        Some(Ok(Message::Binary(data))) => {
                            if let Some(ref cb) = on_binary {
                                cb(conn_id, &data);
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = ws_write.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Close(_))) => {
                            warn!("[ws-{conn_id}] received close frame");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("[ws-{conn_id}] read error: {e}");
                            break;
                        }
                        None => {
                            warn!("[ws-{conn_id}] stream ended");
                            break;
                        }
                        _ => {} // Pong, Frame — ignore
                    }
                }

                // Outbound message from user
                Some(msg) = outbound_rx.recv() => {
                    if let Err(e) = ws_write.send(Message::Text(msg.into())).await {
                        error!("[ws-{conn_id}] send error: {e}");
                        break;
                    }
                }

                // Ping timer
                _ = &mut ping_tick => {
                    let ping_msg = match &config.ping_payload {
                        Some(PingPayload::Text(t)) => Message::Text(t.clone().into()),
                        Some(PingPayload::Json(j)) => Message::Text(j.to_string().into()),
                        Some(PingPayload::WebSocketPing) | None => {
                            Message::Ping(vec![].into())
                        }
                    };
                    if let Err(e) = ws_write.send(ping_msg).await {
                        error!("[ws-{conn_id}] ping send error: {e}");
                        break;
                    }
                }
            }
        }

        // Disconnected — will reconnect at the top of the outer loop
        warn!("[ws-{conn_id}] disconnected, reconnecting in {backoff:?}");
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {},
            _ = shutdown_rx.changed() => return,
        }
        backoff = (backoff * 2).min(max_backoff);
    }
}

/// Establish a TLS WebSocket connection.
async fn connect_ws(
    config: &WsConnConfig,
) -> anyhow::Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    use tokio_tungstenite::tungstenite::http::Request;

    let mut request = Request::builder()
        .uri(&config.url)
        .header("Host", extract_host(&config.url));

    for (key, value) in &config.extra_headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let request = request.body(())?;

    let (stream, _response) = tokio_tungstenite::connect_async(request).await?;
    Ok(stream)
}

/// Extract the host from a URL string.
fn extract_host(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| u.host_str().unwrap_or("").to_string())
        .unwrap_or_default()
}
