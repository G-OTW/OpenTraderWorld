//! Live checks on the broker connectors, **with no account anywhere**.
//!
//! Two things can be established without a single credential:
//!
//!   1. **What is keyless already runs.** The crypto venues price an asset from a public
//!      board, so `quotes` is a whole connector path, end to end, against the real venue.
//!   2. **A junk key proves the request was well formed.** Every venue here answers a
//!      malformed signature differently from an unknown key: Binance says `-1022` for the
//!      first and `-2015` for the second, OKX `50113` against `50111`, Bitget `40009`
//!      against `40037`. Reaching the *key* refusal means the URL, the path, the headers,
//!      the timestamp and the signed string were all accepted, which is the half of the
//!      request a funded account would not test any better. What it cannot prove is the
//!      shape of the answer, so the parsers stay covered by their own unit tests.
//!
//! Every message a venue returns is printed, because the point of the exercise is reading
//! what each one actually said. Network-gated on `OTW_NET_TEST=1`.
//! Run with: `OTW_NET_TEST=1 cargo test -p otw-core brokers::net_tests -- --nocapture`.

use std::collections::HashMap;

use super::{broker_for, client};

fn enabled() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

fn creds(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

// ── 1. The keyless half of a broker connector ────────────────────────────────

/// `quotes` reads a public board, so it is a full connector path with no credential at all:
/// the request, the venue's answer, the pair chosen to price the coin, and the number.
#[tokio::test]
async fn a_crypto_venue_prices_a_coin_without_a_key() {
    if !enabled() {
        return;
    }
    let http = client().unwrap();
    let asked: Vec<String> = ["BTC", "ETH"].iter().map(|s| s.to_string()).collect();
    let mut seen = Vec::new();
    for broker in ["okx", "bitget", "binance_futures"] {
        let b = broker_for(broker).unwrap();
        let quotes = b
            .quotes(&http, &creds(&[]), &asked)
            .await
            .unwrap_or_else(|e| panic!("{broker}: {e:#}"));
        for q in &quotes {
            println!("{broker}: {} = {} {} (via {})", q.symbol, q.price, q.currency, q.pair);
            assert!(q.price > 0.0, "{broker}: {} priced at {}", q.symbol, q.price);
            assert!(!q.currency.is_empty(), "{broker}: {} has no currency", q.symbol);
            assert!(
                q.pair.to_uppercase().contains(&q.symbol.to_uppercase()),
                "{broker}: {} is priced off {}, which is not its market",
                q.symbol,
                q.pair
            );
        }
        let btc = quotes
            .iter()
            .find(|q| q.symbol == "BTC")
            .unwrap_or_else(|| panic!("{broker}: priced no BTC"));
        seen.push((broker, btc.price));
    }
    // Three venues, one coin: a decimal read out of the wrong field shows up as a number
    // the other two disagree with.
    let hi = seen.iter().map(|(_, p)| *p).fold(f64::MIN, f64::max);
    let lo = seen.iter().map(|(_, p)| *p).fold(f64::MAX, f64::min);
    assert!((hi - lo) / lo < 0.01, "BTC priced {lo} to {hi} across venues: {seen:?}");
}

/// **Never guess an instrument.** A coin no venue lists is left out of the answer, not
/// priced off something that merely looks like it.
#[tokio::test]
async fn an_asset_the_venue_does_not_list_is_left_out_rather_than_guessed() {
    if !enabled() {
        return;
    }
    let http = client().unwrap();
    let asked = vec!["ZZZZNOPE".to_string()];
    for broker in ["okx", "bitget", "binance_futures"] {
        let b = broker_for(broker).unwrap();
        let quotes = b.quotes(&http, &creds(&[]), &asked).await.unwrap();
        assert!(quotes.is_empty(), "{broker}: invented a price for a coin that does not exist");
    }
}

// ── 2. What a junk credential proves ─────────────────────────────────────────

/// Credentials shaped the way each venue's own are shaped, so the request is built and
/// signed exactly as a real one would be, and the refusal comes from the venue's
/// authentication layer rather than from our own validation.
fn junk() -> Vec<(&'static str, HashMap<String, String>)> {
    const K: &str = "0123456789abcdef0123456789abcdef";
    // Binance rejects a wrong *length* (-2014) before it looks a key up (-2015), so the
    // probe has to be 64 characters or it never reaches the layer being tested.
    const K64: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    vec![
        ("alpaca", creds(&[("api_key", "PKZZZZZZZZZZZZZZZZZZ"), ("api_secret", K), ("env", "paper")])),
        ("binance", creds(&[("api_key", K64), ("api_secret", K64)])),
        ("binance_futures", creds(&[("api_key", K64), ("api_secret", K64), ("env", "live")])),
        ("bitget", creds(&[("api_key", K), ("api_secret", K), ("api_passphrase", "nope")])),
        ("okx", creds(&[("api_key", K), ("api_secret", K), ("api_passphrase", "nope")])),
        ("kraken", creds(&[("api_key", K), ("api_secret", "aGVsbG8gd29ybGQgdGhpcyBpcyBub3QgYSBrZXkgYXQgYWxs")])), // gitleaks:allow
        ("oanda", creds(&[("api_token", K), ("account_id", "001-004-1234567-001"), ("env", "practice")])),
        ("capitalcom", creds(&[("api_key", K), ("identifier", "nobody@example.invalid"), ("api_password", K), ("env", "demo")])),
        ("tradestation", creds(&[("client_id", K), ("client_secret", K), ("refresh_token", K), ("account_id", "SIM1234567M"), ("env", "sim")])),
        ("forexcom", creds(&[("username", "nobody"), ("password", K), ("app_key", K), ("trading_account_id", "1234567")])),
        ("ninjatrader", creds(&[("username", "nobody"), ("password", K), ("cid", K), ("sec", K), ("account_id", "DEMO1234"), ("env", "demo")])),
        ("ibkr_flex", creds(&[("flex_token", "123456789012345678901"), ("query_id", "1234567")])),
        // A real Ed25519 seed rather than a malformed one, so the probe is refused by
        // Coinbase and not by our own key-type check.
        ("coinbase", creds(&[("key_name", "organizations/00000000-0000-0000-0000-000000000000/apiKeys/00000000-0000-0000-0000-000000000000"), ("api_private_key", "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")])), // gitleaks:allow
    ]
}

/// The probe itself. Every broker has to come back with the venue's own refusal: not a
/// success, not a transport failure, and not a panic on a body it did not expect.
#[tokio::test]
async fn a_junk_credential_reaches_the_venue_and_is_refused_by_it() {
    if !enabled() {
        return;
    }
    let http = client().unwrap();
    let mut transport = Vec::new();
    let mut accepted = Vec::new();
    for (broker, secrets) in junk() {
        let b = broker_for(broker).unwrap();
        match b.test(&http, &secrets).await {
            Ok(msg) => {
                println!("{broker:<16} ACCEPTED: {msg}");
                accepted.push(broker);
            }
            Err(e) => {
                let msg = format!("{e:#}");
                println!("{broker:<16} refused: {}", msg.replace('\n', " "));
                // A refusal that never left the machine proves nothing about the request.
                let lower = msg.to_lowercase();
                if lower.contains("error sending request")
                    || lower.contains("dns error")
                    || lower.contains("connection refused")
                    || lower.contains("operation timed out")
                    || lower.contains("tcp connect")
                {
                    transport.push((broker, msg.clone()));
                    continue;
                }
                // The refusal is the user's to act on, so it names the venue: by its label,
                // by its id, or by the short name the venue is known under (IBKR, StoneX).
                let label = b.capability().label.to_lowercase();
                let names: Vec<String> = label
                    .split(|c: char| !c.is_alphanumeric())
                    .chain(broker.split('_'))
                    .filter(|w| w.len() >= 3)
                    .map(|w| w.to_string())
                    .collect();
                assert!(
                    names.iter().any(|n| lower.contains(n)),
                    "{broker}: the refusal names neither the venue nor the fix: {msg}"
                );
            }
        }
    }
    assert!(accepted.is_empty(), "a made-up credential was accepted by {accepted:?}");
    assert!(transport.is_empty(), "these never reached the venue at all: {transport:?}");
}

/// A missing credential is caught before a request is made: the user is told which field to
/// fill, not handed the venue's 401 for a header that was never sent.
#[tokio::test]
async fn a_missing_credential_is_named_before_anything_is_sent() {
    if !enabled() {
        return;
    }
    let http = client().unwrap();
    for (broker, _) in junk() {
        let b = broker_for(broker).unwrap();
        let Err(e) = b.test(&http, &creds(&[])).await else {
            panic!("{broker}: tested successfully with no credentials at all");
        };
        let msg = format!("{e:#}");
        println!("{broker:<16} empty: {}", msg.replace('\n', " "));
        let named = b
            .capability()
            .required_secrets
            .iter()
            .chain(b.capability().config_fields.iter().filter(|f| f.required).map(|f| &f.name))
            .any(|s| msg.contains(s) || msg.to_lowercase().contains(&s.replace('_', " ")));
        assert!(named, "{broker}: does not say which field is missing: {msg}");
    }
}
