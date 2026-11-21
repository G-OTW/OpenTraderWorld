//! Financial Modeling Prep (stable API): estimates, analyst ratings and targets, earnings,
//! revenue segments, dividends and splits, 13F holders, peers, ESG and pay, ETFs, the
//! market calendar and earnings-call transcripts. One key; what answers depends on the
//! plan (transcripts and 13F need a paid one, and say so with a 402/403).

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::util::{self, num, text};
use super::Ctx;
use crate::histdata::{enc, Capability};

pub const PROVIDER: &str = "fmp";
const BASE: &str = "https://financialmodelingprep.com/stable";

pub static CAP: Capability = super::fcap(
    PROVIDER,
    "Financial Modeling Prep",
    "https://site.financialmodelingprep.com",
    "https://site.financialmodelingprep.com/developer/docs",
    "Key required. Free: 250 calls/day, end-of-day, no transcripts or 13F. Starter ~300 \
     calls/min, Premium ~750/min; transcripts and holders need Ultimate. A company tab costs \
     1 to 4 calls and is stored, so refresh is on demand.",
    &["api_key"],
    &["estimates", "earnings", "segments", "dividends", "holders", "peers", "esg", "etf", "calendar", "transcripts"],
);
pub struct Fmp;
super::fundamentals_connector!(Fmp, CAP);

async fn get(ctx: &Ctx<'_>, path: &str, query: &str) -> Result<Value> {
    let key = util::need_key(ctx.secrets, "Financial Modeling Prep", "site.financialmodelingprep.com/developer")?;
    let sep = if query.is_empty() { "" } else { "&" };
    let url = format!("{BASE}/{path}?{query}{sep}apikey={key}");
    let v = util::get_json(PROVIDER, ctx.client.get(&url)).await?;
    if let Some(msg) = v.get("Error Message").and_then(Value::as_str) {
        return Err(anyhow!("Financial Modeling Prep: {msg}"));
    }
    Ok(v)
}

fn arr(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

fn sym(s: &str) -> String {
    format!("symbol={}", enc(s))
}

pub async fn probe(ctx: &Ctx<'_>) -> Result<String> {
    let v = get(ctx, "profile", &sym("AAPL")).await?;
    Ok(format!("key accepted: {}", text(v.pointer("/0/companyName"))))
}

pub async fn dataset(ctx: &Ctx<'_>, dataset: &str, subject: &str) -> Result<Value> {
    match dataset {
        "estimates" => estimates(ctx, subject).await,
        "earnings" => earnings(ctx, subject).await,
        "segments" => segments(ctx, subject).await,
        "dividends" => dividends(ctx, subject).await,
        "holders" => holders(ctx, subject).await,
        "peers" => peers(ctx, subject).await,
        "esg" => esg(ctx, subject).await,
        "etf" => etf(ctx, subject).await,
        "calendar" => calendar(ctx).await,
        _ => Err(anyhow!("Financial Modeling Prep does not serve {dataset}")),
    }
}

async fn estimates(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let today = OffsetDateTime::now_utc().date();
    let annual = get(ctx, "analyst-estimates", &format!("{}&period=annual&limit=10", sym(t))).await?;
    let quarter = get(ctx, "analyst-estimates", &format!("{}&period=quarter&limit=12", sym(t))).await.unwrap_or(Value::Null);
    let row = |r: &Value, label: String| {
        json!({
            "period": label,
            "date": text(r.get("date")),
            "eps_mean": num(r.get("epsAvg")),
            "eps_low": num(r.get("epsLow")),
            "eps_high": num(r.get("epsHigh")),
            "revenue_mean": num(r.get("revenueAvg")),
            "analysts": num(r.get("numAnalystsEps")),
            "revisions_up_30d": null,
            "revisions_down_30d": null,
        })
    };
    let ahead = |rows: Vec<Value>| -> Vec<Value> {
        let mut r: Vec<Value> = rows.into_iter().filter(|r| util::date(r.get("date")).is_some_and(|d| d >= today)).collect();
        r.sort_by_key(|r| text(r.get("date")));
        r
    };
    let mut consensus: Vec<Value> = ahead(arr(&quarter)).into_iter().take(2).map(|r| {
        let d = text(r.get("date"));
        row(&r, format!("Q ending {d}"))
    }).collect();
    consensus.extend(ahead(arr(&annual)).into_iter().take(2).map(|r| {
        let d = text(r.get("date"));
        row(&r, format!("FY ending {}", d.get(..7).unwrap_or(&d)))
    }));
    let target = get(ctx, "price-target-consensus", &sym(t)).await.ok();
    let grades = get(ctx, "grades-consensus", &sym(t)).await.ok();
    let actions = get(ctx, "grades", &format!("{}&limit=20", sym(t))).await.ok();
    let t0 = target.as_ref().and_then(|v| v.get(0));
    let g0 = grades.as_ref().and_then(|v| v.get(0));
    Ok(json!({
        "consensus": consensus,
        "price_target": t0.map(|r| json!({
            "low": num(r.get("targetLow")),
            "mean": num(r.get("targetConsensus")),
            "median": num(r.get("targetMedian")),
            "high": num(r.get("targetHigh")),
            "current": null,
        })),
        "ratings": g0.map(|r| json!({
            "strong_buy": num(r.get("strongBuy")),
            "buy": num(r.get("buy")),
            "hold": num(r.get("hold")),
            "sell": num(r.get("sell")),
            "strong_sell": num(r.get("strongSell")),
        })),
        "actions": actions.map(|a| arr(&a).iter().take(20).map(|r| json!({
            "date": text(r.get("date")),
            "firm": text(r.get("gradingCompany")),
            "action": text(r.get("action")),
            "rating": text(r.get("newGrade")),
            "previous": text(r.get("previousGrade")),
            "target": null,
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "guidance": [],
    }))
}

async fn earnings(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let rows = arr(&get(ctx, "earnings", &format!("{}&limit=40", sym(t))).await?);
    let mut history = Vec::new();
    let mut next: Option<Value> = None;
    for r in rows.iter() {
        let actual = num(r.get("epsActual"));
        let est = num(r.get("epsEstimated"));
        if actual.is_none() {
            // Upcoming: keep the nearest one.
            if next.as_ref().map_or(true, |n| text(n.get("date")) > text(r.get("date"))) {
                next = Some(json!({ "date": text(r.get("date")), "time": null, "eps_estimate": est }));
            }
            continue;
        }
        history.push(json!({
            "period": text(r.get("date")).get(..7).unwrap_or(""),
            "date": text(r.get("date")),
            "eps_estimate": est,
            "eps_actual": actual,
            "surprise_pct": surprise(actual, est),
            "revenue_estimate": num(r.get("revenueEstimated")),
            "revenue_actual": num(r.get("revenueActual")),
            "move_next_day": null,
        }));
    }
    history.sort_by_key(|r| text(r.get("date")));
    let keep = history.len().saturating_sub(12);
    Ok(json!({ "history": history.split_off(keep), "next": next }))
}

pub fn surprise(actual: Option<f64>, est: Option<f64>) -> Option<f64> {
    match (actual, est) {
        (Some(a), Some(e)) if e != 0.0 => Some((a - e) / e.abs() * 100.0),
        _ => None,
    }
}

async fn segments(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let product = arr(&get(ctx, "revenue-product-segmentation", &format!("{}&period=annual", sym(t))).await?);
    let region = arr(&get(ctx, "revenue-geographic-segmentation", &format!("{}&period=annual", sym(t))).await.unwrap_or(Value::Null));
    // Last five fiscal years present in either.
    let mut years: Vec<i64> = product.iter().chain(region.iter()).filter_map(|r| r.get("fiscalYear").and_then(Value::as_i64)).collect();
    years.sort();
    years.dedup();
    let years: Vec<i64> = years.into_iter().rev().take(5).rev().collect();
    let split = |rows: &[Value]| -> Vec<Value> {
        let mut by_name: BTreeMap<String, Vec<Option<f64>>> = BTreeMap::new();
        for (i, y) in years.iter().enumerate() {
            let Some(r) = rows.iter().find(|r| r.get("fiscalYear").and_then(Value::as_i64) == Some(*y)) else { continue };
            for (name, v) in r.get("data").and_then(Value::as_object).into_iter().flatten() {
                by_name.entry(name.clone()).or_insert_with(|| vec![None; years.len()])[i] = num(Some(v));
            }
        }
        let mut out: Vec<(String, Vec<Option<f64>>)> = by_name.into_iter().collect();
        out.sort_by(|a, b| {
            let last = |v: &Vec<Option<f64>>| v.iter().rev().flatten().next().copied().unwrap_or(0.0);
            last(&b.1).partial_cmp(&last(&a.1)).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.into_iter().map(|(name, values)| json!({ "name": name, "values": values })).collect()
    };
    Ok(json!({
        "years": years.iter().map(|y| format!("FY{y}")).collect::<Vec<_>>(),
        "product": split(&product),
        "region": split(&region),
        "kpis": [],
    }))
}

async fn dividends(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let div = arr(&get(ctx, "dividends", &format!("{}&limit=60", sym(t))).await?);
    let splits = arr(&get(ctx, "splits", &sym(t)).await.unwrap_or(Value::Null));
    Ok(json!({
        "dividends": div.iter().rev().map(|r| json!({
            "period": text(r.get("date")).get(..7).unwrap_or(""),
            "ex_date": text(r.get("date")),
            "pay_date": text(r.get("paymentDate")),
            "amount": num(r.get("dividend")).or_else(|| num(r.get("adjDividend"))),
        })).collect::<Vec<_>>(),
        "splits": splits.iter().map(|r| json!({
            "date": text(r.get("date")),
            "ratio": format!("{}:{}", text(r.get("numerator")), text(r.get("denominator"))),
        })).collect::<Vec<_>>(),
    }))
}

/// Latest complete 13F quarter: the one before the current, or the one before that while
/// the filing deadline (45 days) has not passed.
fn holder_quarters() -> Vec<(i32, u8)> {
    let today = OffsetDateTime::now_utc().date();
    let q = (today.month() as u8 - 1) / 3 + 1;
    let mut out = Vec::new();
    let (mut y, mut qq) = (today.year(), q);
    for _ in 0..3 {
        if qq == 1 {
            y -= 1;
            qq = 4;
        } else {
            qq -= 1;
        }
        out.push((y, qq));
    }
    out
}

async fn holders(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let mut last_err = None;
    for (y, q) in holder_quarters() {
        let query = format!("{}&year={y}&quarter={q}&page=0&limit=25", sym(t));
        match get(ctx, "institutional-ownership/extract-analytics/holder", &query).await {
            Ok(v) if !arr(&v).is_empty() => {
                let summary = get(ctx, "institutional-ownership/symbol-positions-summary", &format!("{}&year={y}&quarter={q}", sym(t))).await.ok();
                let s0 = summary.as_ref().and_then(|v| v.get(0));
                return Ok(json!({
                    "quarter": format!("{y}-Q{q}"),
                    "holders": arr(&v).iter().map(|r| json!({
                        "holder": text(r.get("investorName")),
                        "pct": num(r.get("ownership")),
                        "shares": num(r.get("sharesNumber")),
                        "change_pct": num(r.get("changeInSharesNumberPercentage")),
                        "filed": text(r.get("filingDate")),
                    })).collect::<Vec<_>>(),
                    "institutions_pct": s0.and_then(|r| num(r.get("ownershipPercent"))),
                    "investors": s0.and_then(|r| num(r.get("investorsHolding"))),
                }));
            }
            Ok(_) => continue,
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("Financial Modeling Prep has no 13F holders for {t}")))
}

async fn peers(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let rows = arr(&get(ctx, "stock-peers", &sym(t)).await?);
    Ok(Value::Array(
        rows.iter()
            .map(|r| json!({ "ticker": text(r.get("symbol")), "name": text(r.get("companyName")), "market_cap": num(r.get("mktCap")) }))
            .collect(),
    ))
}

async fn esg(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let disc = get(ctx, "esg-disclosures", &sym(t)).await.ok();
    let pay = get(ctx, "governance-executive-compensation", &sym(t)).await.ok();
    let d0 = disc.as_ref().and_then(|v| v.get(0));
    let pay_rows = pay.map(|p| arr(&p)).unwrap_or_default();
    let latest = pay_rows.iter().filter_map(|r| r.get("year").and_then(Value::as_i64)).max();
    if d0.is_none() && pay_rows.is_empty() {
        return Err(anyhow!("Financial Modeling Prep has no ESG or pay data for {t} on this plan"));
    }
    Ok(json!({
        "esg": d0.map(|r| json!({
            "total": num(r.get("ESGScore")),
            "environment": num(r.get("environmentalScore")),
            "social": num(r.get("socialScore")),
            "governance": num(r.get("governanceScore")),
            "controversy": null,
            "date": text(r.get("date")),
            "provider": "Financial Modeling Prep",
            "higher_is_better": true,
        })),
        "management": pay_rows.iter()
            .filter(|r| r.get("year").and_then(Value::as_i64) == latest)
            .map(|r| json!({ "name": text(r.get("nameAndPosition")), "title": "", "since": r.get("year"), "pay": num(r.get("total")) }))
            .collect::<Vec<_>>(),
    }))
}

async fn etf(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let info = get(ctx, "etf/info", &sym(t)).await?;
    let i0 = info.get(0).ok_or_else(|| anyhow!("Financial Modeling Prep has no ETF {t}"))?;
    let holdings = arr(&get(ctx, "etf/holdings", &sym(t)).await.unwrap_or(Value::Null));
    let countries = arr(&get(ctx, "etf/country-weightings", &sym(t)).await.unwrap_or(Value::Null));
    Ok(json!({
        "ticker": t,
        "name": text(i0.get("name")),
        "issuer": text(i0.get("etfCompany")),
        "aum": num(i0.get("assetsUnderManagement")),
        "expense": num(i0.get("expenseRatio")),
        "inception": text(i0.get("inceptionDate")),
        "holdings": num(i0.get("holdingsCount")),
        "index": "",
        "description": text(i0.get("description")),
        "top_holdings": holdings.iter().take(15).map(|r| json!({
            "name": text(r.get("asset")),
            "label": text(r.get("name")),
            "weight": num(r.get("weightPercentage")),
        })).collect::<Vec<_>>(),
        "sectors": i0.get("sectorsList").and_then(Value::as_array).map(|s| s.iter().map(|r| json!({
            "name": text(r.get("industry")),
            "weight": num(r.get("exposure")),
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "countries": countries.iter().map(|r| json!({
            "name": text(r.get("country")),
            "weight": num(r.get("weightPercentage")),
        })).collect::<Vec<_>>(),
        "flows": [],
    }))
}

async fn calendar(ctx: &Ctx<'_>) -> Result<Value> {
    let today = OffsetDateTime::now_utc().date();
    let window = |days: i64| format!("from={today}&to={}", today + time::Duration::days(days));
    let earnings = arr(&get(ctx, "earnings-calendar", &window(14)).await?);
    let ipos = arr(&get(ctx, "ipos-calendar", &window(30)).await.unwrap_or(Value::Null));
    let splits = arr(&get(ctx, "splits-calendar", &window(30)).await.unwrap_or(Value::Null));
    Ok(json!({
        "earnings": earnings.iter().take(400).map(|r| json!({
            "date": text(r.get("date")),
            "ticker": text(r.get("symbol")),
            "time": null,
            "eps_estimate": num(r.get("epsEstimated")),
            "revenue_estimate": num(r.get("revenueEstimated")),
        })).collect::<Vec<_>>(),
        "ipos": ipos.iter().map(|r| json!({
            "date": text(r.get("date")),
            "company": text(r.get("company")),
            "ticker": text(r.get("symbol")),
            "exchange": text(r.get("exchange")),
            "range": text(r.get("priceRange")),
            "size": num(r.get("marketCap")),
        })).collect::<Vec<_>>(),
        "corporate": splits.iter().map(|r| json!({
            "date": text(r.get("date")),
            "ticker": text(r.get("symbol")),
            "type": "Stock split",
            "detail": format!("{}:{}", text(r.get("numerator")), text(r.get("denominator"))),
        })).collect::<Vec<_>>(),
    }))
}

// ── Transcripts ─────────────────────────────────────────────────────────────

/// (year, quarter, date) of every transcript FMP holds for a symbol, newest first.
pub async fn transcript_list(ctx: &Ctx<'_>, t: &str) -> Result<Vec<(i32, u8, String)>> {
    let rows = arr(&get(ctx, "earning-call-transcript-dates", &sym(t)).await?);
    let mut out: Vec<(i32, u8, String)> = rows
        .iter()
        .filter_map(|r| Some((r.get("fiscalYear")?.as_i64()? as i32, r.get("quarter")?.as_i64()? as u8, text(r.get("date")))))
        .collect();
    out.sort_by(|a, b| b.2.cmp(&a.2));
    Ok(out)
}

/// The transcript text: FMP sends one string of `Speaker: words` lines.
pub async fn transcript(ctx: &Ctx<'_>, t: &str, year: i32, quarter: u8) -> Result<Vec<(String, String, String)>> {
    let v = get(ctx, "earning-call-transcript", &format!("{}&year={year}&quarter={quarter}", sym(t))).await?;
    let content = text(v.pointer("/0/content"));
    if content.is_empty() {
        return Err(anyhow!("Financial Modeling Prep has no transcript for {t} FY{year} Q{quarter}"));
    }
    Ok(super::transcripts::split_speakers(&content))
}

