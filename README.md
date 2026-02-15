# crypto-gateway-rs

Low-latency, multi-source, redundant cryptocurrency market data and trading system — Rust implementation.

## Building

```bash
# Requires Rust 1.85+ (2024 edition)
rustup update stable

# Build all crates
cargo build --release

# Run tests
cargo test
```

## Running

```bash
# Start with a config file
cargo run --release --bin k4-runner -- config/binance_md.json

# With custom log level
cargo run --release --bin k4-runner -- config/binance_md.json --log-level debug
```

## Project Structure

```text
├── Cargo.toml                 # Workspace root
├── crates/
│   ├── k4-core/               # Types, config, SHM, UDP (rkyv), WebSocket, latency, dedup, CPU affinity
│   ├── k4-md/                 # Market data modules (generic pipeline + per-exchange parsers)
│   ├── k4-td/                 # Trading modules (Binance Spot + Futures)
│   └── k4-runner/             # CLI entry point
├── config/                    # Example JSON configs
└── schema/                    # FlatBuffers schema (reference)
```

## Architecture

```text
Exchange WS ──► exchange::build() ──► Vec<StreamDef>
                                          │
                                    GenericMd engine
                                          │
                              ┌───────────┼───────────┐
                              ▼           ▼           ▼
                          WS task    dedup task    SHM store
                        (tokio)   (spawn_blocking)  (mmap)
                                          │
                                     UDP sender
                                     (optional)
```

Each exchange only provides a `build(config) -> Vec<StreamDef>` function describing its WebSocket streams. The generic `GenericMd` engine handles SHM creation, channel wiring, dedup tasks, and WS connections automatically.

### Key design decisions

- **Generic pipeline** — `StreamDef` + `GenericMd` eliminates per-exchange boilerplate. Adding a new exchange requires only a `build()` function + JSON parser.
- **rkyv serialization** — zero-copy ser/deser for UDP, replacing manual `unsafe` pointer operations. SHM uses raw `#[repr(C)]` structs for maximum speed.
- **Redundant connections** — N WebSocket connections per subscription, deduplication by `update_id`. Slowest connection is periodically rotated out.
- **CPU affinity** — dedup threads can be pinned to specific cores via config (`core_affinity` crate).
- **Typed errors** — `K4Error` via `thiserror` for domain-specific errors, `anyhow` at the top level.

## Supported Exchanges

| Exchange | Market Data | Trading | Protocol | Unique features |
|----------|-------------|---------|----------|-----------------|
| Binance  | BBO, AggTrade, Trade, Depth5 | Spot, UBase, CBase | WS JSON + SBE | SBE binary, dual streams |
| OKX      | BBO, Trade, Depth5 | — | WS JSON | Symbol conversion (`BTCUSDT` → `BTC-USDT`) |
| Bitget   | BBO, Trade, Depth5 | — | WS JSON | Batch trade messages |
| Bybit    | BBO, Trade, Depth5 | — | WS JSON | Incremental OrderBook, UUID trade dedup |
| UDP      | All types | — | UDP + rkyv | Receives from other modules |

## Crate Details

### k4-core

Core infrastructure shared by all modules:

| Module | Purpose |
|--------|---------|
| `types/` | Enums (`ProductType`, `MessageType`), market data structs (`Bookticker`, `Trade`, `AggTrade`, `Depth5`), trading structs |
| `config` | JSON config deserialization (`AppConfig`, `ConnectionConfig`) |
| `shm` | `ShmMdStore<T>` — POSIX shared memory ring buffer (Linux mmap, macOS heap fallback) |
| `udp` | `UdpSender` / `UdpReceiver` — async UDP with rkyv zero-copy serialization |
| `ws/` | `WsConnection` (auto-reconnect) + `RedundantWsClient` (N-way redundancy) |
| `dedup` | `UpdateIdDedup` (monotonic sequence) + `UuidDedup` (hash table for Bybit) |
| `latency` | `LatencyCollector` — histogram-based (10µs bins, p50/p90/p99) |
| `cpu_affinity` | Thread-to-core pinning for low-latency dedup |
| `error` | `K4Error` — domain-specific error types via thiserror |
| `logging` | `init_logging()` — tracing + daily file rotation |

### k4-md

Market data modules using the generic pipeline:

| File | Purpose |
|------|---------|
| `pipeline.rs` | `StreamDef` descriptor + `GenericMd` engine (implements `MdModule`) |
| `dedup_worker.rs` | Generic dedup loop with `ProductShmStores` |
| `ws_helper.rs` | WebSocket stream launchers (text + binary) |
| `json_util.rs` | Shared JSON parsing helpers (`parse_str_f64`, `fill_depth5_levels`) |
| `binance/` | `build()` + JSON parser + SBE binary parser |
| `okx/` | `build()` + JSON parser + symbol conversion |
| `bitget/` | `build()` + JSON parser (batch trade handling) |
| `bybit/` | `build()` + JSON parser + `OrderBook<50>` + UUID dedup |
| `udp/` | Direct UDP-to-SHM receiver (no WebSocket) |

### k4-td

Binance trading module:

- **Authentication** — HMAC-SHA256 + Ed25519 signing
- **Spot** — REST (`/api/v3/*`) + WebSocket API (`order.place`, `order.cancel`)
- **Futures** — REST (`/fapi/v1/*`, `/dapi/v1/*`) for UBase and CBase
- **Symbol mapping** — bidirectional `BTCUSDT` ↔ `BTC/USDT`
- **Listen key management** — automatic refresh for user data streams

## Key Dependencies

| Purpose | Crate |
|---------|-------|
| Async runtime | `tokio` |
| WebSocket | `tokio-tungstenite` |
| Serialization | `serde` / `serde_json` / `rkyv` |
| HTTP client | `reqwest` |
| Error handling | `anyhow` / `thiserror` |
| CLI | `clap` |
| Logging | `tracing` / `tracing-subscriber` / `tracing-appender` |
| Channels | `crossbeam-channel` |
| Hashing | `ahash` / `xxhash-rust` |
| Crypto | `hmac` / `sha2` / `ed25519-dalek` |
| CPU affinity | `core_affinity` |

## Configuration

See `config/` directory for example JSON files. Each config has:

```json
{
  "RazorTrade": { "module_name": "binance_md", "log_path": "/tmp/logs" },
  "connections": [{
    "exchange": "binance",
    "md_size": 100000,
    "spot": {
      "symbols": ["BTCUSDT", "ETHUSDT"],
      "bbo_shm_name": "binance_spot_bbo",
      "trade_shm_name": "binance_spot_trade"
    },
    "futures": { ... },
    "udp_sender": { "ip": "127.0.0.1", "port": 9000, "enabled": false }
  }]
}
```

## License

MIT
