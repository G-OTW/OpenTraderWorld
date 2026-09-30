//! Capital.com account (API key + login, read side only).
//!
//! A session broker like FOREX.com, with its own twist: signing in returns **two** tokens,
//! `CST` and `X-SECURITY-TOKEN`, and every later call carries both. They last ten minutes
//! from their last use, so one session is minted and shared rather than one per call.
//!
//! What the API's shape forces, and it is the awkward part of this connector:
//!   • **The activity log answers one day at a time.** `history/activity` caps the range
//!     between `from` and `to` at 24 hours, so a year of trading is walked day by day. That
//!     is hundreds of calls, which is why the period is bounded here and the refusal says so
//!     rather than letting a five-year pull run until the rate limiter stops it.
//!   • **An activity row only carries money in detail.** Without `detailed=true` a row is a
//!     date, an epic and a status; with it, the size, direction, level and currency arrive.
//!   • **A deal id is a position, not a fill.** The same `dealId` appears on the row that
//!     opened a position and on the row that closed it, so the identity a re-sync
//!     deduplicates on is the deal, its timestamp and its direction together.
//!   • **Everything here is a CFD.** A share CFD is not a share and a crypto CFD is not a
//!     coin, so only a currency pair is given a class of its own; the rest are filed as
//!     "other", which is a derivative everywhere downstream rather than an equity.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime, PrimitiveDateTime, Time};

use super::{
    Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Holding, OpenOrder, Position,
    Quote,
};

pub(crate) const LIVE: &str = "https://api-capital.backend-capital.com";
pub(crate) const DEMO: &str = "https://demo-api-capital.backend-capital.com";
/// Tokens die ten minutes after their last use.
const SESSION_TTL: Duration = Duration::from_secs(10 * 60);
/// Widest window the activity log accepts.
const WINDOW_HOURS: i64 = 24;
/// Hard stop on the day-by-day walk. A pull wider than this is refused by name rather than
/// left to run into the rate limiter.
const MAX_DAYS: i64 = 400;

pub struct CapitalCom;

static FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "12345678901234567",
        kind: "text",
        required: false,
        help: "Which financial account to read, when the login has more than one (Capital.com \
               opens one per currency). Leave empty to read the preferred account.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or demo. The demo account is a different host with its own key. Leave \
               empty for live.",
    },
];

static CAP: BrokerCapability = BrokerCapability {
    broker: "capitalcom",
    label: "Capital.com",
    website: "https://capital.com",
    docs_url: "https://open-api.capital.com/",
    rate_limit: "10 requests per second, and one sign-in per second. The activity log answers \
                 24 hours at a time, so a long period costs one call per day.",
    key_note: "An API key from Settings → API integrations (two-factor authentication has to \
               be on to create one), plus your login and the **custom password** you set on \
               the key. Capital.com grades no key read-only, so this login can also trade: \
               treat it as a full-access secret. Nothing here places, changes or cancels an \
               order. Every instrument is a **CFD**, so a share CFD is filed as a derivative \
               rather than as the share, which is what a tax form wants.",
    required_secrets: &["api_key", "identifier", "api_password"],
    config_fields: FIELDS,
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
impl Broker for CapitalCom {
    fn capability(&self) -> &'static BrokerCapability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => Ok(()),
            Some(v) if v.is_empty() || v == "live" || v == "demo" => Ok(()),
            Some(other) => Err(anyhow!(
                "the Capital.com environment is \"live\" or \"demo\", not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let body = get(client, secrets, "/api/v1/accounts", &[]).await?;
        let rows = body.get("accounts").and_then(Value::as_array).cloned().unwrap_or_default();
        let active = rows
            .iter()
            .find(|a| {
                secrets
                    .get("account_id")
                    .map(|id| id.trim() == str_of(a, "accountId"))
                    .unwrap_or_else(|| a.get("preferred").and_then(Value::as_bool).unwrap_or(false))
            })
            .cloned()
            .unwrap_or(Value::Null);
        Ok(format!(
            "{} sign-in accepted, {} account(s); reading {} ({}), balance {}",
            env_label(secrets),
            rows.len(),
            str_of(&active, "accountName"),
            str_of(&active, "currency"),
            active.get("balance").map(|b| num(b, "balance")).unwrap_or(0.0),
        ))
    }

    /// Every accepted position activity of the window, oldest first.
    async fn executions(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        let days = (q.to - q.from).whole_hours().div_euclid(WINDOW_HOURS) + 1;
        if days > MAX_DAYS {
            return Err(anyhow!(
                "Capital.com answers its activity log one day at a time, so this period would \
                 cost {days} calls. Ask for {MAX_DAYS} days or fewer and pull the rest \
                 separately."
            ));
        }
        let wanted: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
        let mut out: Vec<Execution> = Vec::new();
        let mut slice_from = q.from;
        while slice_from < q.to {
            let slice_to = (slice_from + time::Duration::hours(WINDOW_HOURS)).min(q.to);
            let body = get(
                client,
                secrets,
                "/api/v1/history/activity",
                &[
                    ("from".into(), local_stamp(slice_from)?),
                    ("to".into(), local_stamp(slice_to)?),
                    // Without this the row carries no size, no price and no currency.
                    ("detailed".into(), "true".into()),
                    // Only accepted position activity is a fill; an edited stop is not.
                    ("filter".into(), "type==POSITION;status==ACCEPTED".into()),
                ],
            )
            .await?;
            for a in body.get("activities").and_then(Value::as_array).into_iter().flatten() {
                let epic = str_of(a, "epic").to_uppercase();
                if epic.is_empty() || (!wanted.is_empty() && !wanted.contains(&epic)) {
                    continue;
                }
                let Some(e) = fill(a, &epic) else { continue };
                out.push(e);
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
        let body = get(client, secrets, "/api/v1/positions", &[]).await?;
        let mut out = Vec::new();
        for row in body.get("positions").and_then(Value::as_array).into_iter().flatten() {
            let p = row.get("position").unwrap_or(&Value::Null);
            let m = row.get("market").unwrap_or(&Value::Null);
            let size = num(p, "size");
            if size == 0.0 {
                continue;
            }
            out.push(Position {
                symbol: str_of(m, "epic").to_uppercase(),
                side: if str_of(p, "direction").eq_ignore_ascii_case("sell") {
                    "short".into()
                } else {
                    "long".into()
                },
                qty: size.abs(),
                avg_price: Some(num(p, "level")).filter(|v| *v > 0.0),
                currency: str_of(p, "currency").to_uppercase(),
                asset_class: class_of(&str_of(m, "instrumentType")).into(),
                // What one unit of size is worth, as Capital.com publishes it on the deal.
                multiplier: Some(num(p, "contractSize")).filter(|v| *v > 0.0).unwrap_or(1.0),
                venue: "Capital.com".into(),
                account: String::new(),
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
        let body = get(client, secrets, "/api/v1/workingorders", &[]).await?;
        let mut out = Vec::new();
        for row in body.get("workingOrders").and_then(Value::as_array).into_iter().flatten() {
            let o = row.get("workingOrderData").unwrap_or(&Value::Null);
            let m = row.get("marketData").unwrap_or(&Value::Null);
            let kind = str_of(o, "orderType");
            let level = Some(num(o, "orderLevel")).filter(|v| *v > 0.0);
            out.push(OpenOrder {
                id: str_of(o, "dealId"),
                symbol: str_of(o, "epic").to_uppercase(),
                side: if str_of(o, "direction").eq_ignore_ascii_case("sell") {
                    "sell".into()
                } else {
                    "buy".into()
                },
                qty: num(o, "orderSize"),
                filled_qty: 0.0,
                // One level field, filed as whichever price the order type says it is.
                limit_price: kind.eq_ignore_ascii_case("limit").then_some(level).flatten(),
                stop_price: (!kind.eq_ignore_ascii_case("limit")).then_some(level).flatten(),
                order_type: kind,
                currency: str_of(o, "currencyCode").to_uppercase(),
                asset_class: class_of(&str_of(m, "instrumentType")).into(),
                placed_at: utc_stamp(o, "createdDateUTC"),
                venue: "Capital.com".into(),
                account: String::new(),
            });
        }
        Ok(out)
    }

    /// The open positions read as a balance sheet. A CFD book owns positions, not assets:
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

    /// What Capital.com quotes these instruments at right now: the mid of the two sides it
    /// deals on, which is the number its own candles are built from.
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
            "/api/v1/markets",
            &[("epics".into(), wanted.join(","))],
        )
        .await?;
        let mut out = Vec::new();
        for m in body
            .get("marketDetails")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let instrument = m.get("instrument").unwrap_or(&Value::Null);
            let snapshot = m.get("snapshot").unwrap_or(&Value::Null);
            let epic = str_of(instrument, "epic").to_uppercase();
            let (bid, offer) = (num(snapshot, "bid"), num(snapshot, "offer"));
            if bid <= 0.0 || offer <= 0.0 {
                continue;
            }
            let Some(asked) = symbols.iter().find(|s| s.trim().eq_ignore_ascii_case(&epic)) else {
                continue;
            };
            out.push(Quote {
                symbol: asked.clone(),
                price: (bid + offer) / 2.0,
                currency: str_of(instrument, "currency").to_uppercase(),
                pair: epic,
            });
        }
        Ok(out)
    }

    /// The epics this account is in or is working, for a symbol picker.
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

// ── Mapping ──────────────────────────────────────────────────────────────────

/// One detailed activity row as a fill, or `None` when it carries no money.
fn fill(a: &Value, epic: &str) -> Option<Execution> {
    let d = a.get("details")?;
    let size = num(d, "size");
    let level = num(d, "level");
    if size <= 0.0 || level <= 0.0 {
        return None;
    }
    let at = utc_stamp(a, "dateUTC")?;
    let direction = str_of(d, "direction").to_ascii_lowercase();
    Some(Execution {
        // A deal id names the position, not this fill of it: the same id is on the row that
        // opened it and on the row that closed it. The three together are the identity.
        id: format!("{}:{}:{direction}", str_of(a, "dealId"), str_of(a, "dateUTC")),
        order_id: str_of(a, "dealId"),
        at,
        symbol: epic.to_string(),
        side: if direction == "sell" { "sell".into() } else { "buy".into() },
        qty: size,
        price: level,
        // Capital.com bills the spread and an overnight charge, not a commission on a fill.
        fee: 0.0,
        fee_currency: String::new(),
        currency: str_of(d, "currency").to_uppercase(),
        asset_class: "other".into(),
        multiplier: 1.0,
        venue: "Capital.com".into(),
        account: String::new(),
    })
}

/// The journal class of a Capital.com instrument type.
///
/// Every instrument on the platform is a **CFD**. A currency pair is still a currency pair,
/// so it keeps its class; a share CFD is not a share and a crypto CFD is not a coin, so
/// those are filed as "other", which is a derivative everywhere downstream. Calling them
/// equities or crypto would put them on the wrong line of a tax form.
fn class_of(instrument_type: &str) -> &'static str {
    match instrument_type.to_ascii_uppercase().as_str() {
        "CURRENCIES" => "forex",
        _ => "other",
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

pub(crate) fn base(secrets: &HashMap<String, String>) -> &'static str {
    match secrets.get("env").map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "demo" => DEMO,
        _ => LIVE,
    }
}

fn env_label(secrets: &HashMap<String, String>) -> &'static str {
    if base(secrets) == DEMO { "Demo" } else { "Live" }
}

/// The two tokens a Capital.com session hands back, kept together because every call needs
/// both: `CST` says who signed in, `X-SECURITY-TOKEN` says which financial account is live.
#[derive(Clone)]
pub(crate) struct Session {
    pub cst: String,
    pub security: String,
}

/// A session for this login, minted only when the cached one has run out. Shared with the
/// market-data connector: the sign-in endpoint allows one request a second.
pub(crate) async fn session(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
) -> Result<Session> {
    let api_key = super::require(secrets, "api_key")?;
    let identifier = super::require(secrets, "identifier")?;
    let password = super::require(secrets, "api_password")?;
    let account = secrets.get("account_id").map(|s| s.trim()).unwrap_or_default();
    let key = super::tokens::key(&["capitalcom", base(secrets), api_key, account]);
    if let Some(t) = super::tokens::get(&key) {
        if let Some((cst, security)) = t.split_once('\n') {
            return Ok(Session { cst: cst.into(), security: security.into() });
        }
    }
    let res = crate::rate::send(
        "capitalcom",
        client
            .post(format!("{}/api/v1/session", super::at(base(secrets))))
            .header("X-CAP-API-KEY", api_key)
            .json(&json!({
                "identifier": identifier,
                "password": password,
                "encryptedPassword": false,
            })),
    )
    .await
    .context("opening a Capital.com session")?;
    let status = res.status();
    let cst = header(&res, "CST");
    let security = header(&res, "X-SECURITY-TOKEN");
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "Capital.com refused the sign-in ({status}: {}). Check the API key, the login and \
             the custom password set on the key, and that two-factor authentication is on.",
            super::binance::trim_err(&body)
        ));
    }
    let (Some(cst), Some(security)) = (cst, security) else {
        return Err(anyhow!(
            "Capital.com accepted the sign-in but returned no session tokens"
        ));
    };
    let mut out = Session { cst, security };
    // Switch to the financial account this broker account names. Capital.com opens one per
    // currency and the session starts on the preferred one, which may not be the one asked
    // for; the switch answers a fresh security token for the account it moved to.
    if !account.is_empty() {
        let res = crate::rate::send(
            "capitalcom",
            client
                .put(format!("{}/api/v1/session", super::at(base(secrets))))
                .header("X-CAP-API-KEY", api_key)
                .header("CST", &out.cst)
                .header("X-SECURITY-TOKEN", &out.security)
                .json(&json!({ "accountId": account })),
        )
        .await
        .context("switching the Capital.com account")?;
        let status = res.status();
        let fresh = header(&res, "X-SECURITY-TOKEN");
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "Capital.com would not switch to account {account} ({status}: {}). Check the \
                 Account ID setting against the list on the platform.",
                super::binance::trim_err(&body)
            ));
        }
        if let Some(t) = fresh {
            out.security = t;
        }
    }
    super::tokens::put(&key, &format!("{}\n{}", out.cst, out.security), SESSION_TTL);
    Ok(out)
}

fn header(res: &reqwest::Response, name: &str) -> Option<String> {
    res.headers()
        .get(name)?
        .to_str()
        .ok()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub(crate) async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let s = session(client, secrets).await?;
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
        "capitalcom",
        client
            .get(&url)
            .header("X-CAP-API-KEY", super::require(secrets, "api_key")?)
            .header("CST", &s.cst)
            .header("X-SECURITY-TOKEN", &s.security),
    )
    .await
    .with_context(|| format!("calling Capital.com {path}"))?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = super::binance::trim_err(&body);
        if matches!(status.as_u16(), 401 | 403) {
            // The session has timed out or been replaced; the next call signs in again
            // rather than replaying these tokens.
            if let Ok(api_key) = super::require(secrets, "api_key") {
                let account = secrets.get("account_id").map(|s| s.trim()).unwrap_or_default();
                super::tokens::forget(&super::tokens::key(&[
                    "capitalcom",
                    base(secrets),
                    api_key,
                    account,
                ]));
            }
            return Err(anyhow!(
                "Capital.com ended this session ({detail}). It expires after ten minutes \
                 without use; retry, and check the key if it keeps happening."
            ));
        }
        return Err(anyhow!("Capital.com {path} answered {status}: {detail}"));
    }
    serde_json::from_str(&body).with_context(|| format!("reading Capital.com's {path} answer"))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// The date format Capital.com's query parameters take: `YYYY-MM-DDTHH:MM:SS`, no zone.
/// Its `from`/`to` filter on the UTC field, so the value is written in UTC.
pub(crate) fn local_stamp(t: OffsetDateTime) -> Result<String> {
    let t = t.to_offset(time::UtcOffset::UTC);
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    ))
}

/// A `…UTC` field: an ISO date and time with no zone on it, which the docs say is UTC.
pub(crate) fn utc_stamp(v: &Value, key: &str) -> Option<OffsetDateTime> {
    parse_utc(v.get(key)?.as_str()?)
}

pub(crate) fn parse_utc(raw: &str) -> Option<OffsetDateTime> {
    let s = raw.trim();
    // An RFC 3339 stamp (one carrying a zone) is read as it stands.
    if let Ok(t) = OffsetDateTime::parse(s, &Rfc3339) {
        return Some(t);
    }
    let (date, rest) = s.split_once('T')?;
    let (hms, frac) = match rest.split_once('.') {
        Some((h, f)) => (h, f),
        None => (rest, "0"),
    };
    let mut d = date.split('-');
    let (y, m, day) = (d.next()?, d.next()?, d.next()?);
    let mut t = hms.split(':');
    let (h, mi, sec) = (t.next()?, t.next()?, t.next().unwrap_or("0"));
    let date = Date::from_calendar_date(
        y.parse().ok()?,
        time::Month::try_from(m.parse::<u8>().ok()?).ok()?,
        day.parse().ok()?,
    )
    .ok()?;
    // Milliseconds, padded or truncated to three digits.
    let ms: u32 = format!("{frac:0<3}")[..3].parse().ok()?;
    let clock = Time::from_hms_milli(
        h.parse().ok()?,
        mi.parse().ok()?,
        sec.parse().ok()?,
        ms as u16,
    )
    .ok()?;
    Some(PrimitiveDateTime::new(date, clock).assume_utc())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zoneless_utc_stamp_is_read_as_utc() {
        let t = parse_utc("2022-01-17T15:09:47.344").unwrap();
        assert_eq!(t.format(&Rfc3339).unwrap(), "2022-01-17T15:09:47.344Z");
        // No fraction, and a stamp that does carry a zone, both work.
        assert_eq!(parse_utc("2022-01-17T15:09:47").unwrap().second(), 47);
        assert_eq!(
            parse_utc("2022-01-17T15:09:47Z").unwrap(),
            parse_utc("2022-01-17T15:09:47").unwrap()
        );
        assert!(parse_utc("17/01/2022").is_none());
        // What goes back out on a query is the same shape, without the zone.
        assert_eq!(local_stamp(t).unwrap(), "2022-01-17T15:09:47");
    }

    #[test]
    fn a_deal_id_alone_is_not_a_fill_identity() {
        // The docs' own worked example: one deal, opened then closed out, same dealId.
        let open = serde_json::json!({
            "dateUTC": "2022-01-03T15:01:34.228", "epic": "OIL_CRUDE",
            "dealId": "00018509-0001-54c4-0000-0000803c0ec6", "source": "USER",
            "type": "POSITION", "status": "ACCEPTED",
            "details": {"marketName": "Crude Oil", "currency": "USD", "size": 1,
                        "direction": "BUY", "level": 75.31}
        });
        let close = serde_json::json!({
            "dateUTC": "2022-01-03T15:01:48.420", "epic": "OIL_CRUDE",
            "dealId": "00018509-0001-54c4-0000-0000803c0ec6", "source": "CLOSE_OUT",
            "type": "POSITION", "status": "ACCEPTED",
            "details": {"marketName": "Crude Oil", "currency": "USD", "size": 1,
                        "direction": "SELL", "level": 75.2}
        });
        let a = fill(&open, "OIL_CRUDE").unwrap();
        let b = fill(&close, "OIL_CRUDE").unwrap();
        assert_eq!(a.order_id, b.order_id);
        assert_ne!(a.id, b.id, "two fills of one deal must not share an identity");
        assert_eq!((a.side.as_str(), b.side.as_str()), ("buy", "sell"));
        assert_eq!(a.price, 75.31);
        assert_eq!(a.currency, "USD");
        // A row with no money in it is not a fill.
        let edit = serde_json::json!({
            "dateUTC": "2022-01-03T15:01:48.536", "dealId": "x", "type": "EDIT_STOP_AND_LIMIT",
            "details": {"size": 0, "direction": "SELL", "level": 75.31}
        });
        assert!(fill(&edit, "OIL_CRUDE").is_none());
    }

    #[test]
    fn a_cfd_is_not_the_thing_it_tracks() {
        assert_eq!(class_of("CURRENCIES"), "forex");
        // A share CFD is not a share and a crypto CFD is not a coin.
        assert_eq!(class_of("SHARES"), "other");
        assert_eq!(class_of("CRYPTOCURRENCIES"), "other");
        assert_eq!(class_of("INDICES"), "other");
        assert_eq!(class_of("COMMODITIES"), "other");
    }
}
