# K4 Crypto — Rust

Low-latency, multi-source, redundant cryptocurrency market data and trading system.

## Building

```bash
# Ensure Rust 1.85+ is installed
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

```
rust/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── k4-types/           # Core data types and enums
│   ├── k4-core/            # Shared memory, UDP, config, latency, dedup
│   ├── k4-ws/              # WebSocket client with redundancy
│   ├── k4-md/              # Market data modules (Binance/OKX/Bitget/Bybit/UDP)
│   ├── k4-td/              # Trading modules (Binance Spot + Futures)
│   └── k4-runner/          # CLI entry point
├── config/                 # Example JSON configs
└── schema/                 # FlatBuffers schema for UDP
```

## Supported Exchanges

| Exchange | Market Data | Trading | Protocol |
|----------|-------------|---------|----------|
| Binance  | BBO, AggTrade, Trade, Depth5 | Spot, UBase, CBase | WS JSON + SBE |
| OKX      | BBO, Trade, Depth5 | — | WS JSON |
| Bitget   | BBO, Trade, Depth5 | — | WS JSON |
| Bybit    | BBO, Trade, Depth5 | — | WS JSON |
| UDP      | All types | — | UDP + binary |

## Architecture

- **Single binary**: All exchanges compiled in, selected via JSON config
- **Redundant connections**: N WebSocket connections per subscription, fastest wins
- **Shared memory**: Zero-copy ring buffers for downstream consumers
- **Async runtime**: tokio for WebSocket I/O, spawn_blocking for CPU-bound dedup
- **Structured logging**: tracing with daily file rotation

## Key Dependencies

- `tokio` — async runtime
- `tokio-tungstenite` — WebSocket client
- `serde` / `serde_json` — JSON serialization
- `reqwest` — HTTP client (trading)
- `anyhow` / `thiserror` — error handling
- `clap` — CLI argument parsing
- `tracing` — structured logging
- `crossbeam-channel` — lock-free MPMC channels
- `hmac` / `sha2` / `ed25519-dalek` — cryptographic signing

## Configuration

See `config/` directory for example JSON files. Each config has:

- `connections[]` — array of exchange module instances
- Per-connection: exchange type, symbols, SHM names, redundancy settings
- Optional UDP forwarding configuration

## License

MIT
