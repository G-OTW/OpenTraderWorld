//! Security events: the short list of things an exposed instance must be able to answer
//! "did that happen, and when" about.
//!
//! Two sinks, deliberately.
//!
//! * **The log, always.** Every event is a `tracing` line on the `security` target, so it
//!   lands in `app_logs` like everything else and Settings → Logs filters it by typing
//!   `security` in the search box. This is the record that survives a channel being
//!   misconfigured, and it is what a later export or a `docker compose logs core` reads.
//! * **The user's channels, for the ones that cannot wait.** [`alert`] pushes through the
//!   notification broker as the `security` producer, so the message arrives wherever the
//!   user granted that module (Settings → Notifications). Nothing here owns a channel or a
//!   provider; it drops into the broker like every other module.
//!
//! What is *not* here: the access log. Who connected, from where, with what response code
//! is Caddy's job (`log` in `deploy/Caddyfile`), because core never sees a request Caddy
//! turned away, and an access log written by the thing being attacked is the first thing
//! an attacker edits.
//!
//! Events are fire-and-forget: a slow SMTP server must never hold up the sign-in, the
//! token mint or the mode switch that produced the event. Failures are logged and dropped.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{notif_send, AppState};

/// The tracing target every event is written on. Also the string to grep a log export for.
const TARGET: &str = "security";

/// Shortest gap between two notifications sharing a key. A guessing run produces one event
/// per attempt and the owner needs one message, not a thousand; the log keeps every one.
const NOTIFY_COOLDOWN: Duration = Duration::from_secs(10 * 60);

/// Cap on distinct keys the cooldown table holds, so a spray cannot grow it without bound.
/// Full means "forget the lapsed ones, then start over", which errs towards notifying.
const MAX_KEYS: usize = 4_000;

// ── Kinds ────────────────────────────────────────────────────────────────────
//
// One constant per event, because the name is the stable part: it is what the log line
// starts with, what a search filters on and what a future fail2ban rule would match.

/// A password or a second factor was wrong.
pub const LOGIN_FAILED: &str = "login_failed";
/// A correct sign-in from an address this account has never used before.
pub const LOGIN_NEW_SOURCE: &str = "login_new_source";
/// A bearer token was minted. It outlives the session that asked for it.
pub const MCP_TOKEN_CREATED: &str = "mcp_token_created";
/// A minted token was presented for the first time.
pub const MCP_TOKEN_FIRST_USE: &str = "mcp_token_first_use";
/// A token that has run out was presented. Either a client nobody updated, or a copy that
/// outlived the user's intent.
pub const MCP_TOKEN_EXPIRED: &str = "mcp_token_expired";
/// A bearer that matches no token was presented to the gateway.
pub const MCP_AUTH_FAILED: &str = "mcp_auth_failed";
/// The MCP gateway was switched on or off.
pub const MCP_GATEWAY: &str = "mcp_gateway";
/// External control was switched on or off.
pub const CONTROL_GATEWAY: &str = "control_gateway";
/// A chat sender redeemed a pairing code and can now drive the app.
pub const CONTROL_PAIRED: &str = "control_paired";
/// The network mode changed: what the instance is reachable from.
pub const NETWORK_MODE: &str = "network_mode";
/// A data export that included the sealed credential stores left the instance.
pub const EXPORT_CREDENTIALS: &str = "export_credentials";

// ── Emitting ─────────────────────────────────────────────────────────────────

/// Write one event to the security log, and nothing else.
///
/// For the events that are worth a record but not an interruption: every failed password,
/// every rejected bearer. [`alert`] calls this too, so an alerted event is never missing
/// from the log because a channel swallowed it.
pub fn log(kind: &str, what: &str) {
    // One line, so a multi-line detail block does not become several log rows.
    let what = what.replace('\n', " | ");
    tracing::warn!(target: TARGET, "{kind}: {what}");
}

/// Log the event *and* push it to the user's notification channels.
///
/// `key` is what repeats collapse on: at most one notification per key per
/// [`NOTIFY_COOLDOWN`]. Pass the kind alone where the flood *is* the event being reported
/// (guessed passwords, rejected bearers, where a thousand messages say nothing the first
/// one did not), and the kind plus what makes this occurrence distinct where each one
/// matters on its own: a sign-in from a *different* new address is a different event.
pub fn alert(state: &AppState, kind: &str, key: &str, title: &str, details: &str) {
    log(kind, details);
    if !claim(key) {
        return;
    }
    let (pool, cipher, http) = (state.pool.clone(), state.cipher.clone(), state.http.clone());
    let (title, details) = (title.to_string(), details.to_string());
    tokio::spawn(async move {
        match otw_store::reminders::add_notification(&pool, &title, &details).await {
            Ok(n) => {
                notif_send::dispatch(&pool, &cipher, &http, &n, "security", None).await;
            }
            Err(e) => tracing::warn!("could not record the security notice '{title}': {e:#}"),
        }
    });
}

/// Take this key's notification slot, or refuse because one was taken recently.
fn claim(key: &str) -> bool {
    static SENT: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    let mut guard = SENT.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    map.retain(|_, at| at.elapsed() < NOTIFY_COOLDOWN);
    if map.len() >= MAX_KEYS {
        map.clear();
    }
    if map.contains_key(key) {
        return false;
    }
    map.insert(key.to_string(), Instant::now());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_notifies_once_per_window() {
        let key = "test_kind:unique-for-this-test";
        assert!(claim(key));
        assert!(!claim(key), "the repeat is suppressed");
        assert!(claim("test_kind:a-different-subject"), "keys are independent");
    }
}
