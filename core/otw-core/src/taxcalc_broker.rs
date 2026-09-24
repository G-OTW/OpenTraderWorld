//! Reading a broker account into the tax form.
//!
//! `POST /api/taxcalc/broker/preview` pulls a window of executions, folds them into closed
//! positions the way the journal import does, and adds up what was realized inside the tax
//! year, per tax bucket. **It writes nothing**: the answer fills the form, and the scenario
//! is saved by the user as it always was.
//!
//! Two things decide whether the number is right:
//!
//!   - **The window is not the tax year.** A disposal in March was opened at some earlier
//!     date, and without that fill there is no cost basis. So the pull starts where the
//!     user says (defaulting to the first day of the year) and ends with the year, and a
//!     sale whose purchase is not in the window is reported as an error naming the fix,
//!     never valued against nothing.
//!   - **A gain is in the currency it was made in.** Each closed position is converted at
//!     the rate of **its own exit date**, not one rate for the year: that is the date the
//!     disposal happened. A position whose rate is missing is listed and left out of the
//!     totals rather than added in the wrong money.

use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, Result};
use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{format_description::well_known::Iso8601, Date, Month, OffsetDateTime, Time, UtcOffset};
use uuid::Uuid;

use otw_store::{journal, journal_fx};

use crate::brokers::ExecQuery;
use crate::journal_import::broker as jbroker;
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
}

/// One closed position inside the tax year, as the form will count it.
#[derive(Debug, Serialize)]
struct TaxLine {
    ticker: String,
    asset_class: String,
    /// capital | derivative | crypto.
    bucket: &'static str,
    #[serde(with = "time::serde::rfc3339::option")]
    closed_at: Option<OffsetDateTime>,
    quantity: f64,
    /// Realized PnL net of fees, in `currency`.
    pnl: f64,
    /// The currency the position was traded in.
    currency: String,
    /// The same PnL in the reporting currency, absent when no rate was available.
    converted: Option<f64>,
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
    /// Fills the broker returned for the whole window.
    executions: usize,
    /// Positions closed inside the tax year: what the totals are made of.
    closed: usize,
    /// Positions closed before the year started. The window was widened to price what was
    /// sold this year, and those earlier disposals belong to an earlier return.
    before_year: usize,
    /// Positions still open at the end of the year. Not a disposal, not taxable, not counted.
    still_open: usize,
    currency: String,
    capital_gains: f64,
    derivative_gains: f64,
    crypto_gains: f64,
    /// Fees already deducted from the gains above, reported so the figure can be checked.
    fees: f64,
    /// Positions whose currency could not be converted. **Not** in the totals.
    unconverted: Vec<String>,
    /// Currencies the closed positions were traded in, with how many in each.
    currencies: BTreeMap<String, usize>,
    lines: Vec<TaxLine>,
    /// Fills that could not be folded, each naming what to fix (most often: a sale whose
    /// purchase is older than the window).
    errors: Vec<String>,
}

async fn preview(
    State(state): State<AppState>,
    Json(req): Json<Request>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, account) =
        crate::brokers_api::resolve(&state, req.account_id, Some(MODULE)).await?;
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
    let (year_start, year_end) = year_bounds(req.tax_year)?;
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

    let spot_only = !req.allow_short.unwrap_or(!cap.spot_only);
    let q = ExecQuery {
        from,
        to: year_end,
        symbols: req.symbols.clone(),
    };
    // Nothing is stored, so there is no book to file into: the fold only needs a place to
    // put a category id it will never use.
    let folded = jbroker::fold_window(
        &state.pool,
        connector,
        settings,
        &q,
        Uuid::nil(),
        None,
        None,
        spot_only,
    )
    .await?;

    let mut out = Preview {
        account: account.name.clone(),
        broker: account.broker.clone(),
        tax_year: req.tax_year,
        from,
        to: year_end,
        executions: folded.executions,
        closed: 0,
        before_year: 0,
        still_open: 0,
        currency: String::new(),
        capital_gains: 0.0,
        derivative_gains: 0.0,
        crypto_gains: 0.0,
        fees: 0.0,
        unconverted: Vec::new(),
        currencies: BTreeMap::new(),
        lines: Vec::new(),
        errors: folded.errors.into_iter().map(|e| e.message).collect(),
    };

    // First pass: keep the disposals that belong to this return, and note what they are
    // priced in. The reporting currency cannot be chosen before that is known.
    let mut kept = Vec::new();
    for t in &folded.trades {
        let pnl = journal::preview_pnl(&t.input);
        let Some(net) = pnl.net_pnl else {
            out.still_open += 1;
            continue;
        };
        let Some(exit_at) = t.input.exit_at else {
            out.still_open += 1;
            continue;
        };
        if exit_at < year_start {
            out.before_year += 1;
            continue;
        }
        let ccy = t.input.currency.trim().to_uppercase();
        *out.currencies.entry(ccy.clone()).or_insert(0) += 1;
        out.closed += 1;
        // A folded position carries its costs on the legs, not on the flat `fees` field:
        // each fill paid its own commission.
        out.fees += leg_fees(t);
        kept.push((t, net, exit_at, ccy));
    }

    // The reporting currency: what the profile asked for, else whatever most of the
    // disposals are already in, so the common case needs no rate at all.
    let target = req
        .currency
        .as_deref()
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .or_else(|| {
            out.currencies
                .iter()
                .max_by_key(|(_, n)| **n)
                .map(|(c, _)| c.clone())
        })
        .unwrap_or_else(|| "USD".to_string());
    out.currency = target.clone();

    let mut fx = journal_fx::FxCache::new();
    for (t, net, exit_at, ccy) in kept {
        let converted = if ccy.is_empty() || ccy == target {
            Some(net)
        } else {
            fx.convert(&state.pool, net, &ccy, &target, exit_at.date())
                .await
                .ok()
                .flatten()
        };
        match converted {
            Some(v) => match bucket_of(&t.input.asset_class) {
                "derivative" => out.derivative_gains += v,
                "crypto" => out.crypto_gains += v,
                _ => out.capital_gains += v,
            },
            None => out.unconverted.push(format!(
                "{} ({ccy}, {})",
                t.input.ticker,
                exit_at.date()
            )),
        }
        out.lines.push(TaxLine {
            ticker: t.input.ticker.clone(),
            asset_class: t.input.asset_class.clone(),
            bucket: bucket_of(&t.input.asset_class),
            closed_at: Some(exit_at),
            quantity: t.input.exits.iter().map(|l| l.qty).sum(),
            pnl: round2(net),
            currency: ccy,
            converted: converted.map(round2),
        });
    }

    out.lines.sort_by(|a, b| b.closed_at.cmp(&a.closed_at));
    out.capital_gains = round2(out.capital_gains);
    out.derivative_gains = round2(out.derivative_gains);
    out.crypto_gains = round2(out.crypto_gains);
    out.fees = round2(out.fees);
    Ok(out)
}

/// What a folded position paid in commissions: every leg's own fee. The flat `fees` field
/// is zero on these trades by construction, and reading it would report no costs at all.
fn leg_fees(t: &crate::journal_import::build::BuiltTrade) -> f64 {
    t.input.entries.iter().chain(t.input.exits.iter()).map(|l| l.fees).sum()
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
