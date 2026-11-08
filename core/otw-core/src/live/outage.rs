//! Telling the user a live feed is down, once, and again when it is back.
//!
//! The pane's banner says why a feed is not live, but only to someone looking at it: a
//! background chart alert or a pane left open in another room has nobody watching. A
//! supervisor that has not managed to (re)open its socket for [`NOTIFY_AFTER`] sends one
//! notification through the channels granted to `histdata` (the data broker, which owns the
//! connectors), and one more when the socket opens again. A blip the reconnect loop absorbs
//! says nothing.

use std::time::{Duration, Instant};

use otw_store::crypto::SecretCipher;
use reqwest::Client;
use sqlx::PgPool;

/// How long a feed may stay down before the user is told.
const NOTIFY_AFTER: Duration = Duration::from_secs(120);
/// How long a reopened socket has to stay up before the outage counts as over. A socket that
/// opens and drops again at once is one outage, not a stream of short ones that each stay
/// under [`NOTIFY_AFTER`], and not a "back" every time the handshake passes.
pub(super) const STEADY: Duration = Duration::from_secs(60);

/// One supervisor's outage, if it is in one.
#[derive(Default)]
pub(super) struct Outage {
    /// When the current outage started. Kept through a reopen that did not hold.
    since: Option<Instant>,
    /// When the socket last opened while an outage was running.
    up_at: Option<Instant>,
    reason: String,
    told: bool,
}

impl Outage {
    /// The session failed, or could not open, for this reason.
    pub fn down(&mut self, reason: &str) {
        if self.up_at.take().is_some_and(|t| t.elapsed() >= STEADY) {
            self.since = None;
        }
        if self.since.is_none() {
            self.since = Some(Instant::now());
            self.told = false;
        }
        self.reason = reason.to_string();
    }

    /// The socket opened. Nothing is said yet: it has to hold for [`STEADY`] first.
    pub fn up(&mut self) {
        if self.since.is_some() {
            self.up_at = Some(Instant::now());
        }
    }

    /// The reopened socket has held for [`STEADY`]: the outage is over. Returns how long the
    /// feed was down when the user was told it was, so they can be told it is back.
    pub fn steady(&mut self) -> Option<Duration> {
        let since = self.since?;
        let down = self.up_at.map_or_else(|| since.elapsed(), |up| up.duration_since(since));
        let told = self.told;
        *self = Outage::default();
        told.then_some(down)
    }

    /// The reason and duration to report, once, when the outage has lasted long enough.
    pub fn due(&mut self) -> Option<(String, Duration)> {
        let down = self.since?.elapsed();
        if self.told || down < NOTIFY_AFTER {
            return None;
        }
        self.told = true;
        Some((self.reason.clone(), down))
    }
}

pub(super) fn down_text(connector: &str, provider: &str, tickers: &str, reason: &str, down: Duration) -> (String, String) {
    (
        format!("Live feed down: {connector}"),
        format!(
            "{provider} live data for {tickers} has been unavailable for {}. It keeps reconnecting on its own. Last error: {reason}",
            minutes(down)
        ),
    )
}

pub(super) fn back_text(connector: &str, provider: &str, tickers: &str, down: Duration) -> (String, String) {
    (
        format!("Live feed back: {connector}"),
        format!("{provider} live data for {tickers} is streaming again after {} down.", minutes(down)),
    )
}

fn minutes(d: Duration) -> String {
    match d.as_secs() / 60 {
        0 | 1 => "1 min".into(),
        m if m < 90 => format!("{m} min"),
        m => format!("{} h {} min", m / 60, m % 60),
    }
}

/// In-app notification plus the channels granted to the data broker. Detached: a slow mail
/// server must not hold the reconnect loop.
pub(super) fn notify(pool: PgPool, cipher: SecretCipher, http: Client, (name, details): (String, String)) {
    tokio::spawn(async move {
        match otw_store::reminders::add_notification(&pool, &name, &details).await {
            Ok(n) => {
                crate::notif_send::dispatch(&pool, &cipher, &http, &n, "histdata", None).await;
            }
            Err(e) => tracing::warn!("live feed notification failed: {e:#}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aged(d: Duration) -> Instant {
        Instant::now().checked_sub(d).unwrap()
    }

    #[test]
    fn a_short_outage_says_nothing() {
        let mut o = Outage::default();
        o.down("socket closed");
        assert!(o.due().is_none());
        o.up();
        assert!(o.steady().is_none());
    }

    #[test]
    fn a_long_outage_is_told_once_then_its_end() {
        let mut o = Outage::default();
        o.down("first");
        o.since = Some(aged(NOTIFY_AFTER));
        o.down("second");
        let (reason, _) = o.due().expect("due after the threshold");
        assert_eq!(reason, "second");
        assert!(o.due().is_none(), "told once");
        o.up();
        assert!(o.steady().is_some(), "and told it is back once the socket held");
        assert!(o.since.is_none() && !o.told);
    }

    #[test]
    fn a_reopen_that_does_not_hold_continues_the_outage() {
        let mut o = Outage::default();
        o.down("closed");
        let start = aged(NOTIFY_AFTER);
        o.since = Some(start);
        o.up();
        o.down("closed again");
        assert_eq!(o.since, Some(start));
        assert!(o.due().is_some());
    }

    #[test]
    fn a_told_outage_is_not_over_until_the_socket_holds() {
        let mut o = Outage::default();
        o.down("closed");
        o.since = Some(aged(NOTIFY_AFTER));
        assert!(o.due().is_some());
        o.up();
        o.down("closed again");
        assert!(o.told, "still the same outage: no back, no second down");
        assert!(o.due().is_none());
        o.up();
        assert!(o.steady().is_some(), "back once, when the socket finally holds");
    }

    #[test]
    fn a_reopen_that_held_ends_the_untold_outage() {
        let mut o = Outage::default();
        o.down("closed");
        o.since = Some(aged(NOTIFY_AFTER));
        o.up();
        o.up_at = Some(aged(STEADY));
        o.down("closed later");
        assert!(o.due().is_none(), "a fresh outage starts its own clock");
    }
}
