//! Wallet address assets: a public crypto address read on chain.
//!
//! Read only, and only the public balance: BTC through mempool.space's address API (confirmed
//! funded outputs minus spent ones), ETH through a public JSON-RPC node (`eth_getBalance` at
//! the latest block). The address is checked for format only: a well-formed address that
//! nobody uses reads as an honest zero from the chain, which is not the same as a guess.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

const MEMPOOL: &str = "https://mempool.space/api/address";
/// A public Ethereum node. cloudflare-eth.com answers "Internal error" to eth_getBalance
/// since its gateway was retired, so a maintained public endpoint is used instead.
const ETH_RPC: &str = "https://ethereum-rpc.publicnode.com";

/// The CoinGecko id each chain's coin is priced under, through the Portfolio Tracker's quote
/// path (`portfolios::prices::crypto_prices`, USD).
pub fn coin_id(chain: &str) -> Option<&'static str> {
    match chain {
        "btc" => Some("bitcoin"),
        "eth" => Some("ethereum"),
        _ => None,
    }
}

pub fn unit(chain: &str) -> &'static str {
    if chain == "eth" {
        "ETH"
    } else {
        "BTC"
    }
}

const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BECH32: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";

/// Format check only: the right prefix, length and alphabet. No checksum, no lookup.
pub fn validate_address(chain: &str, addr: &str) -> Result<(), String> {
    let a = addr.trim();
    match chain {
        "btc" => {
            let lower = a.to_ascii_lowercase();
            if let Some(data) = lower.strip_prefix("bc1") {
                // Bech32 is single-case: mixed case is malformed by definition.
                let single_case = a == lower || a == a.to_ascii_uppercase();
                if single_case && (11..=71).contains(&data.len()) && data.chars().all(|c| BECH32.contains(c)) {
                    return Ok(());
                }
                return Err("not a valid bc1 address: lowercase letters and digits from the bech32 alphabet".into());
            }
            if (a.starts_with('1') || a.starts_with('3'))
                && (26..=35).contains(&a.len())
                && a.chars().all(|c| BASE58.contains(c))
            {
                return Ok(());
            }
            Err("a Bitcoin address starts with 1, 3 or bc1".into())
        }
        "eth" => {
            let hex = a.strip_prefix("0x").or_else(|| a.strip_prefix("0X"));
            match hex {
                Some(h) if h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit()) => Ok(()),
                _ => Err("an Ethereum address is 0x followed by 40 hexadecimal characters".into()),
            }
        }
        _ => Err("chain must be btc or eth".into()),
    }
}

/// Satoshis to bitcoin.
pub fn sats_to_btc(sats: i64) -> f64 {
    sats as f64 / 100_000_000.0
}

/// A JSON-RPC quantity (`0x…` wei) to ether.
pub fn wei_to_eth(hex: &str) -> Result<f64, String> {
    let digits = hex.strip_prefix("0x").ok_or_else(|| format!("not a hex quantity: {hex}"))?;
    if digits.is_empty() {
        return Ok(0.0);
    }
    let wei = u128::from_str_radix(digits, 16).map_err(|_| format!("not a hex quantity: {hex}"))?;
    // Split so the integer part keeps its precision before the float division.
    let whole = (wei / 1_000_000_000_000_000_000) as f64;
    let frac = (wei % 1_000_000_000_000_000_000) as f64 / 1e18;
    Ok(whole + frac)
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("OpenTraderWorld/wealth")
        .build()
        .map_err(|e| anyhow!("building http client: {e}"))
}

/// Confirmed balance of a Bitcoin address, in BTC.
async fn read_btc(addr: &str) -> Result<f64> {
    let url = format!("{MEMPOOL}/{addr}");
    let res = crate::rate::send("mempool", client()?.get(&url)).await.context("mempool.space request")?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        bail!("mempool.space answered {status}: {}", body.chars().take(200).collect::<String>());
    }
    let body: Value = res.json().await.context("mempool.space answer")?;
    let funded = body.pointer("/chain_stats/funded_txo_sum").and_then(Value::as_i64);
    let spent = body.pointer("/chain_stats/spent_txo_sum").and_then(Value::as_i64);
    match (funded, spent) {
        (Some(f), Some(s)) => Ok(sats_to_btc(f - s)),
        _ => bail!("mempool.space answer has no chain_stats"),
    }
}

/// Balance of an Ethereum address at the latest block, in ETH.
async fn read_eth(addr: &str) -> Result<f64> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "eth_getBalance", "params": [addr, "latest"] });
    let res = crate::rate::send("ethereum-rpc", client()?.post(ETH_RPC).json(&body))
        .await
        .context("Ethereum node request")?;
    let status = res.status();
    if !status.is_success() {
        bail!("the Ethereum node answered {status}");
    }
    let v: Value = res.json().await.context("Ethereum node answer")?;
    if let Some(err) = v.get("error") {
        bail!("the Ethereum node refused: {}", err.get("message").and_then(Value::as_str).unwrap_or("unknown error"));
    }
    let hex = v.get("result").and_then(Value::as_str).ok_or_else(|| anyhow!("the Ethereum node gave no balance"))?;
    wei_to_eth(hex).map_err(|e| anyhow!(e))
}

/// The address's balance in its coin.
pub async fn balance(chain: &str, addr: &str) -> Result<f64> {
    match chain {
        "btc" => read_btc(addr.trim()).await,
        "eth" => read_eth(addr.trim()).await,
        _ => bail!("chain must be btc or eth"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_formats() {
        assert!(validate_address("btc", "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa").is_ok());
        assert!(validate_address("btc", "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy").is_ok());
        assert!(validate_address("btc", "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh").is_ok());
        assert!(validate_address("btc", "BC1QXY2KGDYGJRSQTZQ2N0YRF2493P83KKFJHX0WLH").is_ok());
        // Mixed case, a letter outside the alphabet (b, i, o, 1 after the prefix), too short.
        assert!(validate_address("btc", "bc1Qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh").is_err());
        assert!(validate_address("btc", "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlb").is_err());
        assert!(validate_address("btc", "1A1zP1eP5QGefi2DMPTfTL5SLmv7Divf0a").is_err()); // 0 is not base58
        assert!(validate_address("btc", "0x52908400098527886E0F7030069857D2E4169EE7").is_err());
        assert!(validate_address("eth", "0x52908400098527886E0F7030069857D2E4169EE7").is_ok());
        assert!(validate_address("eth", "0x52908400098527886E0F7030069857D2E4169EE").is_err());
        assert!(validate_address("eth", "52908400098527886E0F7030069857D2E4169EE7").is_err());
        assert!(validate_address("eth", "0xZZ908400098527886E0F7030069857D2E4169EE7").is_err());
        assert!(validate_address("doge", "D").is_err());
    }

    #[test]
    fn units_convert_exactly() {
        assert_eq!(sats_to_btc(100_000_000), 1.0);
        assert_eq!(sats_to_btc(394_526_549), 3.94526549);
        assert_eq!(wei_to_eth("0xde0b6b3a7640000").unwrap(), 1.0);
        assert_eq!(wei_to_eth("0x0").unwrap(), 0.0);
        assert_eq!(wei_to_eth("0x").unwrap(), 0.0);
        // 5 774.55 ETH: past u64 wei, the integer part kept exact.
        let big = wei_to_eth("0x1390a01262bd0146c07").unwrap();
        assert!((big - 5_774.551_794_456_061_5).abs() < 1e-9);
        assert!(wei_to_eth("12").is_err());
        assert!(wei_to_eth("0xzz").is_err());
    }
}
