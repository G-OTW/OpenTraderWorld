//! Daily movers digest: once a day, the three biggest gainers and losers of a list over
//! 24h, sent through the channels granted to Watchlists.
//!
//! Built from the quotes the refresh loop already cached, so it costs no provider request.
//! The once-a-day gate is the UTC calendar day of `digest_last_at`: a digest goes out at
//! the first loop pass at or after `digest_hour` (UTC) on a day none was sent yet.

use anyhow::Result;
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::reminders::FiredNotification;
use otw_store::watchlists as store;

use super::AlertCx;
use crate::notif_send;

/// Movers shown on each side.
const TOP: usize = 3;

/// Whether a digest is due at `now`: past the hour, and none sent yet this UTC day.
pub fn due(last: Option<OffsetDateTime>, now: OffsetDateTime, hour: i16) -> bool {
    if (now.hour() as i16) < hour {
        return false;
    }
    match last {
        Some(t) => t.to_offset(time::UtcOffset::UTC).date() < now.date(),
        None => true,
    }
}

fn fmt_price(n: f64) -> String {
    let digits = if n.abs() >= 1.0 { 2 } else if n.abs() >= 0.01 { 4 } else { 6 };
    format!("{n:.digits$}")
}

/// The digest text: up to three gainers then three losers by 24h change, one line each.
/// Symbols without a 24h change are left out. `None` when no symbol has one: an empty
/// digest is not sent.
pub fn build_text(rows: &[(String, Option<f64>, Option<f64>)]) -> Option<String> {
    let mut moved: Vec<(&str, f64, Option<f64>)> = rows
        .iter()
        .filter_map(|(s, ch, p)| ch.filter(|c| c.is_finite()).map(|c| (s.as_str(), c, *p)))
        .collect();
    if moved.is_empty() {
        return None;
    }
    moved.sort_by(|a, b| b.1.total_cmp(&a.1));
    let line = |(s, c, p): &(&str, f64, Option<f64>)| match p {
        Some(p) => format!("{s} {c:+.2}% ({})", fmt_price(*p)),
        None => format!("{s} {c:+.2}%"),
    };
    let up: Vec<String> = moved.iter().filter(|m| m.1 > 0.0).take(TOP).map(line).collect();
    let down: Vec<String> = moved.iter().rev().filter(|m| m.1 < 0.0).take(TOP).map(line).collect();
    let mut out = String::new();
    if !up.is_empty() {
        out.push_str(&format!("Up: {}", up.join(", ")));
    }
    if !down.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("Down: {}", down.join(", ")));
    }
    if out.is_empty() {
        out = "No move over 24h.".into();
    }
    Some(out)
}

/// Send every digest that is due. Failures are logged per list; the gate is stamped only
/// once the digest was written, so a failed pass retries on the next one.
pub async fn run_due(pool: &PgPool, cx: &AlertCx) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    for wl in store::list_digest_on(pool).await? {
        if !due(wl.digest_last_at, now, wl.digest_hour) {
            continue;
        }
        if let Err(e) = send(pool, cx, &wl).await {
            tracing::error!("watchlist {} digest failed: {e:#}", wl.id);
        }
    }
    Ok(())
}

async fn send(pool: &PgPool, cx: &AlertCx, wl: &store::Watchlist) -> Result<()> {
    let items = store::list_items(pool, wl.id).await?;
    let rows: Vec<(String, Option<f64>, Option<f64>)> = items
        .iter()
        .map(|i| {
            (
                i.symbol.clone(),
                i.quote.get("change_24h").and_then(Value::as_f64),
                i.quote.get("price_usd").and_then(Value::as_f64),
            )
        })
        .collect();
    let Some(details) = build_text(&rows) else {
        // Nothing quoted yet: stamp the day anyway, a digest of nothing is no news.
        store::mark_digest_sent(pool, wl.id).await?;
        return Ok(());
    };
    let name = format!("{}: daily movers", wl.name);
    let url = format!("/watchlists?list={}", wl.id);
    sqlx::query(
        "INSERT INTO notifications (id, name, kind, linked_id, details, url, link_label) \
         VALUES ($1,$2,'watchlist',$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(&name)
    .bind(wl.id)
    .bind(&details)
    .bind(&url)
    .bind(&wl.name)
    .execute(pool)
    .await?;
    store::mark_digest_sent(pool, wl.id).await?;
    let notif = FiredNotification { name, details, url };
    notif_send::dispatch(pool, &cx.cipher, &cx.http, &notif, "watchlists", Some(&wl.digest_channels)).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn digest_goes_out_once_per_utc_day_after_the_hour() {
        let now = datetime!(2026-11-06 21:30 UTC);
        assert!(due(None, now, 21));
        assert!(!due(None, datetime!(2026-11-06 20:59 UTC), 21));
        assert!(!due(Some(datetime!(2026-11-06 21:01 UTC)), now, 21));
        assert!(due(Some(datetime!(2026-11-05 21:01 UTC)), now, 21));
        // Sent late yesterday evening UTC still counts as yesterday.
        assert!(due(Some(datetime!(2026-11-05 23:59 UTC)), datetime!(2026-11-06 00:30 UTC), 0));
    }

    #[test]
    fn text_lists_top_gainers_then_losers() {
        let rows = vec![
            ("AAA".to_string(), Some(5.0), Some(10.0)),
            ("BBB".to_string(), Some(-3.0), Some(2.5)),
            ("CCC".to_string(), Some(1.0), None),
            ("DDD".to_string(), None, Some(1.0)),
            ("EEE".to_string(), Some(9.0), Some(100.0)),
            ("FFF".to_string(), Some(0.5), Some(1.0)),
            ("GGG".to_string(), Some(-7.25), Some(0.5)),
        ];
        let t = build_text(&rows).unwrap();
        assert_eq!(
            t,
            "Up: EEE +9.00% (100.00), AAA +5.00% (10.00), CCC +1.00%\nDown: GGG -7.25% (0.5000), BBB -3.00% (2.50)"
        );
    }

    #[test]
    fn nothing_quoted_sends_nothing() {
        assert!(build_text(&[("AAA".to_string(), None, Some(1.0))]).is_none());
        assert!(build_text(&[]).is_none());
    }
}
