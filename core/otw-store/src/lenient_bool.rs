//! Lenient boolean for query strings: `true/false`, `1/0`, `yes/no`, `on/off`, any case.
//!
//! Serde's own `bool` reads only `true` and `false` from a query string, so `?unread=1` is a
//! 400 although every HTTP client and half the docs out there write it that way. Use it with
//! `#[schemars(with = "bool")]` (or `"Option<bool>"`) so the published schema still says
//! boolean:
//!
//! ```ignore
//! #[serde(default, deserialize_with = "otw_store::lenient_bool::deserialize_lenient_bool")]
//! #[schemars(with = "bool")]
//! full: bool,
//! ```
//!
//! It lives in the store crate because one of the query structs it serves (the mailbox
//! message filter) is defined here; `otw-core` re-exports it as `util::lenient_bool`.

use serde::de::{self, Deserializer, Visitor};
use std::fmt;

/// Every spelling accepted, true ones first. Also what an error message lists.
pub const TRUE_WORDS: &[&str] = &["true", "1", "yes", "on"];
pub const FALSE_WORDS: &[&str] = &["false", "0", "no", "off"];

/// Parse one lenient boolean word (trimmed, case-insensitive).
pub fn parse_lenient_bool(s: &str) -> Option<bool> {
    let s = s.trim();
    if TRUE_WORDS.iter().any(|w| w.eq_ignore_ascii_case(s)) {
        Some(true)
    } else if FALSE_WORDS.iter().any(|w| w.eq_ignore_ascii_case(s)) {
        Some(false)
    } else {
        None
    }
}

/// `true, false, 1, 0, yes, no, on, off`, for error messages.
pub fn accepted_words() -> String {
    TRUE_WORDS
        .iter()
        .zip(FALSE_WORDS)
        .flat_map(|(t, f)| [*t, *f])
        .collect::<Vec<_>>()
        .join(", ")
}

/// `None` stands for an empty value (`?flag=`), which only the optional form accepts.
struct LenientVisitor;

impl<'de> Visitor<'de> for LenientVisitor {
    type Value = Option<bool>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a boolean ({})", accepted_words())
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Some(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        match v {
            0 => Ok(Some(false)),
            1 => Ok(Some(true)),
            _ => Err(E::invalid_value(de::Unexpected::Unsigned(v), &self)),
        }
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        match v {
            0 => Ok(Some(false)),
            1 => Ok(Some(true)),
            _ => Err(E::invalid_value(de::Unexpected::Signed(v), &self)),
        }
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        if v.trim().is_empty() {
            return Ok(None);
        }
        parse_lenient_bool(v)
            .map(Some)
            .ok_or_else(|| E::invalid_value(de::Unexpected::Str(v), &self))
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(LenientVisitor)
    }
}

/// For a `bool` field (pair it with `#[serde(default)]` when the param is optional).
pub fn deserialize_lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    d.deserialize_any(LenientVisitor)?.ok_or_else(|| {
        de::Error::custom(format!("expected a boolean ({}), got an empty value", accepted_words()))
    })
}

/// For an `Option<bool>` field (pair it with `#[serde(default)]`). An empty value is `None`.
pub fn deserialize_lenient_opt_bool<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    d.deserialize_option(LenientVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_trimmed_and_case_insensitive() {
        assert_eq!(parse_lenient_bool(" YES "), Some(true));
        assert_eq!(parse_lenient_bool("Off"), Some(false));
        assert_eq!(parse_lenient_bool("2"), None);
        assert_eq!(accepted_words(), "true, false, 1, 0, yes, no, on, off");
    }
}
