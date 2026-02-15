# k4-runner

Main executable entry point for the crypto-gateway system.

## CLI

```text
k4-runner <CONFIG_FILE> [--log-level <LEVEL>]
```

## Lifecycle

1. Parse CLI args (clap)
2. Init tracing logging
3. Load JSON config into `AppConfig`
4. For each `connections[]` entry:
   - `create_md_module(config)` returns `Box<dyn MdModule>`
   - `module.init_shm()`
   - `module.start()`
5. Wait for SIGINT/SIGTERM (`tokio::signal::ctrl_c`)
6. Stop all modules in reverse order

## Config Format

See `config/*.json` for examples. Top-level structure:

```json
{
  "RazorTrade": { "module_name": "...", "log_path": "..." },
  "connections": [{ "exchange": "binance", "..." : "..." }]
}
```
