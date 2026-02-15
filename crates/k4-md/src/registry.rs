//! Module registry — factory for creating MD modules from config.

use anyhow::{Result, anyhow};
use k4_core::config::ConnectionConfig;

use crate::{MdModule, pipeline::GenericMd, udp::UdpMd};

/// Create an `MdModule` based on the `exchange` field in the config.
///
/// For WebSocket-based exchanges (Binance, OKX, Bitget, Bybit), the exchange
/// module's `build()` function produces `StreamDef`s that are fed into
/// [`GenericMd`]. The UDP module is handled separately since it doesn't use
/// WebSocket.
pub fn create_md_module(config: &ConnectionConfig) -> Result<Box<dyn MdModule>> {
    let exchange = config.exchange.to_lowercase();

    if exchange == "udp" {
        return Ok(Box::new(UdpMd::new(config)?));
    }

    let streams = match exchange.as_str() {
        "binance" => crate::binance::build(config)?,
        "okx" => crate::okx::build(config)?,
        "bitget" => crate::bitget::build(config)?,
        "bybit" => crate::bybit::build(config)?,
        other => return Err(anyhow!("Unknown exchange: {other}")),
    };

    Ok(Box::new(GenericMd::new(config.module_name(), streams)))
}
