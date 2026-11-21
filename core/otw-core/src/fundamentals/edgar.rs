//! SEC EDGAR: company profiles, filings, financial statements (XBRL company facts) and
//! insider transactions (Form 4). Keyless, but the SEC refuses any request without a
//! User-Agent naming a person and an e-mail, so the connector carries that as a setting,
//! and it asks for at most 10 requests per second, which [`pace`] enforces process-wide.
//!
//! Statements are as reported, mapped onto standard keys (`revenue`, `net_income`, ...):
//! for each period the first candidate tag that has a value wins, which is how a company
//! that moved from `SalesRevenueNet` to `RevenueFromContractWithCustomer...` keeps one
//! continuous line. Quarters missing from the filings (a fourth quarter, cash-flow
//! quarters reported only year-to-date) are derived by difference.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::histdata::{Capability, Chunk, ConfigField, Connector};
use otw_store::fundamentals::{self as store, CompanyInput, DocumentInput, InsiderTrade, Statement};

pub const PROVIDER: &str = "edgar";
pub(crate) const TIMEOUT: Duration = Duration::from_secs(60);
/// SEC fair-access ceiling is 10 requests per second; stay a little under it.
const MIN_GAP: Duration = Duration::from_millis(120);
/// Form 4 filings parsed per refresh (newest first); older ones arrive on later refreshes.
const MAX_FORM4: usize = 40;

pub struct Edgar;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "contact",
    label: "Contact (name and e-mail)",
    placeholder: "Jane Doe jane@example.com",
    kind: "text",
    required: true,
    help: "The SEC requires every request to name who is asking. Sent as the User-Agent, \
           never anywhere else.",
}];

static CAP: Capability = Capability {
    provider: PROVIDER,
    label: "SEC EDGAR",
    website: "https://www.sec.gov/edgar",
    docs_url: "https://www.sec.gov/search-filings/edgar-application-programming-interfaces",
    rate_limit: "Keyless. 10 requests per second at most (paced here), and a User-Agent with \
                 your name and e-mail is mandatory: without it the SEC answers 403 and may \
                 block the IP. A company refresh costs 3 requests plus one per new Form 4.",
    required_secrets: &[],
    asset_types: &[],
    timeframes: &[],
    adjusted: false,
    max_bars_per_req: 0,
    min_interval_ms: 120,
    searchable: false,
    config_fields: FIELDS,
    testable: true,
    fundamentals: &["statements", "filings", "insiders"],
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "",
};

#[async_trait::async_trait]
impl Connector for Edgar {
    fn capability(&self) -> &'static Capability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        user_agent(config).map(|_| ())
    }

    async fn fetch_chunk(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        _ticker: &str,
        _asset_type: &str,
        _timeframe: &str,
        _from: OffsetDateTime,
        _to: OffsetDateTime,
    ) -> Result<Chunk> {
        super::refuse_chunk(&CAP).await
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let map = tickers(client, secrets).await?;
        Ok(format!("EDGAR answered: {} listed companies", map.len()))
    }
}

fn user_agent(settings: &HashMap<String, String>) -> Result<String> {
    let contact = settings.get("contact").map(|s| s.trim()).unwrap_or("");
    if !contact.contains('@') || contact.split_whitespace().count() < 2 {
        return Err(anyhow!(
            "the SEC requires a name and an e-mail address (\"Jane Doe jane@example.com\") \
             in the connector's Contact setting"
        ));
    }
    Ok(format!("OpenTraderWorld {contact}"))
}

/// Wait until at least [`MIN_GAP`] has passed since the previous EDGAR request.
async fn pace() {
    static LAST: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);
    let mut last = LAST.lock().await;
    if let Some(at) = *last {
        let since = at.elapsed();
        if since < MIN_GAP {
            tokio::time::sleep(MIN_GAP - since).await;
        }
    }
    *last = Some(Instant::now());
}

/// GET a URL from sec.gov. `Ok(None)` on 404 (a company with no XBRL facts, say).
async fn get(
    client: &reqwest::Client,
    settings: &HashMap<String, String>,
    url: &str,
) -> Result<Option<reqwest::Response>> {
    let ua = user_agent(settings)?;
    pace().await;
    let resp = crate::rate::send(
        PROVIDER,
        client.get(url).header(reqwest::header::USER_AGENT, ua).timeout(TIMEOUT),
    )
    .await
    .context("SEC EDGAR request")?;
    match resp.status().as_u16() {
        200..=299 => Ok(Some(resp)),
        404 => Ok(None),
        403 => Err(anyhow!(
            "SEC EDGAR refused the request (403): check the Contact setting on the EDGAR \
             connector (a real name and e-mail), and that this IP is not over 10 requests/s"
        )),
        429 => Err(anyhow!("SEC EDGAR rate limit hit (429): wait a minute and refresh")),
        s => Err(anyhow!("SEC EDGAR answered HTTP {s} for {url}")),
    }
}

async fn get_json(
    client: &reqwest::Client,
    settings: &HashMap<String, String>,
    url: &str,
) -> Result<Option<Value>> {
    match get(client, settings, url).await? {
        Some(r) => Ok(Some(r.json().await.context("SEC EDGAR JSON")?)),
        None => Ok(None),
    }
}

// ── Ticker map ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Listing {
    pub ticker: String,
    pub cik: String,
    pub name: String,
    pub exchange: String,
}

static TICKERS: Mutex<Option<(Instant, Vec<Listing>)>> = Mutex::new(None);
const TICKERS_TTL: Duration = Duration::from_secs(24 * 3600);

/// Every ticker EDGAR maps to a CIK (cached a day).
async fn tickers(client: &reqwest::Client, settings: &HashMap<String, String>) -> Result<Vec<Listing>> {
    if let Ok(g) = TICKERS.lock() {
        if let Some((at, list)) = g.as_ref() {
            if at.elapsed() < TICKERS_TTL {
                return Ok(list.clone());
            }
        }
    }
    let body = get_json(client, settings, "https://www.sec.gov/files/company_tickers_exchange.json")
        .await?
        .ok_or_else(|| anyhow!("SEC EDGAR ticker map not found"))?;
    let list: Vec<Listing> = body
        .get("data")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let r = r.as_array()?;
                    Some(Listing {
                        cik: format!("{:010}", r.first()?.as_u64()?),
                        name: r.get(1)?.as_str()?.to_string(),
                        ticker: r.get(2)?.as_str()?.to_uppercase(),
                        exchange: r.get(3).and_then(Value::as_str).unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if let Ok(mut g) = TICKERS.lock() {
        *g = Some((Instant::now(), list.clone()));
    }
    Ok(list)
}

/// EDGAR writes share classes with a dash (`BRK-B`); a dot or slash means the same.
pub fn normalize_ticker(t: &str) -> String {
    t.trim().to_uppercase().replace(['.', '/'], "-")
}

/// Companies whose ticker starts with, or whose name contains, `q`.
pub async fn search(
    client: &reqwest::Client,
    settings: &HashMap<String, String>,
    q: &str,
    limit: usize,
) -> Result<Vec<Listing>> {
    let q = normalize_ticker(q);
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let list = tickers(client, settings).await?;
    let mut exact: Vec<Listing> = Vec::new();
    let mut prefix: Vec<Listing> = Vec::new();
    let mut name: Vec<Listing> = Vec::new();
    for l in list {
        if l.ticker == q {
            exact.push(l);
        } else if l.ticker.starts_with(&q) {
            prefix.push(l);
        } else if l.name.to_uppercase().contains(&q) {
            name.push(l);
        }
    }
    prefix.sort_by_key(|l| l.ticker.len());
    exact.extend(prefix);
    exact.extend(name);
    exact.truncate(limit);
    Ok(exact)
}

/// The listing for an exact ticker. Never a nearest match: an unknown ticker is an error.
pub async fn resolve(
    client: &reqwest::Client,
    settings: &HashMap<String, String>,
    ticker: &str,
) -> Result<Listing> {
    let t = normalize_ticker(ticker);
    tickers(client, settings)
        .await?
        .into_iter()
        .find(|l| l.ticker == t)
        .ok_or_else(|| {
            anyhow!(
                "SEC EDGAR has no company with ticker \"{t}\": EDGAR covers SEC filers \
                 (US listings and ADRs), so pick one from the search list"
            )
        })
}

// ── Refresh ─────────────────────────────────────────────────────────────────

pub async fn refresh(state: &crate::AppState, id: Uuid, ticker: &str, cik: &str) -> Result<()> {
    let (connector, settings) = super::creds_for(&state.pool, &state.cipher, PROVIDER)
        .await?
        .ok_or_else(|| anyhow!(super::missing_connector(PROVIDER)))?;
    let client = &state.http;

    super::bump(&state.pool, connector).await;
    let sub = get_json(client, &settings, &format!("https://data.sec.gov/submissions/CIK{cik}.json"))
        .await?
        .ok_or_else(|| anyhow!("SEC EDGAR has no submissions for CIK {cik}"))?;
    let mut profile = profile(ticker, cik, &sub);
    let filings = filings(cik, &sub);

    super::bump(&state.pool, connector).await;
    let facts = get_json(
        client,
        &settings,
        &format!("https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json"),
    )
    .await?;
    let statements = match &facts {
        Some(f) => {
            let parsed = Facts::parse(f);
            profile.currency = parsed.currency.clone();
            profile.shares_out = parsed.shares_out();
            parsed.statements()
        }
        // No XBRL on file (some foreign filers): the profile and filings still stand.
        None => Vec::new(),
    };

    store::update_company(&state.pool, id, &profile).await?;
    let docs: Vec<DocumentInput> = filings.iter().map(|f| f.doc.clone()).collect();
    store::upsert_documents(&state.pool, id, &docs).await?;
    if !statements.is_empty() {
        store::replace_statements(&state.pool, id, PROVIDER, &statements).await?;
    }

    // Insider trades: only filings not parsed yet, newest first. A Form 4 that yields no
    // row (derivatives only, or another issuer's stock) is remembered as parsed too.
    let known = store::form4_seen(&state.pool, id).await?;
    let mut trades = Vec::new();
    let mut seen = Vec::new();
    for f in filings
        .iter()
        .filter(|f| f.doc.form == "4" && !known.contains(&f.doc.accession))
        .take(MAX_FORM4)
    {
        let Some(xml_url) = &f.raw_xml else { continue };
        super::bump(&state.pool, connector).await;
        let Some(resp) = get(client, &settings, xml_url).await? else { continue };
        let xml = resp.text().await.context("Form 4 body")?;
        trades.extend(parse_form4(&xml, &f.doc.accession, f.doc.filed_at, cik));
        seen.push(f.doc.accession.clone());
    }
    store::insert_insider_trades(&state.pool, id, &trades).await?;
    store::mark_form4_seen(&state.pool, id, &seen).await?;
    Ok(())
}

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("").trim()
}

fn profile(ticker: &str, cik: &str, sub: &Value) -> CompanyInput {
    // `ownerOrg` reads "06 Technology": the SEC office, which is the closest EDGAR has to
    // a sector. The SIC description is the industry.
    let sector = text(sub, "ownerOrg").trim_start_matches(|c: char| c.is_ascii_digit()).trim();
    let addr = sub.get("addresses").and_then(|a| a.get("business"));
    let state = addr.map(|a| text(a, "stateOrCountry")).unwrap_or("");
    let is_us = addr.map(|a| a.get("isForeignLocation").and_then(Value::as_i64) != Some(1)).unwrap_or(true)
        && state.len() == 2
        && state.chars().all(|c| c.is_ascii_uppercase());
    let country = if is_us {
        "US".to_string()
    } else {
        addr.map(|a| text(a, "stateOrCountryDescription").to_string()).unwrap_or_default()
    };
    let exchange = sub
        .get("exchanges")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    CompanyInput {
        ticker: ticker.to_string(),
        cik: cik.to_string(),
        name: text(sub, "name").to_string(),
        exchange,
        currency: "USD".into(),
        country,
        sector: sector.to_string(),
        industry: text(sub, "sicDescription").to_string(),
        sic: text(sub, "sic").to_string(),
        fiscal_year_end: text(sub, "fiscalYearEnd").to_string(),
        website: text(sub, "website").to_string(),
        description: text(sub, "description").to_string(),
        shares_out: None,
    }
}

struct Filing {
    doc: DocumentInput,
    /// Raw XML of an ownership form (the primary document minus its XSL rendering dir).
    raw_xml: Option<String>,
}

/// Common form types, titled for a reader who does not know the numbers by heart.
fn form_title(form: &str) -> &'static str {
    match form {
        "10-K" => "Annual report",
        "10-K/A" => "Annual report, amended",
        "10-Q" => "Quarterly report",
        "10-Q/A" => "Quarterly report, amended",
        "8-K" => "Current report",
        "20-F" => "Annual report (foreign issuer)",
        "40-F" => "Annual report (Canadian issuer)",
        "6-K" => "Current report (foreign issuer)",
        "DEF 14A" => "Proxy statement",
        "4" => "Statement of changes in beneficial ownership",
        "3" => "Initial statement of beneficial ownership",
        "144" => "Notice of proposed sale of securities",
        "S-8" => "Securities offered to employees",
        "SC 13G" | "SC 13G/A" | "SCHEDULE 13G" | "SCHEDULE 13G/A" => "Beneficial ownership report (passive)",
        "SC 13D" | "SC 13D/A" | "SCHEDULE 13D" | "SCHEDULE 13D/A" => "Beneficial ownership report (active)",
        _ => "",
    }
}

fn filings(cik: &str, sub: &Value) -> Vec<Filing> {
    let Some(recent) = sub.get("filings").and_then(|f| f.get("recent")) else {
        return Vec::new();
    };
    let col = |k: &str| -> Vec<&str> {
        recent
            .get(k)
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|v| v.as_str().unwrap_or("")).collect())
            .unwrap_or_default()
    };
    let (acc, filed, report, form, doc, desc) = (
        col("accessionNumber"),
        col("filingDate"),
        col("reportDate"),
        col("form"),
        col("primaryDocument"),
        col("primaryDocDescription"),
    );
    let cik_num = cik.trim_start_matches('0');
    let mut out = Vec::new();
    for i in 0..acc.len() {
        let Some(filed_at) = filed.get(i).and_then(|d| super::parse_date(d)) else { continue };
        let form = form.get(i).copied().unwrap_or("").to_string();
        let accession = acc[i].to_string();
        let folder = format!(
            "https://www.sec.gov/Archives/edgar/data/{cik_num}/{}",
            accession.replace('-', "")
        );
        let primary = doc.get(i).copied().unwrap_or("");
        let url = if primary.is_empty() {
            format!("{folder}/{accession}-index.htm")
        } else {
            format!("{folder}/{primary}")
        };
        let raw_xml = (form == "4" && primary.ends_with(".xml"))
            .then(|| format!("{folder}/{}", primary.rsplit('/').next().unwrap_or(primary)));
        let described = desc.get(i).copied().unwrap_or("").trim();
        let title = if !form_title(&form).is_empty() {
            form_title(&form).to_string()
        } else if !described.is_empty() {
            described.to_string()
        } else {
            form.clone()
        };
        out.push(Filing {
            doc: DocumentInput {
                kind: "filing".into(),
                form,
                accession,
                filed_at,
                period: report.get(i).and_then(|d| super::parse_date(d)),
                title,
                url,
                source: "SEC EDGAR".into(),
            },
            raw_xml,
        });
    }
    out
}

// ── Form 4 ──────────────────────────────────────────────────────────────────

/// The text between `<tag ...>` and `</tag>`, first occurrence.
fn inner<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut from = 0;
    let start = loop {
        let at = s[from..].find(&open)? + from;
        let after = at + open.len();
        // `<value>` must not match `<valueX>`.
        match s.as_bytes().get(after) {
            Some(b'>') | Some(b' ') | Some(b'/') | Some(b'\n') | Some(b'\r') | Some(b'\t') => {
                break after;
            }
            _ => from = after,
        }
    };
    let gt = s[start..].find('>')? + start;
    if s.as_bytes().get(gt.wrapping_sub(1)) == Some(&b'/') {
        return Some("");
    }
    let close = format!("</{tag}>");
    let end = s[gt + 1..].find(&close)? + gt + 1;
    Some(&s[gt + 1..end])
}

fn path<'a>(s: &'a str, tags: &[&str]) -> Option<&'a str> {
    tags.iter().try_fold(s, |acc, t| inner(acc, t)).map(str::trim)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v.map(str::trim), Some("1") | Some("true"))
}

/// Every non-derivative transaction of a Form 4 (common stock bought, sold, granted...).
/// A company's filing list also holds the Form 4s it files as the *reporting owner* of
/// another issuer's stock (TotalEnergies as a director of a US company): those are not
/// trades in its own shares and yield nothing.
pub fn parse_form4(xml: &str, accession: &str, filed_at: Date, issuer_cik: &str) -> Vec<InsiderTrade> {
    let cik_num = |c: &str| c.trim().trim_start_matches('0').to_string();
    let issuer = inner(xml, "issuer").and_then(|i| inner(i, "issuerCik")).unwrap_or("");
    if cik_num(issuer) != cik_num(issuer_cik) {
        return Vec::new();
    }
    let owner = inner(xml, "reportingOwner").unwrap_or("");
    let insider = path(owner, &["reportingOwnerId", "rptOwnerName"]).map(unescape).unwrap_or_default();
    let rel = inner(owner, "reportingOwnerRelationship").unwrap_or("");
    let role = if truthy(inner(rel, "isOfficer")) {
        path(rel, &["officerTitle"]).map(unescape).filter(|t| !t.is_empty()).unwrap_or_else(|| "Officer".into())
    } else if truthy(inner(rel, "isDirector")) {
        "Director".into()
    } else if truthy(inner(rel, "isTenPercentOwner")) {
        "10% owner".into()
    } else {
        "Other".into()
    };
    let Some(table) = inner(xml, "nonDerivativeTable") else { return Vec::new() };
    let mut out = Vec::new();
    let mut rest = table;
    let mut ordinal = 0;
    while let Some(block) = inner(rest, "nonDerivativeTransaction") {
        let num = |p: &[&str]| path(block, p).and_then(|v| v.parse::<f64>().ok());
        let tx_date = path(block, &["transactionDate", "value"]).and_then(super::parse_date);
        let code = path(block, &["transactionCoding", "transactionCode"]).unwrap_or("").to_string();
        let shares = num(&["transactionAmounts", "transactionShares", "value"]);
        if let (Some(tx_date), Some(shares), false) = (tx_date, shares, code.is_empty()) {
            out.push(InsiderTrade {
                accession: accession.to_string(),
                ordinal,
                filed_at,
                tx_date,
                insider: insider.clone(),
                role: role.clone(),
                code,
                acquired: path(block, &["transactionAmounts", "transactionAcquiredDisposedCode", "value"]) == Some("A"),
                shares,
                price: num(&["transactionAmounts", "transactionPricePerShare", "value"]).filter(|p| *p > 0.0),
                owned_after: num(&["postTransactionAmounts", "sharesOwnedFollowingTransaction", "value"]),
            });
            ordinal += 1;
        }
        // Move past this block.
        let Some(at) = rest.find("</nonDerivativeTransaction>") else { break };
        rest = &rest[at + "</nonDerivativeTransaction>".len()..];
    }
    out
}

// ── XBRL company facts → statements ─────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Income,
    Balance,
    Cash,
}

impl Kind {
    fn id(self) -> &'static str {
        match self {
            Kind::Income => "income",
            Kind::Balance => "balance",
            Kind::Cash => "cashflow",
        }
    }
}

struct Line {
    key: &'static str,
    kind: Kind,
    /// Candidate tags, `taxonomy:Name`, in order of preference.
    tags: &'static [&'static str],
    /// Per-share (EPS): never derived by difference, read in currency/shares units.
    per_share: bool,
    /// Reported as a positive payment, shown as an outflow.
    negate: bool,
}

const fn line(key: &'static str, kind: Kind, tags: &'static [&'static str]) -> Line {
    Line { key, kind, tags, per_share: false, negate: false }
}

const fn outflow(key: &'static str, tags: &'static [&'static str]) -> Line {
    Line { key, kind: Kind::Cash, tags, per_share: false, negate: true }
}

static LINES: &[Line] = &[
    line("revenue", Kind::Income, &[
        "us-gaap:Revenues",
        "us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax",
        "us-gaap:RevenueFromContractWithCustomerIncludingAssessedTax",
        "us-gaap:SalesRevenueNet",
        "us-gaap:SalesRevenueGoodsNet",
        "ifrs-full:Revenue",
    ]),
    line("cost_of_revenue", Kind::Income, &[
        "us-gaap:CostOfRevenue",
        "us-gaap:CostOfGoodsAndServicesSold",
        "us-gaap:CostOfGoodsSold",
        "ifrs-full:CostOfSales",
    ]),
    line("gross_profit", Kind::Income, &["us-gaap:GrossProfit", "ifrs-full:GrossProfit"]),
    line("rnd", Kind::Income, &["us-gaap:ResearchAndDevelopmentExpense", "ifrs-full:ResearchAndDevelopmentExpense"]),
    line("sga", Kind::Income, &[
        "us-gaap:SellingGeneralAndAdministrativeExpense",
        "ifrs-full:SellingGeneralAndAdministrativeExpense",
    ]),
    line("operating_income", Kind::Income, &["us-gaap:OperatingIncomeLoss", "ifrs-full:ProfitLossFromOperatingActivities"]),
    line("interest_expense", Kind::Income, &[
        "us-gaap:InterestExpense",
        "us-gaap:InterestExpenseNonoperating",
        "ifrs-full:InterestExpense",
        "ifrs-full:FinanceCosts",
    ]),
    line("pretax_income", Kind::Income, &[
        "us-gaap:IncomeLossFromContinuingOperationsBeforeIncomeTaxesExtraordinaryItemsNoncontrollingInterest",
        "us-gaap:IncomeLossFromContinuingOperationsBeforeIncomeTaxesMinorityInterestAndIncomeLossFromEquityMethodInvestments",
        "ifrs-full:ProfitLossBeforeTax",
    ]),
    line("income_tax", Kind::Income, &["us-gaap:IncomeTaxExpenseBenefit", "ifrs-full:IncomeTaxExpenseContinuingOperations"]),
    line("net_income", Kind::Income, &[
        "us-gaap:NetIncomeLoss",
        "us-gaap:ProfitLoss",
        "ifrs-full:ProfitLossAttributableToOwnersOfParent",
        "ifrs-full:ProfitLoss",
    ]),
    Line {
        key: "eps_diluted",
        kind: Kind::Income,
        tags: &["us-gaap:EarningsPerShareDiluted", "ifrs-full:DilutedEarningsLossPerShare"],
        per_share: true,
        negate: false,
    },
    line("cash", Kind::Balance, &[
        "us-gaap:CashAndCashEquivalentsAtCarryingValue",
        "us-gaap:CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalents",
        "ifrs-full:CashAndCashEquivalents",
    ]),
    line("short_term_investments", Kind::Balance, &[
        "us-gaap:ShortTermInvestments",
        "us-gaap:MarketableSecuritiesCurrent",
        "us-gaap:AvailableForSaleSecuritiesDebtSecuritiesCurrent",
        "ifrs-full:CurrentInvestments",
    ]),
    line("receivables", Kind::Balance, &[
        "us-gaap:AccountsReceivableNetCurrent",
        "us-gaap:ReceivablesNetCurrent",
        "ifrs-full:TradeAndOtherCurrentReceivables",
    ]),
    line("inventory", Kind::Balance, &["us-gaap:InventoryNet", "ifrs-full:Inventories"]),
    line("total_current_assets", Kind::Balance, &["us-gaap:AssetsCurrent", "ifrs-full:CurrentAssets"]),
    line("ppe_net", Kind::Balance, &["us-gaap:PropertyPlantAndEquipmentNet", "ifrs-full:PropertyPlantAndEquipment"]),
    line("goodwill", Kind::Balance, &["us-gaap:Goodwill", "ifrs-full:Goodwill"]),
    line("intangibles", Kind::Balance, &[
        "us-gaap:IntangibleAssetsNetExcludingGoodwill",
        "us-gaap:FiniteLivedIntangibleAssetsNet",
        "ifrs-full:IntangibleAssetsOtherThanGoodwill",
    ]),
    line("total_assets", Kind::Balance, &["us-gaap:Assets", "ifrs-full:Assets"]),
    line("accounts_payable", Kind::Balance, &[
        "us-gaap:AccountsPayableCurrent",
        "ifrs-full:TradeAndOtherCurrentPayables",
    ]),
    line("short_term_debt", Kind::Balance, &[
        "us-gaap:LongTermDebtCurrent",
        "us-gaap:DebtCurrent",
        "us-gaap:ShortTermBorrowings",
        "us-gaap:CommercialPaper",
        "ifrs-full:CurrentBorrowings",
    ]),
    line("total_current_liabilities", Kind::Balance, &["us-gaap:LiabilitiesCurrent", "ifrs-full:CurrentLiabilities"]),
    line("long_term_debt", Kind::Balance, &[
        "us-gaap:LongTermDebtNoncurrent",
        "us-gaap:LongTermDebt",
        "ifrs-full:NoncurrentBorrowings",
    ]),
    line("total_liabilities", Kind::Balance, &["us-gaap:Liabilities", "ifrs-full:Liabilities"]),
    line("shareholders_equity", Kind::Balance, &[
        "us-gaap:StockholdersEquity",
        "us-gaap:StockholdersEquityIncludingPortionAttributableToNoncontrollingInterest",
        "ifrs-full:EquityAttributableToOwnersOfParent",
        "ifrs-full:Equity",
    ]),
    line("net_income", Kind::Cash, &[
        "us-gaap:NetIncomeLoss",
        "us-gaap:ProfitLoss",
        "ifrs-full:ProfitLoss",
    ]),
    line("depreciation", Kind::Cash, &[
        "us-gaap:DepreciationDepletionAndAmortization",
        "us-gaap:DepreciationAndAmortization",
        "us-gaap:DepreciationAmortizationAndAccretionNet",
        "us-gaap:Depreciation",
        "ifrs-full:DepreciationAndAmortisationExpense",
    ]),
    line("stock_comp", Kind::Cash, &[
        "us-gaap:ShareBasedCompensation",
        "us-gaap:AllocatedShareBasedCompensationExpense",
    ]),
    line("operating_cash_flow", Kind::Cash, &[
        "us-gaap:NetCashProvidedByUsedInOperatingActivities",
        "us-gaap:NetCashProvidedByUsedInOperatingActivitiesContinuingOperations",
        "ifrs-full:CashFlowsFromUsedInOperatingActivities",
    ]),
    outflow("capex", &[
        "us-gaap:PaymentsToAcquirePropertyPlantAndEquipment",
        "us-gaap:PaymentsToAcquireProductiveAssets",
        "ifrs-full:PurchaseOfPropertyPlantAndEquipmentClassifiedAsInvestingActivities",
    ]),
    outflow("acquisitions", &[
        "us-gaap:PaymentsToAcquireBusinessesNetOfCashAcquired",
        "ifrs-full:CashFlowsUsedInObtainingControlOfSubsidiariesOrOtherBusinessesClassifiedAsInvestingActivities",
    ]),
    line("investing_cash_flow", Kind::Cash, &[
        "us-gaap:NetCashProvidedByUsedInInvestingActivities",
        "us-gaap:NetCashProvidedByUsedInInvestingActivitiesContinuingOperations",
        "ifrs-full:CashFlowsFromUsedInInvestingActivities",
    ]),
    outflow("dividends_paid", &[
        "us-gaap:PaymentsOfDividends",
        "us-gaap:PaymentsOfDividendsCommonStock",
        "ifrs-full:DividendsPaidClassifiedAsFinancingActivities",
    ]),
    outflow("buybacks", &[
        "us-gaap:PaymentsForRepurchaseOfCommonStock",
        "ifrs-full:PaymentsToAcquireOrRedeemEntitysShares",
    ]),
    line("financing_cash_flow", Kind::Cash, &[
        "us-gaap:NetCashProvidedByUsedInFinancingActivities",
        "us-gaap:NetCashProvidedByUsedInFinancingActivitiesContinuingOperations",
        "ifrs-full:CashFlowsFromUsedInFinancingActivities",
    ]),
];

#[derive(Debug, Clone)]
struct Fact {
    start: Option<Date>,
    end: Date,
    val: f64,
    filed: Date,
    fy: Option<i32>,
    fp: String,
}

/// A value for one period and the tag it came from.
type Cell = (f64, &'static str);

/// Quarter and annual lengths in days. 52/53-week years run 364 or 371.
fn is_quarter(days: i64) -> bool {
    (80..=100).contains(&days)
}
fn is_annual(days: i64) -> bool {
    (350..=380).contains(&days)
}

struct Facts {
    by_tag: HashMap<String, Vec<Fact>>,
    shares: Vec<(Date, Date, f64)>,
    currency: String,
}

impl Facts {
    fn parse(body: &Value) -> Self {
        let wanted: std::collections::HashSet<&str> =
            LINES.iter().flat_map(|l| l.tags.iter().copied()).collect();
        let facts = body.get("facts");
        // The reporting currency: the commonest currency unit across the wanted tags.
        let mut currencies: HashMap<String, usize> = HashMap::new();
        for tag in &wanted {
            let (tax, name) = tag.split_once(':').unwrap_or(("", tag));
            if let Some(units) = facts.and_then(|f| f.get(tax)).and_then(|t| t.get(name)).and_then(|t| t.get("units")).and_then(Value::as_object) {
                for (u, rows) in units {
                    if u.len() == 3 && u.chars().all(|c| c.is_ascii_uppercase()) {
                        *currencies.entry(u.clone()).or_default() += rows.as_array().map_or(0, Vec::len);
                    }
                }
            }
        }
        let currency = currencies
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(c, _)| c)
            .unwrap_or_else(|| "USD".into());
        let per_share_unit = format!("{currency}/shares");

        let mut by_tag = HashMap::new();
        for l in LINES {
            for tag in l.tags {
                if by_tag.contains_key(*tag) {
                    continue;
                }
                let (tax, name) = tag.split_once(':').unwrap_or(("", tag));
                let unit = if l.per_share { per_share_unit.as_str() } else { currency.as_str() };
                let rows = facts
                    .and_then(|f| f.get(tax))
                    .and_then(|t| t.get(name))
                    .and_then(|t| t.get("units"))
                    .and_then(|u| u.get(unit))
                    .and_then(Value::as_array);
                let parsed: Vec<Fact> = rows
                    .map(|rows| rows.iter().filter_map(parse_fact).collect())
                    .unwrap_or_default();
                if !parsed.is_empty() {
                    by_tag.insert(tag.to_string(), parsed);
                }
            }
        }

        let shares = facts
            .and_then(|f| f.get("dei"))
            .and_then(|d| d.get("EntityCommonStockSharesOutstanding"))
            .and_then(|t| t.get("units"))
            .and_then(|u| u.get("shares"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        let f = parse_fact(r)?;
                        Some((f.filed, f.end, f.val))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Facts { by_tag, shares, currency }
    }

    /// Shares outstanding on the cover of the latest filing, summed across share classes.
    fn shares_out(&self) -> Option<f64> {
        let (filed, end, _) = self.shares.iter().max_by_key(|(f, e, _)| (*f, *e))?;
        Some(
            self.shares
                .iter()
                .filter(|(f, e, _)| f == filed && e == end)
                .map(|(_, _, v)| v)
                .sum(),
        )
    }

    /// Fiscal year of each annual period end, from the filing that first reported it.
    fn fiscal_years(&self) -> HashMap<Date, i32> {
        let mut first: HashMap<Date, (Date, i32)> = HashMap::new();
        for facts in self.by_tag.values() {
            for f in facts.iter().filter(|f| f.fp == "FY") {
                let (Some(start), Some(fy)) = (f.start, f.fy) else { continue };
                if !is_annual((f.end - start).whole_days()) {
                    continue;
                }
                let e = first.entry(f.end).or_insert((f.filed, fy));
                if f.filed < e.0 {
                    *e = (f.filed, fy);
                }
            }
        }
        first.into_iter().map(|(end, (_, fy))| (end, fy)).collect()
    }

    /// Duration values of one line: annual and quarterly, by period end.
    fn durations(&self, l: &Line) -> (BTreeMap<Date, (Date, Cell)>, BTreeMap<Date, (Date, Cell)>) {
        let mut annual: BTreeMap<Date, (Date, Cell)> = BTreeMap::new();
        let mut quarter: BTreeMap<Date, (Date, Cell)> = BTreeMap::new();
        for tag in l.tags {
            let Some(facts) = self.by_tag.get(*tag) else { continue };
            // (start, end) -> latest-filed value: a restatement replaces the original.
            let mut latest: BTreeMap<(Date, Date), (Date, f64)> = BTreeMap::new();
            for f in facts {
                let Some(start) = f.start else { continue };
                let e = latest.entry((start, f.end)).or_insert((f.filed, f.val));
                if f.filed > e.0 {
                    *e = (f.filed, f.val);
                }
            }
            let mut tag_annual: BTreeMap<Date, (Date, f64)> = BTreeMap::new();
            let mut tag_quarter: BTreeMap<Date, (Date, f64)> = BTreeMap::new();
            // Year-to-date chains, by fiscal-year start: end -> cumulative value.
            let mut chains: BTreeMap<Date, BTreeMap<Date, f64>> = BTreeMap::new();
            for (&(start, end), &(_, v)) in &latest {
                let days = (end - start).whole_days();
                if is_annual(days) {
                    tag_annual.insert(end, (start, v));
                }
                if is_quarter(days) {
                    tag_quarter.insert(end, (start, v));
                }
                if days >= 80 && days <= 380 {
                    chains.entry(start).or_default().insert(end, v);
                }
            }
            // A quarter the filings never state on its own (Q4, or a cash-flow quarter
            // reported only year-to-date) is the difference of two consecutive YTD values.
            if !l.per_share {
                for chain in chains.values() {
                    let points: Vec<(Date, f64)> = chain.iter().map(|(d, v)| (*d, *v)).collect();
                    for w in points.windows(2) {
                        let ((prev_end, prev), (end, cur)) = (w[0], w[1]);
                        let span = (end - prev_end).whole_days();
                        if is_quarter(span) && !tag_quarter.contains_key(&end) {
                            tag_quarter.insert(end, (prev_end + time::Duration::days(1), cur - prev));
                        }
                    }
                }
            }
            for (end, (start, v)) in tag_annual {
                annual.entry(end).or_insert((start, (v, *tag)));
            }
            for (end, (start, v)) in tag_quarter {
                quarter.entry(end).or_insert((start, (v, *tag)));
            }
        }
        (annual, quarter)
    }

    /// Instant values of one line, by date.
    fn instants(&self, l: &Line) -> BTreeMap<Date, Cell> {
        let mut out: BTreeMap<Date, Cell> = BTreeMap::new();
        for tag in l.tags {
            let Some(facts) = self.by_tag.get(*tag) else { continue };
            let mut latest: BTreeMap<Date, (Date, f64)> = BTreeMap::new();
            for f in facts.iter().filter(|f| f.start.is_none()) {
                let e = latest.entry(f.end).or_insert((f.filed, f.val));
                if f.filed > e.0 {
                    *e = (f.filed, f.val);
                }
            }
            for (end, (_, v)) in latest {
                out.entry(end).or_insert((v, *tag));
            }
        }
        out
    }

    fn statements(&self) -> Vec<Statement> {
        let fiscal_years = self.fiscal_years();
        let mut annual_periods: BTreeMap<Date, Date> = BTreeMap::new();
        let mut quarter_periods: BTreeMap<Date, Date> = BTreeMap::new();
        // kind -> key -> end -> cell, for annual and quarterly rows.
        let mut annual_vals: HashMap<(&str, &str), BTreeMap<Date, Cell>> = HashMap::new();
        let mut quarter_vals: HashMap<(&str, &str), BTreeMap<Date, Cell>> = HashMap::new();
        for l in LINES {
            if l.kind == Kind::Balance {
                let inst = self.instants(l);
                annual_vals.insert((l.kind.id(), l.key), inst.clone());
                quarter_vals.insert((l.kind.id(), l.key), inst);
                continue;
            }
            let (a, q) = self.durations(l);
            for (end, (start, _)) in &a {
                annual_periods.entry(*end).or_insert(*start);
            }
            for (end, (start, _)) in &q {
                quarter_periods.entry(*end).or_insert(*start);
            }
            let sign = if l.negate { -1.0 } else { 1.0 };
            let flip = |m: BTreeMap<Date, (Date, Cell)>| -> BTreeMap<Date, Cell> {
                m.into_iter().map(|(e, (_, (v, t)))| (e, (v * sign, t))).collect()
            };
            annual_vals.insert((l.kind.id(), l.key), flip(a));
            quarter_vals.insert((l.kind.id(), l.key), flip(q));
        }

        let mut out = Vec::new();
        let annual_list: Vec<(Date, Date)> = annual_periods.iter().map(|(e, s)| (*s, *e)).collect();
        for &(start, end) in &annual_list {
            let fy = fiscal_years.get(&end).copied().unwrap_or(end.year());
            for kind in [Kind::Income, Kind::Balance, Kind::Cash] {
                if let Some(s) = row(kind, &annual_vals, end, start, "FY", fy) {
                    out.push(s);
                }
            }
        }
        // Quarters numbered by position inside their fiscal year. Quarters after the last
        // annual report belong to the years that follow it.
        let last = annual_list.last().copied();
        for (&end, &start) in &quarter_periods {
            let (year_start, fy) = match annual_list.iter().find(|(s, e)| *s < end && end <= *e) {
                Some(&(s, e)) => (s, fiscal_years.get(&e).copied().unwrap_or(e.year())),
                None => match last {
                    Some((_, last_end)) if end > last_end => {
                        let ahead = ((end - last_end).whole_days() - 1) / 365;
                        let fy0 = fiscal_years.get(&last_end).copied().unwrap_or(last_end.year());
                        (last_end + time::Duration::days(365 * ahead + 1), fy0 + 1 + ahead as i32)
                    }
                    _ => continue,
                },
            };
            let position = quarter_periods.range(year_start..=end).count();
            if !(1..=4).contains(&position) {
                continue;
            }
            let fp = format!("Q{position}");
            for kind in [Kind::Income, Kind::Balance, Kind::Cash] {
                if let Some(s) = row(kind, &quarter_vals, end, start, &fp, fy) {
                    out.push(s);
                }
            }
        }
        out
    }
}

fn parse_fact(r: &Value) -> Option<Fact> {
    Some(Fact {
        start: r.get("start").and_then(Value::as_str).and_then(super::parse_date),
        end: super::parse_date(r.get("end")?.as_str()?)?,
        val: r.get("val")?.as_f64()?,
        filed: super::parse_date(r.get("filed")?.as_str()?)?,
        fy: r.get("fy").and_then(Value::as_i64).map(|v| v as i32),
        fp: r.get("fp").and_then(Value::as_str).unwrap_or("").to_string(),
    })
}

/// One statement row, with the computed lines. None when the period has no value at all.
fn row(
    kind: Kind,
    vals: &HashMap<(&str, &str), BTreeMap<Date, Cell>>,
    end: Date,
    start: Date,
    fp: &str,
    fy: i32,
) -> Option<Statement> {
    let get = |k: Kind, key: &str| vals.get(&(k.id(), key)).and_then(|m| m.get(&end)).copied();
    let mut lines = serde_json::Map::new();
    let mut tags = serde_json::Map::new();
    for l in LINES.iter().filter(|l| l.kind == kind) {
        if let Some((v, t)) = get(kind, l.key) {
            lines.insert(l.key.into(), json!(v));
            tags.insert(l.key.into(), json!(t));
        }
    }
    if lines.is_empty() {
        return None;
    }
    let val = |lines: &serde_json::Map<String, Value>, k: &str| lines.get(k).and_then(Value::as_f64);
    match kind {
        Kind::Income => {
            if val(&lines, "gross_profit").is_none() {
                if let (Some(r), Some(c)) = (val(&lines, "revenue"), val(&lines, "cost_of_revenue")) {
                    lines.insert("gross_profit".into(), json!(r - c));
                }
            }
            let dep = get(Kind::Cash, "depreciation").map(|(v, _)| v);
            if let (Some(op), Some(d)) = (val(&lines, "operating_income"), dep) {
                lines.insert("ebitda".into(), json!(op + d));
            }
        }
        Kind::Balance => {
            let gi: Vec<f64> =
                ["goodwill", "intangibles"].iter().filter_map(|k| val(&lines, k)).collect();
            if !gi.is_empty() {
                lines.insert("goodwill_intangibles".into(), json!(gi.iter().sum::<f64>()));
            }
        }
        Kind::Cash => {
            if let (Some(ocf), Some(capex)) =
                (val(&lines, "operating_cash_flow"), val(&lines, "capex"))
            {
                lines.insert("free_cash_flow".into(), json!(ocf + capex));
            }
        }
    }
    Some(Statement {
        kind: kind.id().into(),
        fiscal_period: fp.into(),
        fiscal_year: fy,
        period_start: start,
        period_end: end,
        source: PROVIDER.into(),
        lines: Value::Object(lines),
        tags: Value::Object(tags),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form4_rows_are_read_from_the_non_derivative_table() {
        let xml = r#"<ownershipDocument>
            <issuer><issuerCik>0000320193</issuerCik><issuerName>Apple Inc.</issuerName></issuer>
            <reportingOwner><reportingOwnerId><rptOwnerName>Doe Jane &amp; Co</rptOwnerName></reportingOwnerId>
            <reportingOwnerRelationship><isDirector>0</isDirector><isOfficer>true</isOfficer><officerTitle>CFO</officerTitle></reportingOwnerRelationship></reportingOwner>
            <nonDerivativeTable>
              <nonDerivativeTransaction>
                <transactionDate><value>2026-09-01</value></transactionDate>
                <transactionCoding><transactionFormType>4</transactionFormType><transactionCode>S</transactionCode></transactionCoding>
                <transactionAmounts><transactionShares><value>1000</value></transactionShares>
                  <transactionPricePerShare><value>201.5</value></transactionPricePerShare>
                  <transactionAcquiredDisposedCode><value>D</value></transactionAcquiredDisposedCode></transactionAmounts>
                <postTransactionAmounts><sharesOwnedFollowingTransaction><value>5000</value></sharesOwnedFollowingTransaction></postTransactionAmounts>
              </nonDerivativeTransaction>
              <nonDerivativeHolding><securityTitle><value>Common</value></securityTitle></nonDerivativeHolding>
            </nonDerivativeTable>
            <derivativeTable><derivativeTransaction><transactionCoding><transactionCode>A</transactionCode></transactionCoding></derivativeTransaction></derivativeTable>
        </ownershipDocument>"#;
        let filed = Date::from_calendar_date(2026, time::Month::September, 3).unwrap();
        assert!(parse_form4(xml, "0001-26-1", filed, "0000789019").is_empty(), "another issuer's stock");
        let rows = parse_form4(xml, "0001-26-1", filed, "320193");
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.insider, "Doe Jane & Co");
        assert_eq!(r.role, "CFO");
        assert_eq!(r.code, "S");
        assert!(!r.acquired);
        assert_eq!(r.shares, 1000.0);
        assert_eq!(r.price, Some(201.5));
        assert_eq!(r.owned_after, Some(5000.0));
    }

    fn fact(start: &str, end: &str, val: f64, filed: &str, fy: i32, fp: &str) -> Value {
        json!({ "start": start, "end": end, "val": val, "filed": filed, "fy": fy, "fp": fp, "form": "10-Q" })
    }

    #[test]
    fn fourth_quarter_and_ytd_cash_flow_are_derived() {
        let body = json!({ "facts": { "us-gaap": {
            "Revenues": { "units": { "USD": [
                fact("2024-10-01", "2025-09-30", 400.0, "2025-11-01", 2025, "FY"),
                fact("2024-10-01", "2024-12-31", 90.0, "2025-02-01", 2025, "Q1"),
                fact("2025-01-01", "2025-03-31", 100.0, "2025-05-01", 2025, "Q2"),
                fact("2025-04-01", "2025-06-30", 95.0, "2025-08-01", 2025, "Q3"),
                fact("2024-10-01", "2025-06-30", 285.0, "2025-08-01", 2025, "Q3"),
                fact("2025-10-01", "2025-12-31", 120.0, "2026-02-01", 2026, "Q1"),
            ]}},
            "NetCashProvidedByUsedInOperatingActivities": { "units": { "USD": [
                fact("2024-10-01", "2024-12-31", 30.0, "2025-02-01", 2025, "Q1"),
                fact("2024-10-01", "2025-03-31", 70.0, "2025-05-01", 2025, "Q2"),
            ]}},
        }}});
        let rows = Facts::parse(&body).statements();
        let get = |kind: &str, fp: &str, fy: i32, key: &str| {
            rows.iter()
                .find(|r| r.kind == kind && r.fiscal_period == fp && r.fiscal_year == fy)
                .and_then(|r| r.lines.get(key))
                .and_then(Value::as_f64)
        };
        assert_eq!(get("income", "FY", 2025, "revenue"), Some(400.0));
        assert_eq!(get("income", "Q4", 2025, "revenue"), Some(115.0));
        assert_eq!(get("income", "Q1", 2026, "revenue"), Some(120.0));
        assert_eq!(get("cashflow", "Q2", 2025, "operating_cash_flow"), Some(40.0));
    }
}
