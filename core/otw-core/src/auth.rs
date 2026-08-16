//! Password hashing (argon2) and opaque session-token generation.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

/// Hash a plaintext password with argon2 (random salt).
pub fn hash_password(plain: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hashing password: {e}"))?
        .to_string();
    Ok(hash)
}

/// Verify a plaintext password against a stored argon2 hash.
pub fn verify_password(plain: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

/// Burn the same argon2 work as a real verification, against a throwaway hash.
///
/// An unknown username would otherwise return in ~1 ms where a known one costs ~100 ms of
/// hashing, which is a timing oracle for "does this account exist". Called on the
/// username-miss path so both outcomes take the same time. Always false.
pub fn verify_dummy(plain: &str) -> bool {
    // Hash of a value no password can be, computed once per process.
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let hash = DUMMY.get_or_init(|| {
        hash_password("otw-nonexistent-account-placeholder").unwrap_or_default()
    });
    !hash.is_empty() && verify_password(plain, hash)
}

/// Generate a random, URL-safe opaque session token (256 bits of OS entropy).
pub fn generate_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
