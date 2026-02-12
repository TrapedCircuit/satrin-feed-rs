# k4-td

Trading (TD) modules for order placement, cancellation, and position management.

## TdModule Trait

All trading modules implement `TdModule` with methods:
- `login()` — authenticate and establish WebSocket user data streams
- `insert_order()` / `cancel_order()` — place or cancel orders
- `query_open_orders()` / `query_positions()` — account queries
- `stop()` — graceful shutdown

## Binance TD

### Authentication (`auth.rs`)
- HMAC-SHA256: `hmac_sha256_sign(secret, message)`
- Ed25519: `ed25519_sign(private_key_pem, message)`
- Signed query: `build_signed_query(params, secret)` — URL-encoded with timestamp + signature

### Account Types
- `SpotClient` — REST: `/api/v3/*`, WS API: `wss://ws-api.binance.com/ws-api/v3`
- `FuturesClient` — UBase: `/fapi/*`, CBase: `/dapi/*`

### Order Flow
1. Strategy sends `InputOrder` → `BinanceTd.insert_order()`
2. Routes to SpotClient or FuturesClient by `AccountType`
3. Client sends via WebSocket API (JSON-RPC: `order.place`)
4. Exchange response parsed → `OrderUpdate` emitted

### Listen Key Management
- Created on login, refreshed every 30 minutes
- Used for user data stream WebSocket (`/ws/{listenKey}`)
