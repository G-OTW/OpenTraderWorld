//! Fundamentals served by connectors that already exist for price bars (Alpha Vantage,
//! EODHD, Massive) plus FINRA's keyless short interest. The market-data connector keeps
//! its key and its quota; these are extra endpoints on the same account.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::util::{self, num, text};
use super::Ctx;
use crate::histdata::{enc, Capability};

fn arr(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

fn csv_rows(body: &str) -> Vec<std::collections::HashMap<String, String>> {
    let mut rdr = csv::Reader::from_reader(body.as_bytes());
    let Ok(headers) = rdr.headers().map(|h| h.iter().map(str::to_string).collect::<Vec<_>>()) else {
        return Vec::new();
    };
    rdr.records()
        .flatten()
        .map(|r| headers.iter().cloned().zip(r.iter().map(str::to_string)).collect())
        .collect()
}

// ── Alpha Vantage ───────────────────────────────────────────────────────────

pub mod av {
    use super::*;

    pub const PROVIDER: &str = "alphavantage";

    async fn query(ctx: &Ctx<'_>, q: &str) -> Result<String> {
        let key = util::need_key(ctx.secrets, "Alpha Vantage", "alphavantage.co/support/#api-key")?;
        let url = format!("https://www.alphavantage.co/query?{q}&apikey={key}");
        let body = util::get_text(PROVIDER, ctx.client.get(&url)).await?;
        // Over the limit or a premium endpoint: HTTP 200 with a note instead of data.
        if body.trim_start().starts_with('{') {
            if let Ok(v) = serde_json::from_str::<Value>(&body) {
                for k in ["Note", "Information", "Error Message"] {
                    if let Some(msg) = v.get(k).and_then(Value::as_str) {
                        if k != "Error Message" {
                            crate::rate::note_limited(PROVIDER, "https://www.alphavantage.co/query", msg);
                        }
                        return Err(anyhow!("Alpha Vantage: {msg}"));
                    }
                }
            }
        }
        Ok(body)
    }

    async fn json(ctx: &Ctx<'_>, q: &str) -> Result<Value> {
        Ok(serde_json::from_str(&query(ctx, q).await?)?)
    }

    pub async fn dataset(ctx: &Ctx<'_>, dataset: &str, t: &str) -> Result<Value> {
        let s = enc(t);
        match dataset {
            "estimates" => {
                let est = json(ctx, &format!("function=EARNINGS_ESTIMATES&symbol={s}")).await?;
                let ov = json(ctx, &format!("function=OVERVIEW&symbol={s}")).await.ok();
                let today = OffsetDateTime::now_utc().date();
                let mut rows: Vec<Value> = est
                    .get("estimates")
                    .map(arr)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|r| util::date(r.get("date")).is_some_and(|d| d >= today))
                    .collect();
                rows.sort_by_key(|r| (text(r.get("horizon")) != "fiscal quarter", text(r.get("date"))));
                let consensus: Vec<Value> = rows
                    .iter()
                    .take(4)
                    .map(|r| {
                        let quarter = text(r.get("horizon")) == "fiscal quarter";
                        let d = text(r.get("date"));
                        json!({
                            "period": if quarter { format!("Q ending {d}") } else { format!("FY ending {}", d.get(..7).unwrap_or(&d)) },
                            "date": d,
                            "eps_mean": num(r.get("eps_estimate_average")),
                            "eps_low": num(r.get("eps_estimate_low")),
                            "eps_high": num(r.get("eps_estimate_high")),
                            "revenue_mean": num(r.get("revenue_estimate_average")),
                            "analysts": num(r.get("eps_estimate_analyst_count")),
                            "revisions_up_30d": num(r.get("eps_estimate_revision_up_trailing_30_days")),
                            "revisions_down_30d": num(r.get("eps_estimate_revision_down_trailing_30_days")),
                        })
                    })
                    .collect();
                Ok(json!({
                    "consensus": consensus,
                    "ratings": ov.as_ref().map(|o| json!({
                        "strong_buy": num(o.get("AnalystRatingStrongBuy")),
                        "buy": num(o.get("AnalystRatingBuy")),
                        "hold": num(o.get("AnalystRatingHold")),
                        "sell": num(o.get("AnalystRatingSell")),
                        "strong_sell": num(o.get("AnalystRatingStrongSell")),
                    })),
                    "price_target": ov.as_ref().and_then(|o| num(o.get("AnalystTargetPrice"))).map(|m| json!({
                        "low": null, "mean": m, "median": null, "high": null, "current": null,
                    })),
                    "actions": [],
                    "guidance": [],
                }))
            }
            "earnings" => {
                let v = json(ctx, &format!("function=EARNINGS&symbol={s}")).await?;
                let mut history: Vec<Value> = v
                    .get("quarterlyEarnings")
                    .map(arr)
                    .unwrap_or_default()
                    .iter()
                    .take(12)
                    .map(|r| json!({
                        "period": text(r.get("fiscalDateEnding")).get(..7).unwrap_or(""),
                        "date": text(r.get("reportedDate")),
                        "eps_estimate": num(r.get("estimatedEPS")),
                        "eps_actual": num(r.get("reportedEPS")),
                        "surprise_pct": num(r.get("surprisePercentage")),
                        "revenue_estimate": null,
                        "revenue_actual": null,
                        "move_next_day": null,
                        "time": match text(r.get("reportTime")).as_str() { "pre-market" => "BMO", "post-market" => "AMC", _ => "" },
                    }))
                    .collect();
                history.sort_by_key(|r| text(r.get("date")));
                let cal = query(ctx, &format!("function=EARNINGS_CALENDAR&symbol={s}&horizon=3month")).await.ok();
                let next = cal.as_deref().map(csv_rows).unwrap_or_default().into_iter().next().map(|r| json!({
                    "date": r.get("reportDate").cloned().unwrap_or_default(),
                    "time": match r.get("timeOfTheDay").map(String::as_str) { Some("pre-market") => "BMO", Some("post-market") => "AMC", _ => "" },
                    "eps_estimate": r.get("estimate").and_then(|e| e.parse::<f64>().ok()),
                }));
                Ok(json!({ "history": history, "next": next }))
            }
            "dividends" => {
                let d = json(ctx, &format!("function=DIVIDENDS&symbol={s}")).await?;
                let sp = json(ctx, &format!("function=SPLITS&symbol={s}")).await.ok();
                let mut divs: Vec<Value> = d.get("data").map(arr).unwrap_or_default().iter().take(60).map(|r| json!({
                    "period": text(r.get("ex_dividend_date")).get(..7).unwrap_or(""),
                    "ex_date": text(r.get("ex_dividend_date")),
                    "pay_date": text(r.get("payment_date")),
                    "amount": num(r.get("amount")),
                })).collect();
                divs.sort_by_key(|r| text(r.get("ex_date")));
                Ok(json!({
                    "dividends": divs,
                    "splits": sp.as_ref().and_then(|v| v.get("data")).map(arr).unwrap_or_default().iter().map(|r| json!({
                        "date": text(r.get("effective_date")),
                        "ratio": format!("{}:1", num(r.get("split_factor")).map(|f| format!("{f}")).unwrap_or_default()),
                    })).collect::<Vec<_>>(),
                }))
            }
            "etf" => {
                let v = json(ctx, &format!("function=ETF_PROFILE&symbol={s}")).await?;
                let pct = |x: Option<&Value>| num(x).map(|f| f * 100.0);
                Ok(json!({
                    "ticker": t,
                    "name": t,
                    "issuer": "",
                    "aum": num(v.get("net_assets")),
                    "expense": pct(v.get("net_expense_ratio")),
                    "inception": text(v.get("inception_date")),
                    "holdings": v.get("holdings").and_then(Value::as_array).map(Vec::len),
                    "index": "",
                    "top_holdings": v.get("holdings").map(arr).unwrap_or_default().iter().take(15).map(|r| json!({
                        "name": text(r.get("symbol")),
                        "label": text(r.get("description")),
                        "weight": pct(r.get("weight")),
                    })).collect::<Vec<_>>(),
                    "sectors": v.get("sectors").map(arr).unwrap_or_default().iter().map(|r| json!({
                        "name": text(r.get("sector")),
                        "weight": pct(r.get("weight")),
                    })).collect::<Vec<_>>(),
                    "countries": [],
                    "flows": [],
                }))
            }
            "calendar" => {
                let today = OffsetDateTime::now_utc().date();
                let horizon = (today + time::Duration::days(14)).to_string();
                let e = query(ctx, "function=EARNINGS_CALENDAR&horizon=3month").await?;
                let i = query(ctx, "function=IPO_CALENDAR").await.ok();
                Ok(json!({
                    "earnings": csv_rows(&e).into_iter()
                        .filter(|r| r.get("reportDate").is_some_and(|d| *d <= horizon))
                        .take(400)
                        .map(|r| json!({
                            "date": r.get("reportDate"),
                            "ticker": r.get("symbol"),
                            "time": match r.get("timeOfTheDay").map(String::as_str) { Some("pre-market") => "BMO", Some("post-market") => "AMC", _ => "" },
                            "eps_estimate": r.get("estimate").and_then(|x| x.parse::<f64>().ok()),
                            "revenue_estimate": null,
                        })).collect::<Vec<_>>(),
                    "ipos": i.as_deref().map(csv_rows).unwrap_or_default().into_iter().map(|r| json!({
                        "date": r.get("ipoDate"),
                        "company": r.get("name"),
                        "ticker": r.get("symbol"),
                        "exchange": r.get("exchange"),
                        "range": format!("{} to {}", r.get("priceRangeLow").cloned().unwrap_or_default(), r.get("priceRangeHigh").cloned().unwrap_or_default()),
                        "size": null,
                    })).collect::<Vec<_>>(),
                    "corporate": [],
                }))
            }
            _ => Err(anyhow!("Alpha Vantage does not serve {dataset}")),
        }
    }

    /// The transcript of one calendar quarter (`2026Q2`), as (speaker, title, text).
    /// Alpha Vantage's per-second or daily cap, as opposed to "nothing for this quarter".
    pub fn is_rate_limit(e: &anyhow::Error) -> bool {
        let m = format!("{e:#}").to_lowercase();
        m.contains("spreading out") || m.contains("rate limit") || m.contains("requests per day")
    }

    pub async fn transcript(ctx: &Ctx<'_>, t: &str, quarter: &str) -> Result<Vec<(String, String, String)>> {
        let v = json(ctx, &format!("function=EARNINGS_CALL_TRANSCRIPT&symbol={}&quarter={quarter}", enc(t))).await?;
        let turns: Vec<(String, String, String)> = v
            .get("transcript")
            .map(arr)
            .unwrap_or_default()
            .iter()
            .map(|r| (text(r.get("speaker")), text(r.get("title")), text(r.get("content"))))
            .filter(|(_, _, c)| !c.is_empty())
            .collect();
        if turns.is_empty() {
            return Err(anyhow!("Alpha Vantage has no transcript for {t} {quarter}"));
        }
        Ok(turns)
    }
}

// ── EODHD ───────────────────────────────────────────────────────────────────

pub mod eodhd {
    use super::*;

    pub const PROVIDER: &str = "eodhd";

    /// EODHD wants the exchange suffix; a bare US ticker gets `.US`.
    fn code(t: &str) -> String {
        if t.contains('.') { t.to_string() } else { format!("{}.US", t.replace('-', ".")) }
    }

    async fn get(ctx: &Ctx<'_>, path: &str, query: &str) -> Result<Value> {
        let key = util::need_key(ctx.secrets, "EODHD", "eodhd.com/register")?;
        let url = format!("https://eodhd.com/api/{path}?{query}&api_token={key}&fmt=json");
        util::get_json(PROVIDER, ctx.client.get(&url)).await
    }

    pub async fn dataset(ctx: &Ctx<'_>, dataset: &str, t: &str) -> Result<Value> {
        match dataset {
            "dividends" => {
                let c = enc(&code(t));
                let d = get(ctx, &format!("div/{c}"), "from=2010-01-01").await?;
                let s = get(ctx, &format!("splits/{c}"), "from=1990-01-01").await.ok();
                Ok(json!({
                    "dividends": arr(&d).iter().map(|r| json!({
                        "period": text(r.get("date")).get(..7).unwrap_or(""),
                        "ex_date": text(r.get("date")),
                        "pay_date": text(r.get("paymentDate")),
                        "amount": num(r.get("value")),
                    })).collect::<Vec<_>>(),
                    "splits": s.as_ref().map(arr).unwrap_or_default().iter().map(|r| {
                        let raw = text(r.get("split"));
                        let ratio = raw.split_once('/').map(|(a, b)| {
                            let f = |x: &str| x.parse::<f64>().map(|v| format!("{v}")).unwrap_or_else(|_| x.to_string());
                            format!("{}:{}", f(a), f(b))
                        }).unwrap_or(raw);
                        json!({ "date": text(r.get("date")), "ratio": ratio })
                    }).collect::<Vec<_>>(),
                }))
            }
            "etf" => {
                let v = get(ctx, &format!("fundamentals/{}", enc(&code(t))), "").await?;
                let e = v.get("ETF_Data").ok_or_else(|| anyhow!("EODHD has no ETF data for {t}"))?;
                let weights = |o: Option<&Value>| -> Vec<Value> {
                    o.and_then(Value::as_object).map(|m| m.iter().map(|(name, w)| json!({
                        "name": name,
                        "weight": num(w.get("Equity_%")).or_else(|| num(Some(w))),
                    })).collect()).unwrap_or_default()
                };
                Ok(json!({
                    "ticker": t,
                    "name": text(v.pointer("/General/Name")),
                    "issuer": text(e.get("Company_Name")),
                    "aum": num(e.get("TotalAssets")),
                    "expense": num(e.get("NetExpenseRatio")).or_else(|| num(e.get("Ongoing_Charge"))).map(|f| if f < 0.05 { f * 100.0 } else { f }),
                    "inception": text(e.get("Inception_Date")),
                    "holdings": num(e.get("Holdings_Count")),
                    "index": text(e.get("Index_Name")),
                    "top_holdings": e.get("Top_10_Holdings").and_then(Value::as_object).map(|m| m.values().map(|r| json!({
                        "name": text(r.get("Code")),
                        "label": text(r.get("Name")),
                        "weight": num(r.get("Assets_%")),
                    })).collect::<Vec<_>>()).unwrap_or_default(),
                    "sectors": weights(e.get("Sector_Weights")),
                    "countries": weights(e.get("World_Regions")),
                    "flows": [],
                }))
            }
            "calendar" => {
                let today = OffsetDateTime::now_utc().date();
                let w = |d: i64| format!("from={today}&to={}", today + time::Duration::days(d));
                let e = get(ctx, "calendar/earnings", &w(14)).await?;
                let i = get(ctx, "calendar/ipos", &w(30)).await.ok();
                let s = get(ctx, "calendar/splits", &w(30)).await.ok();
                let bare = |c: String| c.strip_suffix(".US").map(str::to_string).unwrap_or(c);
                Ok(json!({
                    "earnings": e.get("earnings").map(arr).unwrap_or_default().iter().take(400).map(|r| json!({
                        "date": text(r.get("report_date")),
                        "ticker": bare(text(r.get("code"))),
                        "time": match text(r.get("before_after_market")).as_str() { "BeforeMarket" => "BMO", "AfterMarket" => "AMC", _ => "" },
                        "eps_estimate": num(r.get("estimate")),
                        "revenue_estimate": null,
                    })).collect::<Vec<_>>(),
                    "ipos": i.as_ref().and_then(|v| v.get("ipos")).map(arr).unwrap_or_default().iter().map(|r| json!({
                        "date": text(r.get("start_date")),
                        "company": text(r.get("name")),
                        "ticker": bare(text(r.get("code"))),
                        "exchange": text(r.get("exchange")),
                        "range": format!("{} to {}", text(r.get("price_from")), text(r.get("price_to"))),
                        "size": null,
                    })).collect::<Vec<_>>(),
                    "corporate": s.as_ref().and_then(|v| v.get("splits")).map(arr).unwrap_or_default().iter().map(|r| json!({
                        "date": text(r.get("split_date")),
                        "ticker": bare(text(r.get("code"))),
                        "type": "Stock split",
                        "detail": format!("{}:{}", text(r.get("new_shares")), text(r.get("old_shares"))),
                    })).collect::<Vec<_>>(),
                }))
            }
            _ => Err(anyhow!("EODHD does not serve {dataset}")),
        }
    }
}

// ── Massive ─────────────────────────────────────────────────────────────────

pub mod massive {
    use super::*;

    pub async fn dataset(ctx: &Ctx<'_>, dataset: &str, t: &str) -> Result<Value> {
        let key = util::need_key(ctx.secrets, "Massive", "massive.com/dashboard")?;
        let s = enc(t);
        match dataset {
            "dividends" => {
                let d = crate::histdata::massive_get(ctx.client, &format!(
                    "https://api.massive.com/v3/reference/dividends?ticker={s}&limit=100&order=desc&apiKey={key}"
                )).await?;
                let sp = crate::histdata::massive_get(ctx.client, &format!(
                    "https://api.massive.com/v3/reference/splits?ticker={s}&limit=50&apiKey={key}"
                )).await.ok();
                let mut divs: Vec<Value> = d.get("results").map(arr).unwrap_or_default().iter().map(|r| json!({
                    "period": text(r.get("ex_dividend_date")).get(..7).unwrap_or(""),
                    "ex_date": text(r.get("ex_dividend_date")),
                    "pay_date": text(r.get("pay_date")),
                    "amount": num(r.get("cash_amount")),
                })).collect();
                divs.sort_by_key(|r| text(r.get("ex_date")));
                Ok(json!({
                    "dividends": divs,
                    "splits": sp.as_ref().and_then(|v| v.get("results")).map(arr).unwrap_or_default().iter().map(|r| json!({
                        "date": text(r.get("execution_date")),
                        "ratio": format!("{}:{}", text(r.get("split_to")), text(r.get("split_from"))),
                    })).collect::<Vec<_>>(),
                }))
            }
            "short_interest" => {
                let v = crate::histdata::massive_get(ctx.client, &format!(
                    "https://api.massive.com/stocks/v1/short-interest?ticker={s}&limit=100&sort=settlement_date.desc&apiKey={key}"
                )).await?;
                let mut series: Vec<Value> = v.get("results").map(arr).unwrap_or_default().iter().map(|r| json!({
                    "date": text(r.get("settlement_date")),
                    "shares": num(r.get("short_interest")),
                    "days_to_cover": num(r.get("days_to_cover")),
                    "avg_volume": num(r.get("avg_daily_volume")),
                })).collect();
                series.sort_by_key(|r| text(r.get("date")));
                Ok(json!({ "series": series }))
            }
            _ => Err(anyhow!("Massive does not serve {dataset}")),
        }
    }
}

// ── FINRA ───────────────────────────────────────────────────────────────────

pub mod finra {
    use super::*;

    pub const PROVIDER: &str = "finra";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "FINRA",
        "https://www.finra.org/finra-data",
        "https://developer.finra.org/docs",
        "Keyless public datasets. Consolidated short interest for every US listing, twice a \
         month, about 8 business days after each settlement date.",
        &[],
        &["short_interest"],
    );
    pub struct Finra;
    super::super::fundamentals_connector!(Finra, CAP);

    pub async fn short_interest(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
        let since = OffsetDateTime::now_utc().date() - time::Duration::days(3 * 365);
        let body = json!({
            "limit": 200,
            "compareFilters": [
                { "compareType": "EQUAL", "fieldName": "symbolCode", "fieldValue": t },
                { "compareType": "GREATER", "fieldName": "settlementDate", "fieldValue": since.to_string() },
            ],
        });
        let v = util::get_json(
            PROVIDER,
            ctx.client
                .post("https://api.finra.org/data/group/otcMarket/name/consolidatedShortInterest")
                .header(reqwest::header::ACCEPT, "application/json")
                .json(&body),
        )
        .await?;
        let mut series: Vec<Value> = arr(&v).iter().map(|r| json!({
            "date": text(r.get("settlementDate")),
            "shares": num(r.get("currentShortPositionQuantity")),
            "days_to_cover": num(r.get("daysToCoverQuantity")),
            "avg_volume": num(r.get("averageDailyVolumeQuantity")),
        })).collect();
        if series.is_empty() {
            return Err(anyhow!("FINRA has no short interest for {t}"));
        }
        series.sort_by_key(|r| text(r.get("date")));
        Ok(json!({ "series": series }))
    }
}
