//! Scraper for the Managers' Portfolios module.
//!
//! Fetches Dataroma's public superinvestor pages and parses them into store inputs:
//!   - `managers.php`  → the summary list (one row per manager, with the ?m= slug + value/count).
//!   - `holdings.php?m=SLUG` → one manager's full holdings table + reporting period.
//!
//! Dataroma's markup is plain, server-rendered HTML tables that have been stable for years, so
//! we parse with targeted regexes rather than pulling a full HTML-parsing dependency. The
//! patterns are intentionally narrow (anchored on Dataroma's own class names / cell order) and
//! every numeric field is optional — a layout tweak degrades to missing values, never a panic.

use std::time::Duration;

use anyhow::{Context, Result};

use otw_store::mportfolios::{HoldingInput, PortfolioInput};

const BASE: &str = "https://www.dataroma.com/m";
const UA: &str = "Mozilla/5.0 (compatible; OpenTraderWorld/1.0; +https://github.com/G-OTW/OpenTraderWorld)";

/// Shared HTTP client with a real user agent (Dataroma 403s the default reqwest UA).
pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(30))
        .build()
        .context("building mportfolios http client")
}

async fn get_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let res = crate::rate::send("dataroma", client.get(url)).await.with_context(|| format!("GET {url}"))?;
    let status = res.status();
    if !status.is_success() {
        anyhow::bail!("GET {url} returned {status}");
    }
    res.text().await.with_context(|| format!("reading body of {url}"))
}

/// Fetch + parse the managers summary list.
pub async fn fetch_managers(client: &reqwest::Client) -> Result<Vec<PortfolioInput>> {
    let html = get_text(client, &format!("{BASE}/managers.php")).await?;
    Ok(parse_managers(&html))
}

/// Fetch + parse one manager's holdings detail. Returns the period and the holdings rows.
pub async fn fetch_holdings(
    client: &reqwest::Client,
    slug: &str,
) -> Result<(String, Vec<HoldingInput>)> {
    let url = format!("{BASE}/holdings.php?m={slug}");
    let html = get_text(client, &url).await?;
    Ok((parse_period(&html), parse_holdings(&html)))
}

/// Canonical Dataroma detail URL for a manager slug.
pub fn detail_url(slug: &str) -> String {
    format!("{BASE}/holdings.php?m={slug}")
}

// ── Parsing ───────────────────────────────────────────────────────────────────

/// Strip HTML tags and decode the handful of entities Dataroma emits.
fn text_of(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("&#8801", "")
        .trim()
        .to_string()
}

/// Parse a numeric value, ignoring `$`, commas, `%` and surrounding whitespace.
fn num(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    cleaned.parse().ok()
}

/// Dataroma writes values like "$263.1 B" / "$172 M" / "$181.8 B". Return the USD amount.
fn parse_value(s: &str) -> Option<f64> {
    let n = num(s)?;
    let up = s.to_ascii_uppercase();
    let mult = if up.contains('B') {
        1e9
    } else if up.contains('M') {
        1e6
    } else if up.contains('K') {
        1e3
    } else {
        1.0
    };
    Some(n * mult)
}

/// Extract the inner HTML of every `<tr>…</tr>` in `body`.
fn rows(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("<tr") {
        let after = &rest[start..];
        let open_end = match after.find('>') {
            Some(i) => i + 1,
            None => break,
        };
        let inner_start = start + open_end;
        match rest[inner_start..].find("</tr>") {
            Some(close) => {
                out.push(&rest[inner_start..inner_start + close]);
                rest = &rest[inner_start + close + 5..];
            }
            None => break,
        }
    }
    out
}

/// Split a row's inner HTML into the inner HTML of each `<td>…</td>`.
fn cells(row: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = row;
    while let Some(start) = rest.find("<td") {
        let after = &rest[start..];
        let open_end = match after.find('>') {
            Some(i) => i + 1,
            None => break,
        };
        let inner_start = start + open_end;
        match rest[inner_start..].find("</td>") {
            Some(close) => {
                out.push(&rest[inner_start..inner_start + close]);
                rest = &rest[inner_start + close + 5..];
            }
            None => break,
        }
    }
    out
}

/// Pull the `m=` slug out of a `holdings.php?m=SLUG` link.
fn slug_from(html: &str) -> Option<String> {
    let i = html.find("holdings.php?m=")? + "holdings.php?m=".len();
    let tail = &html[i..];
    let end = tail
        .find(|c: char| c == '"' || c == '&' || c == '\'' || c.is_whitespace())
        .unwrap_or(tail.len());
    Some(tail[..end].to_string())
}

/// Parse the managers summary table. Each manager row carries class `man` on its first cell.
fn parse_managers(html: &str) -> Vec<PortfolioInput> {
    let mut out = Vec::new();
    for row in rows(html) {
        if !row.contains("class=\"man\"") {
            continue;
        }
        let cs = cells(row);
        if cs.len() < 3 {
            continue;
        }
        let slug = match slug_from(cs[0]) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let name = text_of(cs[0]);
        let value_text = text_of(cs[1]);
        out.push(PortfolioInput {
            slug: slug.clone(),
            name,
            value_num: parse_value(&value_text),
            value_text,
            stock_count: num(&text_of(cs[2])).unwrap_or(0.0) as i32,
            period: String::new(),
            source_url: detail_url(&slug),
        });
    }
    out
}

/// Period label from `<p id="p2">Period: <span>Q1 2026</span>`.
fn parse_period(html: &str) -> String {
    let Some(i) = html.find("Period:") else {
        return String::new();
    };
    let tail = &html[i..];
    let Some(s) = tail.find("<span") else {
        return String::new();
    };
    let Some(o) = tail[s..].find('>') else {
        return String::new();
    };
    let from = s + o + 1;
    let Some(end) = tail[from..].find("</span>") else {
        return String::new();
    };
    text_of(&tail[from..from + end])
}

/// Parse the holdings table (`<table id="grid">`). Column order:
/// 0 history, 1 stock(ticker+name), 2 %, 3 activity, 4 shares, 5 reported, 6 value,
/// 7 gap, 8 current, 9 +/- reported, 10 52wk low, 11 52wk high.
fn parse_holdings(html: &str) -> Vec<HoldingInput> {
    // Scope to the body of the grid table so we don't pick up unrelated rows.
    let body = match html.find("id=\"grid\"") {
        Some(g) => {
            let tail = &html[g..];
            match (tail.find("<tbody"), tail.find("</tbody>")) {
                (Some(a), Some(b)) => &tail[a..b],
                _ => tail,
            }
        }
        None => html,
    };

    let mut out = Vec::new();
    let mut position = 0i32;
    for row in rows(body) {
        let cs = cells(row);
        // A holdings row has a `stock` cell with a ticker link.
        if cs.len() < 7 || !row.contains("class=\"stock\"") {
            continue;
        }
        // cs[1] looks like: <a ...>AAPL<span> - Apple Inc.</span></a>
        let (ticker, company) = split_stock(cs[1]);
        position += 1;
        out.push(HoldingInput {
            position,
            ticker,
            company,
            pct: num(&text_of(cs[2])),
            activity: text_of(cs[3]),
            shares: num(&text_of(cs[4])),
            reported_price: num(&text_of(cs[5])),
            value: num(&text_of(cs[6])),
            current_price: cs.get(8).and_then(|c| num(&text_of(c))),
            change_pct: cs.get(9).and_then(|c| num(&text_of(c))),
            week52_low: cs.get(10).and_then(|c| num(&text_of(c))),
            week52_high: cs.get(11).and_then(|c| num(&text_of(c))),
        });
    }
    out
}

/// Split a `stock` cell into (ticker, company). The ticker is the text before the inner
/// `<span>`, the company is the span text minus the leading " - ".
fn split_stock(cell: &str) -> (String, String) {
    let plain = text_of(cell);
    match plain.split_once(" - ") {
        Some((t, c)) => (t.trim().to_string(), c.trim().to_string()),
        None => (plain.trim().to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Option<f64>, b: f64) -> bool {
        a.map(|v| (v - b).abs() < b.abs() * 1e-9 + 1e-6).unwrap_or(false)
    }

    #[test]
    fn parses_value() {
        assert!(approx(parse_value("$263.1 B"), 263.1e9));
        assert!(approx(parse_value("$172 M"), 172e6));
        assert_eq!(parse_value("$0 M"), Some(0.0));
    }

    #[test]
    fn parses_manager_row() {
        let html = r#"<tr>
          <td class="man"><a href="/m/holdings.php?m=BRK" >Warren Buffett - Berkshire Hathaway</a></td>
          <td class="val">$263.1 B</td>
          <td class="cnt">29</td>
          <td class="sym"><a href="/m/stock.php?sym=AAPL">AAPL</a></td>
        </tr>"#;
        let m = parse_managers(html);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].slug, "BRK");
        assert_eq!(m[0].name, "Warren Buffett - Berkshire Hathaway");
        assert_eq!(m[0].stock_count, 29);
        assert!(approx(m[0].value_num, 263.1e9));
    }

    #[test]
    fn parses_holdings_row() {
        let html = r#"<table id="grid"><tbody>
          <tr>
            <td class="hist"><a>x</a></td>
            <td class="stock"><a href="/m/stock.php?sym=AAPL">AAPL<span> - Apple Inc.</span></a></td>
            <td>21.99</td>
            <td class="red">Reduce 10.39%</td>
            <td>227,917,808</td>
            <td>$253.79</td>
            <td>$57,843,261,000</td>
            <td class="gap"></td>
            <td class="quote">$281.74</td>
            <td class="green2">11.01%</td>
            <td>$198.47</td>
            <td>$317.40</td>
          </tr>
        </tbody></table>"#;
        let h = parse_holdings(html);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].ticker, "AAPL");
        assert_eq!(h[0].company, "Apple Inc.");
        assert_eq!(h[0].pct, Some(21.99));
        assert_eq!(h[0].activity, "Reduce 10.39%");
        assert_eq!(h[0].shares, Some(227_917_808.0));
        assert_eq!(h[0].reported_price, Some(253.79));
        assert_eq!(h[0].current_price, Some(281.74));
        assert_eq!(h[0].week52_high, Some(317.40));
    }

    #[test]
    fn parses_period() {
        let html = r#"<p id="p2">Period: <span>Q1 2026</span><br/>"#;
        assert_eq!(parse_period(html), "Q1 2026");
    }
}
