# k4-md

Market data modules for 4 exchanges + UDP receiver.

## Module Registry

`registry::create_md_module(config)` → `Box<dyn MdModule>` dispatches by `config.exchange`:

| Exchange | Module | WS URL | Data Types |
|----------|--------|--------|------------|
| Binance | `binance::BinanceMd` | stream.binance.com, stream-sbe.binance.com, fstream.binance.com | BBO, AggTrade, Trade, Depth5 |
| OKX | `okx::OkxMd` | ws.okx.com:8443 | BBO, Trade, Depth5 |
| Bitget | `bitget::BitgetMd` | ws.bitget.com | BBO, Trade, Depth5 |
| Bybit | `bybit::BybitMd` | stream.bybit.com | BBO, Trade, Depth5 |
| UDP | `udp::UdpMd` | N/A (UDP socket) | All types |

## Exchange-Specific Details

### Binance
- Dual protocol: JSON (aggTrade) + SBE binary (BBO, trade, depth) for spot
- SBE uses Decimal128 encoding: `mantissa × 10^(exponent + 18)`
- Separate dedup threads for JSON and SBE streams

### OKX
- Symbol conversion: `BTCUSDT` → `BTC-USDT` (spot) / `BTC-USDT-SWAP` (swap)
- Routes by `arg.channel` field

### Bitget
- Trade messages are batched (array), processed in reverse order
- `instType`: `"SPOT"` or `"USDT-FUTURES"`

### Bybit
- Futures trade IDs are UUIDs → hashed via xxHash64 for dedup
- Depth5 from incremental `orderbook.50` → `OrderBook<50>` state machine
- Routes by `topic` field prefix

### UDP
- Receives from other modules' UdpSender, writes directly to SHM (no dedup)
