//! Reading a broker account into the tax form.
//!
//! `POST /api/taxcalc/broker/preview` pulls a window of executions, matches every sale to
//! its acquisitions under the profile's cost method (see `taxcalc::lots`) and adds up what
//! was realized inside the tax year, per tax bucket. **It writes nothing**: the answer
//! fills the form, and the scenario is saved by the user as it always was.
//!
//! Three things decide whether the number is right:
//!
//!   - **The window is not the tax year.** A disposal in March was opened at some earlier
//!     date, and without that fill there is no cost basis. So the pull starts where the
//!     user says (defaulting to the first day of the year) and ends with the year, and a
//!     sale whose purchase is not in the window is reported as an error naming the fix,
//!     never valued against nothing.
//!   - **Each leg converts at its own date.** A purchase costs the tax currency of its own
//!     day and a sale fetches the tax currency of its own day, so the currency move is in
//!     the gain. Margin products convert their realized PnL on the day it is realized.
//!   - **French digital assets are not matched lot by lot.** Under a French profile, spot
//!     crypto follows the global-portfolio formula of CGI 150 VH bis: only exits to legal
//!     tender are taxable, and each needs the value of everything held just before it,
//!     which the user supplies.

use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, Result};
use axum::{
    extract::{Query, State},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{format_description::well_known::Iso8601, Date, Month, OffsetDateTime, Time, UtcOffset};
use uuid::Uuid;

use otw_store::journal_fx;

use crate::brokers::{ExecQuery, Execution};
use crate::taxcalc::{self, lots};
use crate::{ApiError, AppState};

/// The module id a broker account must be granted to for the tax form to read it.
const MODULE: &str = "taxcalc";

/// An ISO day from the form, or nothing when the field was left empty.
fn parse_day(s: Option<&str>) -> Result<Option<Date>> {
    match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => Date::parse(s, &Iso8601::DATE)
            .map(Some)
            .map_err(|_| anyhow!("{s} is not a date. Expected YYYY-MM-DD")),
    }
}

/// Which journal asset class feeds which line of the tax form. The same routing as the
/// journal loader: one number per line of the form, and no class silently unaccounted for.
fn bucket_of(asset_class: &str) -> &'static str {
    match asset_class {
        "option" | "future" | "forex" => "derivative",
        "crypto" => "crypto",
        _ => "capital",
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/taxcalc/broker/preview", post(preview))
}

#[derive(Debug, Deserialize)]
struct Request {
    account_id: Uuid,
    tax_year: i32,
    /// First day of the pull, `YYYY-MM-DD`. Earlier than the tax year is normal and often
    /// necessary: it is where the cost basis of what was sold this year comes from. Absent
    /// = 1 January of the tax year.
    ///
    /// Taken as a string and parsed here: `time::Date` is built without
    /// `serde-human-readable`, so its own deserializer expects `[year, ordinal]` and
    /// rejects the ISO day the browser sends, in the extractor, before the handler runs.
    #[serde(default)]
    from: Option<String>,
    /// Instruments to ask for, in the broker's own spelling. Required by the brokers whose
    /// history is per instrument.
    #[serde(default)]
    symbols: Vec<String>,
    /// Currency the totals are reported in, normally the tax profile's. Absent = the
    /// currency most of the closed positions are already in.
    #[serde(default)]
    currency: Option<String>,
    /// Whether a sell with nothing open opens a short. Absent = what the broker declares.
    #[serde(default)]
    allow_short: Option<bool>,
    /// The tax profile: its cost method, its regime (French crypto rule) and its currency.
    #[serde(default)]
    profile_id: Option<Uuid>,
    /// Value USD and EUR stablecoins at par with their currency. The user's explicit choice:
    /// without it a stablecoin-quoted trade has no rate and is left out.
    #[serde(default)]
    stablecoins_at_par: bool,
    /// French crypto: the global value of all digital assets held just before each sale to
    /// fiat, keyed by the sale's execution id.
    #[serde(default)]
    crypto_values: HashMap<String, f64>,
    /// French crypto: the net total acquisition price before the window (other platforms,
    /// earlier years).
    #[serde(default)]
    crypto_opening_cost: Option<f64>,
}

/// One matched disposal inside the tax year, as the form will count it.
#[derive(Debug, Serialize)]
struct TaxLine {
    ticker: String,
    asset_class: String,
    /// capital | derivative | crypto.
    bucket: &'static str,
    #[serde(with = "time::serde::rfc3339::option")]
    opened_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    closed_at: OffsetDateTime,
    quantity: f64,
    proceeds: f64,
    cost: f64,
    /// Realized gain in the reporting currency, each leg at its own rate.
    gain: f64,
    /// The same gain in the currency traded, for checking.
    native_gain: f64,
    currency: String,
    holding_days: Option<i64>,
    /// average | fifo | same_day | bed_breakfast | s104.
    rule: &'static str,
    short: bool,
    /// Loss deferred onto replacement shares by the anti-wash rule.
    disallowed: f64,
}

#[derive(Debug, Serialize)]
struct Preview {
    account: String,
    broker: String,
    tax_year: i32,
    #[serde(with = "time::serde::rfc3339")]
    from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    to: OffsetDateTime,
    /// The cost method the sales were matched with.
    method: &'static str,
    /// Fills the broker returned for the whole window.
    executions: usize,
    /// Disposals inside the tax year: what the totals are made of.
    closed: usize,
    /// Disposals before the year started. The window was widened to price what was sold
    /// this year, and those earlier disposals belong to an earlier return.
    before_year: usize,
    /// Positions still open at the end of the year. Not a disposal, not counted.
    still_open: usize,
    currency: String,
    capital_gains: f64,
    derivative_gains: f64,
    crypto_gains: f64,
    /// The long-term share of each bucket's gain, under the regime's holding tiers. Only
    /// known when the method dates each lot (FIFO).
    long_term: BTreeMap<&'static str, f64>,
    /// A regime splits by holding period but the method pools lots: the split is unknown.
    long_term_unknown: bool,
    /// Sale proceeds inside the year per bucket, for thresholds measured on proceeds.
    proceeds: BTreeMap<&'static str, f64>,
    /// Currencies the fills were traded in, with how many in each.
    currencies: BTreeMap<String, usize>,
    lines: Vec<TaxLine>,
    /// French crypto (150 VH bis), when the profile is French and the account holds some.
    crypto_fr: Option<lots::CryptoOutcome>,
    /// Crypto-to-crypto swaps skipped under the French rule (neutral, not a disposal).
    crypto_swaps: usize,
    /// What was adjusted and said out loud (a fee left out, a stablecoin at par).
    notes: Vec<String>,
    /// What could not be counted, each naming the fix.
    errors: Vec<String>,
}

async fn preview(
    State(state): State<AppState>,
    Query(bg): Query<crate::tasks::Background>,
    Json(req): Json<Request>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, account) =
        crate::brokers_api::resolve(&state, req.account_id, Some(MODULE)).await?;
    if bg.background {
        let (year_start, year_end) =
            year_bounds(req.tax_year).map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
        let from = parse_day(req.from.as_deref())
            .ok()
            .flatten()
            .map(|d| d.with_time(Time::MIDNIGHT).assume_offset(UtcOffset::UTC))
            .unwrap_or(year_start);
        let estimate = crate::brokers::estimate::executions(
            &account.broker,
            from,
            year_end,
            req.symbols.len(),
        );
        let task_state = state.clone();
        let account_id = account.id;
        let task = crate::tasks::spawn(
            &state,
            "taxcalc".into(),
            format!("Tax: broker pull for {}", req.tax_year),
            "/taxcalc?broker=1".into(),
            MODULE,
            estimate,
            async move {
                let out = run(&task_state, connector.as_ref(), &settings, &account, &req).await?;
                Ok(crate::tasks::Done {
                    summary: format!(
                        "{} disposal(s) in {}, ready to fill the form.",
                        out.closed, out.tax_year
                    ),
                    result: json!({ "preview": out, "account_id": account_id }),
                })
            },
        )
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
        return Ok(Json(json!({ "task": task })));
    }
    let out = run(&state, connector.as_ref(), &settings, &account, &req)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!(out)))
}

async fn run(
    state: &AppState,
    connector: &dyn crate::brokers::Broker,
    settings: &HashMap<String, String>,
    account: &otw_store::brokers::BrokerRow,
    req: &Request,
) -> Result<Preview> {
    let cap = connector.capability();
    if !cap.executions {
        return Err(anyhow!("{} does not publish an execution history", cap.label));
    }
    if cap.needs_symbols && req.symbols.is_empty() {
        return Err(anyhow!(
            "{} answers its trade history one instrument at a time. Name the symbols to read",
            cap.label
        ));
    }
    let profile = match req.profile_id {
        Some(id) => otw_store::taxcalc::get_profile(&state.pool, id)
            .await?
            .map(|p| serde_json::to_value(&p))
            .transpose()?,
        None => None,
    };
    // The profile's regime decides the tax year's bounds, which day's rate converts a leg,
    // the cost method, the anti-wash rule and what counts as long term.
    let reg = profile.as_ref().map(|p| taxcalc::regime_for(p, req.tax_year));
    let (year_start, year_end) = match &reg {
        Some(r) => {
            let (s, e) = r.year_bounds(req.tax_year).map_err(|e| anyhow!(e))?;
            (
                s.with_time(Time::MIDNIGHT).assume_offset(UtcOffset::UTC),
                e.with_time(Time::from_hms(23, 59, 59).unwrap()).assume_offset(UtcOffset::UTC),
            )
        }
        None => year_bounds(req.tax_year)?,
    };
    let from = match parse_day(req.from.as_deref())? {
        Some(d) => d.with_time(Time::MIDNIGHT).assume_offset(UtcOffset::UTC),
        None => year_start,
    };
    if from > year_end {
        return Err(anyhow!("the pull starts after the tax year ends"));
    }

    // A venue with a bounded archive answers an older window with an empty page and no
    // error, which would read as a tax year with no disposals in it.
    crate::brokers::check_window(cap, from)?;

    let q = ExecQuery { from, to: year_end, symbols: req.symbols.clone() };
    let execs = crate::brokers::cache::executions(account.id, connector, settings, &q).await?;

    let method = profile.as_ref().map(|p| taxcalc::cost_method(p, req.tax_year)).unwrap_or(lots::Method::Fifo);
    let wash = reg.as_ref().and_then(taxcalc::wash_rule);
    let french = profile
        .as_ref()
        .and_then(|p| p.get("regime"))
        .and_then(Value::as_str)
        .is_some_and(|r| r.starts_with("fr_"));
    let allow_short = req.allow_short.unwrap_or(!cap.spot_only);

    let mut currencies: BTreeMap<String, usize> = BTreeMap::new();
    for e in &execs {
        *currencies.entry(e.currency.trim().to_uppercase()).or_insert(0) += 1;
    }
    // The reporting currency: what the caller asked for, else the profile's, else whatever
    // most fills are already in, so the common case needs no rate at all.
    let target = req
        .currency
        .as_deref()
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .or_else(|| {
            profile
                .as_ref()
                .and_then(|p| p.get("currency"))
                .and_then(Value::as_str)
                .map(str::to_uppercase)
        })
        .or_else(|| currencies.iter().max_by_key(|(_, n)| **n).map(|(c, _)| c.clone()))
        .unwrap_or_else(|| "USD".to_string());

    let mut out = Preview {
        account: account.name.clone(),
        broker: account.broker.clone(),
        tax_year: req.tax_year,
        from,
        to: year_end,
        method: method.key(),
        executions: execs.len(),
        closed: 0,
        before_year: 0,
        still_open: 0,
        currency: target.clone(),
        capital_gains: 0.0,
        derivative_gains: 0.0,
        crypto_gains: 0.0,
        long_term: BTreeMap::new(),
        long_term_unknown: false,
        proceeds: BTreeMap::new(),
        currencies,
        lines: Vec::new(),
        crypto_fr: None,
        crypto_swaps: 0,
        notes: Vec::new(),
        errors: Vec::new(),
    };

    let mut fx = journal_fx::FxCache::new();
    let mut fills = Vec::with_capacity(execs.len());
    let mut crypto_events = Vec::new();
    let mut crypto_unpriced = false;
    let mut at_par_used = false;
    for e in &execs {
        let margin = is_margin(&account.broker, e);
        let bucket = if margin { "derivative" } else { bucket_of(&e.asset_class) };
        let buy = !e.side.eq_ignore_ascii_case("sell");
        let qty = e.qty.abs();
        if qty <= 0.0 {
            continue;
        }
        let mult = if e.multiplier > 0.0 { e.multiplier } else { 1.0 };
        let gross = qty * e.price * mult;
        let quote = e.currency.trim().to_uppercase();
        // The rate of the day the regime says (same day, the day before, last month's end).
        let date = reg.as_ref().map_or(e.at.date(), |r| r.fx_day(e.at.date()));
        let fee = fee_in(&state.pool, &mut fx, e, &quote, &mut out.notes).await?;

        // France, spot crypto: only a crossing into legal tender is an event.
        if french && !margin && e.asset_class == "crypto" {
            if !is_fiat(&quote) {
                out.crypto_swaps += 1;
                continue;
            }
            let amount = if buy { gross + fee } else { gross - fee };
            match fx.convert(&state.pool, amount, &quote, &target, date).await? {
                Some(v) => crypto_events.push(lots::CryptoEvent {
                    id: e.id.clone(),
                    symbol: e.symbol.clone(),
                    at: e.at,
                    acquisition: buy,
                    amount: v,
                }),
                None => {
                    crypto_unpriced = true;
                    out.errors.push(format!(
                        "{} on {date}: no {quote} to {target} rate. Add it (Settings, FX) and read again",
                        e.symbol
                    ));
                }
            }
            continue;
        }

        let ccy = match stable_peg(&quote) {
            Some(peg) if req.stablecoins_at_par => {
                at_par_used = true;
                peg.to_string()
            }
            _ => quote.clone(),
        };
        let rate = fx.convert(&state.pool, 1.0, &ccy, &target, date).await?;
        fills.push(lots::Fill {
            id: e.id.clone(),
            symbol: e.symbol.clone(),
            asset_class: e.asset_class.clone(),
            bucket,
            valuation: if margin { lots::Valuation::Margin } else { lots::Valuation::Spot },
            at: e.at,
            buy,
            qty,
            gross,
            fee,
            currency: ccy,
            rate,
        });
    }
    if at_par_used {
        out.notes.push("Stablecoins valued at par with the currency they track.".into());
    }

    let outcome = lots::match_fills(fills, method, allow_short, wash.as_ref());
    out.errors.extend(outcome.errors);
    out.still_open = outcome.open.len();
    for d in outcome.disposals {
        if d.closed_at < year_start {
            out.before_year += 1;
            continue;
        }
        out.closed += 1;
        if let Some(r) = reg.as_ref().filter(|r| r.long_after(d.bucket).is_some()) {
            match d.opened_at {
                Some(o) if r.term_between(d.bucket, o.date(), d.closed_at.date()) != "short" => {
                    *out.long_term.entry(d.bucket).or_insert(0.0) += d.gain
                }
                Some(_) => {}
                None => out.long_term_unknown = true,
            }
        }
        *out.proceeds.entry(d.bucket).or_insert(0.0) += d.proceeds;
        match d.bucket {
            "derivative" => out.derivative_gains += d.gain,
            "crypto" => out.crypto_gains += d.gain,
            _ => out.capital_gains += d.gain,
        }
        out.lines.push(TaxLine {
            ticker: d.symbol,
            asset_class: d.asset_class,
            bucket: d.bucket,
            opened_at: d.opened_at,
            closed_at: d.closed_at,
            quantity: d.qty,
            proceeds: round2(d.proceeds),
            cost: round2(d.cost),
            gain: round2(d.gain),
            native_gain: round2(d.native_gain),
            currency: d.currency,
            holding_days: d.holding_days,
            rule: d.rule,
            short: d.short,
            disallowed: round2(d.disallowed),
        });
    }

    if !crypto_events.is_empty() {
        let mut fr = lots::fr_global_portfolio(
            crypto_events,
            &req.crypto_values,
            req.crypto_opening_cost.unwrap_or(0.0),
            req.tax_year,
        );
        if crypto_unpriced {
            fr.year_gain = None;
        }
        out.errors.extend(fr.errors.iter().cloned());
        // The 305 EUR threshold is the regime's to apply, on the proceeds passed along.
        *out.proceeds.entry("crypto").or_insert(0.0) += fr.year_proceeds;
        if let Some(g) = fr.year_gain {
            out.crypto_gains += g;
        }
        out.crypto_fr = Some(fr);
    }

    out.lines.sort_by(|a, b| b.closed_at.cmp(&a.closed_at));
    out.capital_gains = round2(out.capital_gains);
    out.derivative_gains = round2(out.derivative_gains);
    out.crypto_gains = round2(out.crypto_gains);
    for v in out.long_term.values_mut().chain(out.proceeds.values_mut()) {
        *v = round2(*v);
    }
    Ok(out)
}

/// Futures, CFDs, margin forex and perpetuals settle the difference only: their PnL
/// converts on the day it is realized. Told by the asset class, by the venues that only
/// sell margin products, and by a futures book's own venue label.
fn is_margin(broker: &str, e: &Execution) -> bool {
    matches!(e.asset_class.as_str(), "future" | "forex")
        || matches!(broker, "binance_futures" | "capitalcom" | "oanda" | "forexcom")
        || e.venue.to_lowercase().contains("futures")
}

/// Legal tender the FX table can price. A stablecoin is a digital asset, not legal tender.
fn is_fiat(ccy: &str) -> bool {
    ccy == "USD" || journal_fx::FX_QUOTES.contains(&ccy)
}

/// The currency a stablecoin tracks, when it is one.
fn stable_peg(ccy: &str) -> Option<&'static str> {
    match ccy {
        "USDT" | "USDC" | "BUSD" | "FDUSD" | "TUSD" | "DAI" | "USDP" | "PYUSD" | "USDE" => Some("USD"),
        "EURC" | "EURT" | "EURI" => Some("EUR"),
        _ => None,
    }
}

/// The fill's commission in its quote currency. A fee billed in another asset converts at
/// the fill's date; one that cannot is left out and said out loud (the gain is then
/// overstated, never understated).
async fn fee_in(
    pool: &sqlx::PgPool,
    fx: &mut journal_fx::FxCache,
    e: &Execution,
    quote: &str,
    notes: &mut Vec<String>,
) -> Result<f64> {
    let fee = e.fee.abs();
    let fee_ccy = e.fee_currency.trim().to_uppercase();
    if fee == 0.0 || fee_ccy.is_empty() || fee_ccy == quote {
        return Ok(fee);
    }
    Ok(match fx.convert(pool, fee, &fee_ccy, quote, e.at.date()).await? {
        Some(v) => v,
        None => {
            notes.push(format!(
                "{} on {}: fee of {fee} {fee_ccy} left out (no {fee_ccy} to {quote} rate)",
                e.symbol,
                e.at.date()
            ));
            0.0
        }
    })
}

/// The tax year as an instant pair, UTC. A broker stamps in its own zone and a fill on
/// 31 December at 23:00 local is still that year's.
fn year_bounds(year: i32) -> Result<(OffsetDateTime, OffsetDateTime)> {
    let start = Date::from_calendar_date(year, Month::January, 1)
        .map_err(|_| anyhow!("{year} is not a year"))?;
    let end = Date::from_calendar_date(year, Month::December, 31)
        .map_err(|_| anyhow!("{year} is not a year"))?;
    Ok((
        start.with_time(Time::MIDNIGHT).assume_offset(UtcOffset::UTC),
        end.with_time(Time::from_hms(23, 59, 59).unwrap())
            .assume_offset(UtcOffset::UTC),
    ))
}

fn round2(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_class_lands_on_a_line_of_the_form() {
        assert_eq!(bucket_of("stock"), "capital");
        assert_eq!(bucket_of("etf"), "capital");
        assert_eq!(bucket_of("other"), "capital");
        assert_eq!(bucket_of("option"), "derivative");
        assert_eq!(bucket_of("future"), "derivative");
        assert_eq!(bucket_of("forex"), "derivative");
        assert_eq!(bucket_of("crypto"), "crypto");
        // An asset class nobody planned for is still counted, on the line that taxes a
        // plain disposal. Dropping it would understate the return.
        assert_eq!(bucket_of("bond"), "capital");
    }

    #[test]
    fn the_year_is_taken_whole() {
        let (a, b) = year_bounds(2025).unwrap();
        assert_eq!(a.to_string(), "2025-01-01 0:00:00.0 +00:00:00");
        assert_eq!(b.to_string(), "2025-12-31 23:59:59.0 +00:00:00");
        assert!(year_bounds(0).is_ok(), "a year with no trades is still a year");
    }
}
