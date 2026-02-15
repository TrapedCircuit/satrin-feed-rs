//! Binance Futures account operations.
//!
//! Provides REST endpoints for listen key management, account/position queries,
//! and signed order requests. Supports both **USDT-margined** (UBase, `/fapi/`)
//! and **coin-margined** (CBase, `/dapi/`) endpoints via the [`FuturesVariant`]
//! enum.
//!
//! # REST endpoints (UBase example)
//!
//! | Operation         | Method | Path                      |
//! |-------------------|--------|---------------------------|
//! | Create listen key | POST   | `/fapi/v1/listenKey`      |
//! | Refresh listen key| PUT    | `/fapi/v1/listenKey`      |
//! | Close listen key  | DELETE | `/fapi/v1/listenKey`      |
//! | Account info      | GET    | `/fapi/v3/account`        |
//! | Open orders       | GET    | `/fapi/v1/openOrders`     |
//! | Positions         | GET    | `/fapi/v3/positionRisk`   |
//! | Exchange info     | GET    | `/fapi/v1/exchangeInfo`   |
//!
//! CBase uses the same structure with `/dapi/v1/*` and `/dapi/v2/*` paths.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::auth;

// ---------------------------------------------------------------------------
// FuturesVariant
// ---------------------------------------------------------------------------

/// Futures account variant — determines API paths and WebSocket URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuturesVariant {
    /// USDT-margined futures (`/fapi/`).
    UBase,
    /// Coin-margined futures (`/dapi/`).
    CBase,
}

impl FuturesVariant {
    /// API path prefix for this variant.
    fn path_prefix(self) -> &'static str {
        match self {
            Self::UBase => "/fapi",
            Self::CBase => "/dapi",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::UBase => "ubase",
            Self::CBase => "cbase",
        }
    }
}

impl std::fmt::Display for FuturesVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// FuturesClient
// ---------------------------------------------------------------------------

/// Binance Futures account client.
///
/// Works for both USDT-margined (UBase) and coin-margined (CBase) accounts.
/// The `variant` field determines which API paths are used.
pub struct FuturesClient {
    /// Shared HTTP client.
    http: reqwest::Client,
    /// API key.
    api_key: String,
    /// Secret key for HMAC-SHA256 signing.
    secret_key: String,
    /// REST base URL (e.g. `https://fapi.binance.com`).
    base_url: String,
    /// `recvWindow` for signed requests.
    recv_window: u64,
    /// Which futures variant this client serves.
    variant: FuturesVariant,
    /// Active listen key for the user-data stream.
    listen_key: Mutex<Option<String>>,
}

impl FuturesClient {
    /// Create a new futures client (no connections opened yet).
    pub fn new(
        api_key: String,
        secret_key: String,
        base_url: String,
        recv_window: u64,
        variant: FuturesVariant,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
            secret_key,
            base_url,
            recv_window,
            variant,
            listen_key: Mutex::new(None),
        }
    }

    /// Returns the futures variant for this client.
    pub fn variant(&self) -> FuturesVariant {
        self.variant
    }

    // -----------------------------------------------------------------------
    // Listen key management
    // -----------------------------------------------------------------------

    /// Create a new listen key for the user-data stream.
    pub async fn create_listen_key(&self) -> Result<String> {
        let url = format!("{}{}/v1/listenKey", self.base_url, self.variant.path_prefix());
        let resp = self
            .http
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .context("create listen key request failed")?;

        let body: serde_json::Value = resp.error_for_status().context("create listen key HTTP error")?.json().await?;

        let key = body
            .get("listenKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("listenKey not found in response"))?
            .to_string();

        *self.listen_key.lock().await = Some(key.clone());
        info!("[{}] listen key created", self.variant);
        Ok(key)
    }

    /// Send a keepalive ping for the current listen key.
    pub async fn keepalive_listen_key(&self) -> Result<()> {
        let key = self.listen_key.lock().await;
        let Some(ref listen_key) = *key else {
            return Err(anyhow!("no active listen key"));
        };

        let url = format!("{}{}/v1/listenKey", self.base_url, self.variant.path_prefix());
        self.http
            .put(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("listenKey", listen_key.as_str())])
            .send()
            .await
            .context("keepalive listen key request failed")?
            .error_for_status()
            .context("keepalive listen key HTTP error")?;

        debug!("[{}] listen key keepalive sent", self.variant);
        Ok(())
    }

    /// Close (delete) the current listen key.
    pub async fn close_listen_key(&self) -> Result<()> {
        let key = self.listen_key.lock().await;
        let Some(ref listen_key) = *key else {
            return Ok(());
        };

        let url = format!("{}{}/v1/listenKey", self.base_url, self.variant.path_prefix());
        self.http
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("listenKey", listen_key.as_str())])
            .send()
            .await
            .context("close listen key request failed")?
            .error_for_status()
            .context("close listen key HTTP error")?;

        info!("[{}] listen key closed", self.variant);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // REST queries
    // -----------------------------------------------------------------------

    /// Query futures account information (balances, assets).
    ///
    /// UBase: `GET /fapi/v3/account`, CBase: `GET /dapi/v2/account`.
    pub async fn get_account_info(&self) -> Result<serde_json::Value> {
        let version = match self.variant {
            FuturesVariant::UBase => "v3",
            FuturesVariant::CBase => "v2",
        };
        let timestamp = current_timestamp_ms();
        let query = auth::build_signed_query(
            &[("recvWindow", &self.recv_window.to_string()), ("timestamp", &timestamp)],
            &self.secret_key,
        );

        let url = format!("{}{}/{version}/account?{query}", self.base_url, self.variant.path_prefix(),);

        let resp: serde_json::Value =
            self.http.get(&url).header("X-MBX-APIKEY", &self.api_key).send().await?.error_for_status()?.json().await?;

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
        let url = format!("{}{}/v1/openOrders?{query}", self.base_url, self.variant.path_prefix(),);

        let resp: serde_json::Value =
            self.http.get(&url).header("X-MBX-APIKEY", &self.api_key).send().await?.error_for_status()?.json().await?;

        Ok(resp)
    }

    /// Query current positions.
    ///
    /// UBase: `GET /fapi/v3/positionRisk`, CBase: `GET /dapi/v2/positionRisk`.
    pub async fn get_positions(&self, symbol: Option<&str>) -> Result<serde_json::Value> {
        let version = match self.variant {
            FuturesVariant::UBase => "v3",
            FuturesVariant::CBase => "v2",
        };
        let timestamp = current_timestamp_ms();
        let mut params: Vec<(&str, &str)> = vec![("timestamp", &timestamp)];
        let recv_str = self.recv_window.to_string();
        params.push(("recvWindow", &recv_str));
        if let Some(sym) = symbol {
            params.push(("symbol", sym));
        }

        let query = auth::build_signed_query(&params, &self.secret_key);
        let url = format!("{}{}/{version}/positionRisk?{query}", self.base_url, self.variant.path_prefix(),);

        let resp: serde_json::Value =
            self.http.get(&url).header("X-MBX-APIKEY", &self.api_key).send().await?.error_for_status()?.json().await?;

        Ok(resp)
    }

    /// Fetch exchange info (symbol list, filters, etc.).
    pub async fn get_exchange_info(&self) -> Result<serde_json::Value> {
        let url = format!("{}{}/v1/exchangeInfo", self.base_url, self.variant.path_prefix(),);

        let resp: serde_json::Value = self.http.get(&url).send().await?.error_for_status()?.json().await?;

        Ok(resp)
    }

    // -----------------------------------------------------------------------
    // Order operations (REST)
    // -----------------------------------------------------------------------

    /// Place a new order via the REST API.
    ///
    /// Returns the full JSON response including the exchange order ID.
    pub async fn place_order(
        &self,
        symbol: &str,
        side: &str,
        order_type: &str,
        quantity: &str,
        price: Option<&str>,
        client_order_id: Option<&str>,
    ) -> Result<serde_json::Value> {
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

        let query = auth::build_signed_query(&params, &self.secret_key);
        let url = format!("{}{}/v1/order?{query}", self.base_url, self.variant.path_prefix(),);

        let resp: serde_json::Value =
            self.http.post(&url).header("X-MBX-APIKEY", &self.api_key).send().await?.error_for_status()?.json().await?;

        Ok(resp)
    }

    /// Cancel an existing order via the REST API.
    pub async fn cancel_order(
        &self,
        symbol: &str,
        order_id: Option<u64>,
        client_order_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let timestamp = current_timestamp_ms();
        let oid_str = order_id.map(|id| id.to_string());
        let mut params: Vec<(&str, &str)> = vec![("symbol", symbol), ("timestamp", &timestamp)];
        let recv_str = self.recv_window.to_string();
        params.push(("recvWindow", &recv_str));
        if let Some(ref oid) = oid_str {
            params.push(("orderId", oid));
        }
        if let Some(cid) = client_order_id {
            params.push(("origClientOrderId", cid));
        }

        let query = auth::build_signed_query(&params, &self.secret_key);
        let url = format!("{}{}/v1/order?{query}", self.base_url, self.variant.path_prefix(),);

        let resp: serde_json::Value = self
            .http
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp)
    }

    /// Cancel all open orders for a symbol.
    pub async fn cancel_all_orders(&self, symbol: &str) -> Result<serde_json::Value> {
        let timestamp = current_timestamp_ms();
        let query = auth::build_signed_query(
            &[("symbol", symbol), ("recvWindow", &self.recv_window.to_string()), ("timestamp", &timestamp)],
            &self.secret_key,
        );

        let url = format!("{}{}/v1/allOpenOrders?{query}", self.base_url, self.variant.path_prefix(),);

        let resp: serde_json::Value = self
            .http
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp)
    }

    /// Clean up: close listen key (best-effort).
    pub async fn shutdown(&self) {
        let _ = self.close_listen_key().await;
    }
}

/// Returns the current Unix timestamp in milliseconds.
fn current_timestamp_ms() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().to_string()
}
