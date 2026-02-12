//! Redundant WebSocket connection manager.
//!
//! Manages N connections to the same exchange endpoint with the same
//! subscription. Market data messages flow through a deduplicator, so only the
//! first (fastest) delivery of each update reaches downstream consumers.
//!
//! Periodically, the connection with the highest average latency is terminated
//! and replaced with a fresh connection — this combats the exchange LB node
//! jitter problem described in the project README.

use super::client::{OnBinaryCallback, OnMessageCallback, WsConnConfig, WsConnection};
use crate::latency::LatencyCollector;
use std::time::Duration;
use tracing::{info, warn};

/// Configuration for the redundancy manager.
#[derive(Debug, Clone)]
pub struct RedundantConfig {
    /// Base connection config (cloned for each redundant connection).
    pub base_config: WsConnConfig,
    /// Number of redundant connections.
    pub conn_count: u32,
    /// Heartbeat interval — triggers latency evaluation.
    pub hb_interval: Option<Duration>,
    /// Whether to reset the slowest connection on each heartbeat.
    pub reset_on_hb: bool,
    /// After this many data points, evaluate and reset slowest.
    pub reset_threshold: u64,
}

/// Manages redundant WebSocket connections.
pub struct RedundantWsClient {
    config: RedundantConfig,
    connections: Vec<WsConnection>,
    latency_collectors: Vec<LatencyCollector>,
    next_conn_id: usize,
}

impl RedundantWsClient {
    /// Create a new redundant client (connections are not started yet).
    pub fn new(config: RedundantConfig) -> Self {
        let count = config.conn_count as usize;
        Self {
            config,
            connections: Vec::with_capacity(count),
            latency_collectors: (0..count).map(|_| LatencyCollector::new()).collect(),
            next_conn_id: 0,
        }
    }

    /// Start all redundant connections.
    pub fn start(&mut self, on_text: OnMessageCallback, on_binary: Option<OnBinaryCallback>) {
        for _ in 0..self.config.conn_count {
            self.add_and_start_connection(on_text.clone(), on_binary.clone());
        }
    }

    /// Record a latency sample for a specific connection.
    pub fn record_latency(&mut self, conn_idx: usize, latency_us: u64) {
        if let Some(lc) = self.latency_collectors.get_mut(conn_idx) {
            lc.record(latency_us);
        }
    }

    /// Evaluate latencies and reset the slowest connection if configured.
    ///
    /// Returns the index of the reset connection, or `None`.
    pub async fn evaluate_and_reset(
        &mut self,
        on_text: OnMessageCallback,
        on_binary: Option<OnBinaryCallback>,
    ) -> Option<usize> {
        if self.connections.len() <= 1 {
            return None;
        }

        // Find the connection with the highest average latency
        let mut worst_idx = None;
        let mut worst_avg = 0.0f64;

        for (i, lc) in self.latency_collectors.iter().enumerate() {
            if let Some(stats) = lc.stats() {
                info!(
                    "[redundant] conn-{} latency: {}",
                    self.connections.get(i).map(|c| c.config.id).unwrap_or(0),
                    stats
                );
                if stats.avg_us > worst_avg {
                    worst_avg = stats.avg_us;
                    worst_idx = Some(i);
                }
            }
        }

        // Reset the worst one
        if let Some(idx) = worst_idx {
            if self.connections.len() > 1 {
                warn!("[redundant] resetting slowest connection (idx={idx}, avg={worst_avg:.0}µs)");
                // Stop the old connection
                if let Some(conn) = self.connections.get_mut(idx) {
                    conn.stop().await;
                }
                // Reset latency collector
                if let Some(lc) = self.latency_collectors.get_mut(idx) {
                    lc.reset();
                }
                // Start a replacement
                let mut new_config = self.config.base_config.clone();
                new_config.id = self.next_conn_id;
                self.next_conn_id += 1;

                let mut new_conn = WsConnection::new(new_config);
                new_conn.start(on_text, on_binary);
                self.connections[idx] = new_conn;

                return Some(idx);
            }
        }

        // Reset all latency collectors for the next evaluation period
        for lc in &mut self.latency_collectors {
            lc.reset();
        }

        None
    }

    /// Stop all connections.
    pub async fn stop(&mut self) {
        for conn in &mut self.connections {
            conn.stop().await;
        }
        self.connections.clear();
    }

    /// Get the number of active connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    fn add_and_start_connection(
        &mut self,
        on_text: OnMessageCallback,
        on_binary: Option<OnBinaryCallback>,
    ) {
        let mut config = self.config.base_config.clone();
        config.id = self.next_conn_id;
        self.next_conn_id += 1;

        let mut conn = WsConnection::new(config);
        conn.start(on_text, on_binary);
        self.connections.push(conn);
    }
}
