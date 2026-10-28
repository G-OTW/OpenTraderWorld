//! Earnings-call transcripts: listed per company as documents (kind `transcript`), their
//! text fetched when opened and stored as speaker turns.
//!
//! The document's accession says where the text comes from, so opening it later needs no
//! guess: `fmp:AAPL:2025:3`, `alphavantage:AAPL:2025Q3`, `finnhub:<id>`.

use anyhow::{anyhow, Result};
use time::Date;
use uuid::Uuid;

use super::{finnhub, fmp, vendors::av, Ctx};
use otw_store::fundamentals::{self as store, DocumentInput, Segment};

/// Providers that serve transcripts, in order of preference.
pub const PROVIDERS: &[&str] = &[fmp::PROVIDER, av::PROVIDER, finnhub::PROVIDER];

fn source_label(provider: &str) -> &'static str {
    match provider {
        fmp::PROVIDER => "Financial Modeling Prep",
        av::PROVIDER => "Alpha Vantage",
        _ => "Finnhub",
    }
}

/// Split FMP's single string of `Speaker: words` lines into turns.
pub fn split_speakers(content: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    for line in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
        // A speaker label is short and has no sentence punctuation before the colon.
        let label = line.split_once(": ").filter(|(name, _)| {
            name.len() <= 60 && !name.contains(['.', '?', '!']) && name.split_whitespace().count() <= 6
        });
        match label {
            Some((name, rest)) => out.push((name.trim().to_string(), String::new(), rest.trim().to_string())),
            None => match out.last_mut() {
                Some(last) => {
                    last.2.push(' ');
                    last.2.push_str(line);
                }
                None => out.push(("Unknown".into(), String::new(), line.to_string())),
            },
        }
    }
    out
}

const EXEC_WORDS: &[&str] = &[
    "chief", "officer", "president", "ceo", "cfo", "coo", "cto", "investor relations", "chairman",
    "treasurer", "head of", "director", "founder", "controller", "general counsel",
];

/// Turns with roles and sections. Prepared remarks run until the operator opens the line
/// for questions; in Q&A, anyone who spoke in the prepared part is management and
/// everyone else an analyst. A provider's own section label wins when it has one.
pub fn segments(turns: Vec<(String, String, String, Option<String>)>) -> Vec<Segment> {
    let mut section = "prepared";
    let mut prepared_speakers: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (i, (speaker, title, text, given)) in turns.into_iter().enumerate() {
        let operator = speaker.eq_ignore_ascii_case("operator");
        if given.is_none()
            && section == "prepared"
            && operator
            && !prepared_speakers.is_empty()
            && text.to_lowercase().contains("question")
        {
            section = "qa";
        }
        let sec = given.as_deref().unwrap_or(section).to_string();
        let lower_title = title.to_lowercase();
        let role = if operator {
            "operator"
        } else if lower_title.contains("analyst") {
            "analyst"
        } else if EXEC_WORDS.iter().any(|w| lower_title.contains(w)) {
            "exec"
        } else if sec == "prepared" || prepared_speakers.contains(&speaker) {
            "exec"
        } else {
            "analyst"
        };
        if sec == "prepared" && !operator && !prepared_speakers.contains(&speaker) {
            prepared_speakers.push(speaker.clone());
        }
        out.push(Segment { ordinal: i as i32, speaker, role: role.into(), section: sec, text });
    }
    out
}

fn no_section(turns: Vec<(String, String, String)>) -> Vec<(String, String, String, Option<String>)> {
    turns.into_iter().map(|(a, b, c)| (a, b, c, None)).collect()
}

/// List (and for Alpha Vantage, which has no list, fetch) a company's transcripts.
/// Returns how many are listed.
pub async fn refresh(
    pool: &sqlx::PgPool,
    ctx: &Ctx<'_>,
    provider: &str,
    company_id: Uuid,
    ticker: &str,
    name: &str,
) -> Result<usize> {
    let title = |label: &str| format!("{name} {label} earnings call");
    let source = source_label(provider);
    let mut docs: Vec<DocumentInput> = Vec::new();
    match provider {
        fmp::PROVIDER => {
            for (year, q, date) in fmp::transcript_list(ctx, ticker).await?.into_iter().take(16) {
                let Some(filed_at) = super::parse_date(&date) else { continue };
                docs.push(doc(format!("fmp:{ticker}:{year}:{q}"), title(&format!("Q{q} FY{year}")), filed_at, source));
            }
        }
        finnhub::PROVIDER => {
            for (id, year, q, date) in finnhub::transcript_list(ctx, ticker).await?.into_iter().take(16) {
                let Some(filed_at) = super::parse_date(&date) else { continue };
                docs.push(doc(format!("finnhub:{id}"), title(&format!("Q{q} {year}")), filed_at, source));
            }
        }
        av::PROVIDER => {
            // No list endpoint: try the last four fiscal quarters (Alpha Vantage labels a
            // call by the company's fiscal quarter, so Apple's July to September is Q4),
            // one call each, a little over a second apart (the free key allows one a
            // second and 25 a day). A rate-limit answer stops the run and is reported.
            let fye_month = store::company_by_ticker(pool, ticker)
                .await?
                .and_then(|c| c.fiscal_year_end.get(..2).and_then(|m| m.parse::<u8>().ok()))
                .filter(|m| (1..=12).contains(m))
                .unwrap_or(12);
            let mut found = 0;
            for (i, (fy, fq, end)) in fiscal_quarters(time::OffsetDateTime::now_utc().date(), fye_month, 4).into_iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                }
                let label = format!("{fy}Q{fq}");
                let turns = match av::transcript(ctx, ticker, &label).await {
                    Ok(t) => t,
                    Err(e) if av::is_rate_limit(&e) => return Err(e),
                    Err(_) => continue,
                };
                let accession = format!("alphavantage:{ticker}:{label}");
                store::upsert_documents(pool, company_id, &[doc(accession.clone(), title(&format!("Q{fq} FY{fy}")), end, source)]).await?;
                if let Some(id) = store::document_id(pool, source, &accession).await? {
                    store::store_transcript(pool, id, &segments(no_section(turns))).await?;
                }
                found += 1;
            }
            if found == 0 {
                return Err(anyhow!("Alpha Vantage has no transcript for {ticker} in the last four fiscal quarters"));
            }
            return Ok(found);
        }
        _ => return Err(anyhow!("{provider} does not serve transcripts")),
    }
    if docs.is_empty() {
        return Err(anyhow!("{source} lists no transcript for {ticker}"));
    }
    store::upsert_documents(pool, company_id, &docs).await?;
    Ok(docs.len())
}

/// The last `n` fiscal quarters that ended at least three weeks before `today` (results
/// come out a few weeks after a quarter closes), newest first: (fiscal year, quarter, last
/// day). The fiscal year is named after the calendar year it ends in.
fn fiscal_quarters(today: Date, fye_month: u8, n: usize) -> Vec<(i32, u8, Date)> {
    let month_end = |y: i32, m: u8| {
        let first = Date::from_calendar_date(y, time::Month::try_from(m).unwrap_or(time::Month::December), 1).unwrap_or(today);
        let next = if m == 12 {
            Date::from_calendar_date(y + 1, time::Month::January, 1)
        } else {
            Date::from_calendar_date(y, time::Month::try_from(m + 1).unwrap_or(time::Month::December), 1)
        };
        next.map(|d| d - time::Duration::days(1)).unwrap_or(first)
    };
    let mut out = Vec::new();
    // Walk quarter-end months backwards from the fiscal year end of next year.
    let (mut y, mut m) = (today.year() + 1, fye_month);
    while out.len() < n {
        let end = month_end(y, m);
        if end + time::Duration::days(21) <= today {
            // Months after the fiscal year end that this quarter closes.
            let into = (m as i32 - fye_month as i32).rem_euclid(12) as u8;
            let fq = if into == 0 { 4 } else { into / 3 };
            let fy = if m > fye_month { y + 1 } else { y };
            out.push((fy, fq, end));
        }
        if m > 3 {
            m -= 3;
        } else {
            m += 9;
            y -= 1;
        }
    }
    out
}

fn doc(accession: String, title: String, filed_at: Date, source: &str) -> DocumentInput {
    DocumentInput {
        kind: "transcript".into(),
        form: "transcript".into(),
        accession,
        filed_at,
        period: None,
        title,
        url: String::new(),
        source: source.into(),
    }
}

/// The provider named by a transcript's accession.
pub fn provider_of(accession: &str) -> Option<&'static str> {
    PROVIDERS.iter().copied().find(|p| accession.starts_with(&format!("{p}:")))
}

/// Fetch and store the text of a listed transcript.
pub async fn fetch_body(pool: &sqlx::PgPool, ctx: &Ctx<'_>, document_id: Uuid, accession: &str) -> Result<()> {
    let parts: Vec<&str> = accession.split(':').collect();
    let turns = match parts.as_slice() {
        ["fmp", t, year, q] => no_section(fmp::transcript(ctx, t, year.parse()?, q.parse()?).await?),
        ["alphavantage", t, quarter] => no_section(av::transcript(ctx, t, quarter).await?),
        ["finnhub", id] => finnhub::transcript(ctx, id).await?,
        _ => return Err(anyhow!("unknown transcript reference {accession}")),
    };
    if turns.is_empty() {
        return Err(anyhow!("the provider returned an empty transcript"));
    }
    store::store_transcript(pool, document_id, &segments(turns)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fiscal_quarters_follow_the_year_end() {
        let d = |y, m, dd| Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), dd).unwrap();
        let today = d(2026, 10, 2);
        // Apple closes in September: July to September 2026 is Q4 FY2026, not reported yet.
        let q: Vec<(i32, u8)> = fiscal_quarters(today, 9, 4).iter().map(|x| (x.0, x.1)).collect();
        assert_eq!(q, vec![(2026, 3), (2026, 2), (2026, 1), (2025, 4)]);
        // A December year end is the calendar.
        let q: Vec<(i32, u8)> = fiscal_quarters(today, 12, 2).iter().map(|x| (x.0, x.1)).collect();
        assert_eq!(q, vec![(2026, 2), (2026, 1)]);
        // Microsoft closes in June: April to June 2026 is Q4 FY2026.
        let q: Vec<(i32, u8)> = fiscal_quarters(today, 6, 2).iter().map(|x| (x.0, x.1)).collect();
        assert_eq!(q, vec![(2026, 4), (2026, 3)]);
    }

    #[test]
    fn speakers_and_sections_are_recovered_from_plain_text() {
        let turns = split_speakers(
            "Operator: Good afternoon and welcome.\nTim Cook: Thank you. Revenue grew.\nIt was a strong quarter.\n\
             Operator: We will now take questions.\nJane Analyst: How is demand? Thanks.\nTim Cook: Demand is good.",
        );
        assert_eq!(turns.len(), 5);
        assert!(turns[1].2.ends_with("strong quarter."));
        let segs = segments(no_section(turns));
        assert_eq!(segs[1].role, "exec");
        assert_eq!(segs[1].section, "prepared");
        assert_eq!(segs[2].section, "qa");
        assert_eq!(segs[3].role, "analyst");
        assert_eq!(segs[4].role, "exec");
    }
}
