//! Process-wide handle to the secret cipher for the histdata download worker.
//!
//! The worker is a detached tokio task spawned with only a pool, but it must decrypt
//! provider credentials. Rather than thread the cipher through every job row, we publish
//! it once at startup into a `OnceLock`. The cipher is cheap-to-clone and identical to the
//! one in `AppState`.

use std::sync::OnceLock;

use otw_store::crypto::SecretCipher;

static CIPHER: OnceLock<SecretCipher> = OnceLock::new();

/// Install the cipher. Called once during startup, before the worker runs.
pub fn init(cipher: SecretCipher) {
    let _ = CIPHER.set(cipher);
}

/// Borrow the installed cipher. Panics only if called before [`init`], which never
/// happens in normal startup ordering.
pub fn get() -> &'static SecretCipher {
    CIPHER.get().expect("histdata cipher not initialized")
}
