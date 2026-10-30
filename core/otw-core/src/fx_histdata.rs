//! Deriving an FX rate from a stored histdata series, the third link in the journal's
//! resolution chain (stored rate, online API, **here**, manual entry).
//!
//! The online source serves the eleven majors and nothing else. A user who imports a
//! Binance book trades in USDT, a Coinbase book in USDC: real currencies with real markets
//! that Frankfurter will never quote. Those markets are already downloadable through the
//! ordinary histdata queue, so the rate is read off a daily candle rather than invented.
//!
//! What is produced is the same number the rest of the journal stores: `1 USD = rate
//! <quote>`, written into `journal_fx_rates` with source `histdata`. It is an ordinary row
//! from then on: carried forward like any other, and overwritten by a manual entry, which
//! always wins.
//!
//! **Nothing is ever guessed.** A currency with no market anywhere in the catalog returns
//! `None` and stays pending until the user types a number. A stablecoin priced against
//! another stablecoin is the one approximation allowed, and it is reported: the ticker that
//! produced the rate travels with it so the UI can show what was used.
//!
//! The conventional tickers still only find what a venue happens to name that way. When
//! they miss, the user states the market instead: a connector plus the pair as that venue
//! writes it (BinanceMain, `USDTUSD`), stored per currency in `journal_fx_sources`. A
//! stated source replaces the conventions rather than leading them, for reading and for
//! downloading both: the user named the market, so no other ticker is tried behind it.

use anyhow::{anyhow, Result};
use time::{Date, OffsetDateTime, Time};
use uuid::Uuid;

use crate::histdata;
use crate::journal_market::MODULE;
use otw_store::connectors as conn_store;
use otw_store::histdata as store;
use otw_store::journal_fx as fx_store;

/// Daily candles are the grain: FX rates are a daily close, and a finer series would make
/// the answer depend on the hour the user happened to trade.
const TF: &str = "1d";

/// How far back a queued download reaches when the user asks for a missing series. A rate
/// is needed for the trade's own date, which can be years back, and a short window would
/// queue a download that still cannot answer the question that triggered it.
const DOWNLOAD_DAYS: i64 = 365 * 6;

/// A rate read off a stored series.
pub struct Derived {
    /// 1 USD = `rate` units of the quote currency.
    pub rate: f64,
    /// The dataset ticker it came from, shown to the user.
    pub ticker: String,
    /// True when the price came from a stablecoin pair rather than a USD pair, so the
    /// rate assumes USDT/USDC ≈ USD. The UI says so; a manual entry overrides it.
    pub proxy: bool,
}

/// The tickers that could price `quote` against USD, best first.
///
/// Both directions are tried because a market is listed one way round only: `USDJPY`
/// exists and `JPYUSD` does not, `BTCUSD` exists and `USDBTC` does not. The stablecoin
/// pairs come last: they price a currency the majors cannot reach, at the cost of assuming
/// the stablecoin holds its peg.
fn candidates(quote: &str) -> Vec<(String, bool, bool)> {
    // (ticker, invert, proxy): `invert` when the ticker prices one unit of `quote` in USD,
    // since what we store is the other way round.
    let q = quote.to_ascii_uppercase();
    let mut out = vec![
        (format!("USD{q}"), false, false),
        (format!("{q}USD"), true, false),
    ];
    for stable in ["USDT", "USDC"] {
        if q != stable {
            out.push((format!("{stable}{q}"), false, true));
            out.push((format!("{q}{stable}"), true, true));
        }
    }
    out
}

/// One way to price `quote`: a ticker, how to read it, and who serves it.
struct Route {
    ticker: String,
    invert: bool,
    proxy: bool,
    asset_type: String,
    /// Set by a stated source only: the provider that must serve this ticker. A guessed
    /// ticker takes whichever connector can, which is exactly the difference.
    provider: Option<String>,
    /// The connector a download goes to. `None` on a stated source means that connector
    /// was deleted, so the mapping is visible but unusable until it is repointed.
    connector_id: Option<Uuid>,
}

/// The routes to try for `quote`, best first.
///
/// A stated source is the only route when there is one. Falling through to a guessed
/// ticker behind it would price the currency off a market the user did not name, which is
/// the guess this table exists to remove.
async fn routes(pool: &sqlx::PgPool, quote: &str) -> Result<Vec<Route>> {
    if let Some(s) = fx_store::get_source(pool, quote).await? {
        return Ok(vec![Route {
            ticker: s.ticker,
            invert: s.invert,
            // The direction the user picked is stated against USD: choosing "quotes USD per
            // USDT" on a USDC pair is the user accepting that peg, not us assuming it.
            proxy: false,
            asset_type: s.asset_type,
            provider: Some(s.provider),
            connector_id: s.connector_id,
        }]);
    }
    Ok(candidates(quote)
        .into_iter()
        .map(|(ticker, invert, proxy)| Route {
            ticker,
            // A stablecoin pair is a crypto market; a USD pair is an FX one. Asking the
            // wrong asset type of a connector is how a download gets queued and refused.
            asset_type: if proxy { "crypto" } else { "fx" }.to_string(),
            invert,
            proxy,
            provider: None,
            connector_id: None,
        })
        .collect())
}

/// Whether a granted connector can serve this asset type at the daily grain.
fn serves(c: &conn_store::ConnectorRow, asset_type: &str) -> bool {
    histdata::validate_request(&c.provider, asset_type, TF).is_ok()
}

/// End of `date` in UTC: the bound a daily close must fall at or before.
fn end_of_day(date: Date) -> OffsetDateTime {
    OffsetDateTime::new_utc(date, Time::from_hms(23, 59, 59).expect("valid constant time"))
}

/// `1 USD = rate <quote>` on `date`, from whichever stored series can price it.
///
/// Carries forward exactly like [`otw_store::journal_fx`] does: the last close at or
/// before the date, so a weekend or a holiday reuses the prior session rather than
/// reporting a hole. Reads only; it never fetches.
pub async fn derive(pool: &sqlx::PgPool, quote: &str, date: Date) -> Result<Option<Derived>> {
    if quote.eq_ignore_ascii_case("USD") {
        return Ok(None); // the base is implicit and never stored
    }
    for r in routes(pool, quote).await? {
        // A stated source reads that connector's series and no other: the same ticker on
        // another venue is another tape.
        let ds = match &r.provider {
            Some(p) => store::find_dataset_any(pool, &r.asset_type, &r.ticker, TF, Some(p))
                .await?
                .filter(|d| &d.provider == p),
            None => store::find_dataset_by_ticker(pool, &r.ticker, TF).await?,
        };
        let Some(ds) = ds else {
            continue;
        };
        let bars = store::read_bars(pool, ds.id, None, Some(end_of_day(date)), 1).await?;
        let Some(bar) = bars.last() else {
            continue; // the dataset exists but holds nothing at or before the date
        };
        if !(bar.close.is_finite() && bar.close > 0.0) {
            continue;
        }
        let rate = if r.invert { 1.0 / bar.close } else { bar.close };
        if !(rate.is_finite() && rate > 0.0) {
            continue;
        }
        return Ok(Some(Derived { rate, ticker: r.ticker, proxy: r.proxy }));
    }
    Ok(None)
}

/// Derive a rate and store it, clearing the pending task. `Ok(None)` means no market for
/// this currency is stored, which is the signal to fall through to manual entry.
pub async fn resolve(pool: &sqlx::PgPool, quote: &str, date: Date) -> Result<Option<Derived>> {
    let Some(d) = derive(pool, quote, date).await? else {
        return Ok(None);
    };
    otw_store::journal_fx::upsert_rates(pool, date, &[(quote.to_string(), d.rate)], "histdata")
        .await?;
    Ok(Some(d))
}

/// Which tickers a download could go and get for this currency, with the provider that
/// would serve each. Empty means nothing can fetch it: no granted connector serves the
/// conventional pairs, or the stated source points at a connector that is gone. The UI
/// then offers naming a market, or manual entry.
pub async fn downloadable(pool: &sqlx::PgPool, quote: &str) -> Result<Vec<(String, String)>> {
    let granted = conn_store::list_for_module(pool, MODULE).await?;
    let mut out = Vec::new();
    for r in routes(pool, quote).await? {
        match &r.provider {
            // A stated source is offered while its connector still exists, is still granted
            // to the journal, and still serves that asset type at this grain.
            Some(p) => {
                let live = r.connector_id.is_some_and(|id| granted.iter().any(|c| c.id == id));
                if live && histdata::validate_request(p, &r.asset_type, TF).is_ok() {
                    out.push((r.ticker, p.clone()));
                }
            }
            None => {
                if let Some(c) = granted.iter().find(|c| serves(c, &r.asset_type)) {
                    out.push((r.ticker, c.provider.clone()));
                }
            }
        }
    }
    Ok(out)
}

/// Queue the download of a series that could price `quote`, so a later resolve can read it.
///
/// The stated source when there is one, otherwise the first candidate ticker a granted
/// connector can serve. Returns the job id and the ticker queued; an error when nothing
/// serves this currency, naming what is missing rather than queueing a job that cannot
/// succeed.
pub async fn enqueue(pool: &sqlx::PgPool, quote: &str, date: Date) -> Result<(Uuid, String)> {
    let granted = conn_store::list_for_module(pool, MODULE).await?;
    for r in routes(pool, quote).await? {
        let (connector_id, provider) = match &r.provider {
            // A stated source names its connector, so a missing one is an error naming the
            // fix, never a quiet fall back to some other venue's tape.
            Some(p) => {
                // The provider first: an imported series says so itself, and reporting a
                // missing connector for it would name the wrong fix.
                histdata::validate_request(p, &r.asset_type, TF)?;
                let Some(id) = r.connector_id.filter(|id| granted.iter().any(|c| c.id == *id))
                else {
                    return Err(anyhow!(
                        "the connector set for {quote} is gone or no longer granted to the \
                         journal: point {quote} at another one"
                    ));
                };
                (id, p.clone())
            }
            None => match granted.iter().find(|c| serves(c, &r.asset_type)) {
                Some(c) => (c.id, c.provider.clone()),
                None => continue,
            },
        };
        // Reach back from the date that needs the rate, not from today: the pending task
        // that triggered this is usually older than any default window.
        let range_to = OffsetDateTime::now_utc();
        let range_from = end_of_day(date) - time::Duration::days(DOWNLOAD_DAYS);
        let dataset_id =
            store::upsert_dataset(pool, &provider, &r.asset_type, &r.ticker, TF).await?;
        let job = store::enqueue_job(
            pool,
            &store::NewJob {
                dataset_id,
                connector_id: Some(connector_id),
                provider: &provider,
                asset_type: &r.asset_type,
                ticker: &r.ticker,
                timeframe: TF,
                range_from,
                range_to,
                kind: "download",
                batch_id: None,
            },
        )
        .await?;
        return Ok((job, r.ticker));
    }
    Err(anyhow!(
        "no connector granted to the journal serves a market for {quote}: name the pair \
         yourself (connector + ticker) or enter the rate by hand"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_directions_and_stablecoins_are_offered() {
        let c = candidates("JPY");
        let tickers: Vec<&str> = c.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(tickers[0], "USDJPY");
        assert_eq!(tickers[1], "JPYUSD");
        assert!(tickers.contains(&"JPYUSDT"));
        // USD pairs come before the stablecoin approximations.
        assert!(!c[0].2 && !c[1].2);
    }

    #[test]
    fn a_stablecoin_is_not_priced_against_itself() {
        let c = candidates("USDT");
        let tickers: Vec<&str> = c.iter().map(|(t, _, _)| t.as_str()).collect();
        assert!(!tickers.iter().any(|t| *t == "USDTUSDT"));
        // USDCUSDT is the pair that actually prices USDT on a crypto venue.
        assert!(tickers.contains(&"USDCUSDT"));
    }

    #[test]
    fn inversion_is_flagged_per_direction() {
        let c = candidates("EUR");
        let usd_eur = c.iter().find(|(t, _, _)| t == "USDEUR").unwrap();
        let eur_usd = c.iter().find(|(t, _, _)| t == "EURUSD").unwrap();
        assert!(!usd_eur.1, "USDEUR already quotes EUR per USD");
        assert!(eur_usd.1, "EURUSD quotes USD per EUR and must be inverted");
    }
}
