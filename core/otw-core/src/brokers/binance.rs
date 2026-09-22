//! Binance Spot account (read-only key).
//!
//! Signed REST: every query string is HMAC-SHA256'd with the secret key and carries the
//! exchange's own clock, which is why `/api/v3/time` is asked first: a host drifting by a
//! few seconds is rejected with "Timestamp for this request is outside of the recvWindow",
//! an error nobody guesses from its wording.
//!
//! Two shapes of the API decide how this connector works:
//!   • `myTrades` is **per symbol**, which is why the capability declares `needs_symbols`:
//!     the account cannot be asked "what did I trade in May". Its 24 h ceiling applies to
//!     `startTime` + `endTime` together, so a period is opened on `startTime` alone and
//!     paged forward by `fromId`: a year costs a handful of calls, not one per day.
//!   • the quote currency of a pair is not on the fill (`BTCUSDT` is a string, not a
//!     price in dollars), so `exchangeInfo` is asked for the base/quote split rather than
//!     the ticker being cut on a guessed suffix.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use super::{Broker, BrokerCapability, ExecQuery, Execution, Holding, OpenOrder, Quote};

const BASE: &str = "https://api.binance.com";
/// Rows one `myTrades` page returns at most.
const PAGE: usize = 1000;
/// Quote assets a holding is priced against, most dollar-like first. An exchange prices
/// BTC in USDT, not in dollars: the market that answers is the one that is reported.
const QUOTE_ASSETS: &[&str] = &["USDT", "USDC", "FDUSD", "EUR", "BTC"];

pub struct Binance;

static CAP: BrokerCapability = BrokerCapability {
    broker: "binance",
    label: "Binance",
    website: "https://www.binance.com",
    docs_url: "https://developers.binance.com/docs/binance-spot-api-docs",
    rate_limit: "6000 request weight per minute; a trade page costs 20.",
    key_note: "An API key with **Enable Reading** only. Do not enable trading or \
               withdrawals: this connector never writes.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: &[],
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: true,
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
impl Broker for Binance {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let acct = signed_get(client, secrets, "/api/v3/account", &[]).await?;
        let kind = acct.get("accountType").and_then(Value::as_str).unwrap_or("SPOT");
        let perms: Vec<&str> = acct
            .get("permissions")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let held = balances(&acct).len();
        let trade = acct.get("canTrade").and_then(Value::as_bool).unwrap_or(false);
        Ok(format!(
            "{kind} account, {held} asset(s) held, permissions [{}]{}",
            perms.join(", "),
            if trade { ". The key can trade; a read-only key is enough here" } else { "" }
        ))
    }

    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.symbols.is_empty() {
            return Err(anyhow!(
                "Binance answers its trade history one instrument at a time. Name the \
                 symbols to pull (BTCUSDT, ETHEUR…)"
            ));
        }
        let quotes = quote_assets(client, &q.symbols).await?;
        let mut out = Vec::new();
        for symbol in &q.symbols {
            let currency = quotes
                .get(symbol.to_ascii_uppercase().as_str())
                .cloned()
                .ok_or_else(|| {
                    anyhow!("Binance does not list a pair named {symbol}, check its spelling")
                })?;
            let start = q.from.unix_timestamp() * 1000;
            let end = q.to.unix_timestamp() * 1000;
            let mut from_id: Option<i64> = None;
            loop {
                let mut params: Vec<(String, String)> = vec![
                    ("symbol".into(), symbol.to_ascii_uppercase()),
                    ("limit".into(), PAGE.to_string()),
                ];
                // The first page is opened on `startTime` alone, the next ones continue by
                // id from the last fill already read. Neither carries an `endTime`: the
                // 24 h ceiling only applies to the two together, and the period is closed
                // here instead, on the first fill past it.
                match from_id {
                    Some(id) => params.push(("fromId".into(), id.to_string())),
                    None => params.push(("startTime".into(), start.to_string())),
                }
                let page = signed_get(client, secrets, "/api/v3/myTrades", &params).await?;
                let rows = page.as_array().cloned().unwrap_or_default();
                let mut last_id = None;
                let mut past_end = false;
                for row in &rows {
                    let at_ms = row.get("time").and_then(Value::as_i64).unwrap_or(0);
                    // Fills come back in id order, which is time order: the first one past
                    // the period ends the walk rather than skipping a row.
                    if at_ms > end {
                        past_end = true;
                        break;
                    }
                    last_id = row.get("id").and_then(Value::as_i64);
                    if at_ms < start {
                        continue;
                    }
                    out.push(fill(row, symbol, &currency, at_ms)?);
                }
                if past_end || rows.len() < PAGE {
                    break;
                }
                from_id = match last_id {
                    Some(id) => Some(id + 1),
                    None => break,
                };
            }
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    /// Pairs worth offering: every listed pair whose base asset this account holds, plus
    /// whatever has a working order. A suggestion, not a filter: an asset sold down to
    /// zero leaves no balance behind, so the user can always type the pair themselves.
    async fn symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        let acct = signed_get(client, secrets, "/api/v3/account", &[]).await?;
        let held: HashSet<String> = balances(&acct).into_iter().collect();
        let mut out: HashSet<String> = HashSet::new();
        if !held.is_empty() {
            let info: Value = crate::rate::send(
                "binance",
                client.get(format!("{BASE}/api/v3/exchangeInfo?permissions=SPOT")),
            )
            .await
            .context("asking Binance for its instrument list")?
            .json()
            .await
            .context("reading Binance's instrument list")?;
            for s in info.get("symbols").and_then(Value::as_array).into_iter().flatten() {
                let (Some(sym), Some(base)) = (
                    s.get("symbol").and_then(Value::as_str),
                    s.get("baseAsset").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if held.contains(base) {
                    out.insert(sym.to_string());
                }
            }
        }
        let orders = signed_get(client, secrets, "/api/v3/openOrders", &[]).await?;
        for o in orders.as_array().into_iter().flatten() {
            if let Some(s) = o.get("symbol").and_then(Value::as_str) {
                out.insert(s.to_string());
            }
        }
        let mut list: Vec<String> = out.into_iter().collect();
        list.sort();
        Ok(list)
    }

    /// Spot balances: free plus what an open order is holding, so the line is what the
    /// account owns rather than what it could spend right now.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        let acct = signed_get(client, secrets, "/api/v3/account", &[]).await?;
        let mut out = Vec::new();
        for b in acct
            .get("balances")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let qty = num(b, "free") + num(b, "locked");
            if qty <= 0.0 {
                continue;
            }
            out.push(Holding {
                symbol: b
                    .get("asset")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: String::new(),
                qty,
                side: "long".into(),
                asset_class: "crypto".into(),
                // A balance carries no cost basis: Binance reports what is there, not what
                // it was paid for.
                avg_price: None,
                currency: String::new(),
                venue: "Binance".into(),
                account: String::new(),
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    /// What the exchange trades these assets at right now.
    ///
    /// The whole ticker board is read in one call rather than a pair at a time: naming a
    /// pair Binance does not list fails the call outright, and which pair prices an asset
    /// is exactly what is being looked for. An asset with no market is left out.
    async fn quotes(
        &self,
        client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        symbols: &[String],
    ) -> Result<Vec<Quote>> {
        let board = ticker_board(client).await?;
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
                let direct = format!("{asset}{q}");
                if let Some(p) = board.get(&direct).copied().filter(|p| *p > 0.0) {
                    out.push(Quote {
                        symbol: s.clone(),
                        price: p,
                        currency: (*q).to_string(),
                        pair: direct,
                    });
                    break;
                }
                // The other way round: USDT has no USDT market, but USDCUSDT prices it.
                let reverse = format!("{q}{asset}");
                if let Some(p) = board.get(&reverse).copied().filter(|p| *p > 0.0) {
                    out.push(Quote {
                        symbol: s.clone(),
                        price: 1.0 / p,
                        currency: (*q).to_string(),
                        pair: reverse,
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
        let rows = signed_get(client, secrets, "/api/v3/openOrders", &[]).await?;
        let symbols: Vec<String> = rows
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| o.get("symbol").and_then(Value::as_str).map(str::to_string))
            .collect();
        let quotes = quote_assets(client, &symbols).await.unwrap_or_default();
        let mut out = Vec::new();
        for o in rows.as_array().into_iter().flatten() {
            let symbol = o.get("symbol").and_then(Value::as_str).unwrap_or_default().to_string();
            let qty = num(o, "origQty");
            out.push(OpenOrder {
                currency: quotes.get(symbol.as_str()).cloned().unwrap_or_default(),
                id: o
                    .get("orderId")
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .unwrap_or_default(),
                side: o
                    .get("side")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_ascii_lowercase(),
                order_type: o.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
                qty,
                filled_qty: num(o, "executedQty"),
                limit_price: Some(num(o, "price")).filter(|p| *p > 0.0),
                stop_price: Some(num(o, "stopPrice")).filter(|p| *p > 0.0),
                asset_class: "crypto".into(),
                placed_at: o
                    .get("time")
                    .and_then(Value::as_i64)
                    .and_then(|ms| OffsetDateTime::from_unix_timestamp(ms / 1000).ok()),
                venue: "Binance".into(),
                account: String::new(),
                symbol,
            });
        }
        Ok(out)
    }
}

/// Assets with a non-zero balance (free or locked).
fn balances(acct: &Value) -> Vec<String> {
    acct.get("balances")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|b| num(b, "free") + num(b, "locked") > 0.0)
                .filter_map(|b| b.get("asset").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn fill(row: &Value, symbol: &str, currency: &str, at_ms: i64) -> Result<Execution> {
    Ok(Execution {
        id: row
            .get("id")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default(),
        order_id: row
            .get("orderId")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default(),
        // Kept to the millisecond: two fills of one order land in the same second, and
        // truncating them leaves their order to the id tiebreak instead of the clock.
        at: OffsetDateTime::from_unix_timestamp_nanos(at_ms as i128 * 1_000_000)
            .map_err(|_| anyhow!("Binance reported an impossible timestamp: {at_ms}"))?,
        symbol: symbol.to_ascii_uppercase(),
        side: if row.get("isBuyer").and_then(Value::as_bool).unwrap_or(false) {
            "buy".into()
        } else {
            "sell".into()
        },
        qty: num(row, "qty"),
        price: num(row, "price"),
        fee: num(row, "commission"),
        fee_currency: row
            .get("commissionAsset")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        currency: currency.to_string(),
        asset_class: "crypto".into(),
        multiplier: 1.0,
        venue: "Binance".into(),
        account: String::new(),
    })
}

/// Quote asset per pair, asked of the exchange rather than cut off the ticker: `BTCUSDT`
/// ends in USDT and `ETHBTC` does not, and no suffix table survives a new quote asset.
async fn quote_assets(
    client: &reqwest::Client,
    symbols: &[String],
) -> Result<HashMap<String, String>> {
    let wanted: Vec<String> = symbols.iter().map(|s| s.trim().to_ascii_uppercase()).collect();
    if wanted.is_empty() {
        return Ok(HashMap::new());
    }
    let list = serde_json::to_string(&wanted)?;
    let url = format!("{BASE}/api/v3/exchangeInfo?symbols={}", enc(&list));
    let res = crate::rate::send("binance", client.get(&url))
        .await
        .context("asking Binance about these pairs")?;
    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("Binance rejected the pair list: {}", trim_err(&body)));
    }
    let info: Value = res.json().await.context("reading Binance's pair list")?;
    let mut out = HashMap::new();
    for s in info.get("symbols").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(sym), Some(quote)) = (
            s.get("symbol").and_then(Value::as_str),
            s.get("quoteAsset").and_then(Value::as_str),
        ) {
            out.insert(sym.to_string(), quote.to_string());
        }
    }
    Ok(out)
}

/// Every listed pair and its last price, in one public call.
async fn ticker_board(client: &reqwest::Client) -> Result<HashMap<String, f64>> {
    let body: Value = crate::rate::send(
        "binance",
        client.get(format!("{BASE}/api/v3/ticker/price")),
    )
    .await
    .context("asking Binance for its prices")?
    .json()
    .await
    .context("reading Binance's prices")?;
    let mut out = HashMap::new();
    for row in body.as_array().into_iter().flatten() {
        if let Some(sym) = row.get("symbol").and_then(Value::as_str) {
            out.insert(sym.to_string(), num(row, "price"));
        }
    }
    Ok(out)
}

/// A signed GET: the exchange's clock, the query string, its HMAC, and the key header.
async fn signed_get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let key = super::require(secrets, "api_key")?;
    let secret = super::require(secrets, "api_secret")?;
    let mut qs: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect();
    qs.push(format!("timestamp={}", server_time(client).await?));
    qs.push("recvWindow=20000".into());
    let query = qs.join("&");
    let sig = hmac_sha256_hex(secret.as_bytes(), query.as_bytes());
    let url = format!("{BASE}{path}?{query}&signature={sig}");
    let res = crate::rate::send("binance", client.get(&url).header("X-MBX-APIKEY", key))
        .await
        .with_context(|| format!("calling Binance {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("Binance {path} answered {status}: {}", trim_err(&body)));
    }
    serde_json::from_str(&body).with_context(|| format!("reading Binance's {path} answer"))
}

/// The exchange's own clock in milliseconds. A signed request is rejected outright when
/// the host's clock drifts, so the timestamp is never taken from this machine.
async fn server_time(client: &reqwest::Client) -> Result<i64> {
    let body: Value = crate::rate::send("binance", client.get(format!("{BASE}/api/v3/time")))
        .await
        .context("asking Binance for its clock")?
        .json()
        .await
        .context("reading Binance's clock")?;
    body.get("serverTime")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("Binance did not report its server time"))
}

fn num(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Raw HMAC-SHA256. Binance wants it in hex, the OKX family in base64, so the digest is
/// produced once and each caller encodes it the way its own signature is specified.
pub(super) fn hmac_sha256(key: &[u8], payload: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<sha2::Sha256>>::new_from_slice(key).expect("HMAC takes a key of any size");
    mac.update(payload);
    mac.finalize().into_bytes().to_vec()
}

pub(super) fn hmac_sha256_hex(key: &[u8], payload: &[u8]) -> String {
    hmac_sha256(key, payload).iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Percent-encode a query value. The signature is computed over the string that is sent,
/// so encoding and signing must not disagree.
pub(super) fn enc(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Provider error bodies are one useful line wrapped in a page of JSON.
pub(super) fn trim_err(body: &str) -> String {
    let t = body.trim();
    if t.len() > 300 {
        format!("{}…", &t[..300])
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_the_string_it_sends() {
        // Binance's own worked example (docs: "SIGNED Endpoint Examples").
        let sig = hmac_sha256_hex(
            b"NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j",
            b"symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559",
        );
        assert_eq!(sig, "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71");
    }

    #[test]
    fn a_fill_keeps_its_milliseconds() {
        let row = serde_json::json!({
            "id": 42, "orderId": 7, "qty": "0.5", "price": "100.0",
            "commission": "0.0007", "commissionAsset": "BNB", "isBuyer": true
        });
        let e = fill(&row, "BTCUSDT", "USDT", 1_700_000_000_123).unwrap();
        assert_eq!(e.at.unix_timestamp_nanos(), 1_700_000_000_123_000_000);
        assert_eq!(e.side, "buy");
        assert_eq!(e.fee_currency, "BNB");
    }

    #[test]
    fn encodes_what_a_signature_covers() {
        assert_eq!(enc("[\"BTCUSDT\"]"), "%5B%22BTCUSDT%22%5D");
        assert_eq!(enc("BTCUSDT"), "BTCUSDT");
    }
}
