//! HTML hygiene for stored mail.
//!
//! Newsletter HTML is hostile by default: tracking pixels, remote CSS, click-wrapped
//! links, sometimes scripts. Two rules make it safe to display:
//!
//! 1. **Sanitise once, at ingest.** Scripts, styles, forms and frames never reach the
//!    database, so a body cannot become dangerous later by being rendered differently.
//! 2. **Park remote images.** Their URL moves from `src` to `data-otw-src`, so opening a
//!    mail sends no request to the sender — the open-tracking pixel that is the entire
//!    economy of newsletter mail simply never fires. The reader can put them back on
//!    demand ([`restore_images`]), which is a deliberate, per-message choice.

use std::sync::OnceLock;

/// Ammonia configuration: the default safe tag set, links opened in a new tab and
/// de-referrered, `data-otw-src` allowed on images so parked URLs survive a re-clean.
fn cleaner() -> &'static ammonia::Builder<'static> {
    static CLEANER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    CLEANER.get_or_init(|| {
        let mut b = ammonia::Builder::default();
        b.link_rel(Some("noopener noreferrer nofollow"))
            .add_tag_attributes("img", ["data-otw-src"])
            .set_tag_attribute_value("a", "target", "_blank")
            .url_schemes(["http", "https", "mailto"].into_iter().collect());
        b
    })
}

/// Sanitise a mail body. Returns the clean HTML and whether it references remote images.
pub fn clean(html: &str) -> (String, bool) {
    let cleaned = cleaner().clean(html).to_string();
    park_images(&cleaned)
}

/// Move remote `src` URLs on `<img>` to `data-otw-src`. Runs on ammonia output, so the
/// markup is normalised (`attr="value"`, double quotes) and a byte scan is exact.
fn park_images(html: &str) -> (String, bool) {
    let mut out = String::with_capacity(html.len() + 32);
    let mut found = false;
    let mut i = 0usize;
    while let Some(rel) = html[i..].find("<img") {
        let tag_start = i + rel;
        let tag_end = match html[tag_start..].find('>') {
            Some(e) => tag_start + e + 1,
            None => break,
        };
        out.push_str(&html[i..tag_start]);
        let tag = &html[tag_start..tag_end];
        match tag.find(" src=\"") {
            // Only http(s) URLs are parked: `cid:` and data URIs load nothing remotely.
            Some(p) if is_remote(&tag[p + 6..]) => {
                found = true;
                out.push_str(&tag[..p]);
                out.push_str(" data-otw-src=\"");
                out.push_str(&tag[p + 6..]);
            }
            _ => out.push_str(tag),
        }
        i = tag_end;
    }
    out.push_str(&html[i..]);
    (out, found)
}

fn is_remote(rest: &str) -> bool {
    let url = rest.split('"').next().unwrap_or("");
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://")
}

/// Put parked images back — called only when the reader explicitly asks for them.
pub fn restore_images(html: &str) -> String {
    html.replace(" data-otw-src=\"", " src=\"")
}

/// Very small tag stripper, used for the list snippet when a mail has no plain-text part.
pub fn to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Collapse whitespace and cut to `max` characters — the one-line preview in the list.
pub fn snippet(text: &str, max: usize) -> String {
    let mut s = String::with_capacity(max + 8);
    let mut space = true; // skip leading whitespace
    for c in text.chars() {
        if c.is_whitespace() {
            if !space {
                s.push(' ');
                space = true;
            }
        } else {
            s.push(c);
            space = false;
        }
        if s.chars().count() >= max {
            break;
        }
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_and_styles_never_survive() {
        let (out, _) = clean("<p>hi</p><script>alert(1)</script><style>p{}</style>");
        assert!(!out.contains("script"));
        assert!(!out.contains("alert"));
        assert!(out.contains("hi"));
    }

    #[test]
    fn remote_images_are_parked_and_restorable() {
        let (out, found) = clean(r#"<img src="https://track.example/pixel.gif?u=42">"#);
        assert!(found);
        assert!(!out.contains(" src="));
        assert!(out.contains("data-otw-src=\"https://track.example/pixel.gif?u=42\""));
        assert!(restore_images(&out).contains(" src=\"https://track.example/pixel.gif?u=42\""));
    }

    #[test]
    fn javascript_urls_are_dropped_before_parking() {
        let (out, found) = clean(r#"<a href="javascript:alert(1)">x</a><img src="cid:logo">"#);
        assert!(!found);
        assert!(!out.contains("javascript"));
    }

    #[test]
    fn snippet_collapses_whitespace() {
        assert_eq!(snippet("  a\n\n  b   c ", 100), "a b c");
        assert_eq!(snippet("abcdef", 3), "abc");
    }
}
