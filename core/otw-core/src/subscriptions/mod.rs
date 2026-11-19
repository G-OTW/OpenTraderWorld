//! Subscriptions background job: renewal reminders and category budget breaches.
//!
//! Runs every hour. Both checks are idempotent on their own record (`reminded_for` per
//! billing date, `budget_notified` per category and month), so a frequent tick never sends
//! twice and a reminder set today still goes out today. Notifications are written in the app
//! and pushed through the channel broker as the `subscriptions` producer.

use std::time::Duration;

use reqwest::Client;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::crypto::SecretCipher;
use otw_store::reminders::FiredNotification;
use otw_store::subscriptions as store;

use crate::notif_send;

const TICK: Duration = Duration::from_secs(60 * 60);
const MODULE: &str = "subscriptions";
const URL: &str = "/subscriptions";

/// One budget check at a time: the hourly tick and an edit can race on the same breach.
static BUDGET_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn spawn(pool: PgPool, cipher: SecretCipher, http: Client) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            if let Err(e) = send_reminders(&pool, &cipher, &http).await {
                tracing::error!("subscriptions reminders failed: {e:#}");
            }
            if let Err(e) = check_budgets(&pool, &cipher, &http).await {
                tracing::error!("subscriptions budget check failed: {e:#}");
            }
        }
    });
}

/// `[]` is every channel granted to Subscriptions; a list narrows to those.
fn only(ids: &[Uuid]) -> Option<&[Uuid]> {
    (!ids.is_empty()).then_some(ids)
}

/// The reminder's text: what renews (or converts from a trial), when, and for how much.
pub fn reminder_text(s: &store::Subscription, billing: time::Date) -> (String, String) {
    let amount = format!("{:.2} {}", s.price * s.share(), s.currency);
    let day = store::iso_date(billing);
    if s.trial_end == Some(billing) {
        (
            format!("{} trial ends {day}", s.name),
            format!("The free trial of {} converts to paid on {day}: {amount}.", s.name),
        )
    } else {
        (
            format!("{} renews {day}", s.name),
            format!("{} bills {amount} on {day}.", s.name),
        )
    }
}

async fn send_reminders(pool: &PgPool, cipher: &SecretCipher, http: &Client) -> anyhow::Result<()> {
    let today = OffsetDateTime::now_utc().date();
    for (s, billing) in store::due_reminders(pool, today).await? {
        let (name, details) = reminder_text(&s, billing);
        store::insert_notification(pool, Some(s.id), &name, &details, URL).await?;
        store::mark_reminded(pool, s.id, billing).await?;
        let notif = FiredNotification { name, details, url: URL.to_string() };
        notif_send::dispatch(pool, cipher, http, &notif, MODULE, only(&s.remind_channels)).await;
    }
    Ok(())
}

/// Notify every category whose monthly total is above its cap, once per calendar month.
pub async fn check_budgets(pool: &PgPool, cipher: &SecretCipher, http: &Client) -> anyhow::Result<()> {
    let _guard = BUDGET_LOCK.lock().await;
    let settings = store::get_settings(pool).await?;
    if settings.category_budgets.is_empty() {
        return Ok(());
    }
    let today = OffsetDateTime::now_utc().date();
    let month = format!("{:04}-{:02}", today.year(), today.month() as u8);
    let totals = store::current_category_totals(pool, &settings.display_currency).await?;
    let breaches =
        store::budget_breaches(&totals, &settings.category_budgets, &settings.budget_notified, &month);
    if breaches.is_empty() {
        return Ok(());
    }
    let mut notified = settings.budget_notified.clone();
    let cur = &settings.display_currency;
    for b in &breaches {
        let name = format!("{} over budget", b.category);
        let details = format!(
            "{} subscriptions cost {:.2} {cur} a month, above the {:.2} {cur} budget.",
            b.category, b.amount, b.cap
        );
        store::insert_notification(pool, None, &name, &details, URL).await?;
        notified.insert(b.category.clone(), month.clone());
        let notif = FiredNotification { name, details, url: URL.to_string() };
        notif_send::dispatch(pool, cipher, http, &notif, MODULE, only(&settings.budget_channels)).await;
    }
    store::set_budget_notified(pool, &notified).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriptions_reminder_text_names_trials_and_renewals() {
        let s = sample();
        let day = time::macros::date!(2026 - 12 - 01);
        let (name, details) = reminder_text(&s, day);
        assert_eq!(name, "Data renews 2026-12-01");
        assert_eq!(details, "Data bills 5.00 USD on 2026-12-01.");
        let mut t = sample();
        t.trial_end = Some(day);
        assert!(reminder_text(&t, day).0.contains("trial ends"));
    }

    fn sample() -> store::Subscription {
        store::Subscription {
            id: Uuid::nil(),
            name: "Data".into(),
            platform: None,
            url: None,
            price: 10.0,
            currency: "USD".into(),
            frequency: "monthly".into(),
            category: None,
            started_on: None,
            active: true,
            remind_days: Some(3),
            remind_channels: Vec::new(),
            reminded_for: None,
            trial_end: None,
            ended_at: None,
            yearly_price: None,
            payment_method: None,
            card_expiry: None,
            my_share_pct: 50.0,
            last_score: None,
            last_rated_at: None,
        }
    }

    #[test]
    fn subscriptions_empty_channel_list_means_every_granted_channel() {
        assert!(only(&[]).is_none());
        let id = Uuid::nil();
        assert_eq!(only(&[id]).map(|s| s.len()), Some(1));
    }
}
