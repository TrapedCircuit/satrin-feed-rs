# K4 Crypto — Rust Implementation

## Overview

Low-latency, multi-source, redundant cryptocurrency market data and trading system.

## Architecture

```
Exchange WS ──► k4-md module ──► dedup ──► SHM (k4-core/shm)
                                       └──► UDP (k4-core/udp)
Strategy ──► k4-td module ──► Exchange REST/WS API
```

## Crate Map (4 crates)

| Crate | Purpose | Key modules |
|-------|---------|-------------|
| `k4-core` | Types, config, SHM, UDP, WebSocket, latency, dedup, logging | `types/`, `config`, `shm`, `udp`, `ws/` |
| `k4-md` | Market data (Binance/OKX/Bitget/Bybit/UDP) | `binance/`, `okx/`, `bitget/`, `bybit/`, `udp/` |
| `k4-td` | Trading (Binance Spot + Futures) | `binance/` |
| `k4-runner` | CLI entry point, module orchestration | `main.rs` |

## Key Patterns

- **Redundant connections**: Same subscription × N connections. Fastest wins.
- **Dedup by update_id**: `UpdateIdDedup` (monotonic) or `UuidDedup` (hash-based for Bybit futures).
- **SHM ring buffer**: `ShmMdStore<T>` with per-symbol circular buffers and atomic indexing.
- **Async + blocking hybrid**: WS connections are tokio tasks; dedup loops run on `spawn_blocking`.
