# k4-runner

Main executable entry point for the K4 Crypto system.

## CLI

```
k4-runner <CONFIG_FILE> [--log-level <LEVEL>]
```

## Lifecycle

1. Parse CLI args (clap)
2. Init tracing logging
3. Load JSON config → `AppConfig`
4. For each `connections[]` entry:
   a. `create_md_module(config)` → `Box<dyn MdModule>`
   b. `module.init_shm()`
   c. `module.start()`
5. Wait for SIGINT/SIGTERM (`tokio::signal::ctrl_c`)
6. Stop all modules in reverse order

## Config Format

See `config/*.json` for examples. Top-level structure:
```json
{
  "RazorTrade": { "module_name": "...", "log_path": "..." },
  "connections": [{ "exchange": "binance", ... }]
}
```
