//! Raw RFC-822 bytes → the few fields this module keeps.
//!
//! Classification lives here too: a mail is *bulk* when it carries the headers a mailing
//! list is required to send (`List-Unsubscribe`, `List-Id`, `Precedence: bulk`). Bulk mail
//! is kept automatically; anything else is only recorded as a sender awaiting the user's
//! decision, so connecting a personal mailbox never silently copies private conversations
//! into the database.

use mail_parser::{MessageParser, MimeHeaders};
use sqlx::types::time::OffsetDateTime;

pub struct Attachment {
    pub filename: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub struct ParsedMail {
    pub from_addr: String,
    pub from_name: String,
    pub subject: String,
    pub message_id: String,
    pub date: Option<OffsetDateTime>,
    pub html: Option<String>,
    pub text: Option<String>,
    pub list_unsubscribe: String,
    /// RFC 8058: the sender accepts a one-click POST to unsubscribe.
    pub list_unsub_post: bool,
    pub is_bulk: bool,
    pub attachments: Vec<Attachment>,
}

/// Parse one mail. `None` only when the bytes are not a message at all.
pub fn parse(raw: &[u8]) -> Option<ParsedMail> {
    let msg = MessageParser::default().parse(raw)?;

    let (from_name, from_addr) = msg
        .from()
        .and_then(|a| a.first())
        .map(|a| {
            (
                a.name().unwrap_or_default().trim().to_string(),
                a.address().unwrap_or_default().trim().to_lowercase(),
            )
        })
        .unwrap_or_default();

    // Raw values, not parsed ones: mail-parser reads `List-Unsubscribe` as an address
    // list, which mangles the `<https://…>` entry this module actually needs.
    let header_text = |name: &str| -> String {
        msg.header_raw(name)
            .unwrap_or_default()
            .replace(['\r', '\n'], " ")
            .trim()
            .to_string()
    };

    let list_unsubscribe = header_text("List-Unsubscribe");
    let list_id = header_text("List-Id");
    let precedence = header_text("Precedence").to_ascii_lowercase();
    let list_unsub_post = header_text("List-Unsubscribe-Post")
        .to_ascii_lowercase()
        .contains("one-click");
    let is_bulk = !list_unsubscribe.is_empty()
        || !list_id.is_empty()
        || matches!(precedence.as_str(), "bulk" | "list" | "junk");

    let attachments = msg
        .attachments()
        .filter(|p| {
            // Inline parts referenced by the body (`cid:` images, signatures) are not
            // attachments a user would look for; only named payloads are kept.
            p.attachment_name().is_some_and(|n| !n.trim().is_empty()) && p.content_id().is_none()
        })
        .map(|p| Attachment {
            filename: sanitize_filename(p.attachment_name().unwrap_or("attachment")),
            mime: p
                .content_type()
                .map(|c| match c.subtype() {
                    Some(sub) => format!("{}/{}", c.ctype(), sub),
                    None => c.ctype().to_string(),
                })
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            bytes: p.contents().to_vec(),
        })
        .collect();

    Some(ParsedMail {
        from_addr,
        from_name,
        subject: msg.subject().unwrap_or_default().trim().to_string(),
        message_id: msg.message_id().unwrap_or_default().trim().to_string(),
        date: msg
            .date()
            .and_then(|d| OffsetDateTime::from_unix_timestamp(d.to_timestamp()).ok()),
        html: msg.body_html(0).map(|s| s.into_owned()),
        text: msg.body_text(0).map(|s| s.into_owned()),
        list_unsubscribe,
        list_unsub_post,
        is_bulk,
        attachments,
    })
}

/// Keep a filename usable as a download name: no path separators, no control characters.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\r' | '\n' | '"' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .take(120)
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "attachment".to_string()
    } else {
        cleaned
    }
}

/// The https URL of a `List-Unsubscribe` header, if it offers one.
/// The header is a list of `<…>` entries mixing `mailto:` and `https:`.
pub fn unsubscribe_url(header: &str) -> Option<String> {
    header
        .split(',')
        .map(|p| p.trim().trim_start_matches('<').trim_end_matches('>').trim())
        .find(|p| p.starts_with("https://") || p.starts_with("http://"))
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEWSLETTER: &[u8] = b"From: The Weekly <hello@weekly.example>\r\n\
Subject: Issue 42\r\n\
Message-ID: <abc@weekly.example>\r\n\
List-Unsubscribe: <mailto:u@weekly.example>, <https://weekly.example/u/42>\r\n\
List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\
Content-Type: text/plain\r\n\r\n\
Hello there.\r\n";

    const PERSONAL: &[u8] = b"From: Ana <ana@example.org>\r\n\
Subject: lunch?\r\n\
Content-Type: text/plain\r\n\r\n\
tomorrow?\r\n";

    #[test]
    fn bulk_mail_is_recognised() {
        let m = parse(NEWSLETTER).expect("parses");
        assert!(m.is_bulk);
        assert!(m.list_unsub_post);
        assert_eq!(m.from_addr, "hello@weekly.example");
        assert_eq!(m.from_name, "The Weekly");
        assert_eq!(m.subject, "Issue 42");
        assert_eq!(
            unsubscribe_url(&m.list_unsubscribe).as_deref(),
            Some("https://weekly.example/u/42")
        );
    }

    #[test]
    fn personal_mail_is_not_bulk() {
        let m = parse(PERSONAL).expect("parses");
        assert!(!m.is_bulk);
        assert!(m.list_unsubscribe.is_empty());
    }

    #[test]
    fn filenames_cannot_escape() {
        let f = sanitize_filename("../../etc/passwd");
        assert!(!f.contains('/') && !f.contains('\\') && !f.starts_with('.'), "{f}");
        assert_eq!(sanitize_filename("statement 2026-07.pdf"), "statement 2026-07.pdf");
        assert_eq!(sanitize_filename("  "), "attachment");
    }
}
