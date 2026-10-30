//! Alternative data: congress trades, lobbying, government contracts and patents.
//!
//! The public sources (USAspending, LDA.gov, the USPTO Open Data Portal) know a company
//! by its registered name, not its ticker. They are queried with the name EDGAR stored
//! for the company and a row only counts when its name is the same once punctuation and
//! the legal suffix are dropped (`Lockheed Martin Corp` = `LOCKHEED MARTIN CORPORATION`),
//! never on a partial match. Quiver Quant maps tickers itself.
//!
//! Every answer is in the module's shape, whoever filled it:
//! - `congress`  `{ trades: [{ member, chamber, party, ticker, type, amount, traded, disclosed }] }`
//! - `lobbying`  `{ match, filings: [{ period, registrant, amount, issues }], series }`
//! - `contracts` `{ match, awards: [{ date, agency, amount, description, url }], series }`
//! - `patents`   `{ match, total, series }`
//!
//! `series` is `[[ms, value]]` per calendar quarter, oldest first.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::{Date, Month, OffsetDateTime};

use super::util::{self, num, text};
use super::{date_ms, parse_date, Ctx};
use crate::histdata::{enc, Capability};

fn arr(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

/// Words that say what kind of company it is, not which one.
const SUFFIXES: &[&str] = &[
    "INC", "INCORPORATED", "CORP", "CORPORATION", "CO", "COMPANY", "LTD", "LIMITED", "PLC",
    "LLC", "LP", "NV", "SA", "AG", "SE", "THE",
];

/// A registered name reduced to what identifies the company: upper case, punctuation and
/// state tags (`/DE/`) gone, legal suffixes dropped.
pub fn name_key(name: &str) -> String {
    let cleaned: String = name
        .split('/')
        .enumerate()
        .filter(|(i, part)| *i == 0 || part.trim().len() > 3)
        .map(|(_, p)| p)
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .filter(|w| !SUFFIXES.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Start of the calendar quarter holding `d`.
fn quarter_start(d: Date) -> Date {
    let m = ((d.month() as u8 - 1) / 3) * 3 + 1;
    Date::from_calendar_date(d.year(), Month::try_from(m).unwrap_or(Month::January), 1).unwrap_or(d)
}

/// Sum per quarter, as `[[ms, value]]` oldest first.
fn quarterly(points: impl IntoIterator<Item = (Date, f64)>) -> Value {
    let mut by: BTreeMap<Date, f64> = BTreeMap::new();
    for (d, v) in points {
        *by.entry(quarter_start(d)).or_default() += v;
    }
    json!(by.into_iter().map(|(d, v)| (date_ms(d), v)).collect::<Vec<_>>())
}

fn years_ago(n: i64) -> Date {
    OffsetDateTime::now_utc().date() - time::Duration::days(365 * n)
}

/// `purchase` | `sale` | `exchange` | `other`, from however the source words it.
fn trade_type(s: &str) -> &'static str {
    let s = s.to_lowercase();
    if s.contains("purchase") || s == "buy" || s == "p" {
        "purchase"
    } else if s.contains("sale") || s.contains("sell") || s == "s" {
        "sale"
    } else if s.contains("exchange") {
        "exchange"
    } else {
        "other"
    }
}

// ── USAspending ─────────────────────────────────────────────────────────────

pub mod usaspending {
    use super::*;

    pub const PROVIDER: &str = "usaspending";
    const BASE: &str = "https://api.usaspending.gov/api/v2";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "USAspending",
        "https://www.usaspending.gov",
        "https://api.usaspending.gov/docs/endpoints",
        "Keyless, no published cap. Federal contract awards per recipient, matched on the \
         company's registered name.",
        &[],
        &["alt"],
    );
    pub struct UsaSpending;
    super::super::fundamentals_connector!(UsaSpending, CAP);

    async fn post(ctx: &Ctx<'_>, path: &str, body: Value) -> Result<Value> {
        util::get_json(PROVIDER, ctx.client.post(format!("{BASE}/{path}")).json(&body)).await
    }

    /// The recipient registered under the company's name: the parent (its total takes in
    /// the subsidiaries filed under it), else a recipient with no parent. Largest first, as
    /// the API sorts. Searched by the bare name, then by the full one when the bare name is
    /// too common to reach it ("APPLE" before "APPLE INC").
    async fn recipient(ctx: &Ctx<'_>, name: &str) -> Result<Option<(String, String)>> {
        let key = name_key(name);
        let full: String = name
            .to_uppercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for keyword in [key.clone(), full] {
            let v = post(ctx, "recipient/", json!({ "keyword": keyword, "award_type": "all", "limit": 100 })).await?;
            let rows = v.get("results").map(arr).unwrap_or_default();
            let same: Vec<&Value> = rows.iter().filter(|r| name_key(&text(r.get("name"))) == key).collect();
            for level in ["P", "R"] {
                if let Some(r) = same.iter().find(|r| text(r.get("recipient_level")) == level) {
                    return Ok(Some((text(r.get("id")), text(r.get("name")))));
                }
            }
        }
        Ok(None)
    }

    /// Fiscal quarter (FY starts in October) to the calendar quarter it opens.
    pub(super) fn fiscal_start(fy: i32, q: u8) -> Option<Date> {
        let start = Date::from_calendar_date(fy - 1, Month::October, 1).ok()?;
        let mut d = start;
        for _ in 1..q {
            d = quarter_start(d + time::Duration::days(95));
        }
        Some(d)
    }

    pub async fn contracts(ctx: &Ctx<'_>, name: &str) -> Result<Value> {
        let key = name_key(name);
        let (id, registered) = recipient(ctx, name)
            .await?
            .ok_or_else(|| anyhow!("USAspending has no recipient registered as {name}"))?;
        let today = OffsetDateTime::now_utc().date();
        let mut points = Vec::new();
        let v = post(
            ctx,
            "search/spending_over_time/",
            json!({
                "group": "quarter",
                "filters": {
                    "recipient_id": id,
                    "award_type_codes": ["A", "B", "C", "D"],
                    "time_period": [{ "start_date": years_ago(5).to_string(), "end_date": today.to_string() }],
                },
            }),
        )
        .await?;
        for r in v.get("results").map(arr).unwrap_or_default() {
            let fy = text(r.pointer("/time_period/fiscal_year")).parse::<i32>().ok();
            let q = text(r.pointer("/time_period/quarter")).parse::<u8>().ok();
            if let (Some(fy), Some(q), Some(a)) = (fy, q, num(r.get("aggregated_amount"))) {
                if let Some(d) = fiscal_start(fy, q) {
                    points.push((d, a));
                }
            }
        }
        let v = post(
            ctx,
            "search/spending_by_award/",
            json!({
                "filters": {
                    "recipient_search_text": [key],
                    "award_type_codes": ["A", "B", "C", "D"],
                    "time_period": [{
                        "start_date": years_ago(2).to_string(),
                        "end_date": today.to_string(),
                        "date_type": "new_awards_only",
                    }],
                },
                "fields": ["Award ID", "Recipient Name", "Award Amount", "Awarding Agency", "Description", "Start Date", "generated_internal_id"],
                "sort": "Start Date",
                "order": "desc",
                "limit": 100,
                "page": 1,
            }),
        )
        .await?;
        let awards: Vec<Value> = v
            .get("results")
            .map(arr)
            .unwrap_or_default()
            .iter()
            .filter(|r| name_key(&text(r.get("Recipient Name"))) == key)
            .filter(|r| parse_date(&text(r.get("Start Date"))).is_some_and(|d| d <= today))
            .take(50)
            .map(|r| {
                json!({
                    "date": text(r.get("Start Date")),
                    "agency": text(r.get("Awarding Agency")),
                    "amount": num(r.get("Award Amount")),
                    "description": text(r.get("Description")),
                    "url": format!("https://www.usaspending.gov/award/{}", text(r.get("generated_internal_id"))),
                })
            })
            .collect();
        Ok(json!({
            "match": registered,
            "awards": awards,
            "series": quarterly(points),
        }))
    }

    pub async fn probe(ctx: &Ctx<'_>) -> Result<String> {
        let (_, name) = recipient(ctx, "Lockheed Martin Corp")
            .await?
            .ok_or_else(|| anyhow!("USAspending answered without Lockheed Martin"))?;
        Ok(format!("USAspending answered: {name} found"))
    }
}

// ── LDA.gov ─────────────────────────────────────────────────────────────────

pub mod lda {
    use super::*;

    pub const PROVIDER: &str = "lda";
    const BASE: &str = "https://lda.gov/api/v1";
    /// A registered key allows 120 requests a minute.
    const PAGE_GAP: std::time::Duration = std::time::Duration::from_secs(1);
    /// 25 filings a page at most; a large filer posts about 100 a year.
    const MAX_PAGES: usize = 24;
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "LDA.gov (lobbying)",
        "https://lda.gov",
        "https://lda.gov/api/redoc/v1/",
        "Free key: 120 requests a minute. Quarterly lobbying reports (LD-2) per client, matched \
         on the company's registered name.",
        &["api_key"],
        &["alt"],
    );
    pub struct Lda;
    super::super::fundamentals_connector!(Lda, CAP);

    async fn page(ctx: &Ctx<'_>, url: &str) -> Result<Value> {
        let key = util::need_key(ctx.secrets, "LDA.gov", "lda.gov/api/redoc/v1/")?;
        let req = ctx
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::AUTHORIZATION, format!("Token {key}"));
        util::get_json(PROVIDER, req).await.map_err(|e| {
            // The edge firewall turns anonymous calls away from some networks (seen from
            // Swiss and French ones in October 2026); a 403 with a key is the key itself.
            if e.to_string().contains("HTTP 403") {
                anyhow!("LDA.gov refused the request (HTTP 403): check the key, or this network is turned away by its firewall; Quiver Quant serves lobbying too")
            } else {
                e
            }
        })
    }

    fn quarter(period: &str) -> Option<u8> {
        Some(match period {
            "first_quarter" => 1,
            "second_quarter" => 2,
            "third_quarter" => 3,
            "fourth_quarter" => 4,
            _ => return None,
        })
    }

    pub async fn lobbying(ctx: &Ctx<'_>, name: &str) -> Result<Value> {
        let key = name_key(name);
        let mut url = Some(format!(
            "{BASE}/filings/?client_name={}&filing_dt_posted_after={}&ordering=-dt_posted&page_size=25",
            enc(&key),
            years_ago(3)
        ));
        let mut rows = Vec::new();
        let mut pages = 0;
        while let Some(u) = url.take() {
            if pages > 0 {
                tokio::time::sleep(PAGE_GAP).await;
            }
            let v = page(ctx, &u).await?;
            rows.extend(v.get("results").map(arr).unwrap_or_default());
            pages += 1;
            url = v.get("next").and_then(Value::as_str).filter(|_| pages < MAX_PAGES).map(str::to_string);
        }
        let mut filings = Vec::new();
        let mut points = Vec::new();
        // Newest posted first, so the first report seen for a registrant, client and quarter
        // is the latest amendment and replaces the original. Registrations (RR, RA) carry
        // no activity.
        let mut seen = std::collections::HashSet::new();
        for r in rows.iter().filter(|r| name_key(&text(r.pointer("/client/name"))) == key) {
            if text(r.get("filing_type")).starts_with('R') {
                continue;
            }
            let Some(q) = quarter(&text(r.get("filing_period"))) else { continue };
            let Some(year) = r.get("filing_year").and_then(Value::as_i64) else { continue };
            let filer = (text(r.pointer("/registrant/id")), text(r.pointer("/client/id")), year, q);
            if !seen.insert(filer) {
                continue;
            }
            // An outside firm reports its income from the client, an in-house filer its
            // own expenses; one of the two is set.
            let amount = num(r.get("income")).or_else(|| num(r.get("expenses")));
            let issues: Vec<String> = r
                .get("lobbying_activities")
                .map(arr)
                .unwrap_or_default()
                .iter()
                .map(|a| text(a.get("general_issue_code_display")))
                .filter(|s| !s.is_empty())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let start = Month::try_from((q - 1) * 3 + 1).and_then(|m| Date::from_calendar_date(year as i32, m, 1));
            if let (Some(a), Ok(d)) = (amount, start) {
                points.push((d, a));
            }
            filings.push(json!({
                "period": format!("{year} Q{q}"),
                "registrant": text(r.pointer("/registrant/name")),
                "amount": amount,
                "issues": issues.join(", "),
                "url": text(r.get("filing_document_url")),
            }));
        }
        if filings.is_empty() {
            return Err(anyhow!("LDA.gov has no quarterly report with {name} as the client since {}", years_ago(3)));
        }
        Ok(json!({ "match": name, "filings": filings, "series": quarterly(points) }))
    }

    pub async fn probe(ctx: &Ctx<'_>) -> Result<String> {
        let v = page(ctx, &format!("{BASE}/filings/?page_size=1")).await?;
        Ok(format!("LDA.gov answered: {} filings on record", num(v.get("count")).unwrap_or(0.0)))
    }
}

// ── USPTO Open Data Portal ──────────────────────────────────────────────────

pub mod uspto {
    use super::*;

    pub const PROVIDER: &str = "uspto";
    const SEARCH: &str = "https://api.uspto.gov/api/v1/patent/applications/search";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "USPTO Open Data Portal",
        "https://data.uspto.gov",
        "https://data.uspto.gov/apis/patent-file-wrapper/search",
        "Free key (data.uspto.gov/key/myapikey). Patents granted per quarter to the company \
         as first applicant, applications filed from 2001. Replaces PatentsView, retired in \
         March 2026.",
        &["api_key"],
        &["alt"],
    );
    pub struct Uspto;
    super::super::fundamentals_connector!(Uspto, CAP);

    /// Applications matching `q` granted in `[from, to]`.
    async fn count(ctx: &Ctx<'_>, q: &str, from: Date, to: Date) -> Result<f64> {
        let key = util::need_key(ctx.secrets, "USPTO Open Data Portal", "data.uspto.gov/key/myapikey")?;
        let body = json!({
            "q": q,
            "rangeFilters": [{ "field": "applicationMetaData.grantDate", "valueFrom": from.to_string(), "valueTo": to.to_string() }],
            "pagination": { "offset": 0, "limit": 1 },
        });
        let req = ctx.client.post(SEARCH).header("X-API-KEY", key).json(&body);
        let v = match util::get_json(PROVIDER, req).await {
            Ok(v) => v,
            // The portal answers 404 when nothing matches.
            Err(e) if e.to_string().contains("HTTP 404") => return Ok(0.0),
            Err(e) => return Err(e),
        };
        Ok(num(v.get("count")).unwrap_or(0.0))
    }

    fn applicant_query(name: &str) -> String {
        format!("applicationMetaData.firstApplicantName:\"{}\"", name.replace('\\', "").replace('"', ""))
    }

    pub async fn patents(ctx: &Ctx<'_>, name: &str) -> Result<Value> {
        // The applicant is written as the company styles itself ("NVIDIA Corporation"),
        // EDGAR often in capitals with an abbreviated suffix: the phrase is the name
        // without its suffix, and the note says which phrase was searched.
        let phrase = name_key(name);
        let q = applicant_query(&phrase);
        let today = OffsetDateTime::now_utc().date();
        let mut start = quarter_start(years_ago(3));
        let mut points = Vec::new();
        while start <= today {
            let next = quarter_start(start + time::Duration::days(95));
            let n = count(ctx, &q, start, next - time::Duration::days(1)).await?;
            points.push((start, n));
            start = next;
        }
        let total: f64 = points.iter().map(|p| p.1).sum();
        if total == 0.0 {
            return Err(anyhow!("the USPTO lists no patent granted to an applicant named {phrase} in three years"));
        }
        Ok(json!({ "match": phrase, "total": total, "series": quarterly(points) }))
    }

    pub async fn probe(ctx: &Ctx<'_>) -> Result<String> {
        let to = OffsetDateTime::now_utc().date();
        let n = count(ctx, &applicant_query("APPLE"), to - time::Duration::days(365), to).await?;
        Ok(format!("key accepted: {n} patents granted to Apple in a year"))
    }
}

// ── Quiver Quant ────────────────────────────────────────────────────────────

pub mod quiver {
    use super::*;

    pub const PROVIDER: &str = "quiver";
    const BASE: &str = "https://api.quiverquant.com/beta";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "Quiver Quant",
        "https://www.quiverquant.com",
        "https://api.quiverquant.com/docs/",
        "Paid key, non-commercial. Congress trades (recent, or per ticker), lobbying, \
         government contracts and patents per ticker.",
        &["api_key"],
        &["alt"],
    );
    pub struct Quiver;
    super::super::fundamentals_connector!(Quiver, CAP);

    async fn get(ctx: &Ctx<'_>, path: &str) -> Result<Vec<Value>> {
        let key = util::need_key(ctx.secrets, "Quiver Quant", "quiverquant.com")?;
        let req = ctx
            .client
            .get(format!("{BASE}/{path}"))
            .header(reqwest::header::AUTHORIZATION, format!("Token {key}"))
            .header(reqwest::header::ACCEPT, "application/json");
        let v = util::get_json(PROVIDER, req).await?;
        Ok(arr(&v))
    }

    fn day(r: &Value, k: &str) -> String {
        text(r.get(k)).get(..10).unwrap_or("").to_string()
    }

    pub async fn congress(ctx: &Ctx<'_>, subject: &str) -> Result<Value> {
        let rows = if subject == "_" {
            get(ctx, "live/congresstrading").await?
        } else {
            get(ctx, &format!("historical/congresstrading/{}", enc(subject))).await?
        };
        let trades: Vec<Value> = rows
            .iter()
            .take(300)
            .map(|r| {
                let house = text(r.get("House"));
                json!({
                    "member": text(r.get("Representative")),
                    "chamber": if house.eq_ignore_ascii_case("senate") { "senate" } else { "house" },
                    "party": text(r.get("Party")),
                    "ticker": text(r.get("Ticker")),
                    "type": trade_type(&text(r.get("Transaction"))),
                    "amount": text(r.get("Range")),
                    "traded": day(r, "TransactionDate"),
                    "disclosed": day(r, "ReportDate"),
                })
            })
            .collect();
        Ok(json!({ "trades": trades }))
    }

    pub async fn lobbying(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
        let rows = get(ctx, &format!("historical/lobbying/{}", enc(t))).await?;
        let since = years_ago(3);
        let mut points = Vec::new();
        let filings: Vec<Value> = rows
            .iter()
            .filter_map(|r| {
                let d = parse_date(&text(r.get("Date")))?;
                if d < since {
                    return None;
                }
                let amount = num(r.get("Amount"));
                if let Some(a) = amount {
                    points.push((d, a));
                }
                let q = quarter_start(d);
                Some(json!({
                    "period": format!("{} Q{}", q.year(), (q.month() as u8 - 1) / 3 + 1),
                    "registrant": text(r.get("Registrant")),
                    "amount": amount,
                    "issues": text(r.get("Issue")),
                    "url": "",
                }))
            })
            .collect();
        Ok(json!({ "match": t, "filings": filings, "series": quarterly(points) }))
    }

    pub async fn contracts(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
        let rows = get(ctx, &format!("historical/govcontractsall/{}", enc(t))).await?;
        let mut points = Vec::new();
        let mut awards = Vec::new();
        for r in &rows {
            let Some(d) = parse_date(&text(r.get("Date"))) else { continue };
            if d < years_ago(5) {
                continue;
            }
            let amount = num(r.get("Amount"));
            if let Some(a) = amount {
                points.push((d, a));
            }
            awards.push(json!({
                "date": d.to_string(),
                "agency": text(r.get("Agency")),
                "amount": amount,
                "description": text(r.get("Description")),
                "url": "",
            }));
        }
        awards.sort_by_key(|a| std::cmp::Reverse(text(a.get("date"))));
        awards.truncate(50);
        Ok(json!({ "match": [t], "awards": awards, "series": quarterly(points) }))
    }

    pub async fn patents(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
        let rows = get(ctx, &format!("historical/allpatents/{}", enc(t))).await?;
        let since = quarter_start(years_ago(3));
        let points: Vec<(Date, f64)> = rows
            .iter()
            .filter_map(|r| parse_date(&text(r.get("Date"))))
            .filter(|d| *d >= since)
            .map(|d| (d, 1.0))
            .collect();
        let total = points.len();
        Ok(json!({ "match": t, "total": total, "series": quarterly(points) }))
    }

    pub async fn probe(ctx: &Ctx<'_>) -> Result<String> {
        let n = get(ctx, "live/congresstrading").await?.len();
        Ok(format!("key accepted: {n} recent congress trades"))
    }
}

// ── Finnhub (premium) ───────────────────────────────────────────────────────

/// Finnhub's congressional trading, per ticker only.
pub async fn finnhub_congress(ctx: &Ctx<'_>, subject: &str) -> Result<Value> {
    if subject == "_" {
        return Err(anyhow!("Finnhub lists congress trades per ticker: pick a company, or add Quiver Quant for the recent ones"));
    }
    let v = super::finnhub::get(
        ctx,
        "stock/congressional-trading",
        &format!("symbol={}&from={}&to={}", enc(subject), years_ago(3), OffsetDateTime::now_utc().date()),
    )
    .await?;
    let range = |r: &Value| match (num(r.get("amountFrom")), num(r.get("amountTo"))) {
        (Some(a), Some(b)) => format!("${a:.0} - ${b:.0}"),
        (Some(a), None) => format!("${a:.0}+"),
        _ => String::new(),
    };
    let trades: Vec<Value> = v
        .get("data")
        .map(arr)
        .unwrap_or_default()
        .iter()
        .map(|r| {
            let position = text(r.get("position")).to_lowercase();
            json!({
                "member": text(r.get("name")),
                "chamber": if position.contains("sen") { "senate" } else { "house" },
                "party": "",
                "ticker": text(r.get("symbol")),
                "type": trade_type(&text(r.get("transactionType"))),
                "amount": range(r),
                "traded": text(r.get("transactionDate")),
                "disclosed": text(r.get("filingDate")),
            })
        })
        .collect();
    Ok(json!({ "trades": trades }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_without_punctuation_or_suffix() {
        assert_eq!(name_key("Lockheed Martin Corp"), "LOCKHEED MARTIN");
        assert_eq!(name_key("LOCKHEED MARTIN CORPORATION"), "LOCKHEED MARTIN");
        assert_eq!(name_key("Amazon.com, Inc."), "AMAZON COM");
        assert_eq!(name_key("AMAZON COM INC"), "AMAZON COM");
        assert_eq!(name_key("MASTERCARD INC /DE/"), "MASTERCARD");
        assert_eq!(name_key("The Boeing Company"), "BOEING");
        assert_ne!(name_key("Apple Hospitality REIT, Inc."), name_key("Apple Inc."));
    }

    #[test]
    fn trades_and_quarters() {
        assert_eq!(trade_type("Sale (Full)"), "sale");
        assert_eq!(trade_type("Purchase"), "purchase");
        let d = Date::from_calendar_date(2026, Month::August, 15).unwrap();
        assert_eq!(quarter_start(d), Date::from_calendar_date(2026, Month::July, 1).unwrap());
        assert_eq!(
            usaspending_fiscal(2026, 1),
            Date::from_calendar_date(2025, Month::October, 1).ok()
        );
        assert_eq!(
            usaspending_fiscal(2026, 4),
            Date::from_calendar_date(2026, Month::July, 1).ok()
        );
    }

    fn usaspending_fiscal(fy: i32, q: u8) -> Option<Date> {
        usaspending::fiscal_start(fy, q)
    }
}

