//! Background poll loop for the Mailbox module.
//!
//! Wakes every minute, polls the accounts whose interval elapsed, and applies the
//! retention setting on a slow cadence. Per-account back-off lives in the store, so a
//! mailbox with a stale password fails quietly and visibly instead of being retried
//! every minute forever.
//!
//! OAuth accounts get two extra guarantees. Their tokens are refreshed **once at boot**,
//! so a grant revoked while the app was down shows up as "reconnect" on the first screen
//! the user opens rather than at the next scheduled poll. And a mailbox whose token has
//! not been exchanged for two months raises a notification: Microsoft drops a refresh
//! token after 90 days of inactivity, and a paused mailbox is exactly the case where
//! nobody would notice the clock running out.

use std::time::Duration;

use otw_store::crypto::SecretCipher;
use reqwest::Client;
use sqlx::PgPool;

const TICK: Duration = Duration::from_secs(60);
/// Retention and the credential-age check are housekeeping, not a hot path.
const HOUSEKEEPING_EVERY_TICKS: u32 = 60 * 6;
/// Days without a token exchange before the user is warned (Microsoft's limit is 90).
const TOKEN_STALE_DAYS: i32 = 60;

pub fn spawn(pool: PgPool, cipher: SecretCipher, http: Client) {
    tokio::spawn(async move {
        // Let startup settle before touching the network.
        tokio::time::sleep(Duration::from_secs(20)).await;
        refresh_oauth_at_boot(&pool, &cipher, &http).await;

        let mut tick = tokio::time::interval(TICK);
        let mut ticks: u32 = 0;
        loop {
            tick.tick().await;
            crate::mailbox::poll_due(&pool, &cipher, &http).await;

            ticks = ticks.wrapping_add(1);
            if ticks % HOUSEKEEPING_EVERY_TICKS == 0 {
                trim_old_mail(&pool).await;
                warn_on_stale_credentials(&pool).await;
            }
        }
    });
}

/// Exchange every OAuth refresh token once, before the first poll. Any account whose
/// grant died during the downtime flips to `needs_reauth` here (inside `config_for`), so
/// the UI can say "reconnect" instead of showing a mailbox that silently stopped.
async fn refresh_oauth_at_boot(pool: &PgPool, cipher: &SecretCipher, http: &Client) {
    let accounts = match otw_store::mailbox::oauth_accounts(pool).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("mailbox: listing oauth accounts failed: {e:#}");
            return;
        }
    };
    for account in accounts {
        match crate::mailbox::config_for(pool, cipher, http, &account).await {
            Ok(_) => tracing::info!("mailbox {}: sign-in still valid", account.name),
            Err(e) => tracing::warn!("mailbox {}: sign-in check failed: {e:#}", account.name),
        }
    }
}

async fn trim_old_mail(pool: &PgPool) {
    match otw_store::mailbox::get_settings(pool).await {
        Ok(s) if s.retention_days > 0 => {
            match otw_store::mailbox::trim_older_than(pool, s.retention_days).await {
                Ok(n) if n > 0 => tracing::info!("mailbox: trimmed {n} old message(s)"),
                Ok(_) => {}
                Err(e) => tracing::warn!("mailbox: trim failed: {e:#}"),
            }
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("mailbox: loading settings failed: {e:#}"),
    }
}

/// One notification per stale mailbox per housekeeping pass. Deliberately not throttled
/// further: it only fires for a mailbox that has been paused or failing for two months,
/// and the alternative is discovering the dead credential on day 91.
async fn warn_on_stale_credentials(pool: &PgPool) {
    let stale = match otw_store::mailbox::stale_oauth_accounts(pool, TOKEN_STALE_DAYS).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("mailbox: checking token age failed: {e:#}");
            return;
        }
    };
    for account in stale {
        let name = account.name.clone();
        let details = format!(
            "This mailbox has not signed in for over {TOKEN_STALE_DAYS} days. Microsoft drops \
             a sign-in after 90 days without use — open it and fetch once to keep it alive."
        );
        if let Err(e) =
            otw_store::reminders::add_notification(pool, &format!("Mailbox: {name}"), &details)
                .await
        {
            tracing::warn!("mailbox: stale-credential notification failed: {e:#}");
        }
    }
}
