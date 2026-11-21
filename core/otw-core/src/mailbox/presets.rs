//! Provider presets — the "which mail services can I connect?" answer, as data.
//!
//! Every entry is plain IMAP with an app password, because that is what a self-hosted app
//! with no public callback URL can do honestly. Providers that dropped password auth for
//! IMAP are still listed, marked `supported: false`, so the screen answers the question
//! instead of leaving the user to discover the dead end themselves.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub host: &'static str,
    pub port: u16,
    pub security: &'static str,
    /// `password` (app password) or `oauth` (browser sign-in).
    pub auth: &'static str,
    /// Where the user creates the app password, or registers the OAuth application.
    pub help_url: &'static str,
    /// False = this provider cannot be connected at all.
    pub supported: bool,
}

const fn p(
    id: &'static str,
    label: &'static str,
    host: &'static str,
    port: u16,
    security: &'static str,
    help_url: &'static str,
) -> Preset {
    Preset {
        id,
        label,
        host,
        port,
        security,
        auth: "password",
        help_url,
        supported: true,
    }
}

pub const PRESETS: &[Preset] = &[
    p(
        "fastmail",
        "Fastmail",
        "imap.fastmail.com",
        993,
        "ssl",
        "https://app.fastmail.com/settings/security/apppasswords",
    ),
    p(
        "mailbox-org",
        "mailbox.org",
        "imap.mailbox.org",
        993,
        "ssl",
        "https://kb.mailbox.org/en/private/e-mail-article/imap-and-smtp-settings/",
    ),
    p(
        "migadu",
        "Migadu",
        "imap.migadu.com",
        993,
        "ssl",
        "https://www.migadu.com/guides/",
    ),
    p(
        "zoho",
        "Zoho Mail",
        "imap.zoho.eu",
        993,
        "ssl",
        "https://www.zoho.com/mail/help/imap-access.html",
    ),
    p(
        "gmail",
        "Gmail",
        "imap.gmail.com",
        993,
        "ssl",
        "https://myaccount.google.com/apppasswords",
    ),
    p(
        "icloud",
        "iCloud Mail",
        "imap.mail.me.com",
        993,
        "ssl",
        "https://support.apple.com/en-us/102654",
    ),
    p(
        "yahoo",
        "Yahoo / AOL",
        "imap.mail.yahoo.com",
        993,
        "ssl",
        "https://help.yahoo.com/kb/SLN15241.html",
    ),
    p(
        "posteo",
        "Posteo",
        "posteo.de",
        993,
        "ssl",
        "https://posteo.de/en/help/which-settings-do-i-need-to-set-in-my-email-program",
    ),
    p(
        "proton-bridge",
        "Proton Mail (Bridge)",
        "127.0.0.1",
        1143,
        "starttls",
        "https://proton.me/mail/bridge",
    ),
    // Microsoft dropped password auth for IMAP, so this one signs in with OAuth:
    // authorization code + PKCE, coming back to this app's own `/mailbox/oauth`, with the
    // device code kept only as a fallback. The user registers a free public client and
    // pastes its id.
    Preset {
        auth: "oauth",
        ..p(
            "outlook",
            "Outlook.com / Microsoft 365",
            "outlook.office365.com",
            993,
            "ssl",
            "https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade",
        )
    },
    p("generic", "Other IMAP server", "", 993, "ssl", ""),
];
