//! Password hashing (argon2), password policy, opaque session tokens and TOTP.
//!
//! Three things live here, all of them about proving who is at the other end:
//!
//! * **Hashing** — argon2id with the crate defaults (19 MiB, t=2), which is the OWASP
//!   floor, plus [`verify_dummy`] so an unknown username costs the same as a wrong password.
//! * **Policy** ([`check_password`]) — length plus a compromised-password screen, per
//!   NIST SP 800-63B §5.1.1.2. The screen is the part that matters: length alone does not
//!   stop `Password123!`, and an instance exposed on the internet is guessed at, not
//!   cracked offline.
//! * **TOTP** ([`totp`]) — RFC 6238, implemented here rather than pulled in, because it is
//!   forty lines over `hmac`/`sha1` (already in the tree) and a second factor should not
//!   add a dependency nobody in this project audits.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

/// Minimum password length. NIST SP 800-63B puts the floor at 8 for a screened password;
/// 12 is the value this project ships because a self-hosted instance in `web` mode is a
/// single account with no lockout, and the screen below only covers *known* passwords.
pub const MIN_PASSWORD_LEN: usize = 12;

/// Upper bound, so a multi-megabyte body cannot turn argon2 into a CPU sink. Well above
/// any passphrase a person types.
pub const MAX_PASSWORD_LEN: usize = 256;

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

// ── Password policy ──────────────────────────────────────────────────────────

/// The shipped blocklist, embedded at compile time so nothing is fetched at runtime and an
/// air-gapped install screens exactly like a connected one. See the file header for how to
/// swap in a larger corpus.
const BLOCKLIST_SRC: &str = include_str!("auth_passwords.txt");

fn blocklist() -> &'static std::collections::HashSet<&'static str> {
    static SET: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| {
        BLOCKLIST_SRC
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    })
}

/// Every form of a candidate the blocklist should be tested against.
///
/// This is what makes a 460-line list worth more than its length. `P@ssw0rd!2024` and
/// `Password` are the same guess to an attacker working from a leaked corpus, so they must
/// be the same guess here. Three transformations, applied in both orders that matter:
/// lowercase, strip the decoration people append to get past a length rule, and undo the
/// character substitutions people make to get past a dictionary.
fn variants(candidate: &str) -> Vec<String> {
    let lowered = candidate.to_lowercase();
    let stripped = strip_decoration(&lowered);
    let mut out = vec![
        lowered.clone(),
        stripped.clone(),
        deleet(&lowered),
        // Strip first, then de-leet: `passw0rd2024` needs the digits gone before `0` can
        // be read as `o`, and `p4ssword!!` needs the reverse.
        deleet(&stripped),
        strip_decoration(&deleet(&lowered)),
    ];
    out.sort();
    out.dedup();
    out.retain(|v| !v.is_empty());
    out
}

/// Drop the leading and trailing runs of non-letters: years, `!!!`, `123`.
fn strip_decoration(s: &str) -> String {
    s.trim_matches(|c: char| !c.is_alphabetic()).to_string()
}

/// Undo the usual character substitutions.
fn deleet(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '0' => 'o',
            '1' | '!' | '|' => 'i',
            '3' => 'e',
            '4' | '@' => 'a',
            '5' | '$' => 's',
            '7' => 't',
            '8' => 'b',
            '9' => 'g',
            other => other,
        })
        .collect()
}

/// Longest run of characters each one code point above (or below) the last: the length of
/// the longest `abcdef` or `987654` inside the string.
fn longest_sequential_run(s: &str) -> usize {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    let (mut best, mut up, mut down) = (1usize, 1usize, 1usize);
    for w in chars.windows(2) {
        let (a, b) = (w[0] as i64, w[1] as i64);
        up = if b - a == 1 { up + 1 } else { 1 };
        down = if a - b == 1 { down + 1 } else { 1 };
        best = best.max(up).max(down);
    }
    best
}

/// A run this long is a keyboard walk or a counted sequence, never a chosen secret. Six
/// keeps `abcdef` out while leaving a real passphrase untouched: English words do not carry
/// six consecutive code points.
const MAX_SEQUENTIAL_RUN: usize = 6;

/// True when the string is a single repeated character.
fn is_one_repeated_char(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        None => true,
        Some(first) => chars.all(|c| c == first),
    }
}

/// Screen a new password. `username` is the account it is being set on, so the password
/// cannot simply be the name. Returns the message to show the user, never a code: there is
/// one person reading it and a precise reason is what gets a better password typed.
pub fn check_password(password: &str, username: &str) -> Result<(), String> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters (a passphrase of a few words is easiest)"
        ));
    }
    if len > MAX_PASSWORD_LEN {
        return Err(format!("password must be at most {MAX_PASSWORD_LEN} characters"));
    }
    if password.trim().is_empty() {
        return Err("password cannot be only whitespace".into());
    }

    let lowered = password.to_lowercase();
    if is_one_repeated_char(&lowered) || longest_sequential_run(&lowered) >= MAX_SEQUENTIAL_RUN {
        return Err(
            "that password is a repeated character or a counted sequence, which is guessed \
             before anything else"
                .into(),
        );
    }

    let forms = variants(password);

    let username = username.trim().to_lowercase();
    if !username.is_empty() && forms.iter().any(|v| v.contains(&username)) {
        return Err("password must not contain the account name".into());
    }

    if forms.iter().any(|v| blocklist().contains(v.as_str())) {
        return Err(
            "that password appears in public breach corpora, with or without the digits and \
             symbols around it. Pick something unrelated to a common word."
                .into(),
        );
    }

    Ok(())
}

// ── TOTP (RFC 6238) ──────────────────────────────────────────────────────────

pub mod totp {
    //! Time-based one-time passwords: SHA-1, 6 digits, 30-second steps, which is what every
    //! authenticator app assumes when a URI omits the parameters.

    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    /// Step length in seconds.
    pub const PERIOD: u64 = 30;
    /// Steps of clock skew accepted either side of now. One step is the interoperable
    /// choice: it forgives a phone that is half a minute off without widening the window
    /// an attacker guesses into.
    const SKEW_STEPS: i64 = 1;
    /// Shared-secret length. RFC 4226 requires at least 128 bits and recommends 160.
    const SECRET_BYTES: usize = 20;

    /// A fresh base32 secret, in the shape an authenticator expects to be handed.
    pub fn generate_secret() -> anyhow::Result<String> {
        let mut bytes = [0u8; SECRET_BYTES];
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
        Ok(data_encoding::BASE32_NOPAD.encode(&bytes))
    }

    /// The `otpauth://` URI the enrolment QR code encodes.
    pub fn uri(secret_b32: &str, account: &str) -> String {
        let issuer = "OpenTraderWorld";
        let label = urlencode(&format!("{issuer}:{account}"));
        format!(
            "otpauth://totp/{label}?secret={secret_b32}&issuer={issuer}\
             &algorithm=SHA1&digits=6&period={PERIOD}"
        )
    }

    /// Percent-encode everything that is not unreserved. Small and local: the only input is
    /// an issuer constant and a username.
    fn urlencode(s: &str) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    (b as char).to_string()
                }
                other => format!("%{other:02X}"),
            })
            .collect()
    }

    /// One HOTP value for a counter (RFC 4226 dynamic truncation).
    fn hotp(secret: &[u8], counter: u64) -> u32 {
        let mut mac =
            Hmac::<Sha1>::new_from_slice(secret).expect("HMAC accepts a key of any length");
        mac.update(&counter.to_be_bytes());
        let digest = mac.finalize().into_bytes();
        let offset = (digest[19] & 0x0f) as usize;
        let truncated = u32::from_be_bytes([
            digest[offset] & 0x7f,
            digest[offset + 1],
            digest[offset + 2],
            digest[offset + 3],
        ]);
        truncated % 1_000_000
    }

    /// The counter for a unix timestamp.
    fn step_at(unix_seconds: u64) -> u64 {
        unix_seconds / PERIOD
    }

    /// The step a code issued right now belongs to. Only the tests need it: the running
    /// code learns the step from [`verify`], which is the one that knows which of the
    /// accepted steps actually matched.
    #[cfg(test)]
    pub fn current_step() -> u64 {
        step_at(now())
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Verify a user-typed code against a base32 secret, allowing [`SKEW_STEPS`] either
    /// side. Returns the step the code matched, so the caller can refuse a replay of the
    /// same code inside its 30-second life.
    ///
    /// The digit comparison is constant-time: a code is a 20-bit secret for half a minute,
    /// and an early-exit compare over an HTTP endpoint is a measurable oracle.
    pub fn verify(secret_b32: &str, code: &str) -> Option<u64> {
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        if code.len() != 6 {
            return None;
        }
        let secret = data_encoding::BASE32_NOPAD
            .decode(secret_b32.trim().to_uppercase().as_bytes())
            .ok()?;
        let here = step_at(now()) as i64;
        let mut matched: Option<u64> = None;
        for delta in -SKEW_STEPS..=SKEW_STEPS {
            let step = (here + delta).max(0) as u64;
            let expected = format!("{:06}", hotp(&secret, step));
            // subtle's ConstantTimeEq over equal-length byte strings.
            use subtle::ConstantTimeEq;
            if expected.as_bytes().ct_eq(code.as_bytes()).into() {
                matched = Some(step);
            }
        }
        matched
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// RFC 6238 appendix B vectors, SHA-1, secret "12345678901234567890".
        #[test]
        fn rfc6238_vectors() {
            let secret = b"12345678901234567890";
            for (time, expected) in [
                (59u64, 287082u32),
                (1111111109, 81804),
                (1111111111, 50471),
                (1234567890, 5924),
                (2000000000, 279037),
                (20000000000, 353130),
            ] {
                assert_eq!(hotp(secret, step_at(time)), expected, "t={time}");
            }
        }

        #[test]
        fn verify_accepts_the_current_code_and_rejects_noise() {
            let secret = generate_secret().unwrap();
            let raw = data_encoding::BASE32_NOPAD.decode(secret.as_bytes()).unwrap();
            let code = format!("{:06}", hotp(&raw, current_step()));
            assert!(verify(&secret, &code).is_some());
            assert!(verify(&secret, "000000").is_none() || code == "000000");
            assert!(verify(&secret, "12345").is_none());
            assert!(verify(&secret, "abcdef").is_none());
        }

        #[test]
        fn uri_carries_the_secret_and_the_issuer() {
            let uri = uri("ABCDEFGH", "alice");
            assert!(uri.starts_with("otpauth://totp/OpenTraderWorld%3Aalice?"));
            assert!(uri.contains("secret=ABCDEFGH"));
            assert!(uri.contains("issuer=OpenTraderWorld"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_passwords_are_refused() {
        assert!(check_password("short", "alice").is_err());
        assert!(check_password(&"a".repeat(MIN_PASSWORD_LEN - 1), "alice").is_err());
    }

    #[test]
    fn decorated_common_passwords_are_refused() {
        for candidate in [
            "password1234",
            "P@ssw0rd!2024",
            "Letmein!!!!!!",
            "qwertyuiop12",
            "Tr4d1ng2024!",
            "opentraderworld",
        ] {
            assert!(
                check_password(candidate, "alice").is_err(),
                "{candidate} should be refused"
            );
        }
    }

    #[test]
    fn runs_and_repeats_are_refused() {
        assert!(check_password("aaaaaaaaaaaaaa", "alice").is_err());
        assert!(check_password("abcdefghijklm", "alice").is_err());
        assert!(check_password("987654321098765", "alice").is_err());
    }

    #[test]
    fn the_account_name_is_refused() {
        assert!(check_password("alice-is-here-ok", "alice").is_err());
        assert!(check_password("XXalice12345678", "Alice").is_err());
    }

    #[test]
    fn a_real_passphrase_is_accepted() {
        for candidate in [
            "fennel-ladder-oxide-73",
            "quiet harbour drifting north",
            "vR9#mqLz2!tWpc",
        ] {
            assert!(
                check_password(candidate, "alice").is_ok(),
                "{candidate} should be accepted: {:?}",
                check_password(candidate, "alice")
            );
        }
    }

    #[test]
    fn overlong_passwords_are_refused() {
        assert!(check_password(&"x".repeat(MAX_PASSWORD_LEN + 1), "alice").is_err());
    }
}
