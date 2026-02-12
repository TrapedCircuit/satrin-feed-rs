//! Generic market data pipeline engine.
//!
//! Provides [`GenericMd`] — a data-driven implementation of [`MdModule`] that
//! replaces per-exchange boilerplate. Each exchange only needs to provide a
//! `build(config) -> Vec<StreamDef>` function describing its streams; the
//! generic engine handles SHM creation, channel wiring, dedup tasks, and
//! WebSocket connections automatically.
//!
//! # Architecture
//!
//! ```text
//! StreamDef ──► GenericMd.init_shm()  ──► ShmMdStore per stream
//!          ──► GenericMd.start()      ──► [channel + dedup task + WS task] per stream
//!          ──► GenericMd.stop()       ──► abort all tasks
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use k4_core::shm::ShmMdStore;
use k4_core::types::*;
use k4_core::udp::UdpSender;
use k4_core::ws::PingPayload;
use tracing::info;

use crate::dedup_worker::{self, ProductShmStores, TradeDeduper};
use crate::ws_helper;

// ---------------------------------------------------------------------------
// StreamDef — describes one WS-to-SHM pipeline
// ---------------------------------------------------------------------------

/// A text message parser: `raw_json -> Vec<MarketDataMsg>`.
pub type TextParser = Box<dyn Fn(&str) -> Vec<MarketDataMsg> + Send + Sync>;

/// A binary message parser: `raw_bytes -> Vec<MarketDataMsg>`.
pub type BinaryParser = Box<dyn Fn(&[u8]) -> Vec<MarketDataMsg> + Send + Sync>;

/// SHM store names for one stream. `None` means "don't create this store".
#[derive(Debug, Clone, Default)]
pub struct ShmNames {
    pub bbo: Option<String>,
    pub agg: Option<String>,
    pub trade: Option<String>,
    pub depth5: Option<String>,
}

/// Ping / keep-alive configuration for a WebSocket connection.
#[derive(Debug, Clone)]
pub struct PingConfig {
    pub interval: Duration,
    pub payload: PingPayload,
}

/// Everything needed to set up one WS-to-SHM pipeline.
///
/// Each exchange's `build()` function returns a `Vec<StreamDef>` — one per
/// WebSocket stream (e.g. Binance produces 3: spot JSON, spot SBE, UBase JSON).
pub struct StreamDef {
    /// Human-readable label (e.g. `"binance_spot_json"`).
    pub label: String,
    /// WebSocket URL (e.g. `"wss://stream.binance.com:443/ws"`).
    pub ws_url: String,
    /// Subscription message sent immediately after WS connect.
    pub subscribe_msg: String,
    /// Ping configuration (exchange-specific format and interval).
    pub ping: Option<PingConfig>,
    /// Extra HTTP headers for the WS handshake (e.g. API key).
    pub extra_headers: HashMap<String, String>,
    /// SHM store names — controls which data types are persisted.
    pub shm: ShmNames,
    /// Symbols this stream covers (used for SHM store creation).
    pub symbols: Vec<String>,
    /// Ring buffer size per symbol in SHM.
    pub md_size: u32,
    /// Text (JSON) message parser. Most exchanges use this.
    pub text_parser: Option<TextParser>,
    /// Binary message parser (Binance SBE only).
    pub binary_parser: Option<BinaryParser>,
    /// Custom trade deduplicator (Bybit UUID dedup).
    pub custom_trade_dedup: Option<TradeDeduper>,
    /// CPU core to pin the dedup thread to.
    pub dedup_cpu_core: Option<i32>,
}

// ---------------------------------------------------------------------------
// GenericMd — the engine
// ---------------------------------------------------------------------------

/// Generic market data module driven by [`StreamDef`] descriptors.
///
/// Implements [`MdModule`] by iterating the stream definitions and
/// automatically creating SHM stores, dedup channels, and WS connections.
pub struct GenericMd {
    name: String,
    streams: Vec<StreamDef>,
    stores: Vec<Option<ProductShmStores>>,
    udp: Option<Arc<UdpSender>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl GenericMd {
    /// Create a new generic MD module.
    ///
    /// `streams` are the exchange-specific stream definitions produced by
    /// `binance::build()`, `okx::build()`, etc.
    pub fn new(name: String, streams: Vec<StreamDef>) -> Self {
        let n = streams.len();
        Self {
            name,
            streams,
            stores: (0..n).map(|_| None).collect(),
            udp: None,
            tasks: Vec::new(),
        }
    }
}

#[async_trait]
impl crate::MdModule for GenericMd {
    fn name(&self) -> &str {
        &self.name
    }

    async fn init_shm(&mut self) -> Result<()> {
        for (i, stream) in self.streams.iter().enumerate() {
            if stream.symbols.is_empty() {
                continue;
            }
            let syms = &stream.symbols;
            let md_size = stream.md_size;
            let shm = &stream.shm;

            let stores = ProductShmStores {
                bbo: shm
                    .bbo
                    .as_ref()
                    .map(|n| ShmMdStore::create(n, syms, md_size))
                    .transpose()?,
                agg: shm
                    .agg
                    .as_ref()
                    .map(|n| ShmMdStore::create(n, syms, md_size))
                    .transpose()?,
                trade: shm
                    .trade
                    .as_ref()
                    .map(|n| ShmMdStore::create(n, syms, md_size))
                    .transpose()?,
                depth5: shm
                    .depth5
                    .as_ref()
                    .map(|n| ShmMdStore::create(n, syms, md_size))
                    .transpose()?,
            };
            self.stores[i] = Some(stores);
        }

        info!(
            "[{}] SHM initialized ({} streams)",
            self.name,
            self.streams.len()
        );
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        // Take ownership of streams and stores for the move closures.
        // We swap each stream out of the vec one at a time.
        let n = self.streams.len();

        for i in 0..n {
            let stores = match self.stores[i].take() {
                Some(s) => s,
                None => continue, // no symbols → no stores → skip
            };

            let stream = &self.streams[i];
            let label = stream.label.clone();
            let url = stream.ws_url.clone();
            let sub_msg = stream.subscribe_msg.clone();
            let headers = stream.extra_headers.clone();
            let ping_interval = stream.ping.as_ref().map(|p| p.interval);
            let ping_payload = stream.ping.as_ref().map(|p| p.payload.clone());
            let cpu_core = stream.dedup_cpu_core;

            // Create dedup channel
            let (tx, rx) = crossbeam_channel::bounded::<MarketDataMsg>(8192);

            // Spawn dedup task
            let udp = self.udp.clone();
            let dedup_label = label.clone();
            let custom_td = self.streams[i].custom_trade_dedup.take();

            self.tasks.push(tokio::task::spawn_blocking(move || {
                dedup_worker::run_dedup_loop(&dedup_label, rx, stores, udp, custom_td, cpu_core);
            }));

            // Spawn WS task
            if let Some(binary_parser) = self.streams[i].binary_parser.take() {
                let ws_label = label.clone();
                let tx_clone = tx.clone();
                self.tasks.push(tokio::spawn(async move {
                    ws_helper::run_ws_binary_stream(
                        url,
                        sub_msg,
                        headers,
                        tx_clone,
                        binary_parser,
                        ws_label,
                    )
                    .await;
                }));
            } else if let Some(text_parser) = self.streams[i].text_parser.take() {
                let ws_label = label.clone();
                self.tasks.push(tokio::spawn(async move {
                    ws_helper::run_ws_text_stream(
                        url,
                        sub_msg,
                        headers,
                        ping_interval,
                        ping_payload,
                        tx,
                        text_parser,
                        ws_label,
                    )
                    .await;
                }));
            }
        }

        info!("[{}] started {} tasks", self.name, self.tasks.len());
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        for task in self.tasks.drain(..) {
            task.abort();
        }
        info!("[{}] stopped", self.name);
        Ok(())
    }
}
