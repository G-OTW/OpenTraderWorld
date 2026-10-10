//! Company, ETF and market datasets from the aggregators, and the views the Company page
//! reads, which combine those snapshots with what EDGAR stored.
//!
//! Each dataset lists the providers that can fill it, best first; a refresh uses the first
//! one with a connector granted to Fundamentals (or the one asked for). What it returns is
//! stored as a snapshot already in the module's shape. Ratios that need a price (P/E,
//! market cap, yields) are computed on read from the latest close and the stored
//! statements, never stored.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::skips::{self, Skip};
use super::{alt, finnhub, fmp, sdmx, vendors, Ctx};
use crate::AppState;
use otw_store::fundamentals::{self as store, CompanyRow, Statement};

pub struct Dataset {
    pub id: &'static str,
    /// Provider ids, best first. `market` = any market-data connector serving daily equity
    /// bars.
    pub providers: &'static [&'static str],
    /// A snapshot older than this is refreshed when the page opens.
    pub ttl_hours: i64,
}

pub const MARKET: &str = "market";

/// Provider of the demo sandbox's seeded snapshots (`demo_fundamentals`).
pub const DEMO: &str = "demo";

pub const DATASETS: &[Dataset] = &[
    Dataset { id: "price", providers: &[MARKET], ttl_hours: 12 },
    Dataset { id: "estimates", providers: &["fmp", "alphavantage", "finnhub"], ttl_hours: 24 },
    Dataset { id: "earnings", providers: &["fmp", "alphavantage", "finnhub"], ttl_hours: 24 },
    Dataset { id: "segments", providers: &["fmp"], ttl_hours: 24 * 7 },
    Dataset { id: "dividends", providers: &["eodhd", "massive", "fmp", "alphavantage"], ttl_hours: 24 },
    Dataset { id: "holders", providers: &["fmp"], ttl_hours: 24 * 7 },
    Dataset { id: "peers", providers: &["fmp", "finnhub"], ttl_hours: 24 * 7 },
    Dataset { id: "short_interest", providers: &["finra", "massive"], ttl_hours: 24 },
    Dataset { id: "esg", providers: &["fmp", "finnhub"], ttl_hours: 24 * 30 },
    Dataset { id: "etf", providers: &["fmp", "alphavantage", "eodhd"], ttl_hours: 24 },
    Dataset { id: "calendar", providers: &["finnhub", "fmp", "eodhd", "alphavantage"], ttl_hours: 6 },
    Dataset { id: "central_banks", providers: &["bis"], ttl_hours: 12 },
    Dataset { id: "congress", providers: &["quiver", "finnhub"], ttl_hours: 12 },
    Dataset { id: "lobbying", providers: &["lda", "quiver"], ttl_hours: 24 * 7 },
    Dataset { id: "contracts", providers: &["usaspending", "quiver"], ttl_hours: 24 },
    Dataset { id: "patents", providers: &["uspto", "quiver"], ttl_hours: 24 * 7 },
];

/// Datasets the public sources match on the company's registered name, not its ticker.
const BY_NAME: &[&str] = &["lobbying", "contracts", "patents"];

pub fn dataset(id: &str) -> Option<&'static Dataset> {
    DATASETS.iter().find(|d| d.id == id)
}

pub fn label(provider: &str) -> String {
    if provider == MARKET {
        return "market data".into();
    }
    if provider == DEMO {
        return "Demo data".into();
    }
    crate::histdata::connector_for(provider)
        .map(|c| c.capability().label.to_string())
        .unwrap_or_else(|_| provider.to_string())
}

/// A provider order with the user's stored one applied: their providers first, in their
/// order, then any the app serves that their list does not name (added since), in the
/// app's order. A name the app no longer serves is dropped.
pub fn merge_order(default: &[&str], stored: Option<&[String]>) -> Vec<String> {
    let mut out: Vec<String> = stored
        .unwrap_or_default()
        .iter()
        .filter(|p| default.contains(&p.as_str()))
        .cloned()
        .collect();
    for p in default {
        if !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// The order a dataset's providers are tried in now (`transcripts` included).
pub async fn effective_order(pool: &sqlx::PgPool, id: &str, default: &[&str]) -> Result<Vec<String>> {
    let stored = store::provider_order(pool, id).await?;
    Ok(merge_order(default, stored.as_deref()))
}

/// One connector a dataset can be fetched through: (provider, connector id, settings).
type Candidate = (String, Uuid, HashMap<String, String>);

/// Whether a connector holds every credential its provider requires. A connector granted
/// to every module but never given its key (an Alpaca added for later) is no candidate.
fn has_creds(provider: &str, secrets: &HashMap<String, String>) -> bool {
    crate::histdata::connector_for(provider)
        .map(|c| c.capability().required_secrets.iter().all(|k| secrets.get(*k).is_some_and(|v| !v.trim().is_empty())))
        .unwrap_or(false)
}

/// The connectors a dataset can use now, best first (or only the one asked for).
async fn candidates(state: &AppState, d: &Dataset, asked: Option<&str>) -> Result<Vec<Candidate>> {
    if d.providers == [MARKET] {
        return market_candidates(state).await;
    }
    let wanted: Vec<String> = match asked {
        Some(p) if d.providers.contains(&p) => vec![p.to_string()],
        Some(p) => return Err(anyhow!("{} does not serve {}", label(p), d.id)),
        None => effective_order(&state.pool, d.id, d.providers).await?,
    };
    let mut out = Vec::new();
    for p in wanted {
        if let Some((id, secrets)) = super::creds_for(&state.pool, &state.cipher, &p).await? {
            if has_creds(&p, &secrets) {
                out.push((p, id, secrets));
            }
        }
    }
    if out.is_empty() {
        let names: Vec<String> = d.providers.iter().map(|p| label(p)).collect();
        return Err(anyhow!(
            "no connector for {}: add {} from the data broker, with its key, and grant it to Fundamentals",
            d.id,
            names.join(" or ")
        ));
    }
    Ok(out)
}

use uuid::Uuid;

/// Market-data connectors granted to Fundamentals that serve daily equity bars and hold
/// their credentials.
async fn market_candidates(state: &AppState) -> Result<Vec<Candidate>> {
    let granted = otw_store::connectors::list_for_module(&state.pool, super::MODULE).await?;
    let mut out = Vec::new();
    for c in granted {
        let Ok(conn) = crate::histdata::connector_for(&c.provider) else { continue };
        let cap = conn.capability();
        if cap.asset_types.contains(&"equity") && cap.timeframes.contains(&"1d") {
            let secrets = otw_store::connectors::load_creds(&state.pool, &state.cipher, c.id).await?;
            if has_creds(&c.provider, &secrets) {
                out.push((c.provider, c.id, secrets));
            }
        }
    }
    if out.is_empty() {
        return Err(anyhow!(
            "no market-data connector with daily equity bars and its key is granted to \
             Fundamentals: grant one (EODHD, Massive, Alpha Vantage, Yahoo, IBKR...) in the data broker"
        ));
    }
    Ok(out)
}

/// What a refresh came back with: a fresh snapshot, or nothing asked because every
/// provider refused recently or has spent its quota (an automatic refresh only).
pub enum Refreshed {
    Stored(store::Snapshot),
    Deferred(Vec<Skip>),
}

/// Fetch a dataset from a provider and store it. `force` (a manual refresh) asks every
/// provider, even one that refused lately or whose quota is spent.
pub async fn refresh(
    state: &AppState,
    id: &str,
    subject: &str,
    asked: Option<&str>,
    force: bool,
) -> Result<Refreshed> {
    let d = dataset(id).ok_or_else(|| anyhow!("unknown dataset {id}"))?;
    // A provider picked by name is asked whatever it answered last.
    let force = force || asked.is_some();
    // Best provider first; one that refuses (a plan without the dataset, a symbol it does
    // not cover) hands over to the next, and the error lists every answer. A refusal is
    // remembered so the next automatic refresh does not spend a request on it.
    let recent = skips::failures(&state.pool, id, subject, force).await?;
    let mut failures = Vec::new();
    let mut deferred = Vec::new();
    let mut answer = None;
    for (provider, connector, secrets) in candidates(state, d, asked).await? {
        if let Some(skip) = skips::check(&state.pool, &recent, &provider, connector, force).await {
            deferred.push(skip);
            continue;
        }
        let ctx = Ctx { client: &state.http, secrets: &secrets };
        let name = if BY_NAME.contains(&id) && provider != alt::quiver::PROVIDER {
            let c = store::company_by_ticker(&state.pool, subject).await?.ok_or_else(|| {
                anyhow!("{} is matched on the company's registered name: open {subject} in Company first", label(&provider))
            })?;
            Some(c.name)
        } else {
            None
        };
        super::bump(&state.pool, connector).await;
        match fetch(&ctx, &provider, id, subject, name.as_deref()).await {
            // Prices: a free plan can answer with a few months only (Alpha Vantage's last
            // 100 sessions). Keep the longest history, stop at the first that spans the
            // five years the chart shows.
            Ok(data) if id == "price" => {
                skips::clear(&state.pool, id, subject, &provider).await;
                let span = price_span_days(&data);
                if answer.as_ref().is_none_or(|(_, best)| price_span_days(best) < span) {
                    answer = Some((provider, data));
                }
                if span >= 4 * 365 {
                    break;
                }
            }
            Ok(data) => {
                skips::clear(&state.pool, id, subject, &provider).await;
                answer = Some((provider, data));
                break;
            }
            Err(e) => {
                let msg = skips::redact(&format!("{e:#}"), &secrets);
                skips::record(&state.pool, id, subject, &provider, &msg).await;
                failures.push(format!("{}: {msg}", label(&provider)));
            }
        }
    }
    let Some((provider, data)) = answer else {
        if failures.is_empty() && !deferred.is_empty() {
            return Ok(Refreshed::Deferred(deferred));
        }
        failures.extend(deferred.iter().map(|s| format!("{}: not asked, to spare its quota", s.label)));
        return Err(anyhow!(failures.join(" | ")));
    };
    store::put_snapshot(&state.pool, subject, id, &provider, &data).await?;
    store::snapshot(&state.pool, subject, id)
        .await?
        .ok_or_else(|| anyhow!("snapshot vanished"))
        .map(Refreshed::Stored)
}

async fn fetch(ctx: &Ctx<'_>, provider: &str, id: &str, subject: &str, name: Option<&str>) -> Result<Value> {
    if id == "price" {
        return price(ctx, provider, subject).await;
    }
    if id == "central_banks" {
        return sdmx::central_banks(ctx.client).await;
    }
    let name = || name.ok_or_else(|| anyhow!("no registered name for {subject}"));
    match (provider, id) {
        (alt::quiver::PROVIDER, "congress") => return alt::quiver::congress(ctx, subject).await,
        (alt::quiver::PROVIDER, "lobbying") => return alt::quiver::lobbying(ctx, subject).await,
        (alt::quiver::PROVIDER, "contracts") => return alt::quiver::contracts(ctx, subject).await,
        (alt::quiver::PROVIDER, "patents") => return alt::quiver::patents(ctx, subject).await,
        (finnhub::PROVIDER, "congress") => return alt::finnhub_congress(ctx, subject).await,
        (alt::lda::PROVIDER, "lobbying") => return alt::lda::lobbying(ctx, name()?).await,
        (alt::usaspending::PROVIDER, "contracts") => return alt::usaspending::contracts(ctx, name()?).await,
        (alt::uspto::PROVIDER, "patents") => return alt::uspto::patents(ctx, name()?).await,
        _ => {}
    }
    match provider {
        fmp::PROVIDER => fmp::dataset(ctx, id, subject).await,
        finnhub::PROVIDER => finnhub::dataset(ctx, id, subject).await,
        vendors::av::PROVIDER => vendors::av::dataset(ctx, id, subject).await,
        vendors::eodhd::PROVIDER => vendors::eodhd::dataset(ctx, id, subject).await,
        "massive" => vendors::massive::dataset(ctx, id, subject).await,
        vendors::finra::PROVIDER if id == "short_interest" => vendors::finra::short_interest(ctx, subject).await,
        _ => Err(anyhow!("{} does not serve {id}", label(provider))),
    }
}

/// Five years of daily closes through the market-data connector, nothing stored in
/// histdata (this is a chart and a quote, not a dataset).
async fn price(ctx: &Ctx<'_>, provider: &str, ticker: &str) -> Result<Value> {
    let conn = crate::histdata::connector_for(provider)?;
    let cap = conn.capability();
    let symbol = if provider == "eodhd" && !ticker.contains('.') {
        format!("{}.US", ticker.replace('-', "."))
    } else {
        ticker.to_string()
    };
    let to = OffsetDateTime::now_utc();
    let from = to - time::Duration::days(5 * 365 + 7);
    let step = time::Duration::days(cap.max_bars_per_req.clamp(1, 5000) as i64);
    let mut bars = Vec::new();
    let mut cursor = from;
    let mut chunks = 0;
    while cursor < to && chunks < 12 {
        let end = (cursor + step).min(to);
        let chunk = conn.fetch_chunk(ctx.client, ctx.secrets, &symbol, "equity", "1d", cursor, end).await?;
        bars.extend(chunk.bars);
        cursor = end;
        chunks += 1;
    }
    bars.sort_by_key(|b| b.ts);
    bars.dedup_by_key(|b| b.ts);
    if bars.is_empty() {
        return Err(anyhow!("{} returned no daily bars for {ticker}", cap.label));
    }
    let series: Vec<(i64, f64)> = bars.iter().map(|b| (b.ts.unix_timestamp() * 1000, b.close)).collect();
    Ok(json!({ "series": series, "symbol": symbol }))
}

/// Days between the first and last close of a `price` answer.
fn price_span_days(data: &Value) -> i64 {
    let s = data.get("series").and_then(Value::as_array);
    let ts = |i: Option<&Value>| i.and_then(|p| p.get(0)).and_then(Value::as_i64).unwrap_or(0);
    s.map_or(0, |s| (ts(s.last()) - ts(s.first())) / 86_400_000)
}

/// Whether a snapshot is past its dataset's freshness. Never in the demo: its snapshots
/// are seeded and nothing may refresh them, so a page must not try.
pub fn stale(s: &store::Snapshot) -> bool {
    if crate::demo::enabled() {
        return false;
    }
    let ttl = dataset(&s.dataset).map_or(24, |d| d.ttl_hours);
    OffsetDateTime::now_utc() - s.fetched_at > time::Duration::hours(ttl)
}

// ── Views ───────────────────────────────────────────────────────────────────

fn line(s: Option<&Statement>, k: &str) -> Option<f64> {
    s.and_then(|s| s.lines.get(k)).and_then(Value::as_f64)
}

/// Trailing twelve months of a flow line: the last four consecutive quarters when the
/// filings have them, else the last fiscal year.
fn ttm(quarters: &[Statement], annual: Option<&Statement>, k: &str) -> Option<f64> {
    if quarters.len() >= 4 {
        let last4 = &quarters[quarters.len() - 4..];
        let span = (last4[3].period_end - last4[0].period_start).whole_days();
        if (350..=380).contains(&span) {
            let vals: Option<Vec<f64>> = last4.iter().map(|q| line(Some(q), k)).collect();
            if let Some(v) = vals {
                return Some(v.iter().sum());
            }
        }
    }
    line(annual, k)
}

/// Latest close and the one before it, from the price snapshot.
pub fn last_close(price: Option<&store::Snapshot>) -> Option<(f64, Option<f64>, i64)> {
    let s = price?.data.get("series")?.as_array()?;
    let last = s.last()?;
    let prev = s.get(s.len().checked_sub(2)?).and_then(|p| p.get(1)).and_then(Value::as_f64);
    Some((last.get(1)?.as_f64()?, prev, last.get(0)?.as_i64()?))
}

/// Ratios from the stored statements and, when a price is stored, the market ones.
pub async fn metrics(pool: &sqlx::PgPool, c: &CompanyRow) -> Result<Value> {
    let inc_a = store::statements(pool, c.id, "income", true).await?;
    let inc_q = store::statements(pool, c.id, "income", false).await?;
    let bal_q = store::statements(pool, c.id, "balance", false).await?;
    let bal_a = store::statements(pool, c.id, "balance", true).await?;
    let cf_a = store::statements(pool, c.id, "cashflow", true).await?;
    let cf_q = store::statements(pool, c.id, "cashflow", false).await?;
    let price = store::snapshot(pool, &c.ticker, "price").await?;
    let close = last_close(price.as_ref());

    let fy = inc_a.last();
    // The most recent balance sheet, quarterly or annual.
    let bal = match (bal_q.last(), bal_a.last()) {
        (Some(q), Some(a)) => Some(if q.period_end >= a.period_end { q } else { a }),
        (q, a) => q.or(a),
    };
    let t = |k: &str| ttm(&inc_q, fy, k);
    let tc = |k: &str| ttm(&cf_q, cf_a.last(), k);
    let revenue = t("revenue");
    let net = t("net_income");
    let op = t("operating_income");
    let equity = line(bal, "shareholders_equity");
    let cash = line(bal, "cash").map(|c| c + line(bal, "short_term_investments").unwrap_or(0.0));
    let debt = match (line(bal, "short_term_debt"), line(bal, "long_term_debt")) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    };
    let ebitda = match (op, tc("depreciation")) {
        (Some(o), Some(d)) => Some(o + d),
        _ => None,
    };
    let fcf = tc("free_cash_flow");
    let div_paid = tc("dividends_paid").map(f64::abs);
    let tax_rate = match (t("income_tax"), t("pretax_income")) {
        (Some(tx), Some(p)) if p > 0.0 => Some((tx / p).clamp(0.0, 0.5)),
        _ => None,
    };
    let ratio = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) if b != 0.0 && b.is_finite() => Some(a / b),
        _ => None,
    };
    let pct = |a: Option<f64>, b: Option<f64>| ratio(a, b).map(|r| r * 100.0);

    let price_now = close.map(|c| c.0);
    let market_cap = match (price_now, c.shares_out) {
        (Some(p), Some(s)) => Some(p * s),
        _ => None,
    };
    let ev = market_cap.map(|m| m + debt.unwrap_or(0.0) - cash.unwrap_or(0.0));
    let invested = match (debt, equity) {
        (d, Some(e)) => Some(d.unwrap_or(0.0) + e - cash.unwrap_or(0.0)),
        _ => None,
    };
    let nopat = match (op, tax_rate) {
        (Some(o), Some(r)) => Some(o * (1.0 - r)),
        _ => None,
    };
    let positive = |v: Option<f64>| v.filter(|x| *x > 0.0);
    Ok(json!({
        "basis": if inc_q.len() >= 4 { "TTM".to_string() } else { fy.map(|s| format!("FY{}", s.fiscal_year)).unwrap_or_default() },
        "price": price_now,
        "price_date": close.map(|c| c.2),
        "change_pct": close.and_then(|(p, prev, _)| prev.map(|q| (p / q - 1.0) * 100.0)),
        "market_cap": market_cap,
        "enterprise_value": ev,
        "pe": ratio(market_cap, positive(net)),
        "forward_pe": null,
        "ev_ebitda": ratio(ev, positive(ebitda)),
        "ps": ratio(market_cap, positive(revenue)),
        "pb": ratio(market_cap, positive(equity)),
        "dividend_yield": pct(div_paid, market_cap),
        "fcf_yield": pct(fcf, market_cap),
        "beta": null,
        "gross_margin": pct(t("gross_profit"), revenue),
        "operating_margin": pct(op, revenue),
        "net_margin": pct(net, revenue),
        "roe": pct(net, positive(equity)),
        "roic": pct(nopat, positive(invested)),
        "debt_equity": ratio(debt, positive(equity)),
        "current_ratio": ratio(line(bal, "total_current_assets"), line(bal, "total_current_liabilities")),
        "shares_out": c.shares_out,
        "float": null,
    }))
}

fn snap_data(s: &Option<store::Snapshot>) -> Option<&Value> {
    s.as_ref().map(|s| &s.data)
}

fn meta(s: &Option<store::Snapshot>) -> Value {
    match s {
        Some(s) => json!({ "provider": label(&s.provider), "fetched_at": s.fetched_at.unix_timestamp() * 1000, "stale": stale(s) }),
        None => Value::Null,
    }
}

/// Earnings with the next-day price move, when a price is stored.
pub async fn earnings_view(pool: &sqlx::PgPool, c: &CompanyRow) -> Result<Value> {
    let snap = store::snapshot(pool, &c.ticker, "earnings").await?;
    let price = store::snapshot(pool, &c.ticker, "price").await?;
    let closes: Vec<(String, f64)> = price
        .as_ref()
        .and_then(|p| p.data.get("series"))
        .and_then(Value::as_array)
        .map(|s| {
            s.iter()
                .filter_map(|p| {
                    let ms = p.get(0)?.as_i64()?;
                    let d = OffsetDateTime::from_unix_timestamp(ms / 1000).ok()?.date().to_string();
                    Some((d, p.get(1)?.as_f64()?))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut data = snap_data(&snap).cloned().unwrap_or(json!({ "history": [], "next": null }));
    if let Some(hist) = data.get_mut("history").and_then(Value::as_array_mut) {
        for r in hist.iter_mut() {
            let Some(day) = r.get("date").and_then(Value::as_str).map(str::to_string) else { continue };
            let time = r.get("time").and_then(Value::as_str).unwrap_or("").to_string();
            // Before the open the reaction is that day's close; after the close, the next one.
            let before = closes.iter().rev().find(|(d, _)| if time == "BMO" { *d < day } else { *d <= day });
            let after = closes.iter().find(|(d, _)| if time == "BMO" { *d >= day } else { *d > day });
            if let (Some((_, b)), Some((_, a))) = (before, after) {
                r["move_next_day"] = json!((a / b - 1.0) * 100.0);
            }
        }
    }
    data["meta"] = meta(&snap);
    Ok(data)
}

/// Dividends and splits from the snapshot, buybacks from the cash-flow statements.
pub async fn capital_view(pool: &sqlx::PgPool, c: &CompanyRow) -> Result<Value> {
    let snap = store::snapshot(pool, &c.ticker, "dividends").await?;
    let cf = store::statements(pool, c.id, "cashflow", true).await?;
    let buybacks: Vec<Value> = cf
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|s| json!({
            "year": format!("FY{}", s.fiscal_year),
            "amount": line(Some(s), "buybacks").map(f64::abs),
            "dividends": line(Some(s), "dividends_paid").map(f64::abs),
            "shares_out": null,
        }))
        .collect();
    let d = snap_data(&snap);
    let dividends: Vec<Value> = d
        .and_then(|d| d.get("dividends"))
        .and_then(Value::as_array)
        .map(|a| a.iter().rev().take(20).rev().cloned().collect())
        .unwrap_or_default();
    Ok(json!({
        "dividends": dividends,
        "buybacks": buybacks,
        "splits": d.and_then(|d| d.get("splits")).cloned().unwrap_or(json!([])),
        "actions": [],
        "meta": meta(&snap),
    }))
}

/// Insiders (EDGAR), 13F holders and short interest, with short interest as % of shares.
pub async fn ownership_view(pool: &sqlx::PgPool, c: &CompanyRow) -> Result<Value> {
    let holders = store::snapshot(pool, &c.ticker, "holders").await?;
    let si = store::snapshot(pool, &c.ticker, "short_interest").await?;
    let series: Vec<Value> = snap_data(&si)
        .and_then(|d| d.get("series"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pct_series: Vec<(i64, f64)> = series
        .iter()
        .filter_map(|r| {
            let d = super::parse_date(r.get("date")?.as_str()?)?;
            let shares = r.get("shares")?.as_f64()?;
            Some((super::date_ms(d), shares / c.shares_out.filter(|s| *s > 0.0)? * 100.0))
        })
        .collect();
    let last = series.last();
    let h = snap_data(&holders);
    Ok(json!({
        "holders": h.and_then(|h| h.get("holders")).cloned().unwrap_or(json!([])),
        "holders_quarter": h.and_then(|h| h.get("quarter")).cloned(),
        "breakdown": {
            "institutions": h.and_then(|h| h.get("institutions_pct")).cloned(),
            "insiders": null,
        },
        "short_interest": pct_series,
        "short_shares": last.and_then(|r| r.get("shares")).cloned(),
        "short_date": last.and_then(|r| r.get("date")).cloned(),
        "days_to_cover": last.and_then(|r| r.get("days_to_cover")).cloned(),
        "borrow_fee": null,
        "meta": { "holders": meta(&holders), "short_interest": meta(&si) },
    }))
}

/// Peers from the snapshot, with ratios for those stored here too.
pub async fn peers_view(pool: &sqlx::PgPool, c: &CompanyRow) -> Result<Value> {
    let snap = store::snapshot(pool, &c.ticker, "peers").await?;
    let mut tickers: Vec<(String, String, Option<f64>)> = vec![(c.ticker.clone(), c.name.clone(), None)];
    for p in snap_data(&snap).and_then(Value::as_array).cloned().unwrap_or_default() {
        let t = p.get("ticker").and_then(Value::as_str).unwrap_or("").to_uppercase();
        if t.is_empty() || tickers.iter().any(|(x, _, _)| *x == t) {
            continue;
        }
        tickers.push((t, p.get("name").and_then(Value::as_str).unwrap_or("").to_string(), p.get("market_cap").and_then(Value::as_f64)));
    }
    let mut rows = Vec::new();
    for (t, name, mcap) in tickers.into_iter().take(12) {
        let stored = store::company_by_ticker(pool, &t).await?;
        let m = match &stored {
            Some(row) => metrics(pool, row).await?,
            None => Value::Null,
        };
        let g = |k: &str| m.get(k).cloned().unwrap_or(Value::Null);
        rows.push(json!({
            "ticker": t,
            "name": stored.as_ref().map(|r| r.name.clone()).filter(|n| !n.is_empty()).unwrap_or(name),
            "market_cap": m.get("market_cap").and_then(Value::as_f64).or(mcap),
            "pe": g("pe"),
            "ev_ebitda": g("ev_ebitda"),
            "gross_margin": g("gross_margin"),
            "net_margin": g("net_margin"),
            "roe": g("roe"),
            "dividend_yield": g("dividend_yield"),
            "stored": stored.is_some(),
            "self": t == c.ticker,
        }));
    }
    Ok(json!({ "peers": rows, "meta": meta(&snap) }))
}

/// ESG and pay snapshot.
pub async fn ratings_view(pool: &sqlx::PgPool, c: &CompanyRow) -> Result<Value> {
    let snap = store::snapshot(pool, &c.ticker, "esg").await?;
    let mut data = snap_data(&snap).cloned().unwrap_or(json!({ "esg": null, "management": [] }));
    data["credit"] = json!([]);
    data["meta"] = meta(&snap);
    Ok(data)
}

/// The market calendar with recent central-bank moves added.
pub async fn calendar_view(pool: &sqlx::PgPool) -> Result<Value> {
    let snap = store::snapshot(pool, "_", "calendar").await?;
    let banks = store::snapshot(pool, "_", "central_banks").await?;
    let mut data = snap_data(&snap).cloned().unwrap_or(json!({ "earnings": [], "ipos": [], "corporate": [] }));
    let since = (OffsetDateTime::now_utc().date() - time::Duration::days(120)).to_string();
    data["central_banks"] = Value::Array(
        snap_data(&banks)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter(|b| b.get("last_change_date").and_then(Value::as_str).is_some_and(|d| d >= since.as_str()))
                    .map(|b| json!({
                        "date": b.get("last_change_date"),
                        "bank": b.get("bank"),
                        "event": "Rate change",
                        "change": b.get("last_change"),
                        "current": b.get("rate"),
                    }))
                    .collect()
            })
            .unwrap_or_default(),
    );
    data["meta"] = meta(&snap);
    Ok(data)
}

#[cfg(test)]
mod order_tests {
    use super::merge_order;

    #[test]
    fn stored_order_wins_and_new_providers_follow() {
        let d = ["fmp", "alphavantage", "finnhub"];
        assert_eq!(merge_order(&d, None), vec!["fmp", "alphavantage", "finnhub"]);
        let stored = vec!["finnhub".to_string(), "gone".to_string(), "fmp".to_string()];
        assert_eq!(merge_order(&d, Some(&stored)), vec!["finnhub", "fmp", "alphavantage"]);
    }
}
