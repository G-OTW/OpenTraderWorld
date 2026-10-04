//! FOREX.com / StoneX Trading API account (session credentials, read side only).
//!
//! The odd authentication of the folder: there is no API key that stands on its own. A
//! session is opened with the account's **username and password** plus the AppKey StoneX
//! issues to a signed-up application, and every later call carries that session token in
//! two headers. So this is not a read-only credential and the form says so: the same login
//! can trade, and the only thing keeping this connector read-only is that it never asks.
//!
//! What the API's shape forces:
//!   • **A session is a resource, not a signature.** It lasts about twenty minutes, and
//!     every log-on opens another one on the account, so the token is minted once and
//!     shared by every call on the same credential.
//!   • **The trade history has no end date and no cursor.** `tradehistory` takes a `from`
//!     and returns at most 200 rows, so the period's end is applied here and the walk moves
//!     `from` forward past the last fill it read.
//!   • **The prices are spread-based.** StoneX bills the spread rather than a commission on
//!     an FX or CFD trade, so an imported trade carries no fee, which is right for a
//!     spread-only account and short of the truth for one that is charged commission.
//!   • **An instrument is a number.** A market has an id and a name (`EUR/USD`); the id is
//!     what the API takes and the name is what a human reads, so both travel together.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

pub(crate) const BASE: &str = "https://ciapi.cityindex.com/TradingAPI";
/// A session is guaranteed for twenty minutes.
const SESSION_TTL: Duration = Duration::from_secs(20 * 60);
/// Rows one `tradehistory` page returns at most; the endpoint's own ceiling.
const PAGE: usize = 200;
/// Hard stop on the walk, so a mis-specified window cannot loop.
const MAX_PAGES: usize = 200;

pub struct ForexCom;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "trading_account_id",
    label: "Trading account id",
    placeholder: "400123456",
    kind: "text",
    required: false,
    help: "Which trading account to read, when the login has more than one. Leave empty and \
           the connector asks StoneX; if the login carries several, it lists them and asks \
           you to name one rather than picking.",
}];

static CAP: BrokerCapability = BrokerCapability {
    broker: "forexcom",
    label: "FOREX.com (StoneX)",
    website: "https://www.forex.com",
    docs_url: "https://docs.labs.gaincapital.com/",
    rate_limit: "500 requests per 5 seconds. The session token is minted once every twenty \
                 minutes and shared; trade history pages 200 rows at a time.",
    key_note: "Your account **username and password**, plus the AppKey StoneX issues once \
               its API terms are signed. **StoneX has no read-only credential**: this login \
               can also trade, so treat it as a full-access secret and change the password \
               when you remove the account. Nothing here places, changes or cancels an \
               order. FX and CFD trades are priced on the spread, so an imported trade \
               carries no commission.",
    required_secrets: &["username", "password", "app_key"],
    config_fields: FIELDS,
    // A currency pair is forex; an index, commodity or share CFD has no journal class of
    // its own, so it is filed as "other" rather than as one it is not.
    asset_classes: &["forex", "other"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    // Margin venue: a sell with nothing open really is a short.
    spot_only: false,
    positions: true,
    orders: true,
    holdings: true,
    // Pricing a market needs its id, which a bare asset name does not carry; see the note
    // in the connector docs.
    quotes: false,
    testable: true,
};

#[async_trait::async_trait]
impl Broker for ForexCom {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("trading_account_id").map(|s| s.trim()) {
            None | Some("") => Ok(()),
            Some(id) if id.chars().all(|c| c.is_ascii_digit()) => Ok(()),
            Some(other) => Err(anyhow!(
                "a StoneX trading account id is a number, not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let accounts = account_tree(client, secrets).await?;
        let picked = pick_account(&accounts, secrets)?;
        let all: Vec<String> = accounts
            .trading
            .iter()
            .map(|(id, name)| format!("{name} ({id})"))
            .collect();
        Ok(format!(
            "signed in as client {}, reading trading account {picked} of [{}]",
            accounts.client_id,
            all.join(", ")
        ))
    }

    /// Every trade of the window, oldest first.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let account = pick_account(&account_tree(client, secrets).await?, secrets)?;
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        let mut out: Vec<Execution> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut from = q.from.unix_timestamp();
        for _ in 0..MAX_PAGES {
            let body = get(
                client,
                secrets,
                "/order/tradehistory",
                &[
                    ("TradingAccountId".into(), account.clone()),
                    ("maxResults".into(), PAGE.to_string()),
                    ("from".into(), from.to_string()),
                ],
            )
            .await?;
            let rows = body
                .get("TradeHistory")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut newest = from;
            for r in &rows {
                let Some(at) = wcf_date(r, "ExecutedDateTimeUtc") else { continue };
                newest = newest.max(at.unix_timestamp());
                if at < q.from || at > q.to {
                    continue;
                }
                let symbol = str_of(r, "MarketName").to_uppercase();
                if symbol.is_empty() || (!wanted.is_empty() && !wanted.contains(&symbol)) {
                    continue;
                }
                let id = num(r, "OrderId").to_string();
                // The endpoint filters on "changed since", so a row already read can come
                // back on the next page; the order id is what tells them apart.
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id.clone());
                out.push(trade(r, &symbol, &id, at, &account));
            }
            if rows.len() < PAGE {
                break;
            }
            // No cursor: the walk moves past the newest row it has read. A page that does
            // not advance the clock would repeat forever, so it ends the walk.
            if newest <= from {
                break;
            }
            from = newest;
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn positions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        let account = pick_account(&account_tree(client, secrets).await?, secrets)?;
        let body = get(
            client,
            secrets,
            "/order/openpositions",
            &[("TradingAccountId".into(), account.clone())],
        )
        .await?;
        let mut out = Vec::new();
        for p in body.get("OpenPositions").and_then(Value::as_array).into_iter().flatten() {
            let qty = num(p, "Quantity");
            if qty == 0.0 {
                continue;
            }
            let symbol = str_of(p, "MarketName").to_uppercase();
            out.push(Position {
                side: if str_of(p, "Direction").eq_ignore_ascii_case("sell") {
                    "short".into()
                } else {
                    "long".into()
                },
                qty: qty.abs(),
                avg_price: Some(num(p, "Price")).filter(|v| *v > 0.0),
                currency: str_of(p, "Currency").to_uppercase(),
                asset_class: class_of(&symbol).into(),
                multiplier: 1.0,
                venue: "FOREX.com".into(),
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
        let account = pick_account(&account_tree(client, secrets).await?, secrets)?;
        let body = get(
            client,
            secrets,
            "/order/activestoplimitorders",
            &[("TradingAccountId".into(), account.clone())],
        )
        .await?;
        let mut out = Vec::new();
        for o in body
            .get("ActiveStopLimitOrders")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let symbol = str_of(o, "MarketName").to_uppercase();
            // StoneX numbers its order types: 1 is a stop, 2 a limit. The trigger is one
            // field, so which price it is comes from the type rather than from the price.
            let kind = num(o, "Type") as i64;
            let trigger = Some(num(o, "TriggerPrice")).filter(|v| *v > 0.0);
            out.push(OpenOrder {
                id: num(o, "OrderId").to_string(),
                side: if str_of(o, "Direction").eq_ignore_ascii_case("sell") {
                    "sell".into()
                } else {
                    "buy".into()
                },
                order_type: match kind {
                    1 => "stop".into(),
                    2 => "limit".into(),
                    other => format!("type {other}"),
                },
                qty: num(o, "Quantity"),
                filled_qty: 0.0,
                limit_price: (kind == 2).then_some(trigger).flatten(),
                stop_price: (kind != 2).then_some(trigger).flatten(),
                currency: str_of(o, "Currency").to_uppercase(),
                asset_class: class_of(&symbol).into(),
                placed_at: wcf_date(o, "CreatedDateTimeUTC"),
                venue: "FOREX.com".into(),
                account: account.clone(),
                symbol,
            });
        }
        Ok(out)
    }

    /// The open positions read as a balance sheet. A margin book owns positions, not assets:
    /// the cash balance is collateral, not a line anyone reconciles against a price.
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

    /// The markets this account is in or is working, for a symbol picker.
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
            "FOREX.com prices a market by its numeric id rather than by its name, and an \
             asset in a portfolio carries a name. Use a data connector to price these lines."
        ))
    }
}

// ── Mapping ──────────────────────────────────────────────────────────────────

fn trade(
    r: &Value,
    symbol: &str,
    id: &str,
    at: OffsetDateTime,
    account: &str,
) -> Execution {
    Execution {
        id: id.to_string(),
        order_id: id.to_string(),
        at,
        side: if str_of(r, "Direction").eq_ignore_ascii_case("sell") {
            "sell".into()
        } else {
            "buy".into()
        },
        // The original quantity, before part closures: what was actually executed here.
        qty: Some(num(r, "OriginalQuantity"))
            .filter(|q| *q > 0.0)
            .unwrap_or_else(|| num(r, "Quantity"))
            .abs(),
        price: num(r, "Price"),
        // StoneX bills the spread rather than a commission; see the capability note.
        fee: 0.0,
        fee_currency: String::new(),
        currency: str_of(r, "Currency").to_uppercase(),
        asset_class: class_of(symbol).into(),
        multiplier: 1.0,
        venue: "FOREX.com".into(),
        account: account.to_string(),
        symbol: symbol.to_string(),
    }
}

/// The journal class of a market, from the only thing StoneX puts on the row: its name.
///
/// A cash pair is written `EUR/USD`, two three-letter codes and a slash, which is the
/// venue's own notation rather than a guess at a shape. Everything else is a CFD on an
/// index, a commodity or a share, and none of those is a class the journal has, so they are
/// filed as "other" instead of being dressed up as an equity.
fn class_of(market_name: &str) -> &'static str {
    match market_name.split_once('/') {
        Some((base, quote))
            if base.len() == 3
                && quote.len() == 3
                && base.chars().all(|c| c.is_ascii_alphabetic())
                && quote.chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            "forex"
        }
        _ => "other",
    }
}

/// The client account and the trading accounts under it.
pub(crate) struct Accounts {
    pub client_id: String,
    /// (id, name) per trading account.
    pub trading: Vec<(String, String)>,
}

pub(crate) async fn account_tree(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<Accounts> {
    let body = get(client, secrets, "/useraccount/ClientAndTradingAccount", &[]).await?;
    let client_id = num(&body, "ClientAccountId").to_string();
    let trading = body
        .get("TradingAccounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| (num(a, "TradingAccountId").to_string(), str_of(a, "TradingAccountCode")))
        .filter(|(id, _)| id != "0")
        .collect();
    Ok(Accounts { client_id, trading })
}

/// Which trading account to read: the setting when there is one, the only one when the
/// login has exactly one, and an error listing them otherwise. A login with several
/// accounts has no obvious "main" one, and picking would file someone else's trades.
fn pick_account(accounts: &Accounts, secrets: &HashMap<String, String>) -> Result<String> {
    if let Some(id) = secrets
        .get("trading_account_id")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if !accounts.trading.iter().any(|(known, _)| known == id) {
            return Err(anyhow!(
                "this login does not carry trading account {id}. It has [{}].",
                accounts
                    .trading
                    .iter()
                    .map(|(i, n)| format!("{n} ({i})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return Ok(id.to_string());
    }
    match accounts.trading.as_slice() {
        [(id, _)] => Ok(id.clone()),
        [] => Err(anyhow!("this FOREX.com login carries no trading account")),
        many => Err(anyhow!(
            "this login carries {} trading accounts, so name one in the Trading account id \
             setting: [{}]",
            many.len(),
            many.iter().map(|(i, n)| format!("{n} ({i})")).collect::<Vec<_>>().join(", ")
        )),
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

/// A session token for this login, minted only when the cached one has run out.
///
/// Shared with the market-data connector, which authenticates the same way: every log-on
/// opens a session on the account, so two connectors on one login share one.
pub(crate) async fn session(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<String> {
    let user = super::require(secrets, "username")?;
    let password = super::require(secrets, "password")?;
    let app_key = super::require(secrets, "app_key")?;
    let key = super::tokens::key(&["forexcom", user, app_key]);
    if let Some(t) = super::tokens::get(&key) {
        return Ok(t);
    }
    let res = crate::rate::send(
        "forexcom",
        client.post(format!("{}/session", super::at(BASE))).json(&json!({
            "UserName": user,
            "Password": password,
            "AppKey": app_key,
            "AppVersion": "1",
            "AppComments": "",
        })),
    )
    .await
    .context("opening a FOREX.com session")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "FOREX.com refused the sign-in ({status}: {}). Check the username, the password \
             and the AppKey, and that the account is enabled for API access.",
            super::binance::trim_err(&body)
        ));
    }
    let v: Value = serde_json::from_str(&body).context("reading FOREX.com's sign-in answer")?;
    let token = v
        .get("Session")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("FOREX.com returned no session token for this login"))?;
    super::tokens::put(&key, token, SESSION_TTL);
    Ok(token.to_string())
}

pub(crate) async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let user = super::require(secrets, "username")?;
    let token = session(client, secrets).await?;
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", super::binance::enc(k), super::binance::enc(v)))
        .collect();
    let url = if query.is_empty() {
        format!("{}{path}", super::at(BASE))
    } else {
        format!("{}{path}?{}", super::at(BASE), query.join("&"))
    };
    let res = crate::rate::send(
        "forexcom",
        client
            .get(&url)
            // The two headers StoneX authenticates on. Never the query string: that copy of
            // the token would end up in every proxy log on the way.
            .header("UserName", user)
            .header("Session", &token),
    )
    .await
    .with_context(|| format!("calling FOREX.com {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        if status.as_u16() == 401 {
            // The session has expired or been closed elsewhere; the next call opens a new
            // one rather than replaying this token.
            if let (Ok(u), Ok(k)) = (
                super::require(secrets, "username"),
                super::require(secrets, "app_key"),
            ) {
                super::tokens::forget(&super::tokens::key(&["forexcom", u, k]));
            }
            return Err(anyhow!(
                "FOREX.com ended this session. Signing in again on another device closes the \
                 one here; retry, and check the password if it keeps happening."
            ));
        }
        return Err(anyhow!(
            "FOREX.com {path} answered {status}: {}",
            super::binance::trim_err(&body)
        ));
    }
    serde_json::from_str(&body).with_context(|| format!("reading FOREX.com's {path} answer"))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

pub(crate) fn num(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// StoneX writes a date as `/Date(1289231327280)/`, milliseconds since the epoch, sometimes
/// with a trailing offset (`/Date(1289231327280+0000)/`) that says nothing: the docs state
/// every time is UTC.
pub(crate) fn wcf_date(v: &Value, key: &str) -> Option<OffsetDateTime> {
    parse_wcf(v.get(key)?.as_str()?)
}

pub(crate) fn parse_wcf(raw: &str) -> Option<OffsetDateTime> {
    let inner = raw.trim().strip_prefix("/Date(")?.strip_suffix(")/")?;
    let digits: String = inner
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    let ms: i64 = digits.parse().ok()?;
    OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stonex_date_is_milliseconds_wrapped_in_a_string() {
        let t = parse_wcf("/Date(1289231327280)/").unwrap();
        assert_eq!(t.unix_timestamp_nanos(), 1_289_231_327_280_000_000);
        // The trailing offset is decoration: the docs say every time is already UTC.
        assert_eq!(parse_wcf("/Date(1289231327280+0000)/").unwrap(), t);
        assert!(parse_wcf("2010-11-08T13:08:47Z").is_none());
        assert!(parse_wcf("/Date(nonsense)/").is_none());
    }

    #[test]
    fn a_cash_pair_is_forex_and_a_cfd_is_not_dressed_up_as_one() {
        assert_eq!(class_of("EUR/USD"), "forex");
        assert_eq!(class_of("GBP/JPY"), "forex");
        // An index or commodity CFD has no journal class of its own.
        assert_eq!(class_of("UK 100"), "other");
        assert_eq!(class_of("GOLD"), "other");
        // A share CFD written with a slash is not two currency codes.
        assert_eq!(class_of("APPLE INC/CFD"), "other");
    }

    #[test]
    fn several_trading_accounts_are_listed_rather_than_picked_from() {
        let two = Accounts {
            client_id: "1".into(),
            trading: vec![("400".into(), "CFD".into()), ("401".into(), "SB".into())],
        };
        let e = pick_account(&two, &HashMap::new()).unwrap_err();
        assert!(format!("{e}").contains("400") && format!("{e}").contains("401"));
        // Named, it is used; named wrongly, the error lists what the login does carry.
        let named = HashMap::from([("trading_account_id".to_string(), "401".to_string())]);
        assert_eq!(pick_account(&two, &named).unwrap(), "401");
        let wrong = HashMap::from([("trading_account_id".to_string(), "999".to_string())]);
        assert!(pick_account(&two, &wrong).is_err());
        // One account needs no setting.
        let one = Accounts { client_id: "1".into(), trading: vec![("400".into(), "CFD".into())] };
        assert_eq!(pick_account(&one, &HashMap::new()).unwrap(), "400");
    }

    #[test]
    fn a_trade_keeps_its_side_and_carries_no_invented_fee() {
        let r = serde_json::json!({
            "OrderId": 12345.0, "MarketId": 401484347.0, "MarketName": "EUR/USD",
            "Direction": "sell", "OriginalQuantity": 10000.0, "Quantity": 5000.0,
            "Price": 1.0812, "Currency": "USD"
        });
        let at = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let e = trade(&r, "EUR/USD", "12345", at, "400");
        assert_eq!(e.side, "sell");
        // The original quantity is what was executed; the current one is what is left open.
        assert_eq!(e.qty, 10000.0);
        assert_eq!(e.asset_class, "forex");
        assert_eq!(e.fee, 0.0);
        assert!(e.fee_currency.is_empty());
    }
}
