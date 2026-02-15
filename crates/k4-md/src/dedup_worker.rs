//! Generic dedup worker that runs on a dedicated thread.
//!
//! Receives [`MarketDataMsg`] from a crossbeam channel, deduplicates by
//! `update_id`, and writes to SHM stores + optional UDP sender. This replaces
//! the per-exchange `dedup_loop` functions that were previously copy-pasted.

use std::sync::Arc;

use crossbeam_channel::Receiver;
use k4_core::{dedup::UpdateIdDedup, shm::ShmMdStore, types::*, udp::UdpSender};
use tracing::info;

/// Bundled SHM stores for one product (spot or futures).
pub struct ProductShmStores {
    pub bbo: Option<ShmMdStore<Bookticker>>,
    pub agg: Option<ShmMdStore<AggTrade>>,
    pub trade: Option<ShmMdStore<Trade>>,
    pub depth5: Option<ShmMdStore<Depth5>>,
}

/// Optional custom trade dedup function (e.g. Bybit UUID dedup).
///
/// Returns `true` if the trade is new (should be forwarded), `false` if duplicate.
pub type TradeDeduper = Box<dyn FnMut(&str, u64) -> bool + Send>;

/// Run a dedup loop on the calling thread.
///
/// Reads messages from `rx`, checks each against an `UpdateIdDedup` per symbol,
/// and writes accepted messages to the appropriate SHM store and UDP sender.
///
/// If `cpu_core` is `Some`, the thread is pinned to that CPU core before
/// entering the hot loop. For most exchanges, pass `custom_trade_dedup = None`
/// to use the standard `UpdateIdDedup`.
pub fn run_dedup_loop(
    label: &str,
    rx: Receiver<MarketDataMsg>,
    stores: ProductShmStores,
    udp: Option<Arc<UdpSender>>,
    custom_trade_dedup: Option<TradeDeduper>,
    cpu_core: Option<i32>,
) {
    // Pin this thread to a specific CPU core if configured.
    k4_core::cpu_affinity::maybe_bind(cpu_core);
    let mut bbo_dedup = UpdateIdDedup::new();
    let mut agg_dedup = UpdateIdDedup::new();
    let mut trade_dedup = UpdateIdDedup::new();
    let mut depth5_dedup = UpdateIdDedup::new();
    let mut custom_td = custom_trade_dedup;

    info!("[{label}] dedup loop started");

    while let Ok(msg) = rx.recv() {
        match msg {
            MarketDataMsg::Bbo(ref bbo) => {
                let sym = symbol_from_bytes(&bbo.symbol);
                if bbo_dedup.check_and_update(sym, bbo.update_id) {
                    if let Some(ref shm) = stores.bbo {
                        shm.write(sym, bbo);
                    }
                    if let Some(ref u) = udp {
                        u.send(msg);
                    }
                }
            }
            MarketDataMsg::AggTrade(ref agg) => {
                let sym = symbol_from_bytes(&agg.symbol);
                if agg_dedup.check_and_update(sym, agg.agg_trade_id) {
                    if let Some(ref shm) = stores.agg {
                        shm.write(sym, agg);
                    }
                    if let Some(ref u) = udp {
                        u.send(msg);
                    }
                }
            }
            MarketDataMsg::Trade(ref trade) => {
                let sym = symbol_from_bytes(&trade.symbol);
                let is_new = if let Some(ref mut dedup_fn) = custom_td {
                    dedup_fn(sym, trade.trade_id)
                } else {
                    trade_dedup.check_and_update(sym, trade.trade_id)
                };
                if is_new {
                    if let Some(ref shm) = stores.trade {
                        shm.write(sym, trade);
                    }
                    if let Some(ref u) = udp {
                        u.send(msg);
                    }
                }
            }
            MarketDataMsg::Depth5(ref depth) => {
                let sym = symbol_from_bytes(&depth.symbol);
                if depth5_dedup.check_and_update(sym, depth.update_id) {
                    if let Some(ref shm) = stores.depth5 {
                        shm.write(sym, depth);
                    }
                    if let Some(ref u) = udp {
                        u.send(msg);
                    }
                }
            }
        }
    }

    info!("[{label}] dedup loop exited");
}
