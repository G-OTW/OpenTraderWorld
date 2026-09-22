//! NinjaTrader account (Trader API, read side only).
//!
//! The REST trading API NinjaTrader runs on the Tradovate platform it acquired, which is
//! why every host and path here says `tradovateapi`. Credentials are the platform login
//! plus the API key pair (`cid` and `sec`) issued with developer access, exchanged for a
//! 90-minute bearer token.
//!
//! **Two sessions per user, and a third closes the oldest.** That is the one operational
//! fact worth knowing: the token is minted once and shared, so this connector holds one
//! session, and a trading application logged in beside it holds the other.
//!
//! What the API's shape forces:
//!   • **Everything is an entity list, not a query.** `fill/list` answers the fills the
//!     session can see, with no date range and no cursor, so the period filters here. How
//!     far back that list reaches is the platform's business and is not published, which is
//!     why `history_days` claims nothing.
//!   • **A fill names no account.** It names its order, and the order names the account, so
//!     the orders are read first and the fills are matched against them.
//!   • **A contract is three hops from its point value.** `contract` gives a maturity,
//!     the maturity gives a product, and the product gives `valuePerPoint` and the currency.
//!     A futures point value is asked for, never assumed: guessing it wrong is a PnL wrong
//!     by the contract size.
//!
//! Only the account role is implemented. NinjaTrader publishes no REST endpoint for
//! historical bars: charts come over its market-data socket, on a separate entitlement and
//! its own framing, so prices are left to a connector that can be verified against a feed.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

const LIVE: &str = "https://live.tradovateapi.com/v1";
const DEMO: &str = "https://demo.tradovateapi.com/v1";
/// A token lasts about ninety minutes; the answer carries the exact moment.
const TOKEN_TTL: Duration = Duration::from_secs(90 * 60);
/// What the app calls itself to the platform. Not a credential.
const APP_ID: &str = "OpenTraderWorld";

pub struct NinjaTrader;

static FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "123456",
        kind: "text",
        required: false,
        help: "Which trading account to read, when the login has more than one. Leave empty \
               and the connector asks; if the login carries several, it lists them and asks \
               you to name one rather than picking.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or demo. The simulation platform is a different host with its own \
               account numbers. Leave empty for live.",
    },
];

static CAP: BrokerCapability = BrokerCapability {
    broker: "ninjatrader",
    label: "NinjaTrader",
    website: "https://ninjatrader.com",
    docs_url: "https://docs.ninjatrader.com/api",
    rate_limit: "The token is minted once every ninety minutes and shared. **Two sessions \
                 per login**: a third closes the oldest, so a trading application on the \
                 same login and this connector are already both of them.",
    key_note: "Your platform username and password plus the API key pair (`cid` and `sec`) \
               from NinjaTrader's developer access. **There is no read-only key**: the same \
               credentials can trade, so treat them as a full-access secret. Nothing here \
               places, changes or cancels an order. The platform answers the fills its \
               session can see and publishes no depth limit, so check the oldest row the \
               first pull returns before relying on it for a tax year.",
    required_secrets: &["username", "password", "cid", "sec"],
    config_fields: FIELDS,
    asset_classes: &["future", "stock", "crypto", "option", "other"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // Futures: a short is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    // Prices are a separate entitlement on a separate socket; see the module note.
    quotes: false,
    testable: true,
};

/// What the platform says a contract is worth and settles in.
#[derive(Clone)]
struct Contract {
    /// The contract's own name, as the platform spells it (`ESH6`).
    name: String,
    /// Journal class, from the product's type.
    class: String,
    /// Point value, from the product rather than from a habit.
    multiplier: f64,
    currency: String,
}

impl Default for Contract {
    fn default() -> Self {
        Contract {
            name: String::new(),
            // An instrument nobody could identify is filed as "other", which is a plain
            // disposal everywhere downstream rather than a guessed class.
            class: "other".into(),
            multiplier: 1.0,
            currency: String::new(),
        }
    }
}

#[async_trait::async_trait]
impl Broker for NinjaTrader {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => {}
            Some(v) if v.is_empty() || v == "live" || v == "demo" => {}
            Some(other) => {
                return Err(anyhow!(
                    "the NinjaTrader environment is \"live\" or \"demo\", not \"{other}\""
                ))
            }
        }
        match config.get("account_id").map(|s| s.trim()) {
            None | Some("") => Ok(()),
            Some(id) if id.chars().all(|c| c.is_ascii_digit()) => Ok(()),
            Some(other) => Err(anyhow!(
                "a NinjaTrader account id is a number, not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let accounts = accounts(client, secrets).await?;
        let picked = pick_account(&accounts, secrets)?;
        Ok(format!(
            "{} sign-in accepted, reading account {} of [{}]",
            env_label(secrets),
            accounts
                .iter()
                .find(|(id, _)| *id == picked)
                .map(|(id, name)| format!("{name} ({id})"))
                .unwrap_or_else(|| picked.to_string()),
            accounts
                .iter()
                .map(|(i, n)| format!("{n} ({i})"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    /// Every fill of the window on the chosen account, oldest first.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let account = pick_account(&accounts(client, secrets).await?, secrets)?;
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        // A fill names its order, and only the order names the account.
        let order_account = order_accounts(client, secrets).await?;
        let fees = fill_fees(client, secrets).await.unwrap_or_default();
        let rows = get(client, secrets, "/fill/list", &[]).await?;
        let mut known: HashMap<i64, Contract> = HashMap::new();
        let mut out = Vec::new();
        for f in rows.as_array().into_iter().flatten() {
            let order_id = int(f, "orderId");
            if order_account.get(&order_id).map(String::as_str) != Some(account.as_str()) {
                continue;
            }
            let Some(at) = stamp(f, "timestamp") else { continue };
            if at < q.from || at > q.to {
                continue;
            }
            let id = int(f, "id");
            let c = contract(client, secrets, &mut known, int(f, "contractId")).await;
            if !wanted.is_empty() && !wanted.contains(&c.name.to_uppercase()) {
                continue;
            }
            out.push(Execution {
                id: id.to_string(),
                order_id: order_id.to_string(),
                at,
                symbol: c.name.clone(),
                side: if str_of(f, "action").eq_ignore_ascii_case("sell") {
                    "sell".into()
                } else {
                    "buy".into()
                },
                qty: num(f, "qty"),
                price: num(f, "price"),
                // Every charge the platform books against the fill, added up as one cost.
                fee: fees.get(&id).copied().unwrap_or(0.0),
                fee_currency: c.currency.clone(),
                currency: c.currency.clone(),
                asset_class: c.class.clone(),
                multiplier: c.multiplier,
                venue: "NinjaTrader".into(),
                account: account.clone(),
            });
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn positions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        let account = pick_account(&accounts(client, secrets).await?, secrets)?;
        let rows = get(client, secrets, "/position/list", &[]).await?;
        let mut known: HashMap<i64, Contract> = HashMap::new();
        let mut out = Vec::new();
        for p in rows.as_array().into_iter().flatten() {
            if int(p, "accountId").to_string() != account {
                continue;
            }
            let net = num(p, "netPos");
            if net == 0.0 {
                continue;
            }
            let c = contract(client, secrets, &mut known, int(p, "contractId")).await;
            out.push(Position {
                symbol: c.name.clone(),
                // The sign of the net position is the side; the platform has no other field.
                side: if net < 0.0 { "short".into() } else { "long".into() },
                qty: net.abs(),
                avg_price: Some(num(p, "netPrice")).filter(|v| *v > 0.0),
                currency: c.currency.clone(),
                asset_class: c.class.clone(),
                multiplier: c.multiplier,
                venue: "NinjaTrader".into(),
                account: account.clone(),
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
        let account = pick_account(&accounts(client, secrets).await?, secrets)?;
        let rows = get(client, secrets, "/order/list", &[]).await?;
        // Price and quantity live on the order's latest version, not on the order.
        let versions = order_versions(client, secrets).await.unwrap_or_default();
        let mut known: HashMap<i64, Contract> = HashMap::new();
        let mut out = Vec::new();
        for o in rows.as_array().into_iter().flatten() {
            if int(o, "accountId").to_string() != account {
                continue;
            }
            // Only what is still working: everything else is history.
            if !matches!(str_of(o, "ordStatus").as_str(), "Working" | "PendingNew" | "PendingReplace")
            {
                continue;
            }
            let id = int(o, "id");
            let v = versions.get(&id).cloned().unwrap_or(Value::Null);
            let c = contract(client, secrets, &mut known, int(o, "contractId")).await;
            out.push(OpenOrder {
                id: id.to_string(),
                symbol: c.name.clone(),
                side: if str_of(o, "action").eq_ignore_ascii_case("sell") {
                    "sell".into()
                } else {
                    "buy".into()
                },
                order_type: str_of(&v, "orderType"),
                qty: num(&v, "orderQty"),
                filled_qty: 0.0,
                limit_price: Some(num(&v, "price")).filter(|p| *p > 0.0),
                stop_price: Some(num(&v, "stopPrice")).filter(|p| *p > 0.0),
                currency: c.currency.clone(),
                asset_class: c.class.clone(),
                placed_at: stamp(o, "timestamp"),
                venue: "NinjaTrader".into(),
                account: account.clone(),
            });
        }
        Ok(out)
    }

    /// The open positions read as a balance sheet. A futures book owns positions, not
    /// assets: the cash balance is margin, not a line anyone reconciles against a price.
    async fn holdings(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        Ok(self
            .positions(client, secrets)
            .await?
            .into_iter()
            .map(|p| Holding {
                symbol: p.symbol,
                name: String::new(),
                qty: p.qty,
                side: p.side,
                asset_class: p.asset_class,
                avg_price: p.avg_price,
                currency: p.currency,
                venue: p.venue,
                account: p.account,
            })
            .collect())
    }

    /// The contracts this account is in or is working, for a symbol picker.
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

    async fn quotes(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        _symbols: &[String],
    ) -> Result<Vec<Quote>> {
        Err(anyhow!(
            "NinjaTrader serves prices through its market-data feed, which is a separate \
             entitlement on its own socket rather than part of the trading API. Use a data \
             connector to price these lines."
        ))
    }
}

// ── Accounts ─────────────────────────────────────────────────────────────────

/// The trading accounts this login carries, as (id, name).
async fn accounts(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<Vec<(String, String)>> {
    let rows = get(client, secrets, "/account/list", &[]).await?;
    Ok(rows
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| !a.get("closed").and_then(Value::as_bool).unwrap_or(false))
        .map(|a| (int(a, "id").to_string(), str_of(a, "name")))
        .filter(|(id, _)| id != "0")
        .collect())
}

/// Which account to read: the setting when there is one, the only one when the login has
/// exactly one, and an error listing them otherwise. A login with several has no obvious
/// main account, and picking would file someone else's trades.
fn pick_account(
    accounts: &[(String, String)],
    secrets: &HashMap<String, String>,
) -> Result<String> {
    if let Some(id) = secrets.get("account_id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if !accounts.iter().any(|(known, _)| known == id) {
            return Err(anyhow!(
                "this login does not carry account {id}. It has [{}].",
                accounts.iter().map(|(i, n)| format!("{n} ({i})")).collect::<Vec<_>>().join(", ")
            ));
        }
        return Ok(id.to_string());
    }
    match accounts {
        [(id, _)] => Ok(id.clone()),
        [] => Err(anyhow!("this NinjaTrader login carries no open trading account")),
        many => Err(anyhow!(
            "this login carries {} accounts, so name one in the Account ID setting: [{}]",
            many.len(),
            many.iter().map(|(i, n)| format!("{n} ({i})")).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Order id → the account it belongs to. A fill names only its order.
async fn order_accounts(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<HashMap<i64, String>> {
    let rows = get(client, secrets, "/order/list", &[]).await?;
    Ok(rows
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| (int(o, "id"), int(o, "accountId").to_string()))
        .collect())
}

/// Order id → its latest version, which is where a working order's price and size live.
async fn order_versions(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<HashMap<i64, Value>> {
    let rows = get(client, secrets, "/orderVersion/list", &[]).await?;
    let mut out: HashMap<i64, Value> = HashMap::new();
    for v in rows.as_array().into_iter().flatten() {
        let order_id = int(v, "orderId");
        // Versions are numbered in the order they were made; the highest id is current.
        let keep = out
            .get(&order_id)
            .map(|prev| int(v, "id") > int(prev, "id"))
            .unwrap_or(true);
        if keep {
            out.insert(order_id, v.clone());
        }
    }
    Ok(out)
}

/// Fill id → every charge the platform booked on it, added up.
async fn fill_fees(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<HashMap<i64, f64>> {
    let rows = get(client, secrets, "/fillFee/list", &[]).await?;
    Ok(rows
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| (int(f, "id"), total_fee(f)))
        .collect())
}

/// Every fee line on a fill as one positive cost. The platform books them separately
/// (clearing, exchange, NFA, brokerage, IP, routing, commission) and a trade that showed
/// only the commission would understate what it cost.
fn total_fee(f: &Value) -> f64 {
    [
        "clearingFee",
        "exchangeFee",
        "nfaFee",
        "brokerageFee",
        "ipFee",
        "commission",
        "orderRoutingFee",
    ]
    .iter()
    .map(|k| num(f, k).abs())
    .sum()
}

// ── Contracts ────────────────────────────────────────────────────────────────

/// What a contract is, asked of the platform once per contract and remembered for the call.
async fn contract(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    cache: &mut HashMap<i64, Contract>,
    contract_id: i64,
) -> Contract {
    if let Some(hit) = cache.get(&contract_id) {
        return hit.clone();
    }
    let c = fetch_contract(client, secrets, contract_id).await.unwrap_or_default();
    cache.insert(contract_id, c.clone());
    c
}

/// Three hops: the contract names a maturity, the maturity names a product, and the product
/// is the only place the point value and the currency are written down.
async fn fetch_contract(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    contract_id: i64,
) -> Result<Contract> {
    let c = get(client, secrets, "/contract/item", &[("id".into(), contract_id.to_string())])
        .await?;
    let name = str_of(&c, "name");
    let maturity = get(
        client,
        secrets,
        "/contractMaturity/item",
        &[("id".into(), int(&c, "contractMaturityId").to_string())],
    )
    .await?;
    let product = get(
        client,
        secrets,
        "/product/item",
        &[("id".into(), int(&maturity, "productId").to_string())],
    )
    .await?;
    let currency = get(
        client,
        secrets,
        "/currency/item",
        &[("id".into(), int(&product, "currencyId").to_string())],
    )
    .await
    .map(|c| str_of(&c, "name"))
    .unwrap_or_default();
    Ok(Contract {
        name,
        class: class_of(&str_of(&product, "productType")).into(),
        // A futures point value is the product's, never the usual one for that root.
        multiplier: Some(num(&product, "valuePerPoint")).filter(|v| *v > 0.0).unwrap_or(1.0),
        currency: currency.to_uppercase(),
    })
}

/// The platform's product type in the journal's vocabulary.
fn class_of(product_type: &str) -> &'static str {
    match product_type {
        "Futures" | "Continuous" => "future",
        "CommonStock" => "stock",
        "Cryptocurrency" => "crypto",
        "Options" => "option",
        // A spread, a swap or a market internal is none of the journal's classes.
        _ => "other",
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

fn base(secrets: &HashMap<String, String>) -> &'static str {
    match secrets.get("env").map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "demo" => DEMO,
        _ => LIVE,
    }
}

fn env_label(secrets: &HashMap<String, String>) -> &'static str {
    if base(secrets) == DEMO { "Demo" } else { "Live" }
}

/// A bearer token for this login, minted only when the cached one has run out.
///
/// The platform allows **two sessions per user** and closes the oldest when a third opens,
/// so a token is shared rather than requested per call: minting one each time would knock
/// the user's own trading application off.
async fn access_token(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<String> {
    let name = super::require(secrets, "username")?;
    let password = super::require(secrets, "password")?;
    let cid = super::require(secrets, "cid")?;
    let sec = super::require(secrets, "sec")?;
    let key = super::tokens::key(&["ninjatrader", base(secrets), name, cid]);
    if let Some(t) = super::tokens::get(&key) {
        return Ok(t);
    }
    let res = crate::rate::send(
        "ninjatrader",
        client
            .post(format!("{}/auth/accesstokenrequest", super::at(base(secrets))))
            .json(&json!({
                "name": name,
                "password": password,
                "appId": APP_ID,
                "appVersion": env!("CARGO_PKG_VERSION"),
                "cid": cid.parse::<i64>().unwrap_or_default(),
                "sec": sec,
            })),
    )
    .await
    .context("asking NinjaTrader for an access token")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "NinjaTrader refused the sign-in ({status}: {}). Check the username, the password \
             and the API key pair, and that the key belongs to the {} environment.",
            super::binance::trim_err(&body),
            env_label(secrets).to_ascii_lowercase()
        ));
    }
    let v: Value = serde_json::from_str(&body).context("reading NinjaTrader's token answer")?;
    // A 200 can still carry a refusal: a login awaiting terms or a captcha answers here.
    let token = v
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "NinjaTrader returned no access token ({}). The login may need to accept new \
                 terms on the platform first.",
                str_of(&v, "errorText")
            )
        })?;
    let ttl = stamp(&v, "expirationTime")
        .and_then(|exp| (exp - OffsetDateTime::now_utc()).try_into().ok())
        .unwrap_or(TOKEN_TTL);
    super::tokens::put(&key, token, ttl);
    Ok(token.to_string())
}

async fn get(
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
        "ninjatrader",
        client.get(&url).header("Authorization", format!("Bearer {token}")),
    )
    .await
    .with_context(|| format!("calling NinjaTrader {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = super::binance::trim_err(&body);
        if matches!(status.as_u16(), 401 | 403) {
            // The token is spent, or another session took the seat; the next call signs in
            // again rather than replaying it.
            if let (Ok(name), Ok(cid)) = (
                super::require(secrets, "username"),
                super::require(secrets, "cid"),
            ) {
                super::tokens::forget(&super::tokens::key(&[
                    "ninjatrader",
                    base(secrets),
                    name,
                    cid,
                ]));
            }
            return Err(anyhow!(
                "NinjaTrader ended this session ({detail}). The platform allows two sessions \
                 per login and closes the oldest when a third opens, so check what else is \
                 signed in."
            ));
        }
        return Err(anyhow!("NinjaTrader {path} answered {status}: {detail}"));
    }
    serde_json::from_str(&body).with_context(|| format!("reading NinjaTrader's {path} answer"))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn int(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn num(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn stamp(v: &Value, key: &str) -> Option<OffsetDateTime> {
    v.get(key)
        .and_then(Value::as_str)
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fee_line_counts_and_a_missing_one_is_zero() {
        let f = serde_json::json!({
            "id": 7, "clearingFee": 0.11, "exchangeFee": 1.33, "nfaFee": 0.02,
            "brokerageFee": 0.5, "commission": -0.25
        });
        // The platform books seven separate charges; a trade showing only the commission
        // would understate what it cost.
        assert!((total_fee(&f) - 2.21).abs() < 1e-9);
        assert_eq!(total_fee(&serde_json::json!({"id": 1})), 0.0);
    }

    #[test]
    fn product_types_land_in_the_journals_vocabulary() {
        assert_eq!(class_of("Futures"), "future");
        assert_eq!(class_of("CommonStock"), "stock");
        assert_eq!(class_of("Cryptocurrency"), "crypto");
        assert_eq!(class_of("Options"), "option");
        // A spread is none of them, and filing it as a future would give it a point value
        // it has not got.
        assert_eq!(class_of("Spread"), "other");
        assert_eq!(class_of("MarketInternals"), "other");
    }

    #[test]
    fn the_current_version_of_an_order_is_the_highest_numbered_one() {
        let rows = serde_json::json!([
            {"orderId": 5, "id": 10, "orderQty": 1, "orderType": "Limit", "price": 100.0},
            {"orderId": 5, "id": 12, "orderQty": 2, "orderType": "Limit", "price": 101.0},
            {"orderId": 6, "id": 11, "orderQty": 3, "orderType": "Stop", "stopPrice": 99.0}
        ]);
        let mut out: HashMap<i64, Value> = HashMap::new();
        for v in rows.as_array().unwrap() {
            let order_id = int(v, "orderId");
            let keep = out.get(&order_id).map(|p| int(v, "id") > int(p, "id")).unwrap_or(true);
            if keep {
                out.insert(order_id, v.clone());
            }
        }
        assert_eq!(num(&out[&5], "price"), 101.0);
        assert_eq!(num(&out[&5], "orderQty"), 2.0);
        assert_eq!(num(&out[&6], "stopPrice"), 99.0);
    }

    #[test]
    fn several_accounts_are_listed_rather_than_picked_from() {
        let two = vec![("1".to_string(), "APEX-1".to_string()), ("2".into(), "DEMO".into())];
        let e = pick_account(&two, &HashMap::new()).unwrap_err();
        assert!(format!("{e}").contains("APEX-1") && format!("{e}").contains("DEMO"));
        let named = HashMap::from([("account_id".to_string(), "2".to_string())]);
        assert_eq!(pick_account(&two, &named).unwrap(), "2");
        let wrong = HashMap::from([("account_id".to_string(), "9".to_string())]);
        assert!(pick_account(&two, &wrong).is_err());
        assert_eq!(pick_account(&two[..1], &HashMap::new()).unwrap(), "1");
    }
}
