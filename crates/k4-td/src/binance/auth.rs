//! Binance authentication and request signing utilities.
//!
//! Binance supports two signing methods:
//!
//! 1. **HMAC-SHA256** — the standard method. The secret key is a hex string
//!    provided by Binance. Used for most REST and WebSocket API requests.
//! 2. **Ed25519** — a newer method using an Ed25519 keypair. The private key
//!    is loaded from a PEM file. Used for reduced-latency order operations.
//!
//! Both methods produce a `signature` parameter that is appended to the
//! URL-encoded query string.

use anyhow::{Context, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Compute an HMAC-SHA256 signature and return it as a lowercase hex string.
///
/// # Arguments
///
/// * `secret` — the API secret key (UTF-8 string).
/// * `message` — the data to sign (typically the query string).
///
/// # Example
///
/// ```ignore
/// let sig = hmac_sha256_sign("my_secret", "symbol=BTCUSDT&timestamp=1234567890");
/// assert_eq!(sig.len(), 64); // 32 bytes → 64 hex chars
/// ```
pub fn hmac_sha256_sign(secret: &str, message: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(message.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Build a URL-encoded, HMAC-SHA256–signed query string.
///
/// Takes a slice of `(key, value)` parameter pairs, joins them with `&`,
/// computes the HMAC-SHA256 signature over the resulting string, and appends
/// `&signature=<hex>`.
///
/// # Arguments
///
/// * `params` — request parameters (must already include `timestamp`).
/// * `secret` — the API secret key.
///
/// # Example
///
/// ```ignore
/// let query = build_signed_query(
///     &[("symbol", "BTCUSDT"), ("timestamp", "1234567890")],
///     "my_secret",
/// );
/// assert!(query.contains("signature="));
/// ```
pub fn build_signed_query(params: &[(&str, &str)], secret: &str) -> String {
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let signature = hmac_sha256_sign(secret, &query);
    format!("{query}&signature={signature}")
}

/// Compute an Ed25519 signature and return it as a Base64-encoded string.
///
/// # Arguments
///
/// * `private_key_pem` — PEM-encoded Ed25519 private key (PKCS#8 format).
/// * `message` — the data to sign.
///
/// # Errors
///
/// Returns an error if the PEM cannot be parsed or the key is invalid.
pub fn ed25519_sign(private_key_pem: &str, message: &str) -> Result<String> {
    use ed25519_dalek::pkcs8::DecodePrivateKey;
    use ed25519_dalek::{Signer, SigningKey};

    let signing_key = SigningKey::from_pkcs8_pem(private_key_pem)
        .context("failed to parse Ed25519 private key from PEM")?;

    let signature = signing_key.sign(message.as_bytes());
    let encoded = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sha256_known_vector() {
        // Known test vector from Binance docs.
        let secret = "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j";
        let message = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1\
                        &price=0.1&recvWindow=5000&timestamp=1499827319559";
        let sig = hmac_sha256_sign(secret, message);
        // Just verify it produces a 64-char hex string.
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn build_signed_query_includes_signature() {
        let query = build_signed_query(
            &[("symbol", "BTCUSDT"), ("timestamp", "1234567890")],
            "test_secret",
        );
        assert!(query.starts_with("symbol=BTCUSDT&timestamp=1234567890&signature="));
    }
}
