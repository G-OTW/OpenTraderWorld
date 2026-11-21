//! MyWealth engine: what the module computes or reads beyond the stored revisions.
//!
//! - `loan`: amortization; a loan's balance is written as one revision per due date.
//! - `wallet`, `broker`: live readings, kept as that day's revision and cached for an hour.
//! - `goals`, `movers`, `targets`: read-side math over the valued book.
//! - `export`: the CSV.
//!
//! The background loop ([`spawn`]) runs hourly: once a day it brings every loan's schedule up
//! to date, and once a week (when asked) it pushes the list of aged valuations through the
//! channel broker.

pub mod broker;
pub mod export;
pub mod goals;
pub mod loan;
pub mod movers;
pub mod targets;
pub mod wallet;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::wealth::{self as store, Asset, AssetFilter, SOURCE_LIVE, SOURCE_LOAN};

/// Producer id for notifications and the grant broker accounts and channels are checked against.
pub const MODULE: &str = "wealth";

/// How long a live reading stands before the venue is asked again.
const LIVE_TTL: Duration = Duration::from_secs(60 * 60);
/// A failed reading is retried sooner, but not on every page load.
const ERROR_TTL: Duration = Duration::from_secs(5 * 60);
const TICK: Duration = Duration::from_secs(60 * 60);
/// Largest attachment stored.
pub const MAX_ATTACHMENT: usize = 10 * 1024 * 1024;

// ── Loans ────────────────────────────────────────────────────────────────────

/// Rewrite a loan's schedule revisions when a payment has fallen due since the last one, or
/// always when `force` (its terms were just saved). Assets without terms are left alone.
pub async fn sync_loan(pool: &PgPool, asset: &Asset, force: bool) -> Result<()> {
    let Some(terms) = asset.loan.as_ref() else {
        return Ok(());
    };
    let loan = loan::Loan::parse(terms).map_err(|e| anyhow!(e))?;
    let today = OffsetDateTime::now_utc().date();
    let history = loan.history(today);
    let last_due = history.last().map(|(d, _)| *d);
    if !force && store::last_auto_revision(pool, asset.id, SOURCE_LOAN).await? == last_due {
        return Ok(());
    }
    store::replace_auto_revisions(pool, asset.id, SOURCE_LOAN, &history, "Loan schedule").await
}

async fn sync_all_loans(pool: &PgPool) -> Result<()> {
    let assets = store::list_assets(pool, &AssetFilter::default()).await?;
    for a in assets.iter().filter(|a| a.loan.is_some()) {
        if let Err(e) = sync_loan(pool, a, false).await {
            tracing::warn!("wealth: loan schedule of {} not updated: {e:#}", a.id);
        }
    }
    Ok(())
}

// ── Live readings ────────────────────────────────────────────────────────────

struct Reading {
    at: Instant,
    error: Option<String>,
}

fn readings() -> &'static Mutex<HashMap<Uuid, Reading>> {
    static R: OnceLock<Mutex<HashMap<Uuid, Reading>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One refresh at a time: the asset list and the breakdown load together, and the second
/// must find the first's readings rather than ask the venue again.
fn refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static L: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Forget an asset's last reading, so the next refresh reads it again (its source changed).
pub fn forget_reading(id: Uuid) {
    if let Ok(mut m) = readings().lock() {
        m.remove(&id);
    }
}

/// The last reading error per asset, for the rows that show it.
pub fn live_errors() -> HashMap<Uuid, String> {
    readings()
        .lock()
        .map(|m| m.iter().filter_map(|(id, r)| r.error.clone().map(|e| (*id, e))).collect())
        .unwrap_or_default()
}

fn is_live(a: &Asset) -> bool {
    a.broker_account_id.is_some() || (a.chain.is_some() && a.address.is_some())
}

/// Read every live asset whose last reading is older than its TTL, and keep each success as
/// today's revision. Errors stay on the asset and are shown on its row, never as a zero.
pub async fn refresh_live(pool: &PgPool, cipher: &otw_store::crypto::SecretCipher) -> Result<()> {
    let assets = store::list_assets(pool, &AssetFilter::default()).await?;
    let live: Vec<&Asset> = assets.iter().filter(|a| is_live(a)).collect();
    if live.is_empty() {
        return Ok(());
    }
    let _guard = refresh_lock().lock().await;
    let due: Vec<&Asset> = {
        let m = readings().lock().map_err(|_| anyhow!("live readings lock"))?;
        live.into_iter()
            .filter(|a| match m.get(&a.id) {
                Some(r) => r.at.elapsed() >= if r.error.is_some() { ERROR_TTL } else { LIVE_TTL },
                None => true,
            })
            .collect()
    };
    let today = OffsetDateTime::now_utc().date();
    for a in due {
        let result = if crate::demo::enabled() {
            Err(anyhow!("Live readings are off in the demo."))
        } else {
            read_one(pool, cipher, a).await
        };
        let error = match result {
            Ok((value, note)) => {
                store::put_auto_revision(pool, a.id, today, value, SOURCE_LIVE, &note).await?;
                None
            }
            Err(e) => Some(format!("{e:#}")),
        };
        if let Ok(mut m) = readings().lock() {
            m.insert(a.id, Reading { at: Instant::now(), error });
        }
    }
    Ok(())
}

/// Value one live asset in its own currency, with the note its revision carries.
async fn read_one(pool: &PgPool, cipher: &otw_store::crypto::SecretCipher, a: &Asset) -> Result<(f64, String)> {
    let today = OffsetDateTime::now_utc().date();
    if let Some(account_id) = a.broker_account_id {
        let (connector, settings, account) =
            crate::brokers_api::resolve_for_job(pool, cipher, account_id, broker::MODULE).await?;
        let label = connector.capability().label;
        let summaries = connector.account(&settings, None).await?;
        let mut total = 0.0;
        for (equity, money) in broker::equity_of(label, &summaries)? {
            total += otw_store::journal_fx::convert(pool, equity, &money, &a.currency, today)
                .await?
                .ok_or_else(|| anyhow!("no FX rate from {money} to {}", a.currency))?;
        }
        return Ok((total, format!("Equity read from {}", account.name)));
    }
    let (Some(chain), Some(addr)) = (a.chain.as_deref(), a.address.as_deref()) else {
        return Err(anyhow!("not a live asset"));
    };
    let coins = wallet::balance(chain, addr).await?;
    let id = wallet::coin_id(chain).ok_or_else(|| anyhow!("chain must be btc or eth"))?;
    let prices = crate::portfolios::prices::crypto_prices(&[id.to_string()])
        .await
        .context("pricing the coin")?;
    let usd = prices.get(id).copied().ok_or_else(|| anyhow!("no {} price from CoinGecko", wallet::unit(chain)))?;
    let value = otw_store::journal_fx::convert(pool, coins * usd, "USD", &a.currency, today)
        .await?
        .ok_or_else(|| anyhow!("no FX rate from USD to {}", a.currency))?;
    Ok((value, format!("{coins} {} on chain at {usd} USD", wallet::unit(chain))))
}

// ── Stale notification ───────────────────────────────────────────────────────

/// Whether the weekly stale pass is due: switched on, and a week since the last pass.
pub fn stale_due(mode: &str, last: Option<OffsetDateTime>, now: OffsetDateTime) -> bool {
    mode == "weekly" && last.is_none_or(|l| now - l >= time::Duration::days(7))
}

/// The notice for a set of aged valuations, or None when there is nothing to say.
pub fn stale_message(stale: &[store::StaleAsset]) -> Option<(String, String)> {
    if stale.is_empty() {
        return None;
    }
    let title = format!("MyWealth: {} valuation(s) to update", stale.len());
    let details = stale
        .iter()
        .take(20)
        .map(|s| match s.age_days {
            Some(d) => format!("{}: valued {d} days ago (review every {} days)", s.name, s.review_days),
            None => format!("{}: never valued", s.name),
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some((title, details))
}

async fn stale_pass(pool: &PgPool, cipher: &otw_store::crypto::SecretCipher, http: &reqwest::Client) -> Result<()> {
    let settings = store::get_settings(pool).await?;
    let now = OffsetDateTime::now_utc();
    if !stale_due(&settings.stale_notify, settings.stale_last_at, now) {
        return Ok(());
    }
    // Recorded first: a channel that fails must not turn a weekly notice into an hourly one.
    store::mark_stale_run(pool, now).await?;
    let stale = store::stale_now(pool).await?;
    let Some((title, details)) = stale_message(&stale) else {
        return Ok(());
    };
    let notif = otw_store::reminders::add_linked_notification(pool, "task", &title, &details, "/wealth").await?;
    // The demo never sends: the in-app row is written, the channels are not called.
    if crate::demo::enabled() {
        return Ok(());
    }
    let mut out = notif;
    // The link is a path inside the app: it means nothing in an email or a chat.
    out.url.clear();
    let only = (!settings.stale_channels.is_empty()).then_some(settings.stale_channels.as_slice());
    crate::notif_send::dispatch(pool, cipher, http, &out, MODULE, only).await;
    Ok(())
}

/// Hourly loop: the daily loan pass and the weekly stale pass.
pub fn spawn(pool: PgPool, cipher: otw_store::crypto::SecretCipher, http: reqwest::Client) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        let mut last_loans: Option<time::Date> = None;
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            let today = OffsetDateTime::now_utc().date();
            if last_loans != Some(today) {
                match sync_all_loans(&pool).await {
                    Ok(()) => last_loans = Some(today),
                    Err(e) => tracing::error!("wealth: loan pass failed: {e:#}"),
                }
            }
            if let Err(e) = stale_pass(&pool, &cipher, &http).await {
                tracing::error!("wealth: stale pass failed: {e:#}");
            }
        }
    });
}

// ── Attachments ──────────────────────────────────────────────────────────────

/// Whether an upload may be stored. The demo keeps no visitor's files.
pub fn check_upload(len: usize, demo: bool) -> Result<(), String> {
    if demo {
        return Err("Attachments are off in the demo.".into());
    }
    if len == 0 {
        return Err("the file is empty".into());
    }
    if len > MAX_ATTACHMENT {
        return Err(format!("the file is {:.1} MB, the limit is 10 MB", len as f64 / 1_048_576.0));
    }
    Ok(())
}

/// Whether a stored attachment may be downloaded. The demo refuses every download.
pub fn check_download(demo: bool) -> Result<(), String> {
    if demo {
        Err("Downloads are off in the demo.".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stale_pass_runs_weekly_and_only_when_asked() {
        let now = OffsetDateTime::now_utc();
        assert!(!stale_due("off", None, now));
        assert!(stale_due("weekly", None, now));
        assert!(!stale_due("weekly", Some(now - time::Duration::days(6)), now));
        assert!(stale_due("weekly", Some(now - time::Duration::days(7)), now));
    }

    #[test]
    fn nothing_stale_sends_nothing() {
        assert!(stale_message(&[]).is_none());
        let (title, details) = stale_message(&[
            store::StaleAsset { id: Uuid::nil(), name: "House".into(), valued_at: None, age_days: Some(400), review_days: 365 },
            store::StaleAsset { id: Uuid::nil(), name: "Watch".into(), valued_at: None, age_days: None, review_days: 90 },
        ])
        .unwrap();
        assert!(title.contains('2'));
        assert!(details.contains("House: valued 400 days ago"));
        assert!(details.contains("Watch: never valued"));
    }

    #[test]
    fn attachment_size_cap_and_demo_refusal() {
        assert!(check_upload(1, false).is_ok());
        assert!(check_upload(MAX_ATTACHMENT, false).is_ok());
        assert!(check_upload(MAX_ATTACHMENT + 1, false).unwrap_err().contains("10 MB"));
        assert!(check_upload(0, false).is_err());
        assert!(check_upload(1, true).is_err());
        assert!(check_download(true).is_err());
        assert!(check_download(false).is_ok());
    }
}
