//! UDP market data module configuration.
//!
//! Extracts UDP receiver settings from the [`ConnectionConfig`] `udp_receiver`
//! section, including listen address and per-product SHM names.

use std::net::SocketAddr;

use anyhow::{Result, anyhow};
use k4_core::config::ConnectionConfig;

/// Parsed UDP receiver configuration.
#[derive(Debug, Clone)]
pub struct UdpMdConfig {
    /// Address to bind the UDP socket on (ip:port).
    pub listen_addr: SocketAddr,

    /// Shared memory buffer size per instrument.
    pub md_size: u32,

    // -- Spot --
    /// Spot symbols to allocate SHM slots for.
    pub spot_symbols: Vec<String>,

    /// SHM name for spot BookTicker data.
    pub spot_bbo_shm_name: Option<String>,
    /// SHM name for spot AggTrade data.
    pub spot_agg_shm_name: Option<String>,
    /// SHM name for spot Trade data.
    pub spot_trade_shm_name: Option<String>,
    /// SHM name for spot Depth5 data.
    pub spot_depth5_shm_name: Option<String>,

    // -- Futures (UBase) --
    /// UBase symbols to allocate SHM slots for.
    pub ubase_symbols: Vec<String>,

    /// SHM name for UBase BookTicker data.
    pub ubase_bbo_shm_name: Option<String>,
    /// SHM name for UBase AggTrade data.
    pub ubase_agg_shm_name: Option<String>,
    /// SHM name for UBase Trade data.
    pub ubase_trade_shm_name: Option<String>,
    /// SHM name for UBase Depth5 data.
    pub ubase_depth5_shm_name: Option<String>,
}

impl UdpMdConfig {
    /// Build a [`UdpMdConfig`] from a [`ConnectionConfig`].
    ///
    /// Returns an error if the `udp_receiver` section is missing.
    pub fn from_connection(conn: &ConnectionConfig) -> Result<Self> {
        let udp = conn
            .udp_receiver
            .as_ref()
            .ok_or_else(|| anyhow!("missing udp_receiver config for UDP module"))?;

        let listen_addr: SocketAddr = format!("{}:{}", udp.ip, udp.port).parse()?;

        Ok(Self {
            listen_addr,
            md_size: conn.effective_md_size(),

            spot_symbols: udp.spot_symbols.clone().unwrap_or_default(),
            spot_bbo_shm_name: udp.spot_bbo_shm_name.clone(),
            spot_agg_shm_name: udp.spot_agg_shm_name.clone(),
            spot_trade_shm_name: udp.spot_trade_shm_name.clone(),
            spot_depth5_shm_name: udp.spot_depth5_shm_name.clone(),

            ubase_symbols: udp.ubase_symbols.clone().unwrap_or_default(),
            ubase_bbo_shm_name: udp.ubase_bbo_shm_name.clone(),
            ubase_agg_shm_name: udp.ubase_agg_shm_name.clone(),
            ubase_trade_shm_name: udp.ubase_trade_shm_name.clone(),
            ubase_depth5_shm_name: udp.ubase_depth5_shm_name.clone(),
        })
    }
}
