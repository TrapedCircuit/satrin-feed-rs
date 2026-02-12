//! # k4-md
//!
//! Market data modules for multiple cryptocurrency exchanges.
//!
//! ## Architecture
//!
//! Each exchange provides a `build(config) -> Vec<StreamDef>` function that
//! describes its WebSocket streams. The generic [`pipeline::GenericMd`] engine
//! handles SHM creation, channel wiring, dedup tasks, and WS connections
//! automatically.
//!
//! ## Shared infrastructure
//!
//! - [`pipeline`] — `StreamDef` + `GenericMd` data-driven engine
//! - [`dedup_worker`] — generic dedup loop
//! - [`ws_helper`] — WebSocket connection helpers
//! - [`json_util`] — JSON parsing helpers

pub mod binance;
pub mod bitget;
pub mod bybit;
pub mod dedup_worker;
pub mod json_util;
pub mod okx;
pub mod pipeline;
pub mod registry;
pub mod udp;
pub mod ws_helper;

use anyhow::Result;
use async_trait::async_trait;

/// Trait implemented by all market data modules.
///
/// Only `Send` is required (not `Sync`) because modules are accessed
/// sequentially by the runner, never concurrently.
#[async_trait]
pub trait MdModule: Send {
    /// Human-readable module name.
    fn name(&self) -> &str;
    /// Initialize shared memory stores.
    async fn init_shm(&mut self) -> Result<()>;
    /// Connect and begin processing market data.
    async fn start(&mut self) -> Result<()>;
    /// Gracefully stop all connections and tasks.
    async fn stop(&mut self) -> Result<()>;
}
