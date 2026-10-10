//! Coinbase Advanced Trade account (read-only CDP key).
//!
//! Neither of the other two auth schemes: Coinbase signs a **JWT per request**, valid two
//! minutes, whose `uri` claim names the very call it authorises
//! (`GET api.coinbase.com/api/v3/brokerage/orders/historical/fills`, query string excluded).
//! A token is therefore built for each call rather than reused, and a replayed one is worth
//! nothing anywhere else.
//!
//! **Ed25519 keys only.** Coinbase recommends them, its own SDK issues them as 64 base64
//! bytes (32-byte seed followed by the public key), and that is exactly the shape `ring`
//! takes. The older ECDSA keys arrive as a SEC1 PEM that would have to be re-encoded to
//! PKCS#8 before any Rust signer would read it, so they are refused with a message naming
//! the fix instead of being half-supported.
//!
//! Two details the API's own shape forces:
//!   • a fill's `size` is in the **quote** currency when `size_in_quote` is set, so the
//!     quantity is divided by the price rather than taken at face value;
//!   • the quote currency is not on the fill, so `products` is asked for the base/quote
//!     split instead of `BTC-USD` being cut on its dash.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Quote,
};

const HOST: &str = "api.coinbase.com";
const PREFIX: &str = "/api/v3/brokerage";
/// A JWT is accepted for two minutes; ours is built per call and never cached.
const JWT_TTL_SECS: i64 = 120;
/// Fills per page, and a hard stop so a wide window cannot walk forever.
const PAGE: usize = 100;
const MAX_PAGES: usize = 500;
/// Quote currencies a holding is priced against, most dollar-like first. The product that
/// answers is the one reported: nothing here converts one money into another.
const QUOTE_ASSETS: &[&str] = &["USD", "USDC", "USDT", "EUR"];

pub struct Coinbase;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "key_name",
    label: "API key name",
    placeholder: "organizations/{org_id}/apiKeys/{key_id}",
    kind: "text",
    required: true,
    help: "The full key name Coinbase shows next to the key, organizations/… included. It \
           is an identifier, not the secret, so it is stored in the clear and can be \
           checked against the dashboard.",
}];

static CAP: BrokerCapability = BrokerCapability {
    broker: "coinbase",
    label: "Coinbase Advanced Trade",
    website: "https://www.coinbase.com",
    docs_url: "https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/list-fills",
    rate_limit: "30 requests per second on the private REST endpoints; a fill page is one \
                 request and the walk pages by cursor.",
    key_note: "A CDP API key with **View** permission only, created as **Ed25519** (the type \
               Coinbase recommends). Do not grant Trade or Transfer: this connector never \
               writes.",
    required_secrets: &["api_private_key"],
    config_fields: FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    spot_only: true,
    positions: false,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

#[async_trait::async_trait]
impl Broker for Coinbase {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let body = get(client, secrets, "/accounts", &[("limit".into(), "250".into())]).await?;
        let accounts = body.get("accounts").and_then(Value::as_array).cloned().unwrap_or_default();
        let funded = accounts.iter().filter(|a| balance(a) > 0.0).count();
        Ok(format!(
            "key accepted, {} account(s), {funded} with a balance",
            accounts.len()
        ))
    }

    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        let quotes = products(client, secrets).await?;
        let mut out = Vec::new();
        let mut cursor = String::new();
        for _ in 0..MAX_PAGES {
            let mut params: Vec<(String, String)> = vec![
                ("limit".into(), PAGE.to_string()),
                ("start_sequence_timestamp".into(), q.from.format(&Rfc3339)?),
                ("end_sequence_timestamp".into(), q.to.format(&Rfc3339)?),
            ];
            // The endpoint takes the filter itself, so a narrowed pull costs fewer pages
            // than filtering the whole account here.
            for s in &q.symbols {
                params.push(("product_ids".into(), s.trim().to_uppercase()));
            }
            if !cursor.is_empty() {
                params.push(("cursor".into(), cursor.clone()));
            }
            let body = get(client, secrets, "/orders/historical/fills", &params).await?;
            let fills = body.get("fills").and_then(Value::as_array).cloned().unwrap_or_default();
            if fills.is_empty() {
                break;
            }
            let page = fills.len();
            for f in &fills {
                out.push(fill(f, &quotes)?);
            }
            cursor = body
                .get("cursor")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if cursor.is_empty() || page < PAGE {
                break;
            }
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    /// Pairs worth offering: every listed product whose base currency this account holds.
    /// A suggestion, not a filter, since an asset sold down to zero leaves no balance.
    async fn symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        let body = get(client, secrets, "/accounts", &[("limit".into(), "250".into())]).await?;
        let held: HashSet<String> = body
            .get("accounts")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter(|a| balance(a) > 0.0)
                    .filter_map(|a| a.get("currency").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if held.is_empty() {
            return Ok(vec![]);
        }
        let body = get(client, secrets, "/products", &[("product_type".into(), "SPOT".into())]).await?;
        let mut out: Vec<String> = body
            .get("products")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter(|p| {
                        p.get("base_currency_id")
                            .and_then(Value::as_str)
                            .is_some_and(|b| held.contains(b))
                    })
                    .filter_map(|p| p.get("product_id").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Account balances: what is available plus what an open order is holding, so the line
    /// is what the account owns rather than what it could spend right now. A fiat wallet is
    /// reported too, spelled as the currency it is: whether it belongs in a portfolio is the
    /// user's call in the preview, not this connector's.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        let body = get(
            client,
            secrets,
            "/accounts",
            &[("limit".into(), "250".into())],
        )
        .await?;
        let mut out = Vec::new();
        for a in body
            .get("accounts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let qty = balance(a) + a.get("hold").map(|h| num(h, "value")).unwrap_or(0.0);
            if qty <= 0.0 {
                continue;
            }
            let fiat = a
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("FIAT"));
            out.push(Holding {
                symbol: a
                    .get("currency")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: a
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                qty,
                side: "long".into(),
                asset_class: if fiat {
                    "forex".into()
                } else {
                    "crypto".into()
                },
                avg_price: None,
                currency: String::new(),
                venue: "Coinbase".into(),
                account: a
                    .get("retail_portfolio_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    /// What these assets trade at right now. The whole spot product list carries its own
    /// last price, so one call prices every line; an asset with no product is left out.
    async fn quotes(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        symbols: &[String],
    ) -> Result<Vec<Quote>> {
        let body = get(
            client,
            secrets,
            "/products",
            &[("product_type".into(), "SPOT".into())],
        )
        .await?;
        let mut board: HashMap<String, f64> = HashMap::new();
        for p in body.get("products").and_then(Value::as_array).into_iter().flatten() {
            if let Some(id) = p.get("product_id").and_then(Value::as_str) {
                board.insert(id.to_string(), num(p, "price"));
            }
        }
        let mut out = Vec::new();
        for s in symbols {
            let asset = s.trim().to_ascii_uppercase();
            if asset.is_empty() {
                continue;
            }
            for q in QUOTE_ASSETS {
                if *q == asset {
                    continue;
                }
                let id = format!("{asset}-{q}");
                if let Some(price) = board.get(&id).copied().filter(|p| *p > 0.0) {
                    out.push(Quote {
                        symbol: s.clone(),
                        price,
                        currency: (*q).to_string(),
                        pair: id,
                    });
                    break;
                }
            }
        }
        Ok(out)
    }

    async fn orders(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<OpenOrder>> {
        let quotes = products(client, secrets).await?;
        let body = get(
            client,
            secrets,
            "/orders/historical/batch",
            &[("order_status".into(), "OPEN".into()), ("limit".into(), "100".into())],
        )
        .await?;
        let mut out = Vec::new();
        for o in body.get("orders").and_then(Value::as_array).into_iter().flatten() {
            let product = o.get("product_id").and_then(Value::as_str).unwrap_or_default();
            // The prices live inside whichever configuration the order was placed with
            // (limit_limit_gtc, stop_limit_stop_limit_gtc…), so they are read by name from
            // whatever shape is there rather than from a list of known ones.
            let cfg = o.get("order_configuration").cloned().unwrap_or(Value::Null);
            let inner = cfg
                .as_object()
                .and_then(|m| m.values().next().cloned())
                .unwrap_or(Value::Null);
            out.push(OpenOrder {
                id: o.get("order_id").and_then(Value::as_str).unwrap_or_default().to_string(),
                symbol: product.to_string(),
                side: o.get("side").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase(),
                order_type: o
                    .get("order_type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                qty: num(&inner, "base_size"),
                filled_qty: num(o, "filled_size"),
                limit_price: Some(num(&inner, "limit_price")).filter(|p| *p > 0.0),
                stop_price: Some(num(&inner, "stop_price")).filter(|p| *p > 0.0),
                currency: quotes.get(product).cloned().unwrap_or_default(),
                asset_class: "crypto".into(),
                placed_at: o
                    .get("created_time")
                    .and_then(Value::as_str)
                    .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok()),
                venue: "Coinbase".into(),
                account: o
                    .get("retail_portfolio_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
        Ok(out)
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        if let Some(name) = config.get("key_name").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if !name.starts_with("organizations/") || !name.contains("/apiKeys/") {
                return Err(anyhow!(
                    "the key name is the whole string Coinbase shows, \
                     organizations/<org>/apiKeys/<key>"
                ));
            }
        }
        Ok(())
    }
}

fn fill(f: &Value, quotes: &HashMap<String, String>) -> Result<Execution> {
    let product = f.get("product_id").and_then(Value::as_str).unwrap_or_default();
    let price = num(f, "price");
    let size = num(f, "size");
    // `size_in_quote` means the size is money, not coins.
    let qty = if f.get("size_in_quote").and_then(Value::as_bool).unwrap_or(false) {
        if price > 0.0 {
            size / price
        } else {
            return Err(anyhow!(
                "Coinbase reported a {product} fill sized in {} at price 0, which cannot be \
                 converted to a quantity",
                quotes.get(product).map(String::as_str).unwrap_or("quote currency")
            ));
        }
    } else {
        size
    };
    let at = f
        .get("trade_time")
        .and_then(Value::as_str)
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
        .ok_or_else(|| anyhow!("a Coinbase fill carries no readable trade time"))?;
    let currency = quotes.get(product).cloned().unwrap_or_default();
    Ok(Execution {
        id: first_str(f, &["entry_id", "trade_id"]),
        order_id: f.get("order_id").and_then(Value::as_str).unwrap_or_default().to_string(),
        at,
        symbol: product.to_string(),
        side: f.get("side").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase(),
        qty,
        price,
        fee: num(f, "commission").abs(),
        // Coinbase bills the commission in the product's quote currency.
        fee_currency: currency.clone(),
        currency,
        asset_class: "crypto".into(),
        multiplier: 1.0,
        venue: "Coinbase".into(),
        account: f
            .get("retail_portfolio_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// Product id to quote currency, asked of the exchange. `BTC-USD` splits on its dash and
/// `BTC-USDC` does too, but reading the exchange's own `quote_currency_id` is one call and
/// never has to be revisited when a product is named otherwise.
async fn products(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let body = get(client, secrets, "/products", &[("product_type".into(), "SPOT".into())]).await?;
    let mut out = HashMap::new();
    for p in body.get("products").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(id), Some(quote)) = (
            p.get("product_id").and_then(Value::as_str),
            p.get("quote_currency_id").and_then(Value::as_str),
        ) {
            out.insert(id.to_string(), quote.to_string());
        }
    }
    Ok(out)
}

/// A signed GET. The JWT names this exact call, so it is built here and thrown away.
async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let full_path = format!("{PREFIX}{path}");
    let token = jwt(secrets, "GET", &full_path)?;
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", super::binance::enc(k), super::binance::enc(v)))
        .collect();
    let url = if query.is_empty() {
        format!("https://{HOST}{full_path}")
    } else {
        format!("https://{HOST}{full_path}?{}", query.join("&"))
    };
    let res = crate::rate::send("coinbase", client.get(&url).bearer_auth(token))
        .await
        .with_context(|| format!("calling Coinbase {path}"))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "Coinbase {path} answered {status}: {}",
            super::binance::trim_err(&text)
        ));
    }
    serde_json::from_str(&text).with_context(|| format!("reading Coinbase's {path} answer"))
}

/// Build the per-request JWT: `{kid, nonce}` header, `{sub, iss, nbf, exp, uri}` payload,
/// signed EdDSA. The `uri` claim carries the method, the host and the path, **without the
/// query string** (that is what Coinbase's own SDK signs).
fn jwt(secrets: &HashMap<String, String>, method: &str, path: &str) -> Result<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let key_name = super::require(secrets, "key_name")?;
    let key = signing_key(super::require(secrets, "api_private_key")?)?;

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let header = json!({
        "alg": "EdDSA",
        "typ": "JWT",
        "kid": key_name,
        "nonce": nonce(),
    });
    let payload = json!({
        "sub": key_name,
        "iss": "cdp",
        "nbf": now,
        "exp": now + JWT_TTL_SECS,
        "uri": format!("{method} {HOST}{path}"),
    });
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?)
    );
    let sig = key.sign(signing_input.as_bytes());
    Ok(format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref())))
}

/// The Ed25519 key Coinbase issues: base64 of 64 bytes, a 32-byte seed followed by its
/// public key (a bare 32-byte seed is accepted too). An ECDSA key is a PEM block, and it is
/// refused by name rather than mis-parsed into a signature nobody can explain.
fn signing_key(secret: &str) -> Result<ring::signature::Ed25519KeyPair> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let trimmed = secret.trim();
    if trimmed.starts_with("-----BEGIN") {
        return Err(anyhow!(
            "this is an ECDSA key in PEM form, which this connector does not sign with. \
             Create a new CDP API key of type Ed25519, the one Coinbase recommends."
        ));
    }
    let raw = STANDARD
        .decode(trimmed)
        .map_err(|_| anyhow!("the Coinbase private key is not the base64 string Coinbase issued"))?;
    match raw.len() {
        64 => ring::signature::Ed25519KeyPair::from_seed_and_public_key(&raw[..32], &raw[32..]),
        32 => ring::signature::Ed25519KeyPair::from_seed_unchecked(&raw),
        n => {
            return Err(anyhow!(
                "a Coinbase Ed25519 key decodes to 64 bytes (32 seed + 32 public), this one \
                 has {n}"
            ))
        }
    }
    .map_err(|e| anyhow!("the Coinbase private key was rejected: {e}"))
}

/// Replay protection: a fresh random value per token.
fn nonce() -> String {
    let mut bytes = [0u8; 16];
    // A failed draw must not silently produce a constant nonce; the clock is a poor but
    // still varying fallback, and the token is only valid for two minutes anyway.
    if getrandom::fill(&mut bytes).is_err() {
        let t = OffsetDateTime::now_utc().unix_timestamp_nanos().to_le_bytes();
        bytes[..16].copy_from_slice(&t[..16]);
    }
    bytes.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Coinbase quotes every amount as a string.
fn num(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn first_str(v: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()))
        .unwrap_or_default()
        .to_string()
}

/// Available balance of one account row.
fn balance(a: &Value) -> f64 {
    a.get("available_balance")
        .map(|b| num(b, "value"))
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8032 test vector 1: a known seed, a known message, a known signature. It proves
    /// the key is loaded and signed the way Coinbase will verify it.
    #[test]
    fn signs_the_rfc8032_vector() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let seed =
            hex(b"9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let public =
            hex(b"d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let mut raw = seed.clone();
        raw.extend_from_slice(&public);
        let key = signing_key(&STANDARD.encode(&raw)).expect("a 64-byte Ed25519 key");
        let sig = key.sign(b"");
        assert_eq!(
            sig.as_ref().to_vec(),
            hex(b"e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b")
        );
        // A bare seed is the same key.
        assert!(signing_key(&STANDARD.encode(&seed)).is_ok());
    }

    #[test]
    fn an_ecdsa_pem_is_refused_by_name() {
        let err = signing_key("-----BEGIN EC PRIVATE KEY-----\nMHc=\n-----END EC PRIVATE KEY-----")
            .unwrap_err()
            .to_string();
        assert!(err.contains("Ed25519"), "{err}");
    }

    #[test]
    fn the_jwt_names_the_call_it_authorises() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let mut secrets = HashMap::new();
        secrets.insert("key_name".into(), "organizations/o/apiKeys/k".to_string());
        secrets.insert(
            "api_private_key".into(),
            STANDARD.encode(hex(b"9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")),
        );
        let token = jwt(&secrets, "GET", "/api/v3/brokerage/accounts").unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let payload = decode(parts[1]);
        assert_eq!(payload["uri"], "GET api.coinbase.com/api/v3/brokerage/accounts");
        assert_eq!(payload["iss"], "cdp");
        assert_eq!(payload["sub"], "organizations/o/apiKeys/k");
        assert_eq!(
            payload["exp"].as_i64().unwrap() - payload["nbf"].as_i64().unwrap(),
            JWT_TTL_SECS
        );
        let header = decode(parts[0]);
        assert_eq!(header["alg"], "EdDSA");
        assert_eq!(header["kid"], "organizations/o/apiKeys/k");
        assert!(header["nonce"].as_str().is_some_and(|n| n.len() == 32));
    }

    #[test]
    fn a_quote_sized_fill_is_converted_to_a_quantity() {
        let quotes: HashMap<String, String> =
            [("BTC-USD".to_string(), "USD".to_string())].into_iter().collect();
        let f = json!({
            "entry_id": "e1", "order_id": "o1", "product_id": "BTC-USD",
            "trade_time": "2026-03-01T10:00:00Z", "side": "BUY",
            "price": "50000", "size": "100", "size_in_quote": true, "commission": "0.5"
        });
        let e = fill(&f, &quotes).unwrap();
        assert_eq!(e.qty, 0.002); // 100 USD at 50 000, not 100 coins
        assert_eq!(e.currency, "USD");
        assert_eq!(e.side, "buy");

        let f = json!({
            "entry_id": "e2", "product_id": "BTC-USD", "trade_time": "2026-03-01T10:00:00Z",
            "side": "SELL", "price": "50000", "size": "0.5", "size_in_quote": false
        });
        assert_eq!(fill(&f, &quotes).unwrap().qty, 0.5);
    }

    fn decode(part: &str) -> Value {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    /// Decode a hex literal from the RFC's test vectors.
    fn hex(s: &[u8]) -> Vec<u8> {
        s.chunks(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }
}
