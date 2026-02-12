//! Binance Spot account operations.
//!
//! Provides REST endpoints for listen key management, account queries, and
//! exchange info, as well as a WebSocket API client for low-latency order
//! placement and cancellation.
//!
//! # REST endpoints
//!
//! | Operation        | Method  | Path                       |
//! |------------------|---------|----------------------------|
//! | Create listen key| POST    | `/api/v3/userDataStream`   |
//! | Refresh listen key| PUT   | `/api/v3/userDataStream`   |
//! | Close listen key | DELETE  | `/api/v3/userDataStream`   |
//! | Account info     | GET     | `/api/v3/account`          |
//! | Open orders      | GET     | `/api/v3/openOrders`       |
//! | Exchange info    | GET     | `/api/v3/exchangeInfo`     |
//!
//! # WebSocket API (`wss://ws-api.binance.com/ws-api/v3`)
//!
//! | Operation    | Method         |
//! |-------------|----------------|
//! | Place order | `order.place`  |
//! | Cancel order| `order.cancel` |

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex, oneshot};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::auth;

// ---------------------------------------------------------------------------
// WebSocket API inner state (Send + Sync)
// ---------------------------------------------------------------------------

/// Shared state for the WebSocket API request-response correlation.
///
/// Stored behind `Arc` so that both the receiver task and callers can access
/// it concurrently.
pub(crate) struct WsApiInner {
    /// Channel for sending outbound JSON messages to the WS writer task.
    tx: tokio::sync::mpsc::Sender<String>,
    /// Map of pending request IDs → oneshot response senders (shared with the
    /// background reader task).
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>,
}

impl WsApiInner {
    /// Send a request and await the response (with timeout).
    async fn request(&self, id: &str, payload: serde_json::Value) -> Result<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            map.insert(id.to_string(), tx);
        }

        self.tx
            .send(payload.to_string())
            .await
            .context("WS API send channel closed")?;

        let response = tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .context("WS API request timed out")?
            .context("WS API response channel dropped")?;

        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// SpotClient
// ---------------------------------------------------------------------------

/// Binance Spot account client.
///
/// Handles REST API calls (signed with HMAC-SHA256) and optionally a
/// WebSocket API connection for order operations.
pub struct SpotClient {
    /// Shared HTTP client.
    http: reqwest::Client,
    /// API key (sent in `X-MBX-APIKEY` header).
    api_key: String,
    /// Secret key for HMAC-SHA256 signing.
    secret_key: String,
    /// REST base URL (e.g. `https://api.binance.com`).
    base_url: String,
    /// WebSocket API base URL.
    ws_api_url: String,
    /// `recvWindow` for signed requests.
    recv_window: u64,
    /// Active listen key for the user-data stream.
    listen_key: Mutex<Option<String>>,
    /// WebSocket API connection (lazy-initialized on first order).
    ws_api: Mutex<Option<Arc<WsApiInner>>>,
    /// Background task handles (WS reader, etc.).
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl SpotClient {
    /// Create a new spot client (no connections opened yet).
    pub fn new(
        api_key: String,
        secret_key: String,
        base_url: String,
        ws_api_url: String,
        recv_window: u64,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
            secret_key,
            base_url,
            ws_api_url,
            recv_window,
            listen_key: Mutex::new(None),
            ws_api: Mutex::new(None),
            tasks: Mutex::new(Vec::new()),
        }
    }

    // -----------------------------------------------------------------------
    // Listen key management
    // -----------------------------------------------------------------------

    /// Create a new listen key for the spot user-data stream.
    pub async fn create_listen_key(&self) -> Result<String> {
        let url = format!("{}/api/v3/userDataStream", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("create listen key request failed")?;

        let body: serde_json::Value = resp
            .error_for_status()
            .context("create listen key HTTP error")?
            .json()
            .await?;

        let key = body
            .get("listenKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("listenKey not found in response"))?
            .to_string();

        *self.listen_key.lock().await = Some(key.clone());
        info!("[spot] listen key created");
        Ok(key)
    }

    /// Send a keepalive ping for the current listen key.
    pub async fn keepalive_listen_key(&self) -> Result<()> {
        let key = self.listen_key.lock().await;
        let Some(ref listen_key) = *key else {
            return Err(anyhow!("no active listen key"));
        };

        let url = format!("{}/api/v3/userDataStream", self.base_url);
        self.http
            .put(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("listenKey", listen_key.as_str())])
            .send()
            .await
            .context("keepalive listen key request failed")?
            .error_for_status()
            .context("keepalive listen key HTTP error")?;

        debug!("[spot] listen key keepalive sent");
        Ok(())
    }

    /// Close (delete) the current listen key.
    pub async fn close_listen_key(&self) -> Result<()> {
        let key = self.listen_key.lock().await;
        let Some(ref listen_key) = *key else {
            return Ok(());
        };

        let url = format!("{}/api/v3/userDataStream", self.base_url);
        self.http
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("listenKey", listen_key.as_str())])
            .send()
            .await
            .context("close listen key request failed")?
            .error_for_status()
            .context("close listen key HTTP error")?;

        info!("[spot] listen key closed");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // REST queries
    // -----------------------------------------------------------------------

    /// Query spot account information (balances, permissions).
    pub async fn get_account_info(&self) -> Result<serde_json::Value> {
        let timestamp = current_timestamp_ms();
        let query = auth::build_signed_query(
            &[
                ("recvWindow", &self.recv_window.to_string()),
                ("timestamp", &timestamp),
            ],
            &self.secret_key,
        );

        let url = format!("{}/api/v3/account?{}", self.base_url, query);
        let resp: serde_json::Value = self
            .http
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp)
    }

    /// Query all open orders (optionally filtered by symbol).
    pub async fn get_open_orders(&self, symbol: Option<&str>) -> Result<serde_json::Value> {
        let timestamp = current_timestamp_ms();
        let mut params: Vec<(&str, &str)> = vec![("timestamp", &timestamp)];
        let recv_str = self.recv_window.to_string();
        params.push(("recvWindow", &recv_str));
        if let Some(sym) = symbol {
            params.push(("symbol", sym));
        }

        let query = auth::build_signed_query(&params, &self.secret_key);
        let url = format!("{}/api/v3/openOrders?{}", self.base_url, query);

        let resp: serde_json::Value = self
            .http
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp)
    }

    /// Fetch exchange info (symbol list, filters, etc.).
    pub async fn get_exchange_info(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v3/exchangeInfo", self.base_url);
        let resp: serde_json::Value = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp)
    }

    // -----------------------------------------------------------------------
    // WebSocket API — order operations
    // -----------------------------------------------------------------------

    /// Ensure the WebSocket API connection is established.
    ///
    /// Lazily connects on first call. Subsequent calls return the existing
    /// connection.
    pub(crate) async fn ensure_ws_api(&self) -> Result<Arc<WsApiInner>> {
        let mut guard = self.ws_api.lock().await;
        if let Some(ref inner) = *guard {
            return Ok(Arc::clone(inner));
        }

        let inner = self.connect_ws_api().await?;
        let arc = Arc::new(inner);
        *guard = Some(Arc::clone(&arc));
        Ok(arc)
    }

    /// Place an order via the WebSocket API.
    ///
    /// Returns the full JSON response from Binance.
    pub async fn ws_place_order(
        &self,
        symbol: &str,
        side: &str,
        order_type: &str,
        quantity: &str,
        price: Option<&str>,
        client_order_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let ws = self.ensure_ws_api().await?;
        let id = Uuid::new_v4().to_string();
        let timestamp = current_timestamp_ms();

        let mut params: Vec<(&str, &str)> = vec![
            ("symbol", symbol),
            ("side", side),
            ("type", order_type),
            ("quantity", quantity),
            ("timestamp", &timestamp),
        ];
        let recv_str = self.recv_window.to_string();
        params.push(("recvWindow", &recv_str));
        if let Some(p) = price {
            params.push(("price", p));
            params.push(("timeInForce", "GTC"));
        }
        if let Some(cid) = client_order_id {
            params.push(("newClientOrderId", cid));
        }

        let signature = auth::hmac_sha256_sign(
            &self.secret_key,
            &params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&"),
        );

        let mut params_map = serde_json::Map::new();
        for (k, v) in &params {
            params_map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
        }
        params_map.insert(
            "apiKey".to_string(),
            serde_json::Value::String(self.api_key.clone()),
        );
        params_map.insert(
            "signature".to_string(),
            serde_json::Value::String(signature),
        );

        let payload = serde_json::json!({
            "id": id,
            "method": "order.place",
            "params": params_map,
        });

        ws.request(&id, payload).await
    }

    /// Cancel an order via the WebSocket API.
    ///
    /// Returns the full JSON response from Binance.
    pub async fn ws_cancel_order(
        &self,
        symbol: &str,
        order_id: Option<u64>,
        client_order_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let ws = self.ensure_ws_api().await?;
        let id = Uuid::new_v4().to_string();
        let timestamp = current_timestamp_ms();

        let mut params: Vec<(&str, String)> = vec![
            ("symbol".into(), symbol.to_string()),
            ("timestamp".into(), timestamp),
            ("recvWindow".into(), self.recv_window.to_string()),
        ];
        if let Some(oid) = order_id {
            params.push(("orderId".into(), oid.to_string()));
        }
        if let Some(cid) = client_order_id {
            params.push(("origClientOrderId".into(), cid.to_string()));
        }

        let query_str: String = params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let signature = auth::hmac_sha256_sign(&self.secret_key, &query_str);

        let mut params_map = serde_json::Map::new();
        for (k, v) in &params {
            params_map.insert(k.to_string(), serde_json::Value::String(v.clone()));
        }
        params_map.insert(
            "apiKey".to_string(),
            serde_json::Value::String(self.api_key.clone()),
        );
        params_map.insert(
            "signature".to_string(),
            serde_json::Value::String(signature),
        );

        let payload = serde_json::json!({
            "id": id,
            "method": "order.cancel",
            "params": params_map,
        });

        ws.request(&id, payload).await
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Establish the WebSocket API connection.
    async fn connect_ws_api(&self) -> Result<WsApiInner> {
        use tokio_tungstenite::tungstenite::Message;

        let (ws_stream, _) = tokio_tungstenite::connect_async(&self.ws_api_url)
            .await
            .context("WS API connection failed")?;

        let (mut ws_write, mut ws_read) = ws_stream.split();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let pending_clone = Arc::clone(&pending);

        // Spawn reader/writer task
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(msg) = rx.recv() => {
                        if let Err(e) = ws_write.send(Message::Text(msg.into())).await {
                            error!("[spot ws-api] send error: {e}");
                            break;
                        }
                    }
                    frame = ws_read.next() => {
                        match frame {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                                    if let Some(id_val) = val.get("id").and_then(|i| i.as_str()) {
                                        let mut map = pending_clone.lock().await;
                                        if let Some(sender) = map.remove(id_val) {
                                            let _ = sender.send(val);
                                        }
                                    }
                                }
                            }
                            Some(Ok(Message::Ping(data))) => {
                                let _ = ws_write.send(Message::Pong(data)).await;
                            }
                            Some(Err(e)) => {
                                error!("[spot ws-api] read error: {e}");
                                break;
                            }
                            None => {
                                warn!("[spot ws-api] stream ended");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        // Store the task handle
        self.tasks.lock().await.push(task);

        info!("[spot] WS API connected to {}", self.ws_api_url);

        Ok(WsApiInner { tx, pending })
    }

    /// Abort all background tasks.
    pub async fn shutdown(&self) {
        let mut tasks = self.tasks.lock().await;
        for task in tasks.drain(..) {
            task.abort();
        }
        // Close listen key (best-effort)
        let _ = self.close_listen_key().await;
    }
}

/// Returns the current Unix timestamp in milliseconds.
fn current_timestamp_ms() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}
