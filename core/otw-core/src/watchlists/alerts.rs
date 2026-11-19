//! Evaluating watchlist price alerts.
//!
//! Runs right after a list is re-quoted — from the background loop and from a manual
//! refresh alike — so an alert fires with the browser closed. Firing writes an in-app
//! notification and pushes it to the alert's chosen channels (see `notif_send::dispatch`).
//!
//! The one subtlety worth stating: a *rolling* alert compares the live price against a
//! sample taken `window_secs` ago, and samples only exist for items that carry such an
//! alert. Until history reaches back far enough the comparison has no reference, and the
//! alert stays quiet rather than firing off a reference that doesn't mean what it claims.

use anyhow::Result;
use otw_store::crypto::SecretCipher;
use otw_store::reminders::FiredNotification;
use otw_store::watchlist_alerts::{self as alerts, Alert};
use otw_store::watchlists as store;
use reqwest::Client;
use sqlx::PgPool;
use uuid::Uuid;

use crate::notif_send;

/// What firing an alert needs beyond the pool: the channel secrets and an HTTP client.
#[derive(Clone)]
pub struct AlertCx {
    pub cipher: SecretCipher,
    pub http: Client,
}

/// The live USD price out of an item's cached quote, if it has one.
fn live_price(item: &store::Item) -> Option<f64> {
    item.quote
        .get("price_usd")
        .and_then(serde_json::Value::as_f64)
        .filter(|p| p.is_finite())
}

/// Whether `price` satisfies the alert, given the reference the caller resolved.
/// `None` reference = not enough information to judge (rolling history too young).
fn triggered(a: &Alert, price: f64, reference: Option<f64>) -> bool {
    match a.metric.as_str() {
        "price" => match a.direction.as_str() {
            "above" => price >= a.threshold,
            "below" => price <= a.threshold,
            _ => false,
        },
        metric => {
            let Some(reference) = reference.filter(|r| r.is_finite() && *r != 0.0) else {
                return false;
            };
            let delta = price - reference;
            let moved = if metric == "pct" { delta / reference.abs() * 100.0 } else { delta };
            match a.direction.as_str() {
                "up" => moved >= a.threshold,
                "down" => moved <= -a.threshold,
                "move" => moved.abs() >= a.threshold,
                _ => false,
            }
        }
    }
}

/// Still inside its cooldown, so a repeating alert shouldn't fire again yet.
fn cooling_down(a: &Alert, now: time::OffsetDateTime) -> bool {
    match a.last_fired_at {
        Some(t) if a.cooldown_secs > 0 => {
            now - t < time::Duration::seconds(a.cooldown_secs as i64)
        }
        _ => false,
    }
}

/// Evaluate every live alert on a watchlist's items and fire the ones that hit.
///
/// Called after the list's quotes are fresh. Individual failures are logged and skipped —
/// a broken channel or a single bad alert never wedges the loop.
pub async fn evaluate(pool: &PgPool, cx: &AlertCx, wl: &store::Watchlist) -> Result<()> {
    let live = alerts::live_for_watchlist(pool, wl.id).await?;
    if live.is_empty() {
        return Ok(());
    }
    let items = store::list_items(pool, wl.id).await?;
    let now = time::OffsetDateTime::now_utc();

    for item in &items {
        let mine: Vec<&Alert> = live.iter().filter(|a| a.item_id == item.id).collect();
        if mine.is_empty() {
            continue;
        }
        let Some(price) = live_price(item) else { continue };

        // Sample the price once per item, only while a rolling alert needs the history.
        if mine.iter().any(|a| a.basis == "rolling" && a.metric != "price") {
            if let Err(e) = alerts::add_sample(pool, item.id, price).await {
                tracing::warn!("watchlist sample for {} failed: {e:#}", item.symbol);
            }
        }

        for a in mine {
            if cooling_down(a, now) {
                continue;
            }
            let reference = match (a.metric.as_str(), a.basis.as_str()) {
                ("price", _) => None,
                (_, "rolling") => alerts::sample_at(pool, item.id, a.window_secs).await?,
                _ => a.ref_price,
            };
            if !triggered(a, price, reference) {
                continue;
            }
            if let Err(e) = fire(pool, cx, wl, item, a, price, reference).await {
                tracing::error!("firing watchlist alert {} failed: {e:#}", a.id);
            }
        }
    }
    Ok(())
}

/// Write the in-app notification, push it to the alert's channels, and record the fire.
async fn fire(
    pool: &PgPool,
    cx: &AlertCx,
    wl: &store::Watchlist,
    item: &store::Item,
    a: &Alert,
    price: f64,
    reference: Option<f64>,
) -> Result<()> {
    let title = format!("{} {}", item.symbol, condition_label(a));
    let details = fired_details(a, price, reference);
    let url = format!("/watchlists?list={}", wl.id);

    sqlx::query(
        "INSERT INTO notifications (id, name, kind, linked_id, details, url, link_label) \
         VALUES ($1,$2,'watchlist',$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(&title)
    .bind(item.id)
    .bind(&details)
    .bind(&url)
    .bind(&wl.name)
    .execute(pool)
    .await?;

    let notif = FiredNotification {
        name: title,
        details,
        url,
    };
    notif_send::dispatch(
        pool,
        &cx.cipher,
        &cx.http,
        &notif,
        "watchlists",
        Some(&a.channel_ids),
    )
    .await;
    alerts::record_fire(pool, a.id, price, a.repeat).await?;
    Ok(())
}

/// "above $120", "up 5%", "down $250" — the condition, for the notification title.
fn condition_label(a: &Alert) -> String {
    match a.metric.as_str() {
        "price" => format!("{} {}", a.direction, fmt_usd(a.threshold)),
        "pct" => format!("{} {}%", a.direction, trim_num(a.threshold)),
        _ => format!("{} {}", a.direction, fmt_usd(a.threshold)),
    }
}

/// The body: where the price is now, and what it moved from when that is the question.
fn fired_details(a: &Alert, price: f64, reference: Option<f64>) -> String {
    let mut s = format!("Now {}", fmt_usd(price));
    if a.metric != "price" {
        if let Some(r) = reference.filter(|r| r.is_finite() && *r != 0.0) {
            let delta = price - r;
            let pct = delta / r.abs() * 100.0;
            s.push_str(&format!(
                " — from {} ({}{} / {}{}%)",
                fmt_usd(r),
                if delta >= 0.0 { "+" } else { "-" },
                fmt_usd(delta.abs()),
                if pct >= 0.0 { "+" } else { "-" },
                trim_num(pct.abs())
            ));
        }
    }
    if !a.note.trim().is_empty() {
        s.push_str(&format!("\n{}", a.note.trim()));
    }
    s
}

/// USD with a precision that follows the magnitude — sub-dollar coins need the decimals.
fn fmt_usd(n: f64) -> String {
    let abs = n.abs();
    let digits = if abs >= 1.0 {
        2
    } else if abs >= 0.01 {
        4
    } else {
        6
    };
    format!("${:.*}", digits, n)
}

/// Two decimals at most, without trailing zeros ("5", "5.5", "5.25").
fn trim_num(n: f64) -> String {
    let s = format!("{n:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(metric: &str, direction: &str, threshold: f64) -> Alert {
        Alert {
            id: Uuid::nil(),
            item_id: Uuid::nil(),
            metric: metric.into(),
            direction: direction.into(),
            threshold,
            basis: "anchor".into(),
            window_secs: 0,
            ref_price: None,
            armed_at: time::OffsetDateTime::now_utc(),
            expires_at: None,
            repeat: false,
            cooldown_secs: 0,
            enabled: true,
            channel_ids: vec![],
            note: String::new(),
            last_fired_at: None,
            last_price: None,
            fire_count: 0,
        }
    }

    #[test]
    fn price_thresholds_are_inclusive_crossings() {
        assert!(triggered(&alert("price", "above", 100.0), 100.0, None));
        assert!(triggered(&alert("price", "above", 100.0), 101.0, None));
        assert!(!triggered(&alert("price", "above", 100.0), 99.9, None));
        assert!(triggered(&alert("price", "below", 100.0), 99.9, None));
        assert!(!triggered(&alert("price", "below", 100.0), 100.1, None));
    }

    #[test]
    fn pct_moves_are_measured_against_the_reference() {
        let up = alert("pct", "up", 5.0);
        assert!(triggered(&up, 105.0, Some(100.0)));
        assert!(!triggered(&up, 104.0, Some(100.0)));
        // A drop never satisfies an "up" alert, however large.
        assert!(!triggered(&up, 50.0, Some(100.0)));

        let down = alert("pct", "down", 5.0);
        assert!(triggered(&down, 95.0, Some(100.0)));
        assert!(!triggered(&down, 96.0, Some(100.0)));

        let mv = alert("pct", "move", 5.0);
        assert!(triggered(&mv, 105.0, Some(100.0)));
        assert!(triggered(&mv, 95.0, Some(100.0)));
        assert!(!triggered(&mv, 102.0, Some(100.0)));
    }

    #[test]
    fn usd_moves_are_absolute() {
        let mv = alert("usd", "move", 250.0);
        assert!(triggered(&mv, 60_250.0, Some(60_000.0)));
        assert!(!triggered(&mv, 60_100.0, Some(60_000.0)));
    }

    #[test]
    fn a_variation_alert_without_a_reference_stays_quiet() {
        // Rolling history too young, or the anchor never captured a price.
        assert!(!triggered(&alert("pct", "move", 1.0), 500.0, None));
        assert!(!triggered(&alert("pct", "move", 1.0), 500.0, Some(0.0)));
    }

    #[test]
    fn cooldown_blocks_a_repeat_until_it_elapses() {
        let now = time::OffsetDateTime::now_utc();
        let mut a = alert("price", "above", 10.0);
        a.cooldown_secs = 600;
        a.last_fired_at = Some(now - time::Duration::seconds(60));
        assert!(cooling_down(&a, now));
        a.last_fired_at = Some(now - time::Duration::seconds(601));
        assert!(!cooling_down(&a, now));
        // No cooldown configured → never blocked.
        a.cooldown_secs = 0;
        a.last_fired_at = Some(now);
        assert!(!cooling_down(&a, now));
    }

    #[test]
    fn labels_read_as_sentences() {
        assert_eq!(condition_label(&alert("price", "above", 120.0)), "above $120.00");
        assert_eq!(condition_label(&alert("pct", "up", 5.5)), "up 5.5%");
        assert_eq!(trim_num(5.0), "5");
        assert_eq!(fmt_usd(0.00123456), "$0.001235");
    }
}
