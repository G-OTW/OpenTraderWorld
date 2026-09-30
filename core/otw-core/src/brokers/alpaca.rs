//! Alpaca: the trading API, read side only.
//!
//! The one broker here that answers all four questions from one key: fills, positions,
//! working orders and a cost basis. Auth is two plain headers, which makes it the simplest
//! connector in the folder; what it costs instead is a lookup, because a fill says only
//! `symbol` and the journal needs to know what kind of instrument that is.
//!
//! Three things worth knowing before reading the numbers:
//!
//!   - **Paper and live are different hosts and different keys.** A paper key against the
//!     live host is a 403, so the environment is a setting on the account rather than
//!     something inferred from a key nobody can tell apart.
//!   - **A fill carries no commission.** Equities are commission-free, and the crypto and
//!     options fees arrive as their own account activities. So a trade imported from here
//!     has zero fees, which is right for stocks and optimistic elsewhere.
//!   - **An instrument is asked for, never inferred.** `/v2/assets` says what a symbol is,
//!     and an option's multiplier comes from its contract, because a hundredfold error in
//!     the point value is a hundredfold error in the PnL.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

const LIVE: &str = "https://api.alpaca.markets";
const PAPER: &str = "https://paper-api.alpaca.markets";
/// Activities page at 100 rows, which is also Alpaca's maximum.
const PAGE: usize = 100;
/// Hard stop on paging, so a mis-specified window cannot walk forever.
const MAX_PAGES: usize = 500;

pub struct Alpaca;

static CAP: BrokerCapability = BrokerCapability {
    broker: "alpaca",
    label: "Alpaca",
    website: "https://alpaca.markets",
    docs_url: "https://docs.alpaca.markets/reference/getaccount-1",
    rate_limit: "200 requests/minute on a free key. Fills page 100 at a time, and each new \
                 instrument costs one lookup to learn what it is.",
    key_note: "A trading API key and secret, from the dashboard of the environment you pick \
               below: a paper key does not work against the live account, or the other way \
               round. Nothing here places, changes or cancels an order. Alpaca reports no \
               commission on a fill, so an imported trade carries no fees: right for \
               equities, which are commission-free, and short of the truth for crypto and \
               options, whose fees arrive as separate account activities.",
    required_secrets: &["api_key", "api_secret"],
    config_fields: FIELDS,
    asset_classes: &["stock", "option", "crypto"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // A margin account can go short, and Alpaca reports it as such.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or paper. Alpaca serves the two from different hosts and issues a separate \
           key for each, so this has to be said rather than guessed. Leave empty for live.",
}];

#[async_trait::async_trait]
impl Broker for Alpaca {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => Ok(()),
            Some(v) if v.is_empty() || v == "live" || v == "paper" => Ok(()),
            Some(other) => Err(anyhow!(
                "the Alpaca environment is \"live\" or \"paper\", not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let acct = get(client, secrets, "/v2/account", &[]).await?;
        let number = str_of(&acct, "account_number");
        let status = str_of(&acct, "status");
        let currency = str_of(&acct, "currency");
        Ok(format!(
            "{} account {number} is {status}, in {currency}",
            env_label(secrets)
        ))
    }

    /// Every fill of the window, oldest first. Alpaca answers the whole account at once, so
    /// `symbols` only narrows what comes back.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let account_ccy = account_currency(client, secrets).await;
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();

        let mut out: Vec<Execution> = Vec::new();
        let mut assets: HashMap<String, AssetInfo> = HashMap::new();
        let mut token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut params = vec![
                ("after".to_string(), q.from.format(&Rfc3339)?),
                ("until".to_string(), q.to.format(&Rfc3339)?),
                ("direction".to_string(), "asc".to_string()),
                ("page_size".to_string(), PAGE.to_string()),
            ];
            if let Some(t) = &token {
                params.push(("page_token".to_string(), t.clone()));
            }
            let body = get(client, secrets, "/v2/account/activities/FILL", &params).await?;
            let rows = body.as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                break;
            }
            for r in &rows {
                let symbol = str_of(r, "symbol");
                if symbol.is_empty() {
                    continue;
                }
                if !wanted.is_empty() && !wanted.contains(&symbol.to_uppercase()) {
                    continue;
                }
                let info = asset_info(client, secrets, &mut assets, &symbol).await;
                let side = str_of(r, "side");
                out.push(Execution {
                    id: str_of(r, "id"),
                    order_id: str_of(r, "order_id"),
                    at: stamp(r, "transaction_time")?,
                    // A short sale is a sell: the direction is the trade's, and whether it
                    // opens or closes is the fold's business, not the connector's.
                    side: if side.starts_with("sell") { "sell".into() } else { "buy".into() },
                    qty: num(r, "qty"),
                    price: num(r, "price"),
                    // Alpaca bills no commission on a fill; see the note on the capability.
                    fee: 0.0,
                    fee_currency: String::new(),
                    currency: currency_of(&symbol, &info, &account_ccy),
                    asset_class: info.class.clone(),
                    multiplier: info.multiplier,
                    venue: info.exchange.clone(),
                    account: String::new(),
                    symbol,
                });
            }
            // The page token is the last row's id; a short page is the last one.
            if rows.len() < PAGE {
                break;
            }
            token = rows.last().map(|r| str_of(r, "id")).filter(|s| !s.is_empty());
            if token.is_none() {
                break;
            }
        }
        Ok(out)
    }

    async fn positions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        let account_ccy = account_currency(client, secrets).await;
        let rows = get(client, secrets, "/v2/positions", &[]).await?;
        let mut assets: HashMap<String, AssetInfo> = HashMap::new();
        let mut out = Vec::new();
        for p in rows.as_array().into_iter().flatten() {
            let symbol = str_of(p, "symbol");
            let info = asset_of_position(client, secrets, &mut assets, p, &symbol).await;
            out.push(Position {
                side: if str_of(p, "side") == "short" { "short".into() } else { "long".into() },
                qty: num(p, "qty").abs(),
                avg_price: Some(num(p, "avg_entry_price")).filter(|v| *v > 0.0),
                currency: currency_of(&symbol, &info, &account_ccy),
                asset_class: info.class.clone(),
                multiplier: info.multiplier,
                venue: str_of(p, "exchange"),
                account: String::new(),
                symbol,
            });
        }
        Ok(out)
    }

    async fn orders(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<OpenOrder>> {
        let account_ccy = account_currency(client, secrets).await;
        let rows = get(
            client,
            secrets,
            "/v2/orders",
            &[
                ("status".into(), "open".into()),
                ("limit".into(), "500".into()),
                ("direction".into(), "desc".into()),
            ],
        )
        .await?;
        let mut assets: HashMap<String, AssetInfo> = HashMap::new();
        let mut out = Vec::new();
        for o in rows.as_array().into_iter().flatten() {
            let symbol = str_of(o, "symbol");
            let info = asset_of_order(client, secrets, &mut assets, o, &symbol).await;
            out.push(OpenOrder {
                id: str_of(o, "id"),
                side: str_of(o, "side"),
                // `type` is the live field; `order_type` is Alpaca's own deprecated alias.
                order_type: first_str(o, &["type", "order_type"]),
                qty: num(o, "qty"),
                filled_qty: num(o, "filled_qty"),
                limit_price: Some(num(o, "limit_price")).filter(|v| *v > 0.0),
                stop_price: Some(num(o, "stop_price")).filter(|v| *v > 0.0),
                currency: currency_of(&symbol, &info, &account_ccy),
                asset_class: info.class.clone(),
                placed_at: o
                    .get("submitted_at")
                    .or_else(|| o.get("created_at"))
                    .and_then(Value::as_str)
                    .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok()),
                venue: String::new(),
                account: String::new(),
                symbol,
            });
        }
        Ok(out)
    }

    /// Positions read as a balance sheet. Alpaca reports a cost basis, so these arrive
    /// priced and the portfolio import has nothing to ask.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        let account_ccy = account_currency(client, secrets).await;
        let rows = get(client, secrets, "/v2/positions", &[]).await?;
        let mut assets: HashMap<String, AssetInfo> = HashMap::new();
        let mut out = Vec::new();
        for p in rows.as_array().into_iter().flatten() {
            let symbol = str_of(p, "symbol");
            let info = asset_of_position(client, secrets, &mut assets, p, &symbol).await;
            out.push(Holding {
                name: info.name.clone(),
                qty: num(p, "qty").abs(),
                side: if str_of(p, "side") == "short" { "short".into() } else { "long".into() },
                asset_class: info.class.clone(),
                avg_price: Some(num(p, "avg_entry_price")).filter(|v| *v > 0.0),
                currency: currency_of(&symbol, &info, &account_ccy),
                venue: str_of(p, "exchange"),
                account: String::new(),
                symbol,
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    /// What these assets are worth right now. Alpaca marks every open position itself, so
    /// the price comes off the position rather than from a market-data plan the account
    /// may not be subscribed to. An asset the account no longer holds is left out.
    async fn quotes(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        symbols: &[String],
    ) -> Result<Vec<Quote>> {
        let account_ccy = account_currency(client, secrets).await;
        let rows = get(client, secrets, "/v2/positions", &[]).await?;
        let mut assets: HashMap<String, AssetInfo> = HashMap::new();
        let mut out = Vec::new();
        for p in rows.as_array().into_iter().flatten() {
            let symbol = str_of(p, "symbol");
            let Some(wanted) = symbols
                .iter()
                .find(|s| s.trim().eq_ignore_ascii_case(&symbol))
            else {
                continue;
            };
            let price = num(p, "current_price");
            if price <= 0.0 {
                continue;
            }
            let info = asset_of_position(client, secrets, &mut assets, p, &symbol).await;
            out.push(Quote {
                symbol: wanted.clone(),
                price,
                currency: currency_of(&symbol, &info, &account_ccy),
                pair: symbol,
            });
        }
        Ok(out)
    }

    /// What the account is in or is working, for a symbol picker.
    async fn symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        let positions = get(client, secrets, "/v2/positions", &[]).await?;
        for p in positions.as_array().into_iter().flatten() {
            out.push(str_of(p, "symbol"));
        }
        if let Ok(orders) = get(
            client,
            secrets,
            "/v2/orders",
            &[("status".into(), "open".into()), ("limit".into(), "500".into())],
        )
        .await
        {
            for o in orders.as_array().into_iter().flatten() {
                out.push(str_of(o, "symbol"));
            }
        }
        out.retain(|s| !s.is_empty());
        out.sort();
        out.dedup();
        Ok(out)
    }
}

// ── What a symbol is ─────────────────────────────────────────────────────────

/// What the broker says an instrument is. Asked once per symbol and remembered for the
/// call: a year of fills touches a handful of instruments, not a handful per fill.
#[derive(Clone)]
struct AssetInfo {
    /// Journal vocabulary, not Alpaca's.
    class: String,
    /// Point value. Only an option has one worth asking for.
    multiplier: f64,
    exchange: String,
    name: String,
}

impl Default for AssetInfo {
    fn default() -> Self {
        Self {
            // An instrument nobody could identify is filed as "other", which is a plain
            // disposal everywhere downstream. It is never turned into a guess at a class
            // that carries a multiplier or a tax treatment of its own.
            class: "other".into(),
            multiplier: 1.0,
            exchange: String::new(),
            name: String::new(),
        }
    }
}

/// Alpaca's asset class in the journal's vocabulary.
fn class_of(alpaca: &str) -> &'static str {
    match alpaca {
        "us_equity" | "global_equity" => "stock",
        "us_option" => "option",
        "crypto" | "crypto_perp" => "crypto",
        _ => "other",
    }
}

/// The asset behind a symbol, from Alpaca itself. A failure is not fatal: the trade is
/// still real, and a missing class costs a label, not a number.
async fn asset_info(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    cache: &mut HashMap<String, AssetInfo>,
    symbol: &str,
) -> AssetInfo {
    if let Some(hit) = cache.get(symbol) {
        return hit.clone();
    }
    let info = fetch_asset(client, secrets, symbol).await.unwrap_or_default();
    cache.insert(symbol.to_string(), info.clone());
    info
}

/// A position already names its own class, so only the multiplier is still a question.
async fn asset_of_position(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    cache: &mut HashMap<String, AssetInfo>,
    row: &Value,
    symbol: &str,
) -> AssetInfo {
    let class = class_of(&str_of(row, "asset_class"));
    if class != "option" {
        return AssetInfo {
            class: class.into(),
            multiplier: 1.0,
            exchange: str_of(row, "exchange"),
            name: String::new(),
        };
    }
    let mut info = asset_info(client, secrets, cache, symbol).await;
    info.class = class.into();
    info
}

/// Same for an order row.
async fn asset_of_order(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    cache: &mut HashMap<String, AssetInfo>,
    row: &Value,
    symbol: &str,
) -> AssetInfo {
    let class = class_of(&str_of(row, "asset_class"));
    if class == "other" {
        return asset_info(client, secrets, cache, symbol).await;
    }
    AssetInfo {
        class: class.into(),
        multiplier: 1.0,
        ..Default::default()
    }
}

async fn fetch_asset(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    symbol: &str,
) -> Result<AssetInfo> {
    // A crypto pair carries a slash, which is a path separator until it is encoded.
    let path = format!("/v2/assets/{}", super::binance::enc(symbol));
    if let Ok(a) = get(client, secrets, &path, &[]).await {
        let class = class_of(&str_of(&a, "class"));
        let mut info = AssetInfo {
            class: class.into(),
            multiplier: 1.0,
            exchange: str_of(&a, "exchange"),
            name: str_of(&a, "name"),
        };
        if class == "option" {
            info.multiplier = option_multiplier(client, secrets, symbol).await;
        }
        return Ok(info);
    }
    // Not in the asset list: an option contract is its own endpoint.
    let c = get(
        client,
        secrets,
        &format!("/v2/options/contracts/{}", super::binance::enc(symbol)),
        &[],
    )
    .await?;
    Ok(AssetInfo {
        class: "option".into(),
        multiplier: Some(num(&c, "multiplier")).filter(|m| *m > 0.0).unwrap_or(1.0),
        exchange: String::new(),
        name: str_of(&c, "name"),
    })
}

/// A contract's point value, asked of the contract. A hundredfold error here is a
/// hundredfold error in the PnL, so it is never assumed to be the usual hundred.
async fn option_multiplier(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    symbol: &str,
) -> f64 {
    match get(
        client,
        secrets,
        &format!("/v2/options/contracts/{}", super::binance::enc(symbol)),
        &[],
    )
    .await
    {
        Ok(c) => Some(num(&c, "multiplier")).filter(|m| *m > 0.0).unwrap_or(1.0),
        Err(_) => 1.0,
    }
}

/// What the instrument settles in. A crypto pair states its quote after the slash, which
/// is Alpaca's own notation and not an alias; everything else settles in the account's
/// currency, which the account itself reports.
fn currency_of(symbol: &str, info: &AssetInfo, account_ccy: &str) -> String {
    if info.class == "crypto" {
        if let Some((_, quote)) = symbol.split_once('/') {
            return quote.trim().to_uppercase();
        }
    }
    account_ccy.to_string()
}

async fn account_currency(client: &reqwest::Client, secrets: &HashMap<String, String>) -> String {
    match get(client, secrets, "/v2/account", &[]).await {
        Ok(a) => {
            let c = str_of(&a, "currency");
            if c.is_empty() { "USD".into() } else { c }
        }
        Err(_) => "USD".into(),
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

fn base(secrets: &HashMap<String, String>) -> &'static str {
    match secrets.get("env").map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "paper" => PAPER,
        _ => LIVE,
    }
}

fn env_label(secrets: &HashMap<String, String>) -> &'static str {
    if base(secrets) == PAPER { "Paper" } else { "Live" }
}

async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let key = super::require(secrets, "api_key")?;
    let secret = super::require(secrets, "api_secret")?;
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", super::binance::enc(k), super::binance::enc(v)))
        .collect();
    let url = if query.is_empty() {
        format!("{}{path}", base(secrets))
    } else {
        format!("{}{path}?{}", base(secrets), query.join("&"))
    };
    let res = crate::rate::send(
        "alpaca",
        client
            .get(&url)
            .header("APCA-API-KEY-ID", key)
            .header("APCA-API-SECRET-KEY", secret),
    )
    .await
    .with_context(|| format!("calling Alpaca {path}"))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        // The commonest failure by far, and the one the message has to name.
        let hint = if status.as_u16() == 403 {
            format!(
                " (this account is set to {}: a key issued for the other environment is \
                 refused here)",
                env_label(secrets).to_lowercase()
            )
        } else {
            String::new()
        };
        return Err(anyhow!(
            "Alpaca {path} answered {status}{hint}: {}",
            super::binance::trim_err(&text)
        ));
    }
    serde_json::from_str(&text).with_context(|| format!("reading Alpaca's {path} answer"))
}

// ── Reading JSON ─────────────────────────────────────────────────────────────

/// Alpaca writes every number as a string, and omits a price that does not apply.
fn num(v: &Value, key: &str) -> f64 {
    v.get(key)
        .and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0.0)
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn first_str(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        let s = str_of(v, k);
        if !s.is_empty() {
            return s;
        }
    }
    String::new()
}

fn stamp(v: &Value, key: &str) -> Result<OffsetDateTime> {
    let raw = str_of(v, key);
    OffsetDateTime::parse(&raw, &Rfc3339)
        .with_context(|| format!("reading Alpaca's {key} \"{raw}\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(env: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        if !env.is_empty() {
            m.insert("env".to_string(), env.to_string());
        }
        m
    }

    #[test]
    fn paper_and_live_are_different_hosts() {
        assert_eq!(base(&cfg("")), LIVE, "no setting means the live account");
        assert_eq!(base(&cfg("live")), LIVE);
        assert_eq!(base(&cfg("paper")), PAPER);
        assert_eq!(base(&cfg(" Paper ")), PAPER, "typed with a capital and a space");
    }

    #[test]
    fn the_environment_is_checked_before_it_is_stored() {
        assert!(Alpaca.validate_config(&cfg("paper")).is_ok());
        assert!(Alpaca.validate_config(&cfg("live")).is_ok());
        assert!(Alpaca.validate_config(&cfg("")).is_ok());
        let e = Alpaca.validate_config(&cfg("sandbox")).unwrap_err().to_string();
        assert!(e.contains("sandbox"), "the message names what was typed: {e}");
    }

    #[test]
    fn every_alpaca_class_lands_in_the_journals_vocabulary() {
        assert_eq!(class_of("us_equity"), "stock");
        assert_eq!(class_of("global_equity"), "stock");
        assert_eq!(class_of("us_option"), "option");
        assert_eq!(class_of("crypto"), "crypto");
        assert_eq!(class_of("crypto_perp"), "crypto");
        // A class this connector has never heard of is a plain disposal, not a crash and
        // not a guess: Alpaca has added several since this was written.
        assert_eq!(class_of("treasury"), "other");
        assert_eq!(class_of(""), "other");
    }

    #[test]
    fn a_crypto_pair_states_its_own_quote_and_nothing_else_does() {
        let crypto = AssetInfo { class: "crypto".into(), ..Default::default() };
        let stock = AssetInfo { class: "stock".into(), ..Default::default() };
        assert_eq!(currency_of("BTC/USDT", &crypto, "USD"), "USDT");
        assert_eq!(currency_of("ETH/USD", &crypto, "USD"), "USD");
        // Old-style crypto symbols carry no separator: the account's currency stands,
        // rather than a quote cut off the end of the ticker.
        assert_eq!(currency_of("BTCUSD", &crypto, "USD"), "USD");
        assert_eq!(currency_of("AAPL", &stock, "EUR"), "EUR");
    }

    #[test]
    fn a_short_sale_is_read_as_a_sell() {
        for side in ["sell", "sell_short"] {
            let row = json!({ "side": side });
            let s = str_of(&row, "side");
            assert!(s.starts_with("sell"), "{side}");
        }
        assert!(!str_of(&json!({ "side": "buy" }), "side").starts_with("sell"));
    }

    #[test]
    fn prices_and_quantities_are_read_out_of_alpacas_strings() {
        let row = json!({ "qty": "1.5", "price": 190.25, "stop_price": null });
        assert_eq!(num(&row, "qty"), 1.5);
        assert_eq!(num(&row, "price"), 190.25);
        assert_eq!(num(&row, "stop_price"), 0.0, "an absent price reads as none, not as an error");
        assert_eq!(num(&row, "missing"), 0.0);
    }

    #[test]
    fn the_live_order_type_wins_over_alpacas_deprecated_alias() {
        let row = json!({ "type": "limit", "order_type": "market" });
        assert_eq!(first_str(&row, &["type", "order_type"]), "limit");
        let old = json!({ "order_type": "stop" });
        assert_eq!(first_str(&old, &["type", "order_type"]), "stop");
    }
}
