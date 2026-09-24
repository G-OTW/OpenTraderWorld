//! Short-lived session tokens, cached for the process.
//!
//! Two brokers here do not authenticate a request with a signature: they exchange a
//! long-lived credential for a token that lasts a few minutes (TradeStation's OAuth access
//! token, FOREX.com's session). Minting one per call would be wasteful, and TradeStation
//! says so in its own documentation; worse on FOREX.com, where every log-on opens a session
//! on the account.
//!
//! So a token is kept until shortly before it expires and shared by every call that uses
//! the same credential. The cache key is a **hash** of the credential rather than the
//! credential: a map that lives for the life of the process should not be the second place
//! a secret is written down.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A token and the moment it stops being usable.
type Entry = (Instant, String);

static CACHE: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

/// A stable, non-reversible key for a credential.
pub(super) fn key(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        // A separator, so two different splits of the same characters are two keys.
        h.update([0u8]);
    }
    h.finalize().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The cached token for this credential, or `None` when there is none left worth using.
pub(super) fn get(key: &str) -> Option<String> {
    let guard = CACHE.lock().ok()?;
    let (until, token) = guard.as_ref()?.get(key)?;
    (Instant::now() < *until).then(|| token.clone())
}

/// Remember a token for `ttl`, minus a margin so it is never handed out on its last breath.
pub(super) fn put(key: &str, token: &str, ttl: Duration) {
    let margin = Duration::from_secs(60).min(ttl / 4);
    if let Ok(mut guard) = CACHE.lock() {
        guard.get_or_insert_with(HashMap::new).insert(
            key.to_string(),
            (Instant::now() + ttl.saturating_sub(margin), token.to_string()),
        );
    }
}

/// Forget a token the broker has just refused, so the next call mints a fresh one instead
/// of replaying the one that failed.
pub(super) fn forget(key: &str) {
    if let Ok(mut guard) = CACHE.lock() {
        if let Some(map) = guard.as_mut() {
            map.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_kept_then_dropped_and_the_credential_is_never_the_key() {
        let k = key(&["client", "secret"]);
        // The key is a digest: the credential itself does not appear in it.
        assert_eq!(k.len(), 64);
        assert!(!k.contains("secret"));
        // Two different splits of the same characters are two different keys.
        assert_ne!(k, key(&["clients", "ecret"]));

        put(&k, "abc", Duration::from_secs(600));
        assert_eq!(get(&k).as_deref(), Some("abc"));
        forget(&k);
        assert!(get(&k).is_none());

        // The safety margin is a quarter of a short life, so a token with none left to
        // speak of is never handed out.
        put(&k, "stale", Duration::ZERO);
        assert!(get(&k).is_none());
    }
}
