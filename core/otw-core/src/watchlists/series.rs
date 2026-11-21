//! Daily close series of a watchlist's symbols, for the compare chart and the correlation
//! matrix.
//!
//! 1M comes from the daily history every quote already caches (no request). 3M and 1Y
//! come from stored candles in the data broker (Historical Data downloads), read only:
//! nothing is fetched here. A symbol whose window is not covered is reported with the
//! reason, never drawn from a partial series.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use super::quotes::{self, Coords};
use otw_store::watchlists as store;

const DAY: i64 = 86_400;

/// Correlation needs at least this many overlapping daily returns; below it the number is
/// noise and is refused.
pub const MIN_OVERLAP: usize = 10;

/// Window in days for the accepted `window` query values.
pub fn window_days(w: &str) -> Option<i64> {
    match w {
        "1m" => Some(30),
        "3m" => Some(91),
        "1y" => Some(365),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
pub struct Series {
    pub item_id: Uuid,
    pub symbol: String,
    /// "quote" (cached daily history) or "stored" (Historical Data candles).
    pub source: &'static str,
    /// Ascending (unix_sec, close).
    pub points: Vec<(i64, f64)>,
}

#[derive(Debug, Serialize)]
pub struct NotCovered {
    pub item_id: Uuid,
    pub symbol: String,
    pub reason: String,
}

/// Whether a series covers `[start, now]`: it must begin within 5 days of the window start
/// (weekends and holidays) and end within 7 days of now, with at least two points.
pub fn window_covered(points: &[(i64, f64)], start: i64, now: i64) -> Result<(), String> {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Err("no daily closes".into());
    };
    if points.len() < 2 {
        return Err("only one daily close".into());
    }
    if first.0 > start + 5 * DAY {
        return Err(format!("history starts on {}, after the window start", day_label(first.0)));
    }
    if last.0 < now - 7 * DAY {
        return Err(format!("history ends on {}", day_label(last.0)));
    }
    Ok(())
}

fn day_label(t: i64) -> String {
    OffsetDateTime::from_unix_timestamp(t)
        .map(|d| d.date().to_string())
        .unwrap_or_default()
}

/// The cached quote history of an item as one close per UTC day, ascending, with the live
/// spot as today's point.
fn cached_closes(item: &store::Item, now: i64) -> Vec<(i64, f64)> {
    let mut by_day: BTreeMap<i64, (i64, f64)> = BTreeMap::new();
    for p in item.quote.get("history").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(t), Some(v)) = (p.get(0).and_then(Value::as_i64), p.get(1).and_then(Value::as_f64)) {
            by_day.insert(t / DAY, (t, v));
        }
    }
    if let Some(live) = item.quote.get("price_usd").and_then(Value::as_f64) {
        by_day.insert(now / DAY, (now, live));
    }
    by_day.into_values().collect()
}

/// One item's closes over the window, or why it is not covered.
pub async fn item_series(
    pool: &PgPool,
    item: &store::Item,
    coords: Option<&Coords>,
    days: i64,
    now: i64,
) -> Result<(Vec<(i64, f64)>, &'static str), String> {
    let start = now - days * DAY;
    if days <= 31 {
        let pts: Vec<(i64, f64)> =
            cached_closes(item, now).into_iter().filter(|(t, _)| *t >= start - 5 * DAY).collect();
        // Trim to the last close at or before the window start, so the series begins on it.
        let from = pts.iter().rposition(|(t, _)| *t <= start).unwrap_or(0);
        let pts = pts[from..].to_vec();
        window_covered(&pts, start, now).map_err(|e| format!("cached quote: {e}; refresh the list"))?;
        return Ok((pts, "quote"));
    }
    let Some(c) = coords else {
        return Err("no stored candles for the default crypto source: pin a quote source to this symbol and download its daily candles in Historical Data".into());
    };
    let Some(bars) = quotes::stored_bars(pool, c, days + 7).await else {
        return Err(format!(
            "no stored daily candles for {} ({}): download them in Historical Data",
            c.ticker, c.provider
        ));
    };
    let pts: Vec<(i64, f64)> = bars.iter().map(|b| (b.0, b.3)).collect();
    let from = pts.iter().rposition(|(t, _)| *t <= start).unwrap_or(0);
    let pts = pts[from..].to_vec();
    window_covered(&pts, start, now).map_err(|e| format!("stored candles for {}: {e}", c.ticker))?;
    Ok((pts, "stored"))
}

/// Every item's series over the window, split into covered and not covered.
pub async fn list_series(
    pool: &PgPool,
    wl: &store::Watchlist,
    items: &[store::Item],
    days: i64,
) -> (Vec<Series>, Vec<NotCovered>) {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let providers = quotes::resolved_providers(pool, wl, items).await;
    let mut ok = Vec::new();
    let mut missing = Vec::new();
    for item in items {
        let coords = quotes::chart_coords(item, providers.get(&item.id).map(String::as_str));
        match item_series(pool, item, coords.as_ref(), days, now).await {
            Ok((points, source)) => {
                ok.push(Series { item_id: item.id, symbol: item.symbol.clone(), source, points })
            }
            Err(reason) => {
                missing.push(NotCovered { item_id: item.id, symbol: item.symbol.clone(), reason })
            }
        }
    }
    (ok, missing)
}

/// Pearson correlation of two series' daily returns over the days both have a close.
/// Returns are taken between consecutive common days, so a stock (no weekend close)
/// against a coin compares Friday to Monday on both sides. `None` below [`MIN_OVERLAP`]
/// returns, or when either side never moves.
pub fn pearson_common(a: &[(i64, f64)], b: &[(i64, f64)]) -> Option<(f64, usize)> {
    let am: HashMap<i64, f64> = a.iter().map(|(t, v)| (t / DAY, *v)).collect();
    let mut common: Vec<(i64, f64, f64)> = b
        .iter()
        .filter_map(|(t, v)| am.get(&(t / DAY)).map(|x| (t / DAY, *x, *v)))
        .collect();
    common.sort_by_key(|c| c.0);
    common.dedup_by_key(|c| c.0);
    let rets: Vec<(f64, f64)> = common
        .windows(2)
        .filter(|w| w[0].1 != 0.0 && w[0].2 != 0.0)
        .map(|w| (w[1].1 / w[0].1 - 1.0, w[1].2 / w[0].2 - 1.0))
        .collect();
    let n = rets.len();
    if n < MIN_OVERLAP {
        return None;
    }
    let nf = n as f64;
    let mx = rets.iter().map(|r| r.0).sum::<f64>() / nf;
    let my = rets.iter().map(|r| r.1).sum::<f64>() / nf;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in &rets {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    if sxx == 0.0 || syy == 0.0 {
        return None;
    }
    Some(((sxy / (sxx.sqrt() * syy.sqrt())).clamp(-1.0, 1.0), n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(closes: &[f64]) -> Vec<(i64, f64)> {
        closes.iter().enumerate().map(|(i, c)| (1_000 * DAY + i as i64 * DAY, *c)).collect()
    }

    fn wiggle(n: usize) -> Vec<f64> {
        let mut v = vec![100.0];
        for i in 1..n {
            let step = if i % 3 == 0 { -1.5 } else { 1.0 + (i % 5) as f64 * 0.3 };
            v.push(v[i - 1] * (1.0 + step / 100.0));
        }
        v
    }

    #[test]
    fn perfect_correlation_is_one() {
        let a = wiggle(20);
        let b: Vec<f64> = a.iter().map(|x| x * 3.0).collect();
        let (r, n) = pearson_common(&series(&a), &series(&b)).unwrap();
        assert!((r - 1.0).abs() < 1e-9);
        assert_eq!(n, 19);
    }

    #[test]
    fn mirrored_returns_are_anti_correlated() {
        let a = wiggle(20);
        // Same daily returns with the sign flipped.
        let mut b = vec![100.0];
        for w in a.windows(2) {
            let r = w[1] / w[0] - 1.0;
            let last = *b.last().unwrap();
            b.push(last * (1.0 - r));
        }
        let (r, _) = pearson_common(&series(&a), &series(&b)).unwrap();
        assert!((r + 1.0).abs() < 1e-9);
    }

    #[test]
    fn insufficient_overlap_is_refused() {
        let a = wiggle(20);
        // Only 10 common days give 9 returns: refused.
        let b: Vec<(i64, f64)> = series(&a).into_iter().skip(10).collect();
        assert!(pearson_common(&series(&a), &b).is_none());
        // Disjoint days: nothing to compare.
        let far: Vec<(i64, f64)> = series(&a).into_iter().map(|(t, v)| (t + 100 * DAY, v)).collect();
        assert!(pearson_common(&series(&a), &far).is_none());
    }

    #[test]
    fn coverage_needs_the_window_start_and_a_recent_end() {
        let now = 2_000 * DAY;
        let start = now - 30 * DAY;
        let full: Vec<(i64, f64)> = (0..=30).map(|i| (start + i * DAY, 1.0)).collect();
        assert!(window_covered(&full, start, now).is_ok());
        // Starts 10 days late.
        assert!(window_covered(&full[10..], start, now).is_err());
        // Ends 20 days ago.
        assert!(window_covered(&full[..10], start, now).is_err());
        assert!(window_covered(&[], start, now).is_err());
        // A weekend at the start is tolerated.
        assert!(window_covered(&full[2..], start, now).is_ok());
    }

    #[test]
    fn windows_map_to_days() {
        assert_eq!(window_days("1m"), Some(30));
        assert_eq!(window_days("1y"), Some(365));
        assert_eq!(window_days("5y"), None);
    }
}
