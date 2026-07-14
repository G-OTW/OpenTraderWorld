//! Secret encryption at rest.
//!
//! Secrets (API keys/tokens for feeds) are sealed with XChaCha20-Poly1305 AEAD
//! under a single master key (`OTW_SECRET_KEY`). The master key never encrypts a
//! value directly: each secret gets a fresh random 24-byte nonce, so 100 feeds
//! with different credentials yield 100 independent ciphertexts — even identical
//! plaintexts encrypt differently. Plaintext is never logged or persisted.

use anyhow::{anyhow, Context};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};

/// A handle to the AEAD cipher derived from the master key.
#[derive(Clone)]
pub struct SecretCipher {
    cipher: XChaCha20Poly1305,
}

impl SecretCipher {
    /// Derive the cipher from the raw master key string. The string is hashed to
    /// a 32-byte key, so any non-empty master key length is accepted.
    pub fn from_master(master: &str) -> anyhow::Result<Self> {
        if master.trim().is_empty() {
            return Err(anyhow!("OTW_SECRET_KEY must be set and non-empty"));
        }
        let digest = Sha256::digest(master.as_bytes());
        let key = Key::from_slice(&digest);
        Ok(Self {
            cipher: XChaCha20Poly1305::new(key),
        })
    }

    /// Seal a plaintext secret, returning (nonce, ciphertext+tag).
    pub fn seal(&self, plaintext: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
        // 24-byte XChaCha nonce — random per secret. getrandom is already a dep
        // tree member via the workspace; use it directly for the nonce bytes.
        let mut nonce_bytes = [0u8; 24];
        getrandom::fill(&mut nonce_bytes).map_err(|e| anyhow!("nonce rng: {e}"))?;
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| anyhow!("secret encryption failed"))?;
        Ok((nonce_bytes.to_vec(), ciphertext))
    }

    /// Open a sealed secret back to plaintext.
    pub fn open(&self, nonce: &[u8], ciphertext: &[u8]) -> anyhow::Result<String> {
        let nonce = XNonce::from_slice(nonce);
        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow!("secret decryption failed (wrong OTW_SECRET_KEY?)"))?;
        String::from_utf8(plaintext).context("decrypted secret is not valid UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let c = SecretCipher::from_master("test-master-key").unwrap();
        let (n, ct) = c.seal("super-secret-token").unwrap();
        assert_eq!(c.open(&n, &ct).unwrap(), "super-secret-token");
    }

    #[test]
    fn same_plaintext_differs() {
        let c = SecretCipher::from_master("k").unwrap();
        let (_, a) = c.seal("dup").unwrap();
        let (_, b) = c.seal("dup").unwrap();
        assert_ne!(a, b, "nonce reuse would make ciphertexts equal");
    }

    #[test]
    fn wrong_key_fails() {
        let a = SecretCipher::from_master("key-a").unwrap();
        let b = SecretCipher::from_master("key-b").unwrap();
        let (n, ct) = a.seal("x").unwrap();
        assert!(b.open(&n, &ct).is_err());
    }
}
