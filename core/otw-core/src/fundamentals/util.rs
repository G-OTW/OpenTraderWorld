//! Helpers shared by the fundamentals providers: HTTP with the key kept out of errors,
//! period parsing, lenient number reading, and curated series catalogs.

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::{Date, Month};

use otw_store::fundamentals::SeriesMeta;

use super::edgar::TIMEOUT;

/// GET and decode JSON. A provider failure keeps the provider's own words but never the
/// URL, which carries the key for most of them.
pub async fn get_json(provider: &str, req: reqwest::RequestBuilder) -> Result<Value> {
    let text = get_text(provider, req).await?;
    if text.trim().is_empty() {
        // The US Census answers an empty 200/204 both when nothing matches and when a key
        // has not been activated from its e-mail link yet.
        return Err(anyhow!(
            "{provider} answered with no content: nothing matches this query, or the key is not activated yet (check the provider's sign-up e-mail)"
        ));
    }
    serde_json::from_str(&text).with_context(|| format!("{provider}: response is not JSON"))
}

pub async fn get_text(provider: &str, req: reqwest::RequestBuilder) -> Result<String> {
    let resp = crate::rate::send(provider, req.timeout(TIMEOUT))
        .await
        .map_err(|e| e.without_url())
        .with_context(|| format!("{provider} request"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.without_url()).with_context(|| format!("{provider} response"))?;
    if !status.is_success() {
        let snippet: String = text.chars().take(240).collect();
        return Err(match status.as_u16() {
            401 | 403 => anyhow!("{provider} refused the key or the plan (HTTP {}): {snippet}", status.as_u16()),
            404 => anyhow!("{provider} has no such series or symbol (HTTP 404)"),
            429 => anyhow!("{provider} rate limit hit (HTTP 429): wait and retry"),
            s => anyhow!("{provider} answered HTTP {s}: {snippet}"),
        });
    }
    Ok(text)
}

/// A number the provider may send as a number, a numeric string, or "None"/"-"/"".
pub fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_end_matches('%').replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
    .filter(|f| f.is_finite())
}

/// A string field; Alpha Vantage's "None" placeholder reads as empty.
pub fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) if s.trim() == "None" => String::new(),
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub fn date(v: Option<&Value>) -> Option<Date> {
    super::parse_date(v?.as_str()?)
}

fn ymd(y: i32, m: u8, d: u8) -> Option<Date> {
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

/// A statistical period as its start date plus the frequency it implies:
/// `2026`, `2026-Q2`, `2026Q2`, `2026-S1`, `2026-08`, `2026-M08`, `2026M08`, `2026-W05`,
/// `2026-08-15`.
pub fn parse_period(s: &str) -> Option<(Date, &'static str)> {
    let s = s.trim();
    let year: i32 = s.get(..4)?.parse().ok()?;
    let rest = s[4..].trim_start_matches('-');
    if rest.is_empty() {
        return Some((ymd(year, 1, 1)?, "A"));
    }
    if let Some(q) = rest.strip_prefix('Q') {
        let q: u8 = q.parse().ok()?;
        return Some((ymd(year, (q.checked_sub(1)?) * 3 + 1, 1)?, "Q"));
    }
    if let Some(h) = rest.strip_prefix('S').or_else(|| rest.strip_prefix('H')) {
        let h: u8 = h.parse().ok()?;
        return Some((ymd(year, (h.checked_sub(1)?) * 6 + 1, 1)?, "A"));
    }
    if let Some(w) = rest.strip_prefix('W') {
        let w: u8 = w.parse().ok()?;
        return Some((Date::from_iso_week_date(year, w, time::Weekday::Monday).ok()?, "W"));
    }
    let rest = rest.trim_start_matches('M');
    if rest.len() <= 2 {
        return Some((ymd(year, rest.parse().ok()?, 1)?, "M"));
    }
    super::parse_date(s).map(|d| (d, "D"))
}

/// One entry of a provider's curated catalog: what the add-series dialog offers before the
/// user types a code of their own.
pub struct Entry {
    pub code: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub country: &'static str,
    pub unit: &'static str,
}

pub fn entry_meta(provider: &str, e: &Entry, frequency: &str) -> SeriesMeta {
    SeriesMeta {
        provider: provider.into(),
        code: e.code.into(),
        title: e.title.into(),
        category: e.category.into(),
        country: e.country.into(),
        unit: e.unit.into(),
        frequency: frequency.into(),
        seasonal_adj: false,
        notes: String::new(),
    }
}

/// Filter a curated catalog by free text; an empty query lists it all.
pub fn search_catalog(provider: &str, entries: &[Entry], q: &str) -> Vec<SeriesMeta> {
    let q = q.trim().to_lowercase();
    entries
        .iter()
        .filter(|e| {
            q.is_empty()
                || e.title.to_lowercase().contains(&q)
                || e.code.to_lowercase().contains(&q)
                || e.country.to_lowercase() == q
        })
        .map(|e| entry_meta(provider, e, ""))
        .collect()
}

pub fn find_entry<'a>(entries: &'a [Entry], code: &str) -> Option<&'a Entry> {
    entries.iter().find(|e| e.code.eq_ignore_ascii_case(code))
}

/// Observations sorted, one per period (the last one wins on a duplicate).
pub fn tidy(mut obs: Vec<(Date, f64)>) -> Vec<(Date, f64)> {
    obs.sort_by_key(|(d, _)| *d);
    obs.dedup_by(|b, a| {
        if a.0 == b.0 {
            a.1 = b.1;
            true
        } else {
            false
        }
    });
    obs
}

/// The connector's `api_key`. A key is printable ASCII; anything else came with a copy and
/// paste (a zero-width space, a byte-order mark) and makes the provider reject a valid key.
pub fn need_key(secrets: &std::collections::HashMap<String, String>, label: &str, url: &str) -> Result<String> {
    secrets
        .get("api_key")
        .map(|k| k.chars().filter(char::is_ascii_graphic).collect::<String>())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| anyhow!("{label} needs an 'api_key' credential: get one at {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_read_as_their_start() {
        let d = |y, m, dd| ymd(y, m, dd).unwrap();
        assert_eq!(parse_period("2026"), Some((d(2026, 1, 1), "A")));
        assert_eq!(parse_period("2026-Q3"), Some((d(2026, 7, 1), "Q")));
        assert_eq!(parse_period("2026Q3"), Some((d(2026, 7, 1), "Q")));
        assert_eq!(parse_period("2026-08"), Some((d(2026, 8, 1), "M")));
        assert_eq!(parse_period("2026-M08"), Some((d(2026, 8, 1), "M")));
        assert_eq!(parse_period("2026M08"), Some((d(2026, 8, 1), "M")));
        assert_eq!(parse_period("2026-08-15"), Some((d(2026, 8, 15), "D")));
        assert_eq!(parse_period("2026-W01").map(|p| p.1), Some("W"));
        assert_eq!(num(Some(&Value::from("1,234.5"))), Some(1234.5));
        assert_eq!(num(Some(&Value::from("None"))), None);
    }
}
