//! Logging initialization using the `tracing` ecosystem.
//!
//! Replaces the C++ spdlog-based `K4_LoggerBase`. Provides:
//! - Console output (colored, human-readable)
//! - File output (daily rotation via `tracing-appender`)
//! - Configurable log level via env var `RUST_LOG` or explicit parameter

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize the global tracing subscriber.
///
/// Should be called once at program start. After this, all `tracing::info!()`
/// etc. macros will produce output.
///
/// # Parameters
///
/// - `log_level`: default level if `RUST_LOG` env var is not set (e.g. `"info"`)
/// - `log_dir`: optional directory for daily-rotating log files
/// - `module_name`: used as the log file prefix (e.g. `"binance_md"`)
pub fn init_logging(log_level: &str, log_dir: Option<&str>, module_name: &str) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    let console_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_ansi(true);

    if let Some(dir) = log_dir {
        let file_appender = tracing_appender::rolling::daily(dir, module_name);
        let file_layer = fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .with(file_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .init();
    }
}
