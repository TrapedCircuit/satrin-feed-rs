//! WebSocket client with auto-reconnect and redundancy support.

pub mod client;
pub mod redundant;

pub use client::{OnBinaryCallback, OnMessageCallback, PingPayload, WsConnConfig, WsConnection};
pub use redundant::RedundantWsClient;
