//! UDP market data module.
//!
//! Receives market data over UDP from other MD modules' [`UdpSender`] instances
//! and writes directly to shared memory. **No deduplication is needed** because
//! the sending module already performs dedup before forwarding.
//!
//! # Architecture
//!
//! ```text
//! BinanceMd ──► UdpSender ─── UDP ──► UdpMd ──► SHM (spot + futures)
//! ```
//!
//! Incoming packets are routed by [`ProductType`] to the appropriate SHM store:
//! - `ProductType::Spot` → spot SHM stores
//! - All other variants → futures (UBase) SHM stores
//!
//! Configuration is read from the `udp_receiver` section of the connection JSON.

pub mod config;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use k4_core::config::ConnectionConfig;
use k4_core::shm::ShmMdStore;
use k4_core::udp::{UdpCallbackHandler, UdpReceiver};
use k4_core::*;
use tracing::{error, info};

use self::config::UdpMdConfig;

/// UDP market data module — receives pre-deduped data and writes to SHM.
///
/// Listens on a single UDP socket and demultiplexes incoming messages by
/// [`ProductType`] into separate spot and futures SHM stores.
pub struct UdpMd {
    /// Parsed configuration.
    config: UdpMdConfig,

    // -- Spot SHM stores --
    spot_bbo_shm: Option<Arc<ShmMdStore<Bookticker>>>,
    spot_agg_shm: Option<Arc<ShmMdStore<AggTrade>>>,
    spot_trade_shm: Option<Arc<ShmMdStore<Trade>>>,
    spot_depth5_shm: Option<Arc<ShmMdStore<Depth5>>>,

    // -- Futures SHM stores --
    ubase_bbo_shm: Option<Arc<ShmMdStore<Bookticker>>>,
    ubase_agg_shm: Option<Arc<ShmMdStore<AggTrade>>>,
    ubase_trade_shm: Option<Arc<ShmMdStore<Trade>>>,
    ubase_depth5_shm: Option<Arc<ShmMdStore<Depth5>>>,

    /// Background receiver task handle.
    task: Option<tokio::task::JoinHandle<()>>,
}

impl UdpMd {
    /// Create a new UDP MD module from the connection config.
    ///
    /// No connections are opened until [`MdModule::start`] is called.
    pub fn new(conn_config: &ConnectionConfig) -> Result<Self> {
        let config = UdpMdConfig::from_connection(conn_config)?;
        Ok(Self {
            config,
            spot_bbo_shm: None,
            spot_agg_shm: None,
            spot_trade_shm: None,
            spot_depth5_shm: None,
            ubase_bbo_shm: None,
            ubase_agg_shm: None,
            ubase_trade_shm: None,
            ubase_depth5_shm: None,
            task: None,
        })
    }
}

#[async_trait]
impl crate::MdModule for UdpMd {
    fn name(&self) -> &str {
        "udp_md"
    }

    async fn init_shm(&mut self) -> Result<()> {
        let md_size = self.config.md_size;

        // -- Spot SHM --
        if !self.config.spot_symbols.is_empty() {
            let syms = &self.config.spot_symbols;
            if let Some(ref name) = self.config.spot_bbo_shm_name {
                self.spot_bbo_shm = Some(Arc::new(ShmMdStore::create(name, syms, md_size)?));
            }
            if let Some(ref name) = self.config.spot_agg_shm_name {
                self.spot_agg_shm = Some(Arc::new(ShmMdStore::create(name, syms, md_size)?));
            }
            if let Some(ref name) = self.config.spot_trade_shm_name {
                self.spot_trade_shm = Some(Arc::new(ShmMdStore::create(name, syms, md_size)?));
            }
            if let Some(ref name) = self.config.spot_depth5_shm_name {
                self.spot_depth5_shm = Some(Arc::new(ShmMdStore::create(name, syms, md_size)?));
            }
        }

        // -- Futures (UBase) SHM --
        if !self.config.ubase_symbols.is_empty() {
            let syms = &self.config.ubase_symbols;
            if let Some(ref name) = self.config.ubase_bbo_shm_name {
                self.ubase_bbo_shm = Some(Arc::new(ShmMdStore::create(name, syms, md_size)?));
            }
            if let Some(ref name) = self.config.ubase_agg_shm_name {
                self.ubase_agg_shm = Some(Arc::new(ShmMdStore::create(name, syms, md_size)?));
            }
            if let Some(ref name) = self.config.ubase_trade_shm_name {
                self.ubase_trade_shm = Some(Arc::new(ShmMdStore::create(name, syms, md_size)?));
            }
            if let Some(ref name) = self.config.ubase_depth5_shm_name {
                self.ubase_depth5_shm = Some(Arc::new(ShmMdStore::create(name, syms, md_size)?));
            }
        }

        info!(
            "[udp] SHM initialized — spot symbols: {}, ubase symbols: {}",
            self.config.spot_symbols.len(),
            self.config.ubase_symbols.len(),
        );
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        let receiver = UdpReceiver::bind(self.config.listen_addr).await?;

        // Clone Arc references for the move closures.
        let spot_bbo = self.spot_bbo_shm.clone();
        let ubase_bbo = self.ubase_bbo_shm.clone();

        let spot_trade = self.spot_trade_shm.clone();
        let ubase_trade = self.ubase_trade_shm.clone();

        let spot_agg = self.spot_agg_shm.clone();
        let ubase_agg = self.ubase_agg_shm.clone();

        let spot_depth5 = self.spot_depth5_shm.clone();
        let ubase_depth5 = self.ubase_depth5_shm.clone();

        let handler = UdpCallbackHandler {
            on_bbo: Some(Box::new(move |bbo: Bookticker| {
                let sym = symbol_from_bytes(&bbo.symbol);
                let store = match bbo.product_type {
                    ProductType::Spot => &spot_bbo,
                    _ => &ubase_bbo,
                };
                if let Some(s) = store {
                    s.write(sym, &bbo);
                }
            })),

            on_trade: Some(Box::new(move |trade: Trade| {
                let sym = symbol_from_bytes(&trade.symbol);
                let store = match trade.product_type {
                    ProductType::Spot => &spot_trade,
                    _ => &ubase_trade,
                };
                if let Some(s) = store {
                    s.write(sym, &trade);
                }
            })),

            on_agg_trade: Some(Box::new(move |agg: AggTrade| {
                let sym = symbol_from_bytes(&agg.symbol);
                let store = match agg.product_type {
                    ProductType::Spot => &spot_agg,
                    _ => &ubase_agg,
                };
                if let Some(s) = store {
                    s.write(sym, &agg);
                }
            })),

            on_depth5: Some(Box::new(move |depth: Depth5| {
                let sym = symbol_from_bytes(&depth.symbol);
                let store = match depth.product_type {
                    ProductType::Spot => &spot_depth5,
                    _ => &ubase_depth5,
                };
                if let Some(s) = store {
                    s.write(sym, &depth);
                }
            })),
        };

        info!("[udp] starting receiver on {}", self.config.listen_addr);

        let task = tokio::spawn(async move {
            if let Err(e) = receiver.run(handler).await {
                error!("[udp] receiver error: {e}");
            }
        });

        self.task = Some(task);
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        info!("[udp] stopped");
        Ok(())
    }
}
