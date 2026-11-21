//! Portfolio notifications: drift, concentration, planned trades and the digest.
//!
//! Every message goes through `notif_send::dispatch` as the `portfolios` producer, so only
//! channels granted to the module are ever sent to, and each one is also written as an
//! in-app notification. Each alert fires **once per breach**: the stored state says the user
//! was told, and only a return inside the limit re-arms it.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::crypto::SecretCipher;
use otw_store::portfolios::{self as store, Book, Plan, Portfolio};
use otw_store::portfolios_risk as risk_store;

use super::analytics::{self, Analysis, Needs, Substrate, Window};

const MODULE: &str = "portfolios";
const TICK: Duration = Duration::from_secs(60 * 60);

// ── Pure rules ────────────────────────────────────────────────────────────────

/// Which keys to tell about and which to re-arm, given what was already told and what is
/// out of line now. A key still out is not told twice; a key back in line is re-armed.
pub fn transitions(alerted: &[String], out_now: &[String]) -> (Vec<String>, Vec<String>) {
    let fire = out_now.iter().filter(|k| !alerted.contains(k)).cloned().collect();
    let rearm = alerted.iter().filter(|k| !out_now.contains(k)).cloned().collect();
    (fire, rearm)
}

/// Holdings strictly above the limit. At the limit is within it.
pub fn over_limit(weights: &[(Uuid, f64)], max_pct: f64) -> Vec<Uuid> {
    weights.iter().filter(|(_, w)| *w > max_pct + 1e-9).map(|(id, _)| *id).collect()
}

/// Each holding's share of net worth, percent. Empty when there is no positive base.
pub fn weights(b: &Book) -> Vec<(Uuid, f64)> {
    if b.net_worth <= 0.0 {
        return Vec::new();
    }
    b.positions
        .iter()
        .filter(|p| p.quantity > 0.0)
        .filter_map(|p| p.market_value.map(|mv| (p.asset.id, mv / b.net_worth * 100.0)))
        .collect()
}

/// Whether a digest is due: daily once per UTC day, weekly once seven days have passed.
pub fn digest_due(mode: &str, last: Option<OffsetDateTime>, now: OffsetDateTime) -> bool {
    match (mode, last) {
        ("daily" | "weekly", None) => true,
        ("daily", Some(l)) => l.date() < now.date(),
        ("weekly", Some(l)) => (now.date() - l.date()).whole_days() >= 7,
        _ => false,
    }
}

/// A buy waits for the quote at or below its level, a sell at or above.
pub fn plan_hit(side: &str, level: f64, price: f64) -> bool {
    match side {
        "buy" => price <= level,
        "sell" => price >= level,
        _ => false,
    }
}

/// What the digest says.
pub struct DigestInput<'a> {
    pub name: &'a str,
    pub currency: &'a str,
    pub net_worth: f64,
    pub previous: Option<f64>,
    /// `(symbol, percent change)` since the last digest.
    pub movers: Vec<(String, f64)>,
    pub income: f64,
}

pub fn digest_text(d: &DigestInput<'_>) -> String {
    let mut out = format!("{}: {:.2} {}", d.name, d.net_worth, d.currency);
    if let Some(prev) = d.previous.filter(|p| p.abs() > 1e-9) {
        out.push_str(&format!(" ({:+.2}% since the last digest)", (d.net_worth / prev - 1.0) * 100.0));
    }
    let mut movers = d.movers.clone();
    movers.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));
    movers.truncate(3);
    if movers.is_empty() {
        out.push_str("\nTop movers: measured from the next digest on.");
    } else {
        let list: Vec<String> = movers.iter().map(|(s, p)| format!("{s} {p:+.2}%")).collect();
        out.push_str(&format!("\nTop movers: {}", list.join(", ")));
    }
    out.push_str(&format!("\nIncome since the last digest: {:.2} {}", d.income, d.currency));
    out
}

// ── Sending ───────────────────────────────────────────────────────────────────

async fn send(pool: &PgPool, cipher: &SecretCipher, http: &Client, title: &str, body: &str, only: &[Uuid]) {
    let notif = match otw_store::reminders::add_notification(pool, title, body).await {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!("portfolio notification: {e:#}");
            return;
        }
    };
    let only = (!only.is_empty()).then_some(only);
    crate::notif_send::dispatch(pool, cipher, http, &notif, MODULE, only).await;
}

// ── Checks ────────────────────────────────────────────────────────────────────

/// Drift: tell once per bucket that left its band.
pub async fn check_drift(pool: &PgPool, cipher: &SecretCipher, http: &Client, pf: &Portfolio) -> anyhow::Result<()> {
    if !pf.drift_notify {
        return Ok(());
    }
    let s = Substrate::build(pool, pf, Window::inception(), Needs::BOOK | Needs::TARGETS, None).await?;
    let block = analytics::drift::Drift.run(&s);
    let Some(data) = block.data else { return Ok(()) };
    let Some(rows) = data.get("rows").and_then(Value::as_array) else { return Ok(()) };
    let out_now: Vec<String> = rows
        .iter()
        .filter(|r| r.get("out_of_band").and_then(Value::as_bool) == Some(true))
        .filter_map(|r| r.get("bucket").and_then(Value::as_str).map(str::to_string))
        .collect();
    let targets = risk_store::list_targets(pool, pf.id).await?;
    let alerted: Vec<String> =
        targets.iter().filter(|t| t.dimension == "asset_class" && t.alerted).map(|t| t.bucket.clone()).collect();
    let (fire, rearm) = transitions(&alerted, &out_now);
    for bucket in &fire {
        let row = rows.iter().find(|r| r.get("bucket").and_then(Value::as_str) == Some(bucket.as_str()));
        let cur = row.and_then(|r| r.get("current_pct")).and_then(Value::as_f64).unwrap_or(0.0);
        let tgt = row.and_then(|r| r.get("target_pct")).and_then(Value::as_f64).unwrap_or(0.0);
        let band = row.and_then(|r| r.get("band_pct")).and_then(Value::as_f64).unwrap_or(0.0);
        let title = format!("Portfolio: {} {bucket} off target", pf.name);
        let body = format!("{bucket} is at {cur:.1}% against a {tgt:.1}% target (band {band:.1} points).");
        send(pool, cipher, http, &title, &body, &pf.alert_channels).await;
        risk_store::set_target_alerted(pool, pf.id, "asset_class", bucket, true).await?;
    }
    for bucket in &rearm {
        risk_store::set_target_alerted(pool, pf.id, "asset_class", bucket, false).await?;
    }
    Ok(())
}

/// Concentration: tell once per holding that went above the limit.
pub async fn check_concentration(
    pool: &PgPool,
    cipher: &SecretCipher,
    http: &Client,
    pf: &Portfolio,
    b: &Book,
) -> anyhow::Result<()> {
    let (Some(max), true) = (pf.max_weight_pct, pf.concentration_notify) else { return Ok(()) };
    let w = weights(b);
    let over = over_limit(&w, max);
    let key = |ids: &[Uuid]| ids.iter().map(Uuid::to_string).collect::<Vec<_>>();
    let (fire, _) = transitions(&key(&pf.concentration_alerted), &key(&over));
    for id in &fire {
        let Some(p) = b.positions.iter().find(|p| p.asset.id.to_string() == *id) else { continue };
        let pct = w.iter().find(|(a, _)| a.to_string() == *id).map(|(_, x)| *x).unwrap_or(0.0);
        let title = format!("Portfolio: {} {} above {max:.1}%", pf.name, p.asset.symbol);
        let body = format!("{} weighs {pct:.1}% of the book, above the {max:.1}% limit.", p.asset.symbol);
        send(pool, cipher, http, &title, &body, &pf.alert_channels).await;
    }
    // The stored set is what is over now: a holding back under the limit re-arms by leaving it.
    if over != pf.concentration_alerted {
        store::set_concentration_alerted(pool, pf.id, &over).await?;
    }
    Ok(())
}

/// The digest, when its cadence says it is due.
pub async fn check_digest(
    pool: &PgPool,
    cipher: &SecretCipher,
    http: &Client,
    pf: &Portfolio,
    b: &Book,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    if !digest_due(&pf.digest, pf.digest_last_at, now) {
        return Ok(());
    }
    let prev_prices = pf.digest_marks.get("prices").cloned().unwrap_or(json!({}));
    let previous = pf.digest_marks.get("net_worth").and_then(Value::as_f64);
    let mut movers = Vec::new();
    let mut prices = serde_json::Map::new();
    for p in b.positions.iter().filter(|p| p.quantity > 0.0) {
        let Some(px) = p.price else { continue };
        prices.insert(p.asset.id.to_string(), json!(px));
        if let Some(old) = prev_prices.get(p.asset.id.to_string()).and_then(Value::as_f64).filter(|o| *o > 0.0) {
            movers.push((p.asset.symbol.clone(), (px / old - 1.0) * 100.0));
        }
    }
    let since = match pf.digest_last_at {
        Some(l) => l.date().next_day().unwrap_or(l.date()),
        None => now.date() - time::Duration::days(if pf.digest == "weekly" { 7 } else { 1 }),
    };
    let income: f64 = store::income_by_asset_since(pool, pf, since).await?.values().sum();
    let text = digest_text(&DigestInput {
        name: &pf.name,
        currency: &pf.currency,
        net_worth: b.net_worth,
        previous,
        movers,
        income,
    });
    let title = format!("Portfolio digest: {}", pf.name);
    send(pool, cipher, http, &title, &text, &pf.digest_channels).await;
    store::set_digest_sent(pool, pf.id, &json!({ "net_worth": b.net_worth, "prices": prices })).await?;
    Ok(())
}

/// Planned trades: price each waiting plan's asset and mark the ones reached.
pub async fn check_plans(pool: &PgPool, cipher: &SecretCipher, http: &Client) -> anyhow::Result<()> {
    let plans = store::waiting_plans(pool).await?;
    let mut by_asset: HashMap<Uuid, Vec<Plan>> = HashMap::new();
    for p in plans {
        by_asset.entry(p.asset_id).or_default().push(p);
    }
    let today = OffsetDateTime::now_utc().date();
    for (asset_id, plans) in by_asset {
        let Some(asset) = store::get_asset(pool, asset_id).await? else { continue };
        // The asset's own quote source, the same one the positions table is priced from.
        let asset = match super::prices::price_new_asset(pool, &asset).await {
            Ok(Some(a)) => a,
            Ok(None) => asset,
            Err(e) => {
                tracing::warn!("plan check: pricing {}: {e:#}", asset.symbol);
                asset
            }
        };
        let Some(usd) = asset.last_price_usd else { continue };
        let px = otw_store::journal_fx::convert(pool, usd, "USD", &asset.currency, today).await?.unwrap_or(usd);
        for plan in plans.iter().filter(|p| plan_hit(&p.side, p.price, px)) {
            if !store::mark_plan_hit(pool, plan.id, px).await? || !plan.notify {
                continue;
            }
            let Some(pf) = store::get_portfolio(pool, plan.portfolio_id).await? else { continue };
            let title = format!("Portfolio: {} {} level reached", pf.name, asset.symbol);
            let body = format!(
                "Planned {} of {} at {:.4} {}: the quote is {:.4}.",
                plan.side, asset.symbol, plan.price, asset.currency, px
            );
            send(pool, cipher, http, &title, &body, &pf.alert_channels).await;
        }
    }
    Ok(())
}

/// Run every daily check for one portfolio. Errors are logged, never fatal.
pub async fn daily(pool: &PgPool, cipher: &SecretCipher, http: &Client, pf: &Portfolio) {
    if !(pf.drift_notify || pf.concentration_notify || pf.digest != "off") {
        return;
    }
    if let Err(e) = check_drift(pool, cipher, http, pf).await {
        tracing::warn!("portfolio {} drift check: {e:#}", pf.id);
    }
    let b = match store::book(pool, pf).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("portfolio {} book: {e:#}", pf.id);
            return;
        }
    };
    if let Err(e) = check_concentration(pool, cipher, http, pf, &b).await {
        tracing::warn!("portfolio {} concentration check: {e:#}", pf.id);
    }
    if let Err(e) = check_digest(pool, cipher, http, pf, &b).await {
        tracing::warn!("portfolio {} digest: {e:#}", pf.id);
    }
}

/// Hourly loop: planned trades every hour, the daily checks once per UTC day.
pub fn spawn(pool: PgPool, cipher: SecretCipher, http: Client) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut last_day: Option<time::Date> = None;
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            if let Err(e) = check_plans(&pool, &cipher, &http).await {
                tracing::warn!("portfolio plans check: {e:#}");
            }
            let today = OffsetDateTime::now_utc().date();
            if last_day == Some(today) {
                continue;
            }
            match store::list_portfolios(&pool).await {
                Ok(pfs) => {
                    for pf in &pfs {
                        daily(&pool, &cipher, &http, pf).await;
                    }
                    last_day = Some(today);
                }
                Err(e) => tracing::warn!("portfolio alerts: listing portfolios: {e:#}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_breach_fires_once_then_rearms_after_returning() {
        // Goes out: told.
        let (fire, rearm) = transitions(&[], &v(&["stock"]));
        assert_eq!(fire, v(&["stock"]));
        assert!(rearm.is_empty());
        // Still out: silent.
        let (fire, rearm) = transitions(&v(&["stock"]), &v(&["stock"]));
        assert!(fire.is_empty() && rearm.is_empty());
        // Back in band: re-armed, nothing sent.
        let (fire, rearm) = transitions(&v(&["stock"]), &[]);
        assert!(fire.is_empty());
        assert_eq!(rearm, v(&["stock"]));
        // Out again: told again.
        let (fire, _) = transitions(&[], &v(&["stock"]));
        assert_eq!(fire, v(&["stock"]));
    }

    #[test]
    fn the_limit_itself_is_not_a_breach() {
        let (a, b) = (Uuid::from_bytes([1; 16]), Uuid::from_bytes([2; 16]));
        assert_eq!(over_limit(&[(a, 20.0), (b, 20.01)], 20.0), vec![b]);
        assert!(over_limit(&[(a, 19.99)], 20.0).is_empty());
    }

    #[test]
    fn a_plan_triggers_on_its_side_of_the_level() {
        assert!(plan_hit("buy", 100.0, 99.5));
        assert!(plan_hit("buy", 100.0, 100.0));
        assert!(!plan_hit("buy", 100.0, 100.5));
        assert!(plan_hit("sell", 100.0, 101.0));
        assert!(!plan_hit("sell", 100.0, 99.0));
    }

    #[test]
    fn the_weekly_digest_waits_seven_days() {
        let now = OffsetDateTime::from_unix_timestamp(1_780_000_000).unwrap();
        assert!(digest_due("weekly", None, now));
        assert!(!digest_due("weekly", Some(now - time::Duration::days(6)), now));
        assert!(digest_due("weekly", Some(now - time::Duration::days(7)), now));
        assert!(!digest_due("daily", Some(now), now));
        assert!(digest_due("daily", Some(now - time::Duration::days(1)), now));
        assert!(!digest_due("off", None, now));
    }

    #[test]
    fn the_digest_names_value_change_movers_and_income() {
        let t = digest_text(&DigestInput {
            name: "Main",
            currency: "EUR",
            net_worth: 1100.0,
            previous: Some(1000.0),
            movers: vec![("A".into(), 1.0), ("B".into(), -8.0), ("C".into(), 3.0), ("D".into(), 0.5)],
            income: 12.5,
        });
        assert!(t.starts_with("Main: 1100.00 EUR (+10.00% since the last digest)"), "{t}");
        assert!(t.contains("Top movers: B -8.00%, C +3.00%, A +1.00%"), "{t}");
        assert!(t.contains("Income since the last digest: 12.50 EUR"));
        let first = digest_text(&DigestInput { name: "Main", currency: "EUR", net_worth: 1.0, previous: None, movers: vec![], income: 0.0 });
        assert!(first.contains("measured from the next digest on"));
    }
}
