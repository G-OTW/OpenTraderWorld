//! Finnhub: on the free key, the earnings and IPO calendars, recommendation trends, the
//! last earnings surprises and peers. Price targets, EPS estimates, ESG and transcripts
//! are premium and answer 403 on a free key, which the dataset reports as such.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use super::util::{self, num, text};
use super::Ctx;
use crate::histdata::{enc, Capability};

pub const PROVIDER: &str = "finnhub";
const BASE: &str = "https://finnhub.io/api/v1";

pub static CAP: Capability = super::fcap(
    PROVIDER,
    "Finnhub",
    "https://finnhub.io",
    "https://finnhub.io/docs/api",
    "Free key: 60 calls/min, personal use; calendars, recommendations, earnings surprises and \
     peers. Estimates, price targets, ESG, transcripts and congress trades are premium add-ons.",
    &["api_key"],
    &["estimates", "earnings", "peers", "esg", "calendar", "transcripts", "alt"],
);
pub struct Finnhub;
super::fundamentals_connector!(Finnhub, CAP);

pub(super) async fn get(ctx: &Ctx<'_>, path: &str, query: &str) -> Result<Value> {
    let key = util::need_key(ctx.secrets, "Finnhub", "finnhub.io/register")?;
    let url = format!("{BASE}/{path}?{query}&token={key}");
    let v = util::get_json(PROVIDER, ctx.client.get(&url)).await?;
    if let Some(msg) = v.get("error").and_then(Value::as_str) {
        return Err(anyhow!("Finnhub: {msg}"));
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
    let v = get(ctx, "stock/peers", &sym("AAPL")).await?;
    Ok(format!("key accepted: {} peers for AAPL", arr(&v).len()))
}

pub async fn dataset(ctx: &Ctx<'_>, dataset: &str, subject: &str) -> Result<Value> {
    match dataset {
        "estimates" => estimates(ctx, subject).await,
        "earnings" => earnings(ctx, subject).await,
        "peers" => {
            let v = get(ctx, "stock/peers", &sym(subject)).await?;
            Ok(Value::Array(
                arr(&v).iter().map(|t| json!({ "ticker": text(Some(t)), "name": "", "market_cap": null })).collect(),
            ))
        }
        "esg" => {
            let v = get(ctx, "stock/esg", &sym(subject)).await?;
            Ok(json!({
                "esg": {
                    "total": num(v.get("totalESGScore")),
                    "environment": num(v.get("environmentScore")),
                    "social": num(v.get("socialScore")),
                    "governance": num(v.get("governanceScore")),
                    "controversy": null,
                    "provider": "Finnhub",
                    "higher_is_better": true,
                },
                "management": [],
            }))
        }
        "calendar" => calendar(ctx).await,
        _ => Err(anyhow!("Finnhub does not serve {dataset}")),
    }
}

async fn estimates(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let recs = arr(&get(ctx, "stock/recommendation", &sym(t)).await?);
    // Premium on a free key: absent rather than failing the whole tab.
    let target = get(ctx, "stock/price-target", &sym(t)).await.ok();
    let eps = get(ctx, "stock/eps-estimate", &format!("{}&freq=annual", sym(t))).await.ok();
    let r0 = recs.first();
    let year = OffsetDateTime::now_utc().year() as i64;
    let consensus: Vec<Value> = eps
        .as_ref()
        .map(|v| arr(v.get("data").unwrap_or(&Value::Null)))
        .unwrap_or_default()
        .iter()
        .filter(|r| r.get("year").and_then(Value::as_i64).is_some_and(|y| y >= year))
        .take(2)
        .map(|r| json!({
            "period": format!("FY{}", text(r.get("year"))),
            "eps_mean": num(r.get("epsAvg")),
            "eps_low": num(r.get("epsLow")),
            "eps_high": num(r.get("epsHigh")),
            "revenue_mean": null,
            "analysts": num(r.get("numberAnalysts")),
            "revisions_up_30d": null,
            "revisions_down_30d": null,
        }))
        .collect();
    Ok(json!({
        "consensus": consensus,
        "ratings": r0.map(|r| json!({
            "strong_buy": num(r.get("strongBuy")),
            "buy": num(r.get("buy")),
            "hold": num(r.get("hold")),
            "sell": num(r.get("sell")),
            "strong_sell": num(r.get("strongSell")),
            "period": text(r.get("period")),
        })),
        "price_target": target.filter(|t| t.get("targetMean").is_some()).map(|r| json!({
            "low": num(r.get("targetLow")),
            "mean": num(r.get("targetMean")),
            "median": num(r.get("targetMedian")),
            "high": num(r.get("targetHigh")),
            "current": null,
        })),
        "actions": [],
        "guidance": [],
    }))
}

async fn earnings(ctx: &Ctx<'_>, t: &str) -> Result<Value> {
    let rows = arr(&get(ctx, "stock/earnings", &sym(t)).await?);
    let today = OffsetDateTime::now_utc().date();
    let upcoming = get(
        ctx,
        "calendar/earnings",
        &format!("from={today}&to={}&{}", today + time::Duration::days(120), sym(t)),
    )
    .await
    .ok();
    let next = upcoming
        .as_ref()
        .and_then(|v| v.get("earningsCalendar"))
        .map(arr)
        .unwrap_or_default()
        .into_iter()
        .min_by_key(|r| text(r.get("date")))
        .map(|r| json!({
            "date": text(r.get("date")),
            "time": hour(&text(r.get("hour"))),
            "eps_estimate": num(r.get("epsEstimate")),
        }));
    let mut history: Vec<Value> = rows
        .iter()
        .map(|r| json!({
            "period": format!("Q{} {}", text(r.get("quarter")), text(r.get("year"))),
            "date": text(r.get("period")),
            "eps_estimate": num(r.get("estimate")),
            "eps_actual": num(r.get("actual")),
            "surprise_pct": num(r.get("surprisePercent")),
            "revenue_estimate": null,
            "revenue_actual": null,
            "move_next_day": null,
        }))
        .collect();
    history.sort_by_key(|r| text(r.get("date")));
    Ok(json!({ "history": history, "next": next }))
}

fn hour(h: &str) -> Value {
    match h {
        "bmo" => json!("BMO"),
        "amc" => json!("AMC"),
        "dmh" => json!("DMH"),
        _ => Value::Null,
    }
}

async fn calendar(ctx: &Ctx<'_>) -> Result<Value> {
    let today = OffsetDateTime::now_utc().date();
    let e = get(ctx, "calendar/earnings", &format!("from={today}&to={}", today + time::Duration::days(14))).await?;
    let i = get(ctx, "calendar/ipo", &format!("from={today}&to={}", today + time::Duration::days(30))).await.ok();
    Ok(json!({
        "earnings": e.get("earningsCalendar").map(arr).unwrap_or_default().iter().take(400).map(|r| json!({
            "date": text(r.get("date")),
            "ticker": text(r.get("symbol")),
            "time": hour(&text(r.get("hour"))),
            "eps_estimate": num(r.get("epsEstimate")),
            "revenue_estimate": num(r.get("revenueEstimate")),
        })).collect::<Vec<_>>(),
        "ipos": i.as_ref().and_then(|v| v.get("ipoCalendar")).map(arr).unwrap_or_default().iter().map(|r| json!({
            "date": text(r.get("date")),
            "company": text(r.get("name")),
            "ticker": text(r.get("symbol")),
            "exchange": text(r.get("exchange")),
            "range": text(r.get("price")),
            "size": num(r.get("totalSharesValue")),
            "status": text(r.get("status")),
        })).collect::<Vec<_>>(),
        "corporate": [],
    }))
}

// ── Transcripts (premium) ───────────────────────────────────────────────────

/// (id, year, quarter, date) of each transcript, newest first.
pub async fn transcript_list(ctx: &Ctx<'_>, t: &str) -> Result<Vec<(String, i32, u8, String)>> {
    let v = get(ctx, "stock/transcripts/list", &sym(t)).await?;
    let mut out: Vec<(String, i32, u8, String)> = v
        .get("transcripts")
        .map(arr)
        .unwrap_or_default()
        .iter()
        .filter_map(|r| {
            Some((
                text(r.get("id")),
                r.get("year")?.as_i64()? as i32,
                r.get("quarter")?.as_i64()? as u8,
                text(r.get("time")).get(..10).unwrap_or("").to_string(),
            ))
        })
        .collect();
    out.sort_by(|a, b| b.3.cmp(&a.3));
    Ok(out)
}

/// Speaker turns (speaker, title, text, section).
pub async fn transcript(ctx: &Ctx<'_>, id: &str) -> Result<Vec<(String, String, String, Option<String>)>> {
    let v = get(ctx, "stock/transcripts", &format!("id={}", enc(id))).await?;
    let roles: Vec<(String, String)> = v
        .get("participant")
        .map(arr)
        .unwrap_or_default()
        .iter()
        .map(|p| (text(p.get("name")), text(p.get("description"))))
        .collect();
    Ok(v.get("transcript")
        .map(arr)
        .unwrap_or_default()
        .iter()
        .map(|t| {
            let name = text(t.get("name"));
            let title = roles.iter().find(|(n, _)| *n == name).map(|(_, d)| d.clone()).unwrap_or_default();
            let speech = t
                .get("speech")
                .and_then(Value::as_array)
                .map(|s| s.iter().map(|x| text(Some(x))).collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            let section = match text(t.get("session")).as_str() {
                "question_answer_session" => Some("qa".to_string()),
                "management_discussion" => Some("prepared".to_string()),
                _ => None,
            };
            (name, title, speech, section)
        })
        .collect())
}
