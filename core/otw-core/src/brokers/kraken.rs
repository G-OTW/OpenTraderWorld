//! Kraken Spot account (read-only key).
//!
//! Kraken signs a POST body rather than a query string: `API-Sign` is
//! `HMAC-SHA512(base64-decoded secret, path ++ SHA256(nonce ++ body))`, which is why the
//! nonce appears twice: once inside the body that is hashed, once as the prefix of that
//! hash. It must also strictly increase, so it is taken from a process-wide counter rather
//! than from the clock alone: two calls in the same millisecond would otherwise be rejected
//! with `EAPI:Invalid nonce`, an error that looks like a bad key.
//!
//! Unlike Binance, `TradesHistory` takes a window and covers the whole account, so a period
//! is pulled in one walk, `ofs` paging 50 fills at a time.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use super::{Broker, BrokerCapability, ExecQuery, Execution, Holding, OpenOrder, Quote};

const BASE: &str = "https://api.kraken.com";
/// Rows one `TradesHistory` page returns.
const PAGE: usize = 50;
/// Hard stop on paging, so a mis-specified window cannot walk forever.
const MAX_PAGES: usize = 400;
/// Quote currencies a holding is priced against, most dollar-like first. The market that
/// answers is the one reported: XBT is priced in USD here, not converted into anything.
const QUOTE_ASSETS: &[&str] = &["USD", "USDT", "USDC", "EUR"];

pub struct Kraken;

static CAP: BrokerCapability = BrokerCapability {
    broker: "kraken",
    label: "Kraken",
    website: "https://www.kraken.com",
    docs_url: "https://docs.kraken.com/api/docs/rest-api/get-trade-history",
    rate_limit: "A private-call counter that decays over time; a history page costs 2, so \
                 a long period is walked with a pause between pages.",
    key_note: "An API key with **Query Ledger & Trade History** (and Query Open Orders) \
               only. No order permission is needed: this connector never writes.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: &[],
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
impl Broker for Kraken {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let res = private(client, secrets, "Balance", &[]).await?;
        let held = res
            .as_object()
            .map(|m| m.values().filter(|v| as_f64(v) > 0.0).count())
            .unwrap_or(0);
        Ok(format!("key accepted, {held} asset(s) with a balance"))
    }

    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        let pairs = asset_pairs(client).await?;
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        let mut out = Vec::new();
        let mut offset = 0usize;
        for _ in 0..MAX_PAGES {
            let params = [
                ("type".to_string(), "all".to_string()),
                ("trades".to_string(), "false".to_string()),
                ("start".to_string(), q.from.unix_timestamp().to_string()),
                ("end".to_string(), q.to.unix_timestamp().to_string()),
                ("ofs".to_string(), offset.to_string()),
            ];
            let res = private(client, secrets, "TradesHistory", &params).await?;
            let rows = res
                .get("trades")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if rows.is_empty() {
                break;
            }
            let page = rows.len();
            for (txid, row) in rows {
                let raw_pair = row.get("pair").and_then(Value::as_str).unwrap_or_default();
                let (symbol, currency) = pairs
                    .get(raw_pair)
                    .cloned()
                    .unwrap_or_else(|| (raw_pair.to_string(), String::new()));
                if !wanted.is_empty()
                    && !wanted.contains(&symbol.to_uppercase())
                    && !wanted.contains(&raw_pair.to_uppercase())
                {
                    continue;
                }
                let secs = as_f64(row.get("time").unwrap_or(&Value::Null));
                out.push(Execution {
                    id: txid,
                    order_id: row
                        .get("ordertxid")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    at: OffsetDateTime::from_unix_timestamp(secs as i64)
                        .map_err(|_| anyhow!("Kraken reported an impossible timestamp: {secs}"))?,
                    side: row
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_ascii_lowercase(),
                    qty: as_f64(row.get("vol").unwrap_or(&Value::Null)),
                    price: as_f64(row.get("price").unwrap_or(&Value::Null)),
                    fee: as_f64(row.get("fee").unwrap_or(&Value::Null)),
                    // Kraken bills the fee in the pair's quote currency.
                    fee_currency: currency.clone(),
                    currency,
                    asset_class: "crypto".into(),
                    multiplier: 1.0,
                    venue: "Kraken".into(),
                    account: String::new(),
                    symbol,
                });
            }
            offset += page;
            if page < PAGE {
                break;
            }
            // The private-call counter decays over time; pacing the walk keeps a long
            // period under the ceiling instead of tripping it and losing the rest.
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        let pairs = asset_pairs(client).await?;
        let balances = private(client, secrets, "Balance", &[]).await?;
        let held: Vec<String> = balances
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(_, v)| as_f64(v) > 0.0)
                    .map(|(k, _)| k.trim_start_matches(['X', 'Z']).to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut out: Vec<String> = pairs
            .values()
            .filter(|(wsname, _)| {
                wsname
                    .split('/')
                    .next()
                    .is_some_and(|base| held.iter().any(|h| h == base))
            })
            .map(|(wsname, _)| wsname.clone())
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Balances, named the way the rest of the world names them. Kraken's own asset codes
    /// are its internal spelling (`XXBT`, `ZUSD`), and the mapping to `XBT` and `USD` is a
    /// question for its `Assets` endpoint, never a prefix to cut off: trimming an X off
    /// `XLM` would turn Stellar into `LM`.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        let names = asset_names(client).await.unwrap_or_default();
        let body = private(client, secrets, "Balance", &[]).await?;
        let mut out = Vec::new();
        for (code, v) in body.as_object().cloned().unwrap_or_default() {
            let qty = as_f64(&v);
            if qty <= 0.0 {
                continue;
            }
            out.push(Holding {
                symbol: names.get(&code).cloned().unwrap_or_else(|| code.clone()),
                name: String::new(),
                qty,
                side: "long".into(),
                asset_class: "crypto".into(),
                avg_price: None,
                currency: String::new(),
                venue: "Kraken".into(),
                account: String::new(),
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    /// What these assets last traded at on Kraken. The pair is found in the exchange's own
    /// pair list rather than spelled out here, because `XBT/USD` is a key Kraken owns; an
    /// asset with no market against one of the quote currencies is left out.
    async fn quotes(
        &self,
        client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        symbols: &[String],
    ) -> Result<Vec<Quote>> {
        let pairs = asset_pairs(client).await?;
        // symbol, pair key, wsname, quote currency.
        let mut wanted: Vec<(String, String, String, String)> = Vec::new();
        for s in symbols {
            let asset = s.trim().to_ascii_uppercase();
            if asset.is_empty() {
                continue;
            }
            for q in QUOTE_ASSETS {
                if *q == asset {
                    continue;
                }
                let ws = format!("{asset}/{q}");
                if let Some((key, (name, _))) =
                    pairs.iter().find(|(_, (n, _))| n.eq_ignore_ascii_case(&ws))
                {
                    wanted.push((s.clone(), key.clone(), name.clone(), (*q).to_string()));
                    break;
                }
            }
        }
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let list: Vec<&str> = wanted.iter().map(|(_, k, _, _)| k.as_str()).collect();
        let body: Value = crate::rate::send(
            "kraken",
            client.get(format!("{BASE}/0/public/Ticker?pair={}", list.join(","))),
        )
        .await
        .context("asking Kraken for its prices")?
        .json()
        .await
        .context("reading Kraken's prices")?;
        if let Some(err) = body
            .get("error")
            .and_then(Value::as_array)
            .filter(|e| !e.is_empty())
        {
            return Err(anyhow!(
                "Kraken refused the price call: {}",
                err.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
            ));
        }
        let result = body.get("result").cloned().unwrap_or(Value::Null);
        let mut out = Vec::new();
        for (symbol, key, name, currency) in wanted {
            // Kraken answers under its own key for the pair, which is not always the one
            // that was asked for: the altname is the same market under another spelling.
            let row = result
                .get(&key)
                .or_else(|| result.get(name.replace('/', "")))
                .cloned()
                .unwrap_or(Value::Null);
            let price = row
                .get("c")
                .and_then(Value::as_array)
                .and_then(|c| c.first())
                .map(as_f64)
                .unwrap_or(0.0);
            if price > 0.0 {
                out.push(Quote {
                    symbol,
                    price,
                    currency,
                    pair: name,
                });
            }
        }
        Ok(out)
    }

    async fn orders(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<OpenOrder>> {
        let pairs = asset_pairs(client).await?;
        let res = private(client, secrets, "OpenOrders", &[]).await?;
        let mut out = Vec::new();
        for (id, o) in res.get("open").and_then(Value::as_object).cloned().unwrap_or_default() {
            let descr = o.get("descr").cloned().unwrap_or(Value::Null);
            let raw_pair = descr.get("pair").and_then(Value::as_str).unwrap_or_default();
            let (symbol, currency) = pairs
                .get(raw_pair)
                .cloned()
                .unwrap_or_else(|| (raw_pair.to_string(), String::new()));
            out.push(OpenOrder {
                id,
                symbol,
                side: descr
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
                order_type: descr
                    .get("ordertype")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                qty: as_f64(o.get("vol").unwrap_or(&Value::Null)),
                filled_qty: as_f64(o.get("vol_exec").unwrap_or(&Value::Null)),
                limit_price: Some(as_f64(descr.get("price").unwrap_or(&Value::Null)))
                    .filter(|p| *p > 0.0),
                stop_price: Some(as_f64(descr.get("price2").unwrap_or(&Value::Null)))
                    .filter(|p| *p > 0.0),
                currency,
                asset_class: "crypto".into(),
                placed_at: OffsetDateTime::from_unix_timestamp(
                    as_f64(o.get("opentm").unwrap_or(&Value::Null)) as i64,
                )
                .ok(),
                venue: "Kraken".into(),
                account: String::new(),
            });
        }
        Ok(out)
    }
}

/// Pair name → (display symbol, quote currency), asked of the exchange. `XXBTZUSD` is a
/// key, not a ticker; `wsname` is Kraken's own readable spelling of the same pair, so the
/// journal stores `XBT/USD` without anyone guessing where the two assets split.
/// Kraken's asset code to the name it is traded under (`XXBT` to `XBT`, `ZEUR` to `EUR`).
/// Asked of the exchange rather than derived, for the reason given at the call site.
async fn asset_names(client: &reqwest::Client) -> Result<HashMap<String, String>> {
    let body: Value = crate::rate::send("kraken", client.get(format!("{BASE}/0/public/Assets")))
        .await
        .context("asking Kraken for its asset list")?
        .json()
        .await
        .context("reading Kraken's asset list")?;
    Ok(body
        .get("result")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(code, a)| {
            a.get("altname")
                .and_then(Value::as_str)
                .map(|alt| (code, alt.to_string()))
        })
        .collect())
}

async fn asset_pairs(client: &reqwest::Client) -> Result<HashMap<String, (String, String)>> {
    let body: Value = crate::rate::send("kraken", client.get(format!("{BASE}/0/public/AssetPairs")))
        .await
        .context("asking Kraken for its pair list")?
        .json()
        .await
        .context("reading Kraken's pair list")?;
    let mut out = HashMap::new();
    for (name, p) in body.get("result").and_then(Value::as_object).cloned().unwrap_or_default() {
        let wsname = p
            .get("wsname")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| p.get("altname").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| name.clone());
        let quote = wsname.split('/').nth(1).unwrap_or_default().to_string();
        out.insert(name, (wsname, quote));
    }
    Ok(out)
}

/// A signed private call. Kraken answers 200 with an `error` array, so a failure is read
/// from the body, not from the status.
async fn private(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    method: &str,
    params: &[(String, String)],
) -> Result<Value> {
    use base64::Engine as _;
    let key = super::require(secrets, "api_key")?;
    let secret = super::require(secrets, "api_secret")?;
    let raw_secret = base64::engine::general_purpose::STANDARD
        .decode(secret.trim())
        .map_err(|_| anyhow!("the Kraken private key is not the base64 string Kraken issued"))?;

    let path = format!("/0/private/{method}");
    let nonce = nonce();
    let mut body = format!("nonce={nonce}");
    for (k, v) in params {
        body.push_str(&format!("&{}={}", super::binance::enc(k), super::binance::enc(v)));
    }
    let sign = kraken_sign(&raw_secret, &path, &nonce.to_string(), &body);

    let res = crate::rate::send(
        "kraken",
        client
            .post(format!("{BASE}{path}"))
            .header("API-Key", key)
            .header("API-Sign", sign)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body),
    )
    .await
    .with_context(|| format!("calling Kraken {method}"))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "Kraken {method} answered {status}: {}",
            super::binance::trim_err(&text)
        ));
    }
    let body: Value =
        serde_json::from_str(&text).with_context(|| format!("reading Kraken's {method} answer"))?;
    let errors: Vec<&str> = body
        .get("error")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !errors.is_empty() {
        return Err(anyhow!("Kraken {method}: {}", errors.join("; ")));
    }
    Ok(body.get("result").cloned().unwrap_or(Value::Null))
}

/// `HMAC-SHA512(secret, path ++ SHA256(nonce ++ body))`, base64.
fn kraken_sign(secret: &[u8], path: &str, nonce: &str, body: &str) -> String {
    use base64::Engine as _;
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256, Sha512};

    let mut sha = Sha256::new();
    sha.update(nonce.as_bytes());
    sha.update(body.as_bytes());
    let digest = sha.finalize();

    let mut mac = <Hmac<Sha512>>::new_from_slice(secret).expect("HMAC takes a key of any size");
    mac.update(path.as_bytes());
    mac.update(&digest);
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Strictly increasing, whatever the clock does: Kraken refuses a nonce it has already seen.
fn nonce() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as u64;
    // `fetch_update` hands back the *previous* value, so the nonce actually used is
    // recomputed from it: returning what it gives would repeat the last one inside a
    // millisecond, which is exactly what Kraken refuses.
    let prev = LAST
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |p| Some(now.max(p + 1)))
        .unwrap_or(0);
    now.max(prev + 1)
}

/// Kraken quotes every number as a string.
fn as_f64(v: &Value) -> f64 {
    match v {
        Value::String(s) => s.parse().unwrap_or(0.0),
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_the_documented_example() {
        // Kraken's own worked example (docs: "Authentication → Example").
        use base64::Engine as _;
        let secret = base64::engine::general_purpose::STANDARD
            .decode("kQH5HW/8p1uGOVjbgWA7FunAmGO8lsSUXNsu3eow76sz84Q18fWxnyRzBHCd3pd5nE9qa99HAZtuZuj6F1huXg==")
            .unwrap();
        let sig = kraken_sign(
            &secret,
            "/0/private/AddOrder",
            "1616492376594",
            "nonce=1616492376594&ordertype=limit&pair=XBTUSD&price=37500&type=buy&volume=1.25",
        );
        assert_eq!(
            sig,
            "4/dpxb3iT4tp/ZCVEwSnEsLxx0bqyhLpdfOpc6fn7OR8+UClSV5n9E6aSS8MPtnRfp32bAb0nmbRn6H8ndwLUQ=="
        );
    }

    #[test]
    fn a_nonce_never_repeats_even_inside_one_millisecond() {
        // The clock alone is not enough: a burst of calls lands in the same millisecond and
        // Kraken rejects the second one as an invalid nonce.
        let mut last = 0;
        for _ in 0..1000 {
            let n = nonce();
            assert!(n > last, "{n} must be greater than {last}");
            last = n;
        }
    }
}
