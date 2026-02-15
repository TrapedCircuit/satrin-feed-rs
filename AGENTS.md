# crypto-gateway-rs

## Overview

Rust implementation of a low-latency cryptocurrency market
data and trading system.

## Architecture

Each exchange provides `build(config) -> Vec<StreamDef>`.
The generic `GenericMd` engine (in `pipeline.rs`) handles
SHM, channels, dedup, and WS automatically.

```text
Exchange WS ──► StreamDef ──► GenericMd ──► dedup ──► SHM + UDP
Strategy ──► k4-td ──► Exchange REST/WS API
```

## Crate Map (4 crates)

| Crate | Purpose |
|-------|---------|
| `k4-core` | Types, config, SHM, UDP, WS, dedup, logging |
| `k4-md` | MD: generic pipeline + per-exchange parsers |
| `k4-td` | TD: Binance Spot + Futures |
| `k4-runner` | CLI entry point |

## Key Patterns

- **StreamDef pipeline** — data-driven exchange modules
- **rkyv for UDP** — safe zero-copy serialization
- **SHM ring buffers** — atomic indexing, raw repr(C)
- **CPU pinning** — core_affinity in dedup threads
- **Redundant WS** — N connections, fastest wins
