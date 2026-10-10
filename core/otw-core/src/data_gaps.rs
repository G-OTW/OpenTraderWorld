//! Holes in stored candles, as a backtest or a paper session meets them.
//!
//! The engine finds them ([`crate::backtest::holes`]) and refuses to trade through them. This
//! side knows the rest: whether a download is already filling a hole, whether the provider
//! has confirmed it has nothing there, and how to ask for it. A hole is asked for once: a fill
//! that finishes without closing it confirms it, and a confirmed hole is never queued again.

use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::histdata as hd;

use crate::backtest::holes;

/// One hole of one dataset, with where its fill stands.
#[derive(Debug, Serialize, Clone)]
pub struct Gap {
    pub dataset_id: Uuid,
    pub ticker: String,
    #[serde(with = "time::serde::rfc3339")]
    pub from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub to: OffsetDateTime,
    /// `missing` (never asked for, or the last fill failed), `filling` (a download is on it)
    /// or `confirmed` (the provider has nothing there).
    pub status: &'static str,
}

/// The holes in a run input, as (last candle before, first candle after).
pub fn of(ts: &[String]) -> Vec<(OffsetDateTime, OffsetDateTime)> {
    holes::find(ts)
        .into_iter()
        .filter_map(|h| {
            let a = OffsetDateTime::parse(&ts[h.before], &Rfc3339).ok()?;
            let b = OffsetDateTime::parse(&ts[h.after], &Rfc3339).ok()?;
            Some((a, b))
        })
        .collect()
}

/// Where each hole of one dataset stands. A fill that finished without closing its hole
/// confirms it here, the first time anyone looks.
pub async fn statuses(
    pool: &sqlx::PgPool,
    dataset_id: Uuid,
    ticker: &str,
    found: &[(OffsetDateTime, OffsetDateTime)],
) -> anyhow::Result<Vec<Gap>> {
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let rows = hd::gap_rows(pool, dataset_id).await?;
    let mut out = Vec::with_capacity(found.len());
    for &(from, to) in found {
        let row = rows.iter().find(|r| r.gap_from == from && r.gap_to == to);
        let status = match row {
            None => "missing",
            Some(r) if r.status == "confirmed" => "confirmed",
            Some(r) => match r.job_status.as_deref() {
                Some("queued" | "running" | "waiting" | "cancelling") => "filling",
                // The download ran to its end and the hole is still here.
                Some("done" | "partial") => {
                    hd::confirm_gap(pool, dataset_id, from, to).await?;
                    "confirmed"
                }
                // Failed, cancelled or gone: worth asking again.
                _ => "missing",
            },
        };
        out.push(Gap { dataset_id, ticker: ticker.to_string(), from, to, status });
    }
    Ok(out)
}

/// Queue a download for every hole still `missing`. Returns the holes with their new status.
pub async fn fill(pool: &sqlx::PgPool, gaps: Vec<Gap>) -> anyhow::Result<Vec<Gap>> {
    let mut out = Vec::with_capacity(gaps.len());
    for mut g in gaps {
        if g.status != "missing" {
            out.push(g);
            continue;
        }
        let Some(ds) = hd::get_dataset(pool, g.dataset_id).await? else { continue };
        let step = crate::histdata::timeframe_secs(&ds.timeframe)?;
        let range_from = g.from + time::Duration::seconds(step);
        if range_from >= g.to {
            out.push(g);
            continue;
        }
        let connector_id = conn_store::default_for(pool, &ds.provider).await?.map(|c| c.id);
        let job_id = hd::enqueue_job(
            pool,
            &hd::NewJob {
                dataset_id: ds.id,
                connector_id,
                provider: &ds.provider,
                asset_type: &ds.asset_type,
                ticker: &ds.ticker,
                timeframe: &ds.timeframe,
                range_from,
                range_to: g.to,
                kind: "gap_fill",
                batch_id: None,
            },
        )
        .await?;
        hd::record_gap_fill(pool, ds.id, g.from, g.to, job_id).await?;
        g.status = "filling";
        out.push(g);
    }
    Ok(out)
}
