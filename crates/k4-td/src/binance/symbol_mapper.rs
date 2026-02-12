//! Bidirectional symbol mapping between exchange and display formats.
//!
//! Binance uses concatenated symbols (e.g. `BTCUSDT`), while many internal
//! systems and UIs prefer the slash-separated form (`BTC/USDT`). This module
//! provides a [`SymbolMapper`] that converts between the two formats.
//!
//! Default mappings for common pairs are pre-loaded, and additional mappings
//! can be dynamically built from the Binance `exchangeInfo` response.

use std::collections::HashMap;

/// Bidirectional symbol mapper.
///
/// Maintains two hash maps for O(1) lookups in either direction.
#[derive(Debug, Clone)]
pub struct SymbolMapper {
    /// Exchange format → display format (e.g. `BTCUSDT` → `BTC/USDT`).
    exchange_to_display: HashMap<String, String>,
    /// Display format → exchange format (e.g. `BTC/USDT` → `BTCUSDT`).
    display_to_exchange: HashMap<String, String>,
}

/// Common quote assets used by Binance.
const COMMON_QUOTES: &[&str] = &["USDT", "USDC", "BUSD", "BTC", "ETH", "BNB", "TUSD", "FDUSD"];

/// Common base assets for pre-populated default mappings.
const COMMON_BASES: &[&str] = &[
    "BTC", "ETH", "BNB", "SOL", "XRP", "DOGE", "ADA", "AVAX", "DOT", "MATIC", "LINK", "UNI",
    "ATOM", "LTC", "ETC", "FIL", "APT", "ARB", "OP", "NEAR",
];

impl SymbolMapper {
    /// Create a new mapper pre-loaded with default common pair mappings.
    pub fn new() -> Self {
        let mut mapper = Self {
            exchange_to_display: HashMap::new(),
            display_to_exchange: HashMap::new(),
        };
        mapper.load_defaults();
        mapper
    }

    /// Create an empty mapper with no default mappings.
    pub fn empty() -> Self {
        Self {
            exchange_to_display: HashMap::new(),
            display_to_exchange: HashMap::new(),
        }
    }

    /// Convert an exchange symbol to display format.
    ///
    /// Returns the original string if no mapping exists.
    pub fn to_display<'a>(&'a self, exchange: &'a str) -> &'a str {
        self.exchange_to_display
            .get(exchange)
            .map(|s| s.as_str())
            .unwrap_or(exchange)
    }

    /// Convert a display symbol to exchange format.
    ///
    /// Returns the original string if no mapping exists.
    pub fn to_exchange<'a>(&'a self, display: &'a str) -> &'a str {
        self.display_to_exchange
            .get(display)
            .map(|s| s.as_str())
            .unwrap_or(display)
    }

    /// Add a single bidirectional mapping.
    ///
    /// # Arguments
    ///
    /// * `exchange` — exchange format (e.g. `"BTCUSDT"`).
    /// * `display` — display format (e.g. `"BTC/USDT"`).
    pub fn add_mapping(&mut self, exchange: &str, display: &str) {
        self.exchange_to_display
            .insert(exchange.to_string(), display.to_string());
        self.display_to_exchange
            .insert(display.to_string(), exchange.to_string());
    }

    /// Load mappings from a Binance `exchangeInfo` JSON response.
    ///
    /// Expects a JSON object with a `"symbols"` array where each element has
    /// `"symbol"`, `"baseAsset"`, and `"quoteAsset"` fields.
    pub fn load_from_exchange_info(&mut self, info: &serde_json::Value) {
        let Some(symbols) = info.get("symbols").and_then(|s| s.as_array()) else {
            return;
        };

        for sym_info in symbols {
            let Some(symbol) = sym_info.get("symbol").and_then(|s| s.as_str()) else {
                continue;
            };
            let Some(base) = sym_info.get("baseAsset").and_then(|s| s.as_str()) else {
                continue;
            };
            let Some(quote) = sym_info.get("quoteAsset").and_then(|s| s.as_str()) else {
                continue;
            };

            let display = format!("{base}/{quote}");
            self.add_mapping(symbol, &display);
        }
    }

    /// Returns the number of mappings currently stored.
    pub fn len(&self) -> usize {
        self.exchange_to_display.len()
    }

    /// Returns `true` if no mappings are stored.
    pub fn is_empty(&self) -> bool {
        self.exchange_to_display.is_empty()
    }

    /// Pre-populate with common `BASE+QUOTE` pairs.
    fn load_defaults(&mut self) {
        for base in COMMON_BASES {
            for quote in COMMON_QUOTES {
                if *base != *quote {
                    let exchange = format!("{base}{quote}");
                    let display = format!("{base}/{quote}");
                    self.add_mapping(&exchange, &display);
                }
            }
        }
    }
}

impl Default for SymbolMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_round_trip() {
        let mapper = SymbolMapper::new();
        assert_eq!(mapper.to_display("BTCUSDT"), "BTC/USDT");
        assert_eq!(mapper.to_exchange("BTC/USDT"), "BTCUSDT");
    }

    #[test]
    fn unknown_symbol_passthrough() {
        let mapper = SymbolMapper::new();
        assert_eq!(mapper.to_display("UNKNOWNPAIR"), "UNKNOWNPAIR");
        assert_eq!(mapper.to_exchange("FOO/BAR"), "FOO/BAR");
    }

    #[test]
    fn custom_mapping() {
        let mut mapper = SymbolMapper::empty();
        mapper.add_mapping("1000PEPEUSDT", "1000PEPE/USDT");
        assert_eq!(mapper.to_display("1000PEPEUSDT"), "1000PEPE/USDT");
        assert_eq!(mapper.to_exchange("1000PEPE/USDT"), "1000PEPEUSDT");
    }

    #[test]
    fn load_from_exchange_info() {
        let info = serde_json::json!({
            "symbols": [
                {"symbol": "WIFUSDT", "baseAsset": "WIF", "quoteAsset": "USDT"},
                {"symbol": "TRUMPUSDT", "baseAsset": "TRUMP", "quoteAsset": "USDT"},
            ]
        });
        let mut mapper = SymbolMapper::empty();
        mapper.load_from_exchange_info(&info);
        assert_eq!(mapper.to_display("WIFUSDT"), "WIF/USDT");
        assert_eq!(mapper.to_exchange("TRUMP/USDT"), "TRUMPUSDT");
    }
}
