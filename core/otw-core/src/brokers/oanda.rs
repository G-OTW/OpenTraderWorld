//! OANDA v20 account (personal access token, read side only).
//!
//! Auth is one bearer token and nothing else, which makes it the second simplest connector
//! here. What costs something instead is the shape of its history:
//!
//!   - **Fills are transactions, and transactions come back in two calls.** `GET
//!     /v3/accounts/{id}/transactions` answers a list of *page URLs*, not rows; each page is
//!     an `idrange` query that has to be fetched in turn. The window it accepts is capped at
//!     **365 days**, so a wider period is walked one year at a time.
//!   - **The environment is a host, not a flag.** fxPractice and fxTrade are separate
//!     services with separate tokens and separate account ids, so a practice token against
//!     the live host is a 401 rather than thinner data.
//!   - **An instrument names its own quote currency.** `EUR_USD`, `SPX500_USD` and `XAU_USD`
//!     are all `base_quote` in OANDA's own notation, so the settlement currency is read off
//!     the name rather than looked up. What the instrument *is* (`CURRENCY`, `CFD`, `METAL`)
//!     is asked of the account's instrument list, never inferred from the ticker.
//!
//! OANDA is a margin venue: a short is a short, and `spot_only` is false.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

const LIVE: &str = "https://api-fxtrade.oanda.com";
const PRACTICE: &str = "https://api-fxpractice.oanda.com";
/// Widest window `GET /transactions` accepts, in days. A longer period is cut into slices.
const MAX_WINDOW_DAYS: i64 = 365;
/// Transactions per transaction page, which is also OANDA's maximum.
const PAGE: usize = 1000;
/// Hard stop on the page walk, so a mis-specified window cannot loop.
const MAX_PAGES: usize = 500;

pub struct Oanda;

static FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "001-004-1234567-001",
        kind: "text",
        required: true,
        help: "The v20 account number, as OANDA writes it on the account page. One OANDA \
               account per broker account here: the token may reach several, and which one \
               is read has to be said rather than picked.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or practice. fxTrade and fxPractice are different hosts with different \
               tokens and different account numbers, so this is stated, not guessed. Leave \
               empty for live.",
    },
];

static CAP: BrokerCapability = BrokerCapability {
    broker: "oanda",
    label: "OANDA",
    website: "https://www.oanda.com",
    docs_url: "https://developer.oanda.com/rest-live-v20/transaction-ep/",
    rate_limit: "120 requests per second per token, 30 per second on the instrument list and \
                 the transaction id-range pages. A year of fills is a handful of calls.",
    key_note: "A personal access token from *Manage API Access*. **OANDA issues no read-only \
               token**: the same token that reads this account can also trade it, so treat it \
               as a full-access credential and revoke it when the account is removed. Nothing \
               here places, changes or cancels an order. Commission and the guaranteed-stop \
               fee are billed in the account's own currency, not the instrument's.",
    required_secrets: &["api_token"],
    config_fields: FIELDS,
    // CURRENCY is forex; a CFD or a metal has no journal class of its own, so it is filed as
    // "other" rather than dressed up as one it is not.
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
    quotes: true,
    testable: true,
};

#[async_trait::async_trait]
impl Broker for Oanda {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => {}
            Some(v) if v.is_empty() || v == "live" || v == "practice" => {}
            Some(other) => {
                return Err(anyhow!(
                    "the OANDA environment is \"live\" or \"practice\", not \"{other}\""
                ))
            }
        }
        // An account id is four dash-separated groups; anything else is a token or an alias
        // pasted into the wrong box, and the 404 it would cause names nothing.
        if let Some(id) = config.get("account_id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if id.split('-').count() != 4 || !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
                return Err(anyhow!(
                    "an OANDA account id looks like 001-004-1234567-001, not \"{id}\""
                ));
            }
        }
        Ok(())
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let body = get(client, secrets, &format!("{}/summary", account_path(secrets)?), &[]).await?;
        let a = body.get("account").unwrap_or(&Value::Null);
        let alias = str_of(a, "alias");
        Ok(format!(
            "{} account {}{}, balance {} {}, {} open trade(s) and {} pending order(s)",
            env_label(secrets),
            str_of(a, "id"),
            if alias.is_empty() { String::new() } else { format!(" ({alias})") },
            str_of(a, "balance"),
            str_of(a, "currency"),
            a.get("openTradeCount").and_then(Value::as_i64).unwrap_or(0),
            a.get("pendingOrderCount").and_then(Value::as_i64).unwrap_or(0),
        ))
    }

    /// Every ORDER_FILL of the window, oldest first.
    ///
    /// Two nested walks: the period is cut into the 365 days the endpoint accepts, and each
    /// slice answers a list of id-range pages that are fetched in turn. `symbols` only
    /// narrows the result, since OANDA answers the whole account at once.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let acct = account_path(secrets)?;
        let account_id = account_id(secrets)?;
        let kinds = instrument_types(client, secrets).await.unwrap_or_default();
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        let mut out: Vec<Execution> = Vec::new();
        let mut pages_left = MAX_PAGES;

        let mut slice_from = q.from;
        while slice_from < q.to {
            let slice_to = (slice_from + time::Duration::days(MAX_WINDOW_DAYS)).min(q.to);
            let index = get(
                client,
                secrets,
                &format!("{acct}/transactions"),
                &[
                    ("from".into(), slice_from.format(&Rfc3339)?),
                    ("to".into(), slice_to.format(&Rfc3339)?),
                    ("pageSize".into(), PAGE.to_string()),
                    ("type".into(), "ORDER_FILL".into()),
                ],
            )
            .await?;
            for page in index.get("pages").and_then(Value::as_array).into_iter().flatten() {
                if pages_left == 0 {
                    return Err(anyhow!(
                        "OANDA split this period into more pages than one pull reads. Ask for \
                         a shorter period."
                    ));
                }
                pages_left -= 1;
                let Some((first, last)) = idrange_of(page.as_str().unwrap_or_default()) else {
                    continue;
                };
                let body = get(
                    client,
                    secrets,
                    &format!("{acct}/transactions/idrange"),
                    &[
                        ("from".into(), first),
                        ("to".into(), last),
                        ("type".into(), "ORDER_FILL".into()),
                    ],
                )
                .await?;
                for t in body.get("transactions").and_then(Value::as_array).into_iter().flatten() {
                    if str_of(t, "type") != "ORDER_FILL" {
                        continue;
                    }
                    let symbol = str_of(t, "instrument");
                    if symbol.is_empty() {
                        continue;
                    }
                    if !wanted.is_empty() && !wanted.contains(&symbol.to_uppercase()) {
                        continue;
                    }
                    out.push(fill(t, &symbol, &kinds, &account_id)?);
                }
            }
            slice_from = slice_to;
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn positions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        let kinds = instrument_types(client, secrets).await.unwrap_or_default();
        let account_id = account_id(secrets)?;
        let body = get(
            client,
            secrets,
            &format!("{}/openPositions", account_path(secrets)?),
            &[],
        )
        .await?;
        let mut out = Vec::new();
        for p in body.get("positions").and_then(Value::as_array).into_iter().flatten() {
            let symbol = str_of(p, "instrument");
            for (key, side) in [("long", "long"), ("short", "short")] {
                let Some(leg) = p.get(key) else { continue };
                let units = num(leg, "units");
                if units == 0.0 {
                    continue;
                }
                out.push(Position {
                    symbol: symbol.clone(),
                    side: side.into(),
                    qty: units.abs(),
                    avg_price: Some(num(leg, "averagePrice")).filter(|v| *v > 0.0),
                    currency: quote_ccy(&symbol),
                    asset_class: class_of(&symbol, &kinds).into(),
                    multiplier: 1.0,
                    venue: "OANDA".into(),
                    account: account_id.clone(),
                });
            }
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    async fn orders(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<OpenOrder>> {
        let kinds = instrument_types(client, secrets).await.unwrap_or_default();
        let account_id = account_id(secrets)?;
        let body = get(
            client,
            secrets,
            &format!("{}/pendingOrders", account_path(secrets)?),
            &[],
        )
        .await?;
        let mut out = Vec::new();
        for o in body.get("orders").and_then(Value::as_array).into_iter().flatten() {
            // A take-profit or stop-loss order hangs off a trade and names no instrument of
            // its own. There is nothing to draw it on, so it is left out rather than filed
            // against a guessed symbol.
            let symbol = str_of(o, "instrument");
            if symbol.is_empty() {
                continue;
            }
            let kind = str_of(o, "type");
            let units = num(o, "units");
            let price = Some(num(o, "price")).filter(|v| *v > 0.0);
            // OANDA's one `price` field is a limit on a LIMIT order and a trigger on a STOP
            // or a market-if-touched, so it is filed as whichever the order type says.
            let stops = kind.contains("STOP") || kind.contains("MARKET_IF_TOUCHED");
            out.push(OpenOrder {
                id: str_of(o, "id"),
                side: if units < 0.0 { "sell".into() } else { "buy".into() },
                order_type: kind.clone(),
                qty: units.abs(),
                filled_qty: 0.0,
                limit_price: if stops { None } else { price },
                stop_price: if stops { price } else { None },
                currency: quote_ccy(&symbol),
                asset_class: class_of(&symbol, &kinds).into(),
                placed_at: o
                    .get("createTime")
                    .and_then(Value::as_str)
                    .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok()),
                venue: "OANDA".into(),
                account: account_id.clone(),
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

    /// The account's own pricing stream, asked once for every instrument wanted. The number
    /// is the mid of the closeout bid and ask: the two sides OANDA would actually fill at.
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
            &format!("{}/pricing", account_path(secrets)?),
            &[("instruments".into(), wanted.join(","))],
        )
        .await?;
        let mut out = Vec::new();
        for p in body.get("prices").and_then(Value::as_array).into_iter().flatten() {
            let pair = str_of(p, "instrument");
            let bid = num(p, "closeoutBid");
            let ask = num(p, "closeoutAsk");
            if bid <= 0.0 || ask <= 0.0 {
                continue;
            }
            let Some(asked) = symbols.iter().find(|s| s.trim().eq_ignore_ascii_case(&pair)) else {
                continue;
            };
            out.push(Quote {
                symbol: asked.clone(),
                price: (bid + ask) / 2.0,
                currency: quote_ccy(&pair),
                pair,
            });
        }
        Ok(out)
    }

    /// Every instrument this account may trade: OANDA's list is a few hundred names, which
    /// is exactly what a picker wants.
    async fn symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        let mut out: Vec<String> = instrument_types(client, secrets)
            .await?
            .into_keys()
            .collect();
        out.sort();
        Ok(out)
    }
}

// ── Mapping ──────────────────────────────────────────────────────────────────

/// One ORDER_FILL as a journal execution.
fn fill(
    t: &Value,
    symbol: &str,
    kinds: &HashMap<String, String>,
    account: &str,
) -> Result<Execution> {
    let units = num(t, "units");
    // `price` is deprecated in favour of the volume-weighted price of the whole fill; the
    // old field is only read when the new one is absent.
    let price = Some(num(t, "fullVWAP")).filter(|p| *p > 0.0).unwrap_or_else(|| num(t, "price"));
    Ok(Execution {
        id: str_of(t, "id"),
        order_id: str_of(t, "orderID"),
        at: OffsetDateTime::parse(&str_of(t, "time"), &Rfc3339)
            .map_err(|e| anyhow!("OANDA reported a timestamp that cannot be read: {e}"))?,
        symbol: symbol.to_string(),
        side: if units < 0.0 { "sell".into() } else { "buy".into() },
        qty: units.abs(),
        price,
        // Both are charges, and OANDA signs them as it pleases; a cost is filed positive.
        fee: num(t, "commission").abs() + num(t, "guaranteedExecutionFee").abs(),
        // Commission is in AccountUnits, the account's own currency, not the instrument's.
        fee_currency: String::new(),
        currency: quote_ccy(symbol),
        asset_class: class_of(symbol, kinds).into(),
        multiplier: 1.0,
        venue: "OANDA".into(),
        account: account.to_string(),
    })
}

/// What an instrument settles in, read off OANDA's own `base_quote` name. `EUR_USD` settles
/// in USD and so does `SPX500_USD`: this is the venue's notation, not a suffix guess.
fn quote_ccy(instrument: &str) -> String {
    instrument
        .rsplit_once('_')
        .map(|(_, q)| q.to_uppercase())
        .unwrap_or_default()
}

/// The journal class of an instrument, from the type the account's instrument list gives it.
/// A CFD and a metal have no class of their own here, so they are filed as "other" rather
/// than as an equity or a future they are not.
fn class_of(instrument: &str, kinds: &HashMap<String, String>) -> &'static str {
    match kinds.get(instrument).map(String::as_str) {
        Some("CURRENCY") => "forex",
        _ => "other",
    }
}

/// Instrument name → OANDA type (`CURRENCY`, `CFD`, `METAL`), asked once per call.
async fn instrument_types(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let body = get(
        client,
        secrets,
        &format!("{}/instruments", account_path(secrets)?),
        &[],
    )
    .await?;
    let mut out = HashMap::new();
    for i in body.get("instruments").and_then(Value::as_array).into_iter().flatten() {
        let name = str_of(i, "name");
        if !name.is_empty() {
            out.insert(name, str_of(i, "type"));
        }
    }
    Ok(out)
}

/// The two transaction ids a page URL covers.
///
/// The index answers absolute URLs carrying OANDA's own host. Only the id pair is taken from
/// them and the call is rebuilt against the configured host: a URL a remote service hands
/// back is data, not a place to send a bearer token.
fn idrange_of(url: &str) -> Option<(String, String)> {
    let query = url.split_once('?')?.1;
    let mut from = None;
    let mut to = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("from", v)) if v.chars().all(|c| c.is_ascii_digit()) && !v.is_empty() => {
                from = Some(v.to_string())
            }
            Some(("to", v)) if v.chars().all(|c| c.is_ascii_digit()) && !v.is_empty() => {
                to = Some(v.to_string())
            }
            _ => {}
        }
    }
    Some((from?, to?))
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

fn base(secrets: &HashMap<String, String>) -> &'static str {
    match secrets.get("env").map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "practice" => PRACTICE,
        _ => LIVE,
    }
}

fn env_label(secrets: &HashMap<String, String>) -> &'static str {
    if base(secrets) == PRACTICE { "Practice" } else { "Live" }
}

fn account_id(secrets: &HashMap<String, String>) -> Result<String> {
    Ok(super::require(secrets, "account_id")?.to_string())
}

fn account_path(secrets: &HashMap<String, String>) -> Result<String> {
    Ok(format!("/v3/accounts/{}", super::binance::enc(&account_id(secrets)?)))
}

async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let token = super::require(secrets, "api_token")?;
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
        "oanda",
        client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept-Datetime-Format", "RFC3339"),
    )
    .await
    .with_context(|| format!("calling OANDA {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = super::binance::trim_err(&body);
        return Err(match status.as_u16() {
            401 => anyhow!(
                "OANDA refused the token ({detail}). Check it was generated for the {} \
                 environment this account is set to.",
                env_label(secrets).to_ascii_lowercase()
            ),
            404 => anyhow!(
                "OANDA does not know account {} on the {} environment ({detail})",
                secrets.get("account_id").map(String::as_str).unwrap_or(""),
                env_label(secrets).to_ascii_lowercase()
            ),
            429 => anyhow!("OANDA is rate-limiting this token ({detail}), retry in a moment"),
            _ => anyhow!("OANDA {path} answered {status}: {detail}"),
        });
    }
    serde_json::from_str(&body).with_context(|| format!("reading OANDA's {path} answer"))
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// OANDA writes every number as a string; a missing one is zero, never a guess.
fn num(v: &Value, key: &str) -> f64 {
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
    fn a_fill_keeps_its_sign_its_fees_and_its_quote_currency() {
        // Shape taken from the v20 docs' own ORDER_FILL example, with a short and a fee.
        let t = serde_json::json!({
            "type": "ORDER_FILL", "id": "6410", "orderID": "6409", "instrument": "EUR_USD",
            "units": "-100", "price": "1.13031", "fullVWAP": "1.13030",
            "commission": "-0.40000", "guaranteedExecutionFee": "0.10000",
            "time": "2016-06-22T18:41:52.655959788Z"
        });
        let kinds = HashMap::from([("EUR_USD".to_string(), "CURRENCY".to_string())]);
        let e = fill(&t, "EUR_USD", &kinds, "001-004-1-001").unwrap();
        assert_eq!(e.side, "sell");
        assert_eq!(e.qty, 100.0);
        // The volume-weighted price wins over the deprecated single one.
        assert_eq!(e.price, 1.13030);
        // A charge is positive whichever way OANDA signed it, and both charges count.
        assert!((e.fee - 0.5).abs() < 1e-9);
        assert_eq!(e.currency, "USD");
        assert_eq!(e.asset_class, "forex");
        assert_eq!(e.at.unix_timestamp(), 1_466_620_912);
    }

    #[test]
    fn a_cfd_is_not_dressed_up_as_a_class_it_has_not_got() {
        let kinds = HashMap::from([
            ("SPX500_USD".to_string(), "CFD".to_string()),
            ("XAU_USD".to_string(), "METAL".to_string()),
        ]);
        assert_eq!(class_of("SPX500_USD", &kinds), "other");
        assert_eq!(class_of("XAU_USD", &kinds), "other");
        // An instrument the list does not carry is not promoted to forex by its shape.
        assert_eq!(class_of("EUR_USD", &kinds), "other");
        assert_eq!(quote_ccy("SPX500_USD"), "USD");
        assert_eq!(quote_ccy("EUR_USD"), "USD");
        assert_eq!(quote_ccy("nonsense"), "");
    }

    #[test]
    fn only_the_id_pair_is_taken_from_a_page_url() {
        let (a, b) =
            idrange_of("https://api-fxtrade.oanda.com/v3/accounts/X/transactions/idrange?from=6409&to=6412")
                .unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("6409", "6412"));
        // A page URL pointing somewhere else carries no usable id pair, so nothing is called.
        assert!(idrange_of("https://evil.example/steal?from=abc&to=1").is_none());
        assert!(idrange_of("no-query-at-all").is_none());
    }

    #[test]
    fn an_account_id_is_checked_before_it_becomes_a_404() {
        let ok = HashMap::from([("account_id".to_string(), "001-004-1234567-001".to_string())]);
        assert!(Oanda.validate_config(&ok).is_ok());
        let bad = HashMap::from([("account_id".to_string(), "my-oanda-token".to_string())]);
        assert!(Oanda.validate_config(&bad).is_err());
        let env = HashMap::from([("env".to_string(), "demo".to_string())]);
        assert!(Oanda.validate_config(&env).is_err());
    }
}
