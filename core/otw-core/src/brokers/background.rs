//! Background trading: the permission, signed once per broker account, for orders sent with
//! nobody watching (a trade on alert, a strategy on a broker account).
//!
//! The signature is the only one the app asks for. The `trading` grant, live trading, the
//! module grants and the credentials keep their own flows; this adds the one thing those do not
//! say: that an order may leave while the browser is closed. Without it, an order from a
//! background sender is refused at every send ([`super::dispatch::send_internal`]), so a
//! permission taken back between arming and firing stops the order.
//!
//! What the record holds is what the user saw: the terms sections and the discharge statements
//! in the language shown, as the page rendered them. The language packs live in the browser,
//! so the server cannot render the text itself; it checks instead that every statement it
//! requires came back ticked with its text, and that the account name was typed. Whether such a
//! discharge binds is a matter for the user's jurisdiction; the record only proves what was
//! shown and accepted, when, and with which second factor.

use serde::{Deserialize, Serialize};

/// The discharge statements, each a checkbox in the modal (`brokers.background.statement.*`).
/// All of them must come back ticked: a signature with one missing is refused, never recorded
/// as partial.
pub const STATEMENTS: &[&str] = &["free", "tested", "venue", "unattended", "losses", "paper_first"];

/// The terms sections the modal shows verbatim (`settings.terms.<key>.*`) and the record keeps.
/// `trading` is shown too where the language pack has it; these three always exist.
pub const TERMS_REQUIRED: &[&str] = &["testing", "warranty", "liability"];
pub const TERMS_SHOWN: &[&str] = &["trading", "testing", "warranty", "liability"];

/// Languages the app ships (same list as Settings, Appearance).
const LOCALES: &[&str] = &["en", "fr", "it", "es", "de", "pt", "zh"];

/// A record is proof, not a document store: past this the request is not a rendered modal.
const MAX_BYTES: usize = 64 * 1024;

/// One statement as it was shown and ticked.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Statement {
    pub key: String,
    pub text: String,
    #[serde(default)]
    pub accepted: bool,
}

/// One terms section as it was shown: its title and its paragraphs and bullets, in order.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TermsSection {
    pub key: String,
    pub title: String,
    pub lines: Vec<String>,
}

/// What the modal sends to sign.
#[derive(Debug, Clone, Deserialize)]
pub struct Signature {
    pub statements: Vec<Statement>,
    pub terms: Vec<TermsSection>,
    /// The terms' "last updated" line as shown.
    pub terms_version: String,
    pub locale: String,
    /// The account name as typed.
    pub typed: String,
    /// The authenticator code, when the trading lock is on and the account is not paper.
    #[serde(default)]
    pub code: String,
}

/// Check a signature against the account it is for. One sentence naming what is missing.
pub fn check(s: &Signature, account_name: &str) -> Result<(), String> {
    if s.typed.trim() != account_name.trim() {
        return Err(format!("type the account name “{account_name}” exactly to sign"));
    }
    for key in STATEMENTS {
        match s.statements.iter().find(|x| x.key == *key) {
            Some(x) if x.accepted && !x.text.trim().is_empty() => {}
            _ => return Err("tick every statement to sign: each one is part of the permission".into()),
        }
    }
    if let Some(x) = s.statements.iter().find(|x| !STATEMENTS.contains(&x.key.as_str())) {
        return Err(format!("unknown statement “{}”: reload the page and sign again", x.key));
    }
    for key in TERMS_REQUIRED {
        let shown = s.terms.iter().any(|t| t.key == *key && !t.title.trim().is_empty() && t.lines.iter().any(|l| !l.trim().is_empty()));
        if !shown {
            return Err("the terms did not come with the signature: reload the page and sign again".into());
        }
    }
    if let Some(t) = s.terms.iter().find(|t| !TERMS_SHOWN.contains(&t.key.as_str())) {
        return Err(format!("unknown terms section “{}”: reload the page and sign again", t.key));
    }
    if s.terms_version.trim().is_empty() || s.terms_version.len() > 200 {
        return Err("the terms' date did not come with the signature: reload the page and sign again".into());
    }
    if !LOCALES.contains(&s.locale.as_str()) {
        return Err(format!("unknown language “{}”: reload the page and sign again", s.locale));
    }
    let size = serde_json::to_vec(&s.statements).map_or(0, |v| v.len()) + serde_json::to_vec(&s.terms).map_or(0, |v| v.len());
    if size > MAX_BYTES {
        return Err("the signature is too large to be a page as shown: reload the page and sign again".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Signature {
        Signature {
            statements: STATEMENTS.iter().map(|k| Statement { key: (*k).into(), text: format!("statement {k}"), accepted: true }).collect(),
            terms: TERMS_SHOWN.iter().map(|k| TermsSection { key: (*k).into(), title: format!("Title {k}"), lines: vec![format!("line of {k}")] }).collect(),
            terms_version: "Last updated: 28 October 2026".into(),
            locale: "fr".into(),
            typed: "Binance main".into(),
            code: String::new(),
        }
    }

    #[test]
    fn a_complete_signature_passes() {
        assert_eq!(check(&full(), "Binance main"), Ok(()));
        // The trading section is optional: a language pack without it still signs.
        let mut s = full();
        s.terms.retain(|t| t.key != "trading");
        assert_eq!(check(&s, "Binance main"), Ok(()));
    }

    #[test]
    fn the_account_name_must_be_typed_exactly() {
        let mut s = full();
        s.typed = "binance main".into();
        assert!(check(&s, "Binance main").unwrap_err().contains("type the account name"));
        s.typed = "  Binance main ".into();
        assert_eq!(check(&s, "Binance main"), Ok(()), "surrounding spaces are not part of a name");
    }

    #[test]
    fn every_statement_must_be_ticked_with_its_text() {
        for key in STATEMENTS {
            let mut s = full();
            s.statements.retain(|x| x.key != *key);
            assert!(check(&s, "Binance main").unwrap_err().contains("tick every statement"), "{key} missing");
            let mut s = full();
            s.statements.iter_mut().find(|x| x.key == *key).unwrap().accepted = false;
            assert!(check(&s, "Binance main").is_err(), "{key} unticked");
            let mut s = full();
            s.statements.iter_mut().find(|x| x.key == *key).unwrap().text = " ".into();
            assert!(check(&s, "Binance main").is_err(), "{key} without its text");
        }
        let mut s = full();
        s.statements.push(Statement { key: "extra".into(), text: "x".into(), accepted: true });
        assert!(check(&s, "Binance main").unwrap_err().contains("unknown statement"));
    }

    #[test]
    fn the_terms_shown_are_part_of_the_record() {
        for key in TERMS_REQUIRED {
            let mut s = full();
            s.terms.retain(|t| t.key != *key);
            assert!(check(&s, "Binance main").unwrap_err().contains("terms"), "{key} missing");
        }
        let mut s = full();
        s.terms_version = String::new();
        assert!(check(&s, "Binance main").is_err());
        let mut s = full();
        s.locale = "xx".into();
        assert!(check(&s, "Binance main").unwrap_err().contains("language"));
        let mut s = full();
        s.terms[0].lines = vec!["x".repeat(MAX_BYTES)];
        assert!(check(&s, "Binance main").unwrap_err().contains("too large"));
    }
}
