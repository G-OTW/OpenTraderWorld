//! Building the substrate: everything the analyzers read, built once per request.
//!
//! Only what the request needs is built. A `?blocks=book` call issues no bar query and no
//! connector call, which is what keeps the page instant on an install with nothing
//! configured; a call that wants stress pays for the bars once and every analyzer that
//! asked for them borrows the same copy.

use std::collections::BTreeMap;

use anyhow::Result;
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};
use uuid::Uuid;

use otw_store::histdata as hist;
use otw_store::portfolios::{self as store, Book, Portfolio};
use otw_store::portfolios_risk::{self as risk_store, Target};

use super::{Curve, Needs, Window};

/// Data module id the portfolio reads market data as. Its connector grants are what decide
/// which provider's stored bars it prefers.
pub const MODULE: &str = "portfolios";

/// Correlations, betas and replays are measured on **daily** candles whatever else the
/// module does. A session-anchored 4h bar and an epoch-anchored one are 90 minutes apart, so
/// an intraday cross-asset measure measures the stamping convention, not the market.
pub const TIMEFRAME: &str = "1d";

/// Daily closes of one instrument, oldest first.
#[derive(Debug, Clone, Default)]
pub struct Series {
    pub dates: Vec<String>,
    pub closes: Vec<f64>,
}

impl Series {
    fn from_bars(bars: &[hist::Bar]) -> Series {
        Series {
            dates: bars.iter().map(|b| b.ts.date().to_string()).collect(),
            closes: bars.iter().map(|b| b.close).collect(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.closes.is_empty()
    }
    /// Closes indexed by day, for aligning two series without a merge pass.
    pub fn by_date(&self) -> BTreeMap<&str, f64> {
        self.dates.iter().map(|d| d.as_str()).zip(self.closes.iter().copied()).collect()
    }
    /// Closes over a window, inclusive.
    pub fn between(&self, from: &str, to: &str) -> Series {
        let mut out = Series::default();
        for (d, c) in self.dates.iter().zip(&self.closes) {
            if d.as_str() >= from && d.as_str() <= to {
                out.dates.push(d.clone());
                out.closes.push(*c);
            }
        }
        out
    }
}

/// Daily closes of the held instruments, keyed by asset id.
///
/// `uncovered` names the assets that have none, and it is never empty by accident: an asset
/// with no bar symbol, or one whose symbol has no stored series, is *named*, never proxied
/// to something that happens to price.
#[derive(Debug, Default)]
pub struct Marks {
    pub series: BTreeMap<Uuid, Series>,
    pub uncovered: Vec<String>,
}

/// A factor is a real instrument, never an abstract axis: "rates +200bp" has to become a
/// price move on something with a tape before it can move a portfolio.
pub struct FactorDef {
    pub id: &'static str,
    /// Tickers that would serve as the proxy, best first. The first one with stored bars wins.
    pub proxies: &'static [&'static str],
    /// A rates factor is quoted in basis points and reaches the book through a duration;
    /// everything else is quoted as a price move.
    pub in_bps: bool,
}

pub const FACTOR_DEFS: &[FactorDef] = &[
    FactorDef { id: "sp500", proxies: &["SPY", "^GSPC", "VOO", "IVV", "ES=F"], in_bps: false },
    FactorDef { id: "nasdaq", proxies: &["QQQ", "^IXIC", "^NDX", "NQ=F"], in_bps: false },
    FactorDef { id: "rates", proxies: &["IEF", "TLT", "AGG", "ZN=F"], in_bps: true },
    FactorDef { id: "eurusd", proxies: &["EURUSD", "EURUSD=X", "EUR/USD", "EURUSDT"], in_bps: false },
    FactorDef { id: "oil", proxies: &["USO", "CL=F", "BNO", "BZ=F"], in_bps: false },
    FactorDef { id: "credit", proxies: &["HYG", "JNK"], in_bps: false },
];

pub fn factor_def(id: &str) -> Option<&'static FactorDef> {
    FACTOR_DEFS.iter().find(|f| f.id == id)
}

/// The factor proxies that actually resolved, and which did not.
#[derive(Debug, Default)]
pub struct Factors {
    pub series: BTreeMap<String, Series>,
    /// Factor ids with no stored proxy series. A scenario naming one says so instead of
    /// pretending the leg had no effect.
    pub missing: Vec<String>,
    /// The proxy each factor actually resolved to, so the answer can be audited.
    pub resolved: BTreeMap<String, String>,
}

/// Everything the analyzers read. Each field is `Some` exactly when a selected analysis
/// asked for it.
pub struct Substrate<'a> {
    pub pf: &'a Portfolio,
    pub window: Window,
    pub today: Date,
    pub book: Option<Book>,
    pub curve: Option<Curve>,
    pub marks: Option<Marks>,
    pub benchmark: Option<Series>,
    pub factors: Option<Factors>,
    pub targets: Option<Vec<Target>>,
    /// Days of history the market-data substrates were read over.
    pub lookback_days: i64,
}

/// How far back the bar reads go. Two years is long enough for a beta to mean something and
/// short enough that it describes the market the book is actually in.
pub const LOOKBACK_DAYS: i64 = 730;

/// Bars needed to cover a historical replay window, capped so a 2008 scenario does not read
/// the entire table.
const REPLAY_CAP: i64 = 8000;

impl<'a> Substrate<'a> {
    /// Build exactly what `needs` asks for.
    pub async fn build(
        pool: &PgPool,
        pf: &'a Portfolio,
        window: Window,
        needs: Needs,
        history_from: Option<Date>,
    ) -> Result<Substrate<'a>> {
        let today = Window::today();
        // Marks are the only substrate whose depth varies: a historical replay reaches back
        // to whatever window it names, everything else to the beta lookback.
        let lookback_days = match history_from {
            Some(d) => (today - d).whole_days().max(LOOKBACK_DAYS),
            None => LOOKBACK_DAYS,
        };

        // The book is also the source of *which* instruments the market substrates need, so
        // anything reading bars implies it.
        let needs_book = needs.has(Needs::BOOK)
            || needs.has(Needs::MARKS)
            || needs.has(Needs::FACTORS)
            || needs.has(Needs::TARGETS);
        let book = if needs_book { Some(store::book(pool, pf).await?) } else { None };

        // The curve is read **whole**, not windowed: `performance` reports every named
        // window from one series, and each analyzer slices what it needs. Reading per window
        // would be one range scan per label for the same rows.
        let curve = if needs.has(Needs::CURVE) {
            let snaps = store::snapshots_between(pool, pf.id, None, window.to).await?;
            Some(Curve::build(&pf.currency, &snaps))
        } else {
            None
        };

        let marks = match (needs.has(Needs::MARKS) || needs.has(Needs::FACTORS), &book) {
            (true, Some(b)) => Some(load_marks(pool, b, lookback_days).await?),
            _ => None,
        };

        let benchmark = if needs.has(Needs::BENCHMARK) {
            // The index has to reach as far back as the curve does, not as far as the beta
            // lookback: read over two years, a five-year book would be compared against its
            // last two and the block would still call the answer "since inception".
            let days = curve
                .as_ref()
                .and_then(|c| c.dates.first())
                .and_then(|d| Date::parse(d, &time::format_description::well_known::Iso8601::DATE).ok())
                .map(|d| (today - d).whole_days())
                .unwrap_or(0)
                .max(lookback_days);
            match pf.benchmark.as_ref().and_then(benchmark_symbol) {
                Some(sym) => load_series(pool, &sym, days).await?,
                None => None,
            }
        } else {
            None
        };

        let factors = if needs.has(Needs::FACTORS) {
            Some(load_factors(pool, lookback_days).await?)
        } else {
            None
        };

        let targets = if needs.has(Needs::TARGETS) {
            Some(risk_store::list_targets(pool, pf.id).await?)
        } else {
            None
        };

        Ok(Substrate {
            pf,
            window,
            today,
            book,
            curve,
            marks,
            benchmark,
            factors,
            targets,
            lookback_days,
        })
    }
}

/// The ticker a stored benchmark coordinate names.
pub fn benchmark_symbol(v: &serde_json::Value) -> Option<String> {
    v.get("symbol")?.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Daily bars of one ticker, whatever asset type it was filed under and whichever provider
/// filled it. Bars are bars.
async fn load_series(pool: &PgPool, ticker: &str, days: i64) -> Result<Option<Series>> {
    let Some(ds) = hist::find_dataset_by_ticker(pool, ticker, TIMEFRAME).await? else {
        return Ok(None);
    };
    let from = OffsetDateTime::now_utc() - Duration::days(days);
    let bars = hist::read_bars(pool, ds.id, Some(from), None, REPLAY_CAP).await?;
    if bars.is_empty() {
        return Ok(None);
    }
    Ok(Some(Series::from_bars(&bars)))
}

async fn load_marks(pool: &PgPool, book: &Book, days: i64) -> Result<Marks> {
    let mut out = Marks::default();
    // No connector call: the grant decides which provider's copy is preferred when one is
    // downloaded, not whether bars already on disk may be read.
    for p in &book.positions {
        if p.quantity <= 0.0 {
            continue;
        }
        let symbol = p.asset.bar_symbol();
        if symbol.is_empty() {
            out.uncovered.push(p.asset.symbol.clone());
            continue;
        }
        match load_series(pool, symbol, days).await? {
            Some(s) => {
                out.series.insert(p.asset.id, s);
            }
            None => out.uncovered.push(p.asset.symbol.clone()),
        }
    }
    Ok(out)
}

async fn load_factors(pool: &PgPool, days: i64) -> Result<Factors> {
    let mut out = Factors::default();
    for def in FACTOR_DEFS {
        let mut found = false;
        for proxy in def.proxies {
            if let Some(s) = load_series(pool, proxy, days).await? {
                out.resolved.insert(def.id.to_string(), (*proxy).to_string());
                out.series.insert(def.id.to_string(), s);
                found = true;
                break;
            }
        }
        if !found {
            out.missing.push(def.id.to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(days: &[(&str, f64)]) -> Series {
        Series {
            dates: days.iter().map(|(d, _)| d.to_string()).collect(),
            closes: days.iter().map(|(_, c)| *c).collect(),
        }
    }

    #[test]
    fn a_window_keeps_only_its_days() {
        let s = series(&[("2026-01-01", 1.0), ("2026-01-05", 2.0), ("2026-02-01", 3.0)]);
        let w = s.between("2026-01-02", "2026-01-31");
        assert_eq!(w.dates, vec!["2026-01-05"]);
        assert_eq!(w.closes, vec![2.0]);
    }

    #[test]
    fn every_factor_has_at_least_one_proxy() {
        for f in FACTOR_DEFS {
            assert!(!f.proxies.is_empty(), "{} has no proxy", f.id);
        }
        assert!(factor_def("rates").unwrap().in_bps);
        assert!(!factor_def("sp500").unwrap().in_bps);
        assert!(factor_def("nope").is_none());
    }

    #[test]
    fn a_benchmark_needs_a_symbol() {
        assert_eq!(
            benchmark_symbol(&serde_json::json!({"asset_type":"equity","symbol":"SPY"})),
            Some("SPY".into())
        );
        assert_eq!(benchmark_symbol(&serde_json::json!({"symbol":"  "})), None);
        assert_eq!(benchmark_symbol(&serde_json::json!({"asset_type":"equity"})), None);
    }
}
