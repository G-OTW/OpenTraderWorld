//! OKX v5 account (read-only HMAC key): spot, margin, swaps, futures and options.
//!
//! Signed REST: a prehash of `timestamp + METHOD + path?query`, HMAC SHA-256'd with the
//! secret and base64'd, with the timestamp in ISO-8601 to the millisecond rather than the
//! epoch integer every other exchange here uses. The passphrase chosen when the key was
//! created is a third credential, not an option.
//!
//! What the API's shape forces:
//!   • **Three months and no further.** `fills-history` is the archive, and it stops at 90
//!     days, so `history_days` says 90 rather than leaving the user to discover it.
//!   • **One call per instrument type.** The endpoint is asked per `instType`, so a full
//!     pull walks SPOT, MARGIN, SWAP, FUTURES and OPTION in turn; an account that never
//!     touched options pays one empty page for them.
//!   • **A contract is not a coin.** A derivative fill is sized in *contracts*, and what a
//!     contract is worth is `ctVal × ctMult` from the instrument list. It is read from
//!     there rather than assumed to be one, because a point value off by a hundred is a
//!     PnL off by a hundred.
//!   • **An inverse contract settles in the coin, not in the quote.** `settleCcy` says
//!     which, and it is reported as such instead of being called dollars.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

const BASE: &str = "https://www.okx.com";
/// The books a full pull walks. The endpoint answers one at a time.
const INST_TYPES: &[&str] = &["SPOT", "MARGIN", "SWAP", "FUTURES", "OPTION"];
/// Rows a fill page returns at most.
const PAGE: usize = 100;
/// Hard stop on the cursor walk, so a mis-specified window cannot loop.
const MAX_PAGES: usize = 500;

pub struct Okx;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or demo. OKX's demo trading shares the host but needs its own key and an \
           extra header, so this is stated rather than guessed. Leave empty for live.",
}];

static CAP: BrokerCapability = BrokerCapability {
    broker: "okx",
    label: "OKX",
    website: "https://www.okx.com",
    docs_url: "https://www.okx.com/docs-v5/en/",
    rate_limit: "10 requests per 2 seconds per account on the fill archive, and the same on \
                 positions and orders. A pull walks five instrument types.",
    key_note: "An API key with **Read** permission only, plus the passphrase you chose when \
               creating it. Do not grant Trade or Withdraw: this connector never writes. \
               OKX serves **90 days** of fills; anything older is a download from the web \
               site. Derivative fills are counted in contracts, and the contract size is \
               read from OKX's own instrument list.",
    required_secrets: &["api_key", "api_secret", "api_passphrase"],
    config_fields: FIELDS,
    asset_classes: &["crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 90,
    // Margin, swaps and futures: a short is a short. A spot-only account is worth unticking
    // "this account can go short" for, which the import modal offers.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

/// What OKX says an instrument is worth and settles in, from its own instrument list.
#[derive(Clone)]
struct Contract {
    /// Point value: `ctVal × ctMult`. One for anything sized in the coin itself.
    multiplier: f64,
    /// What a position in it is settled in: the quote coin on spot, `settleCcy` elsewhere.
    currency: String,
}

impl Default for Contract {
    fn default() -> Self {
        Contract { multiplier: 1.0, currency: String::new() }
    }
}

#[async_trait::async_trait]
impl Broker for Okx {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => Ok(()),
            Some(v) if v.is_empty() || v == "live" || v == "demo" => Ok(()),
            Some(other) => Err(anyhow!(
                "the OKX environment is \"live\" or \"demo\", not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let rows = get(client, secrets, "/api/v5/account/balance", &[]).await?;
        let acct = first_row(&rows);
        let details = acct.get("details").and_then(Value::as_array).cloned().unwrap_or_default();
        let funded = details.iter().filter(|d| num(d, "eq") > 0.0).count();
        Ok(format!(
            "{} key accepted, {funded} currency(ies) with a balance, total equity {} USD",
            env_label(secrets),
            str_of(&acct, "totalEq")
        ))
    }

    /// Every fill of the window across every book, oldest first.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        let mut contracts: HashMap<String, Contract> = HashMap::new();
        let mut priced: Vec<&str> = Vec::new();
        let mut out: Vec<Execution> = Vec::new();
        let mut pages_left = MAX_PAGES;

        for inst_type in INST_TYPES {
            let mut cursor: Option<String> = None;
            loop {
                if pages_left == 0 {
                    return Err(anyhow!(
                        "this period holds more OKX fills than one pull reads. Ask for a \
                         shorter period."
                    ));
                }
                pages_left -= 1;
                let mut params = vec![
                    ("instType".to_string(), (*inst_type).to_string()),
                    ("begin".to_string(), ms(q.from).to_string()),
                    ("end".to_string(), ms(q.to).to_string()),
                    ("limit".to_string(), PAGE.to_string()),
                ];
                if let Some(c) = &cursor {
                    params.push(("after".into(), c.clone()));
                }
                let rows = get(client, secrets, "/api/v5/trade/fills-history", &params).await?;
                let rows = rows.as_array().cloned().unwrap_or_default();
                if rows.is_empty() {
                    break;
                }
                // Contract sizes are asked once per instrument type, and only for the types
                // that answered anything.
                if !priced.contains(inst_type) {
                    contracts.extend(
                        instruments(client, secrets, inst_type).await.unwrap_or_default(),
                    );
                    priced.push(inst_type);
                }
                for r in &rows {
                    let symbol = str_of(r, "instId").to_uppercase();
                    if symbol.is_empty() || (!wanted.is_empty() && !wanted.contains(&symbol)) {
                        continue;
                    }
                    let c = contracts.get(&symbol).cloned().unwrap_or_default();
                    out.push(fill(r, &symbol, &c)?);
                }
                if rows.len() < PAGE {
                    break;
                }
                cursor = rows.last().map(|r| str_of(r, "billId")).filter(|s| !s.is_empty());
                if cursor.is_none() {
                    break;
                }
            }
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn positions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        let rows = get(client, secrets, "/api/v5/account/positions", &[]).await?;
        let mut contracts: HashMap<String, Contract> = HashMap::new();
        let mut seen: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for p in rows.as_array().into_iter().flatten() {
            let qty = num(p, "pos");
            if qty == 0.0 {
                continue;
            }
            let symbol = str_of(p, "instId").to_uppercase();
            let inst_type = str_of(p, "instType");
            if !seen.contains(&inst_type) {
                contracts.extend(instruments(client, secrets, &inst_type).await.unwrap_or_default());
                seen.push(inst_type);
            }
            let c = contracts.get(&symbol).cloned().unwrap_or_default();
            out.push(Position {
                // OKX reports the side on `posSide` in hedge mode and in the sign of `pos`
                // in one-way mode, so both are read rather than one being assumed.
                side: match str_of(p, "posSide").as_str() {
                    "short" => "short".into(),
                    "long" => "long".into(),
                    _ if qty < 0.0 => "short".into(),
                    _ => "long".into(),
                },
                qty: qty.abs(),
                avg_price: Some(num(p, "avgPx")).filter(|v| *v > 0.0),
                currency: if c.currency.is_empty() { str_of(p, "ccy") } else { c.currency.clone() },
                asset_class: "crypto".into(),
                multiplier: c.multiplier,
                venue: "OKX".into(),
                account: String::new(),
                symbol,
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    async fn orders(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<OpenOrder>> {
        let rows = get(
            client,
            secrets,
            "/api/v5/trade/orders-pending",
            &[("limit".into(), PAGE.to_string())],
        )
        .await?;
        let mut out = Vec::new();
        for o in rows.as_array().into_iter().flatten() {
            let symbol = str_of(o, "instId").to_uppercase();
            out.push(OpenOrder {
                id: str_of(o, "ordId"),
                side: str_of(o, "side").to_ascii_lowercase(),
                order_type: str_of(o, "ordType"),
                qty: num(o, "sz"),
                filled_qty: num(o, "accFillSz"),
                limit_price: Some(num(o, "px")).filter(|v| *v > 0.0),
                // A conditional order carries its trigger separately; a plain one has none.
                stop_price: Some(num(o, "slTriggerPx")).filter(|v| *v > 0.0),
                currency: quote_of(&symbol),
                asset_class: "crypto".into(),
                placed_at: ms_stamp(o, "cTime"),
                venue: "OKX".into(),
                account: String::new(),
                symbol,
            });
        }
        Ok(out)
    }

    /// What the account holds, currency by currency. OKX's unified account reports one
    /// balance per coin whatever book it is working in, and no cost basis at all.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        let rows = get(client, secrets, "/api/v5/account/balance", &[]).await?;
        let acct = first_row(&rows);
        let mut out = Vec::new();
        for d in acct.get("details").and_then(Value::as_array).into_iter().flatten() {
            let qty = num(d, "eq");
            if qty <= 0.0 {
                continue;
            }
            out.push(Holding {
                symbol: str_of(d, "ccy").to_uppercase(),
                name: String::new(),
                qty,
                side: "long".into(),
                asset_class: "crypto".into(),
                // A balance carries no cost basis: OKX reports what is there, not what it
                // was paid for.
                avg_price: None,
                currency: String::new(),
                venue: "OKX".into(),
                account: String::new(),
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    /// What the exchange trades these coins at right now, from the whole spot board in one
    /// public call: which pair prices a coin is exactly what is being looked for.
    async fn quotes(
        &self,
        client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        symbols: &[String],
    ) -> Result<Vec<Quote>> {
        let board = ticker_board(client).await?;
        let mut out = Vec::new();
        for s in symbols {
            let asset = s.trim().to_uppercase();
            if asset.is_empty() {
                continue;
            }
            for q in QUOTE_COINS {
                if *q == asset {
                    continue;
                }
                let direct = format!("{asset}-{q}");
                if let Some(p) = board.get(&direct).copied().filter(|p| *p > 0.0) {
                    out.push(Quote { symbol: s.clone(), price: p, currency: (*q).into(), pair: direct });
                    break;
                }
                // The other way round: USDT has no USDT market, but USDC-USDT prices it.
                let reverse = format!("{q}-{asset}");
                if let Some(p) = board.get(&reverse).copied().filter(|p| *p > 0.0) {
                    out.push(Quote {
                        symbol: s.clone(),
                        price: 1.0 / p,
                        currency: (*q).into(),
                        pair: reverse,
                    });
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Instruments worth offering: what the account is positioned in or is working.
    async fn symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        let mut out: Vec<String> = self
            .positions(client, secrets)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.symbol)
            .collect();
        out.extend(
            self.orders(client, secrets)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|o| o.symbol),
        );
        out.retain(|s| !s.is_empty());
        out.sort();
        out.dedup();
        Ok(out)
    }
}

/// Quote coins a holding is priced against, most dollar-like first.
const QUOTE_COINS: &[&str] = &["USDT", "USDC", "USD", "EUR", "BTC"];

// ── Mapping ──────────────────────────────────────────────────────────────────

fn fill(r: &Value, symbol: &str, c: &Contract) -> Result<Execution> {
    Ok(Execution {
        id: str_of(r, "tradeId"),
        order_id: str_of(r, "ordId"),
        at: stamp(r, "ts")?,
        symbol: symbol.to_string(),
        side: str_of(r, "side").to_ascii_lowercase(),
        // Contracts on a derivative, coins on spot: the point value carries the difference.
        qty: num(r, "fillSz"),
        price: num(r, "fillPx"),
        // OKX signs a charge negative and a rebate positive; a cost is filed positive, and a
        // rebate is not turned into one.
        fee: -num(r, "fee"),
        fee_currency: str_of(r, "feeCcy").to_uppercase(),
        currency: if c.currency.is_empty() { quote_of(symbol) } else { c.currency.clone() },
        asset_class: "crypto".into(),
        multiplier: c.multiplier,
        venue: "OKX".into(),
        account: String::new(),
    })
}

/// Instrument id → what a contract of it is worth and settles in.
async fn instruments(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    inst_type: &str,
) -> Result<HashMap<String, Contract>> {
    let rows = get(
        client,
        secrets,
        "/api/v5/public/instruments",
        &[("instType".into(), inst_type.to_string())],
    )
    .await?;
    let mut out = HashMap::new();
    for i in rows.as_array().into_iter().flatten() {
        let id = str_of(i, "instId").to_uppercase();
        if id.is_empty() {
            continue;
        }
        out.insert(id, contract_of(i));
    }
    Ok(out)
}

/// One instrument-list row as a point value and a settlement currency.
fn contract_of(i: &Value) -> Contract {
    let val = num(i, "ctVal");
    let mult = num(i, "ctMult");
    Contract {
        // Spot and margin rows carry neither field: their quantity is already the coin.
        multiplier: if val > 0.0 { val * if mult > 0.0 { mult } else { 1.0 } } else { 1.0 },
        currency: match str_of(i, "settleCcy") {
            s if !s.is_empty() => s.to_uppercase(),
            _ => str_of(i, "quoteCcy").to_uppercase(),
        },
    }
}

/// The quote coin of an instrument id. OKX writes them dash-separated, so the second field
/// is the quote: `BTC-USDT`, `BTC-USDT-SWAP`, `BTC-USD-241227`.
fn quote_of(inst_id: &str) -> String {
    inst_id.split('-').nth(1).unwrap_or_default().to_uppercase()
}

/// Every listed spot pair and its last price, in one public call.
async fn ticker_board(client: &reqwest::Client) -> Result<HashMap<String, f64>> {
    let body: Value = crate::rate::send(
        "okx",
        client.get(format!("{}/api/v5/market/tickers?instType=SPOT", super::at(BASE))),
    )
    .await
    .context("asking OKX for its prices")?
    .json()
    .await
    .context("reading OKX's prices")?;
    let mut out = HashMap::new();
    for row in body.get("data").and_then(Value::as_array).into_iter().flatten() {
        let id = str_of(row, "instId").to_uppercase();
        if !id.is_empty() {
            out.insert(id, num(row, "last"));
        }
    }
    Ok(out)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn ms(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
}

fn stamp(v: &Value, key: &str) -> Result<OffsetDateTime> {
    let raw = str_of(v, key);
    let at: i64 = raw
        .parse()
        .map_err(|_| anyhow!("OKX reported a timestamp that cannot be read: {raw}"))?;
    OffsetDateTime::from_unix_timestamp_nanos(at as i128 * 1_000_000)
        .map_err(|_| anyhow!("OKX reported an impossible timestamp: {at}"))
}

fn ms_stamp(v: &Value, key: &str) -> Option<OffsetDateTime> {
    stamp(v, key).ok()
}

/// The first row of a `data` array, or null when it is empty.
fn first_row(v: &Value) -> Value {
    v.as_array().and_then(|a| a.first().cloned()).unwrap_or(Value::Null)
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn num(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

/// The string OKX signs: the ISO timestamp, the verb, and the path *with* its query.
pub(super) fn prehash(ts: &str, method: &str, path: &str, query: &str) -> String {
    if query.is_empty() {
        format!("{ts}{method}{path}")
    } else {
        format!("{ts}{method}{path}?{query}")
    }
}

/// OKX's timestamp: ISO-8601 UTC to the millisecond, `2020-12-08T09:08:57.715Z`.
fn iso_ms(t: OffsetDateTime) -> String {
    let ms = t.millisecond();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        ms
    )
}

fn env_label(secrets: &HashMap<String, String>) -> &'static str {
    if demo(secrets) { "Demo" } else { "Live" }
}

fn demo(secrets: &HashMap<String, String>) -> bool {
    matches!(secrets.get("env").map(|s| s.trim().to_ascii_lowercase()), Some(v) if v == "demo")
}

/// A signed GET. Returns the envelope's `data` array, or an error naming what to fix.
async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    use base64::Engine as _;

    let key = super::require(secrets, "api_key")?;
    let secret = super::require(secrets, "api_secret")?;
    let pass = super::require(secrets, "api_passphrase")?;
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", super::binance::enc(k), super::binance::enc(v)))
        .collect::<Vec<_>>()
        .join("&");
    let ts = iso_ms(OffsetDateTime::now_utc());
    let sign = base64::engine::general_purpose::STANDARD.encode(super::binance::hmac_sha256(
        secret.as_bytes(),
        prehash(&ts, "GET", path, &query).as_bytes(),
    ));
    let url = if query.is_empty() {
        format!("{}{path}", super::at(BASE))
    } else {
        format!("{}{path}?{query}", super::at(BASE))
    };
    let mut req = client
        .get(&url)
        .header("OK-ACCESS-KEY", key)
        .header("OK-ACCESS-SIGN", sign)
        .header("OK-ACCESS-TIMESTAMP", &ts)
        .header("OK-ACCESS-PASSPHRASE", pass)
        .header("Content-Type", "application/json");
    if demo(secrets) {
        req = req.header("x-simulated-trading", "1");
    }
    let res = crate::rate::send("okx", req)
        .await
        .with_context(|| format!("calling OKX {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() && body.is_empty() {
        return Err(anyhow!("OKX {path} answered {status}"));
    }
    let v: Value = serde_json::from_str(&body)
        .with_context(|| format!("reading OKX's {path} answer"))?;
    // OKX answers 200 with a code in the envelope, so success is the code, not the status.
    let code = str_of(&v, "code");
    if code != "0" {
        let msg = str_of(&v, "msg");
        return Err(match code.as_str() {
            "50111" | "50113" | "50101" | "50102" | "50103" | "50104" | "50105" => anyhow!(
                "OKX refused the key ({code} {msg}). Check the key, its secret and the \
                 passphrase chosen when it was created, that the host's clock is right, and \
                 that the key is not IP-restricted to another machine."
            ),
            "50114" => anyhow!(
                "OKX says this key is not allowed to read {path} ({msg}). The Read permission \
                 is enough, but it has to be granted."
            ),
            "50011" => anyhow!("OKX is rate-limiting this key ({msg}), retry in a moment"),
            _ => anyhow!("OKX refused {path}: {code} {msg}"),
        });
    }
    Ok(v.get("data").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signed_string_is_the_path_with_its_query() {
        assert_eq!(
            prehash("2020-12-08T09:08:57.715Z", "GET", "/api/v5/trade/fills-history", ""),
            "2020-12-08T09:08:57.715ZGET/api/v5/trade/fills-history"
        );
        assert_eq!(
            prehash("2020-12-08T09:08:57.715Z", "GET", "/api/v5/account/positions", "instType=SWAP"),
            "2020-12-08T09:08:57.715ZGET/api/v5/account/positions?instType=SWAP"
        );
    }

    #[test]
    fn the_timestamp_is_iso_to_the_millisecond() {
        let t = OffsetDateTime::from_unix_timestamp_nanos(1_607_418_537_715_000_000).unwrap();
        assert_eq!(iso_ms(t), "2020-12-08T09:08:57.715Z");
    }

    #[test]
    fn a_contract_carries_its_point_value_and_its_settlement_coin() {
        // A linear swap: one contract is 0.01 BTC, settled in USDT.
        let linear = serde_json::json!({
            "instId": "BTC-USDT-SWAP", "ctVal": "0.01", "ctMult": "1",
            "ctValCcy": "BTC", "settleCcy": "USDT", "ctType": "linear"
        });
        let c = contract_of(&linear);
        assert_eq!(c.multiplier, 0.01);
        assert_eq!(c.currency, "USDT");
        // An inverse one is 100 USD a contract and settles in the coin.
        let inverse = serde_json::json!({
            "instId": "BTC-USD-SWAP", "ctVal": "100", "ctMult": "1",
            "ctValCcy": "USD", "settleCcy": "BTC", "ctType": "inverse"
        });
        let c = contract_of(&inverse);
        assert_eq!(c.multiplier, 100.0);
        assert_eq!(c.currency, "BTC");
        // A spot row has neither field: its quantity is already the coin.
        let spot = serde_json::json!({"instId": "BTC-USDT", "baseCcy": "BTC", "quoteCcy": "USDT"});
        let c = contract_of(&spot);
        assert_eq!(c.multiplier, 1.0);
        assert_eq!(c.currency, "USDT");
    }

    #[test]
    fn a_fill_files_a_charge_as_a_cost_and_keeps_a_rebate_a_rebate() {
        let r = serde_json::json!({
            "instType": "SWAP", "instId": "BTC-USDT-SWAP", "tradeId": "1", "ordId": "2",
            "billId": "3", "side": "sell", "fillSz": "5", "fillPx": "30000",
            "fee": "-0.075", "feeCcy": "USDT", "ts": "1597026383085"
        });
        let c = Contract { multiplier: 0.01, currency: "USDT".into() };
        let e = fill(&r, "BTC-USDT-SWAP", &c).unwrap();
        assert_eq!(e.side, "sell");
        assert_eq!(e.qty, 5.0);
        assert_eq!(e.multiplier, 0.01);
        assert!((e.fee - 0.075).abs() < 1e-12);
        assert_eq!(e.currency, "USDT");
        assert_eq!(e.at.unix_timestamp_nanos(), 1_597_026_383_085_000_000);
        // A maker rebate is money in, and turning it into a cost would misstate the trade.
        let mut rebate = r.clone();
        rebate["fee"] = serde_json::json!("0.01");
        assert!(fill(&rebate, "BTC-USDT-SWAP", &c).unwrap().fee < 0.0);
    }

    #[test]
    fn an_instrument_id_names_its_own_quote_coin() {
        assert_eq!(quote_of("BTC-USDT"), "USDT");
        assert_eq!(quote_of("BTC-USDT-SWAP"), "USDT");
        assert_eq!(quote_of("BTC-USD-241227"), "USD");
        assert_eq!(quote_of("BTC"), "");
    }
}
