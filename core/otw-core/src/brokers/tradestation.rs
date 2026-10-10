//! TradeStation v3 account (OAuth, read scopes only).
//!
//! The only OAuth broker here. TradeStation issues no static API key: an application
//! exchanges a **refresh token** for a 20-minute access token, so the credentials asked for
//! are the API key pair and the refresh token the one-time browser sign-in produced. That
//! sign-in is where the scopes are chosen, and `Trade` is not one of them.
//!
//! Three shapes of the API decide how this connector works:
//!   • **A fill is an order leg.** There is no executions endpoint: closed orders come back
//!     from `historicalorders` carrying `Legs`, each with its executed quantity and price.
//!     The identity a re-sync deduplicates on is therefore the order id plus the leg's
//!     position in it, which is stable because the broker returns the legs in order.
//!   • **The archive is 90 days**, and the endpoint takes a `since` date and no end date, so
//!     the period's end filters here.
//!   • **A point value is asked for, never assumed.** An option is not usually a hundred
//!     shares after a corporate action and a future is never one, so the contract's
//!     `PriceFormat.PointValue` is read from the symbol details. That call also says what
//!     currency the instrument settles in.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

pub(crate) const LIVE: &str = "https://api.tradestation.com";
pub(crate) const SIM: &str = "https://sim-api.tradestation.com";
/// The sign-in host is its own: the token is minted there and the data read from the API.
const TOKEN_HOST: &str = "https://signin.tradestation.com";
/// An access token is good for twenty minutes.
const TOKEN_TTL: Duration = Duration::from_secs(20 * 60);
/// Orders per page; the endpoint's own ceiling.
const PAGE: usize = 600;
/// Hard stop on the page walk, so a mis-specified window cannot loop.
const MAX_PAGES: usize = 200;

pub struct TradeStation;

static FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "11111111",
        kind: "text",
        required: true,
        help: "The TradeStation account number this broker account reads. One account per \
               entry: the key may reach several (cash, margin, futures) and which one is \
               read has to be said rather than picked.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or sim. Simulated trading is a different host and its own account \
               numbers. Leave empty for live.",
    },
];

static CAP: BrokerCapability = BrokerCapability {
    broker: "tradestation",
    label: "TradeStation",
    website: "https://www.tradestation.com",
    docs_url: "https://api.tradestation.com/docs/",
    rate_limit: "The access token is minted once every twenty minutes and shared. Orders \
                 page 600 at a time.",
    key_note: "An API key (client id and secret) plus the **refresh token** from a one-time \
               sign-in granting `ReadAccount`, `MarketData`, `openid` and `offline_access` \
               only. **Do not grant `Trade`**: this connector never writes, and a token that \
               could trade would be a standing risk for nothing. TradeStation serves 90 days \
               of closed orders; anything older is a statement from its own site.",
    required_secrets: &["client_id", "client_secret", "refresh_token"],
    config_fields: FIELDS,
    asset_classes: &["stock", "option", "future"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 90,
    // A margin or futures account can be short, and TradeStation reports it as such.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    quotes: true,
    testable: true,
};

/// What the venue says an instrument is worth and settles in.
#[derive(Clone)]
struct Instrument {
    /// Journal class, from TradeStation's own `AssetType`.
    class: String,
    /// Point value, from the contract rather than from a habit.
    multiplier: f64,
    currency: String,
    exchange: String,
    name: String,
}

impl Default for Instrument {
    fn default() -> Self {
        Instrument {
            // An instrument nobody could identify is filed as "other", which is a plain
            // disposal everywhere downstream rather than a guessed class with a tax
            // treatment of its own.
            class: "other".into(),
            multiplier: 1.0,
            currency: String::new(),
            exchange: String::new(),
            name: String::new(),
        }
    }
}

#[async_trait::async_trait]
impl Broker for TradeStation {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => Ok(()),
            Some(v) if v.is_empty() || v == "live" || v == "sim" => Ok(()),
            Some(other) => Err(anyhow!(
                "the TradeStation environment is \"live\" or \"sim\", not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let wanted = account_id(secrets)?;
        let body = get(client, secrets, "/v3/brokerage/accounts", &[]).await?;
        let rows = body.get("Accounts").and_then(Value::as_array).cloned().unwrap_or_default();
        let found = rows
            .iter()
            .find(|a| str_of(a, "AccountID") == wanted)
            .ok_or_else(|| {
                anyhow!(
                    "the key reached TradeStation but account {wanted} is not one of the {} \
                     it can see ({}). Check the Account ID setting.",
                    rows.len(),
                    rows.iter()
                        .map(|a| str_of(a, "AccountID"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        Ok(format!(
            "{} account {wanted}, type {}, status {}",
            env_label(secrets),
            str_of(found, "AccountType"),
            str_of(found, "Status"),
        ))
    }

    /// Every executed order leg of the window, oldest first.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let account = account_id(secrets)?;
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        let mut known: HashMap<String, Instrument> = HashMap::new();
        let mut out: Vec<Execution> = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut params = vec![
                ("since".to_string(), format!("{}", q.from.date())),
                ("pageSize".to_string(), PAGE.to_string()),
            ];
            if let Some(t) = &token {
                params.push(("nextToken".into(), t.clone()));
            }
            let body = get(
                client,
                secrets,
                &format!("/v3/brokerage/accounts/{}/historicalorders", super::binance::enc(&account)),
                &params,
            )
            .await?;
            refuse_errors(&body)?;
            let rows = body.get("Orders").and_then(Value::as_array).cloned().unwrap_or_default();
            for o in &rows {
                // The order's own closing stamp is when it filled; the period's end is
                // applied here, since the endpoint takes no end date.
                let Some(at) = stamp(o, "ClosedDateTime").or_else(|| stamp(o, "OpenedDateTime"))
                else {
                    continue;
                };
                if at < q.from || at > q.to {
                    continue;
                }
                let legs = o.get("Legs").and_then(Value::as_array).cloned().unwrap_or_default();
                let fee = num(o, "CommissionFee").abs();
                for (i, leg) in legs.iter().enumerate() {
                    let qty = num(leg, "ExecQuantity");
                    if qty <= 0.0 {
                        continue;
                    }
                    let symbol = str_of(leg, "Symbol").to_uppercase();
                    if symbol.is_empty() || (!wanted.is_empty() && !wanted.contains(&symbol)) {
                        continue;
                    }
                    let info = instrument(client, secrets, &mut known, &symbol).await;
                    out.push(Execution {
                        // The order id plus the leg's place in the order: stable across
                        // re-syncs, which is what a deduplication key has to be.
                        id: format!("{}-{i}", str_of(o, "OrderID")),
                        order_id: str_of(o, "OrderID"),
                        at,
                        side: if str_of(leg, "BuyOrSell").eq_ignore_ascii_case("sell") {
                            "sell".into()
                        } else {
                            "buy".into()
                        },
                        qty,
                        price: num(leg, "ExecutionPrice"),
                        // The commission is the order's, so it is carried on its first
                        // executed leg rather than counted once per leg.
                        fee: if i == 0 { fee } else { 0.0 },
                        fee_currency: str_of(o, "Currency").to_uppercase(),
                        currency: currency_of(o, &info),
                        asset_class: info.class.clone(),
                        multiplier: info.multiplier,
                        venue: info.exchange.clone(),
                        account: account.clone(),
                        symbol,
                    });
                }
            }
            token = str_opt(&body, "NextToken");
            if token.is_none() || rows.is_empty() {
                break;
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
        let account = account_id(secrets)?;
        let body = get(
            client,
            secrets,
            &format!("/v3/brokerage/accounts/{}/positions", super::binance::enc(&account)),
            &[],
        )
        .await?;
        refuse_errors(&body)?;
        let mut known: HashMap<String, Instrument> = HashMap::new();
        let mut out = Vec::new();
        for p in body.get("Positions").and_then(Value::as_array).into_iter().flatten() {
            let qty = num(p, "Quantity");
            if qty == 0.0 {
                continue;
            }
            let symbol = str_of(p, "Symbol").to_uppercase();
            let info = instrument(client, secrets, &mut known, &symbol).await;
            out.push(Position {
                side: if str_of(p, "LongShort").eq_ignore_ascii_case("short") {
                    "short".into()
                } else {
                    "long".into()
                },
                qty: qty.abs(),
                avg_price: Some(num(p, "AveragePrice")).filter(|v| *v > 0.0),
                currency: info.currency.clone(),
                asset_class: class_of(&str_of(p, "AssetType")).to_string(),
                multiplier: info.multiplier,
                venue: info.exchange.clone(),
                account: account.clone(),
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
        let account = account_id(secrets)?;
        let body = get(
            client,
            secrets,
            &format!("/v3/brokerage/accounts/{}/orders", super::binance::enc(&account)),
            &[],
        )
        .await?;
        refuse_errors(&body)?;
        let mut known: HashMap<String, Instrument> = HashMap::new();
        let mut out = Vec::new();
        for o in body.get("Orders").and_then(Value::as_array).into_iter().flatten() {
            let legs = o.get("Legs").and_then(Value::as_array).cloned().unwrap_or_default();
            // An order's instrument is its first leg's; a spread has no single one, so only
            // the leg that names a symbol is drawn.
            let Some(leg) = legs.first() else { continue };
            let symbol = str_of(leg, "Symbol").to_uppercase();
            if symbol.is_empty() {
                continue;
            }
            let info = instrument(client, secrets, &mut known, &symbol).await;
            out.push(OpenOrder {
                id: str_of(o, "OrderID"),
                side: if str_of(leg, "BuyOrSell").eq_ignore_ascii_case("sell") {
                    "sell".into()
                } else {
                    "buy".into()
                },
                order_type: str_of(o, "OrderType"),
                qty: num(leg, "QuantityOrdered"),
                filled_qty: num(leg, "ExecQuantity"),
                limit_price: Some(num(o, "LimitPrice")).filter(|v| *v > 0.0),
                stop_price: Some(num(o, "StopPrice")).filter(|v| *v > 0.0),
                currency: currency_of(o, &info),
                asset_class: class_of(&str_of(leg, "AssetType")).to_string(),
                placed_at: stamp(o, "OpenedDateTime"),
                venue: info.exchange.clone(),
                account: account.clone(),
                symbol,
            });
        }
        Ok(out)
    }

    /// Positions read as a balance sheet: TradeStation publishes an average price, so these
    /// arrive with a cost basis and the portfolio import has nothing to ask.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        let mut known: HashMap<String, Instrument> = HashMap::new();
        let mut out = Vec::new();
        for p in self.positions(client, secrets).await? {
            let info = instrument(client, secrets, &mut known, &p.symbol).await;
            out.push(Holding {
                name: info.name.clone(),
                symbol: p.symbol,
                qty: p.qty,
                side: p.side,
                asset_class: p.asset_class,
                avg_price: p.avg_price,
                currency: p.currency,
                venue: p.venue,
                account: p.account,
            });
        }
        Ok(out)
    }

    /// What these instruments trade at right now, from the market-data scope the key
    /// already has.
    async fn quotes(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        symbols: &[String],
    ) -> Result<Vec<Quote>> {
        let wanted: Vec<String> = symbols
            .iter()
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let body = get(
            client,
            secrets,
            &format!("/v3/marketdata/quotes/{}", super::binance::enc(&wanted.join(","))),
            &[],
        )
        .await?;
        let mut known: HashMap<String, Instrument> = HashMap::new();
        let mut out = Vec::new();
        for q in body.get("Quotes").and_then(Value::as_array).into_iter().flatten() {
            let pair = str_of(q, "Symbol").to_uppercase();
            let price = num(q, "Last");
            if price <= 0.0 {
                continue;
            }
            let Some(asked) = symbols.iter().find(|s| s.trim().eq_ignore_ascii_case(&pair)) else {
                continue;
            };
            let info = instrument(client, secrets, &mut known, &pair).await;
            out.push(Quote {
                symbol: asked.clone(),
                price,
                currency: info.currency.clone(),
                pair,
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

// ── Instruments ──────────────────────────────────────────────────────────────

/// TradeStation's asset type in the journal's vocabulary.
fn class_of(asset_type: &str) -> &'static str {
    match asset_type.to_ascii_uppercase().as_str() {
        "STOCK" => "stock",
        "STOCKOPTION" | "INDEXOPTION" => "option",
        "FUTURE" => "future",
        "FOREX" => "forex",
        "CRYPTO" => "crypto",
        _ => "other",
    }
}

/// What an instrument is, asked of TradeStation once per symbol and remembered for the
/// call: a quarter of fills touches a handful of instruments, not one per fill.
async fn instrument(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    cache: &mut HashMap<String, Instrument>,
    symbol: &str,
) -> Instrument {
    if let Some(hit) = cache.get(symbol) {
        return hit.clone();
    }
    let info = fetch_instrument(client, secrets, symbol).await.unwrap_or_default();
    cache.insert(symbol.to_string(), info.clone());
    info
}

async fn fetch_instrument(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    symbol: &str,
) -> Result<Instrument> {
    let body = get(
        client,
        secrets,
        &format!("/v3/marketdata/symbols/{}", super::binance::enc(symbol)),
        &[],
    )
    .await?;
    let row = body
        .get("Symbols")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .cloned()
        .ok_or_else(|| anyhow!("TradeStation does not know the symbol {symbol}"))?;
    Ok(details(&row))
}

/// One symbol-details row as a class, a point value and a currency.
fn details(row: &Value) -> Instrument {
    let point = row
        .get("PriceFormat")
        .map(|f| num(f, "PointValue"))
        .filter(|v| *v > 0.0)
        .unwrap_or(1.0);
    Instrument {
        class: class_of(&str_of(row, "AssetType")).to_string(),
        multiplier: point,
        currency: str_of(row, "Currency").to_uppercase(),
        exchange: str_of(row, "Exchange"),
        name: str_of(row, "Description"),
    }
}

/// What an order settles in: its own currency when it names one, the instrument's otherwise.
fn currency_of(order: &Value, info: &Instrument) -> String {
    match str_of(order, "Currency") {
        c if !c.is_empty() => c.to_uppercase(),
        _ => info.currency.clone(),
    }
}

/// TradeStation reports per-account failures inside a 200, so a partial answer is refused
/// rather than filed as an empty one.
fn refuse_errors(body: &Value) -> Result<()> {
    let Some(errors) = body.get("Errors").and_then(Value::as_array) else {
        return Ok(());
    };
    let Some(first) = errors.first() else {
        return Ok(());
    };
    Err(anyhow!(
        "TradeStation refused account {}: {} {}",
        str_of(first, "AccountID"),
        str_of(first, "Error"),
        str_of(first, "Message"),
    ))
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

pub(crate) fn base(secrets: &HashMap<String, String>) -> &'static str {
    match secrets.get("env").map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "sim" => SIM,
        _ => LIVE,
    }
}

fn env_label(secrets: &HashMap<String, String>) -> &'static str {
    if base(secrets) == SIM { "Simulated" } else { "Live" }
}

fn account_id(secrets: &HashMap<String, String>) -> Result<String> {
    Ok(super::require(secrets, "account_id")?.to_string())
}

/// A bearer token for this credential, minted only when the cached one has run out.
///
/// Shared with the market-data connector, which authenticates the same way against the same
/// key: two connectors on one API key should not each hold their own token.
pub(crate) async fn access_token(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<String> {
    let id = super::require(secrets, "client_id")?;
    let secret = super::require(secrets, "client_secret")?;
    let refresh = super::require(secrets, "refresh_token")?;
    let key = super::tokens::key(&["tradestation", id, refresh]);
    if let Some(t) = super::tokens::get(&key) {
        return Ok(t);
    }
    let res = crate::rate::send(
        "tradestation",
        client.post(format!("{}/oauth/token", super::at(TOKEN_HOST))).form(&[
            ("grant_type", "refresh_token"),
            ("client_id", id),
            ("client_secret", secret),
            ("refresh_token", refresh),
        ]),
    )
    .await
    .context("asking TradeStation for an access token")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "TradeStation refused the refresh token ({status}: {}). Sign in again to issue a \
             new one, and make sure the scopes include offline_access.",
            super::binance::trim_err(&body)
        ));
    }
    let v: Value = serde_json::from_str(&body).context("reading TradeStation's token answer")?;
    let token = str_opt(&v, "access_token").ok_or_else(|| {
        anyhow!("TradeStation returned no access token; sign in again to issue a new refresh token")
    })?;
    // The answer's own lifetime when it gives one, the documented twenty minutes otherwise.
    let ttl = v
        .get("expires_in")
        .and_then(Value::as_u64)
        .map(Duration::from_secs)
        .unwrap_or(TOKEN_TTL);
    super::tokens::put(&key, &token, ttl);
    Ok(token)
}

pub(crate) async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let token = access_token(client, secrets).await?;
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", super::binance::enc(k), super::binance::enc(v)))
        .collect();
    let url = if query.is_empty() {
        format!("{}{path}", super::at(base(secrets)))
    } else {
        format!("{}{path}?{}", super::at(base(secrets)), query.join("&"))
    };
    let res = crate::rate::send(
        "tradestation",
        client.get(&url).header("Authorization", format!("Bearer {token}")),
    )
    .await
    .with_context(|| format!("calling TradeStation {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = super::binance::trim_err(&body);
        if status.as_u16() == 401 {
            // The token this call used is spent or was refused; the next call mints one
            // rather than replaying it.
            if let (Ok(id), Ok(refresh)) = (
                super::require(secrets, "client_id"),
                super::require(secrets, "refresh_token"),
            ) {
                super::tokens::forget(&super::tokens::key(&["tradestation", id, refresh]));
            }
            return Err(anyhow!(
                "TradeStation refused this call ({detail}). Check the refresh token belongs \
                 to the {} environment and that the sign-in granted ReadAccount and MarketData.",
                env_label(secrets).to_ascii_lowercase()
            ));
        }
        return Err(anyhow!("TradeStation {path} answered {status}: {detail}"));
    }
    serde_json::from_str(&body).with_context(|| format!("reading TradeStation's {path} answer"))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn str_opt(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn stamp(v: &Value, key: &str) -> Option<OffsetDateTime> {
    v.get(key)
        .and_then(Value::as_str)
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
}

pub(crate) fn num(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_contract_carries_the_point_value_the_venue_publishes() {
        let future = serde_json::json!({
            "AssetType": "FUTURE", "Currency": "USD", "Exchange": "CME",
            "Description": "E-Mini S&P 500", "Symbol": "ESH26",
            "PriceFormat": {"Format": "Decimal", "Decimals": "2", "PointValue": "50"}
        });
        let i = details(&future);
        assert_eq!(i.class, "future");
        assert_eq!(i.multiplier, 50.0);
        assert_eq!(i.currency, "USD");
        // An option's hundred is read, not assumed: an adjusted contract is not a hundred.
        let opt = serde_json::json!({
            "AssetType": "STOCKOPTION", "Currency": "USD",
            "PriceFormat": {"PointValue": "100"}
        });
        assert_eq!(details(&opt).multiplier, 100.0);
        // A row with no point value is one, which is right for a share.
        let stock = serde_json::json!({"AssetType": "STOCK", "Currency": "USD"});
        let i = details(&stock);
        assert_eq!((i.class.as_str(), i.multiplier), ("stock", 1.0));
    }

    #[test]
    fn asset_types_land_in_the_journals_vocabulary() {
        assert_eq!(class_of("STOCK"), "stock");
        assert_eq!(class_of("stockoption"), "option");
        assert_eq!(class_of("INDEXOPTION"), "option");
        assert_eq!(class_of("FUTURE"), "future");
        // Something new is filed as "other" rather than as a class it might not be.
        assert_eq!(class_of("SOMETHING_NEW"), "other");
    }

    #[test]
    fn a_per_account_refusal_inside_a_200_is_still_a_refusal() {
        let body = serde_json::json!({
            "Orders": [],
            "Errors": [{"AccountID": "123", "Error": "Forbidden", "Message": "no access"}]
        });
        let e = refuse_errors(&body).unwrap_err();
        assert!(format!("{e}").contains("123"));
        assert!(refuse_errors(&serde_json::json!({"Orders": []})).is_ok());
        assert!(refuse_errors(&serde_json::json!({"Orders": [], "Errors": []})).is_ok());
    }

    #[test]
    fn the_environment_is_checked_rather_than_guessed() {
        let sim = HashMap::from([("env".to_string(), "sim".to_string())]);
        assert_eq!(base(&sim), SIM);
        assert_eq!(base(&HashMap::new()), LIVE);
        assert!(TradeStation.validate_config(&sim).is_ok());
        let bad = HashMap::from([("env".to_string(), "paper".to_string())]);
        assert!(TradeStation.validate_config(&bad).is_err());
    }
}
