//! The SDMX publishers: ECB, Eurostat, BIS, OECD and IMF. All keyless, all answer the same
//! REST shape (`data/{flow}/{key}`) in SDMX-CSV, so one client reads them all; only the
//! base URL and the way CSV is asked for differ.
//!
//! A series code is `{flow}/{key}` exactly as the publisher writes it
//! (`HICP/M.U2.N.000000.4D0.ANR`, `OECD.SDD.STES,DSD_STES@DF_CLI,/USA.M.LI...AA...H`). A key
//! with wildcards that matches several series is refused: one code is one line on a chart.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use time::Date;

use super::util::{self, Entry};
use crate::histdata::Capability;
use otw_store::fundamentals::SeriesMeta;

pub struct Publisher {
    pub id: &'static str,
    pub label: &'static str,
    url: fn(&str, &str) -> String,
    /// Sent as `Accept` when the publisher picks CSV from content negotiation.
    accept: Option<&'static str>,
    pub catalog: &'static [Entry],
}

pub const ECB: Publisher = Publisher {
    id: "ecb",
    label: "ECB",
    url: |flow, key| format!("https://data-api.ecb.europa.eu/service/data/{flow}/{key}?format=csvdata"),
    accept: None,
    catalog: &[
        Entry { code: "HICP/M.U2.N.000000.4D0.ANR", title: "Euro area HICP, y/y", category: "inflation", country: "EA", unit: "%" },
        Entry { code: "HICP/M.U2.N.XEF000.4D0.ANR", title: "Euro area core HICP (ex energy, food), y/y", category: "inflation", country: "EA", unit: "%" },
        Entry { code: "FM/D.U2.EUR.4F.KR.DFR.LEV", title: "ECB deposit facility rate", category: "rates", country: "EA", unit: "%" },
        Entry { code: "FM/B.U2.EUR.4F.KR.MRR_FR.LEV", title: "ECB main refinancing rate", category: "rates", country: "EA", unit: "%" },
        Entry { code: "YC/B.U2.EUR.4F.G_N_A.SV_C_YM.SR_10Y", title: "Euro area 10-year AAA government yield", category: "rates", country: "EA", unit: "%" },
        Entry { code: "BSI/M.U2.Y.V.M30.X.I.U2.2300.Z01.A", title: "Euro area M3, y/y", category: "money", country: "EA", unit: "%" },
        Entry { code: "EXR/D.USD.EUR.SP00.A", title: "EUR/USD reference rate", category: "other", country: "EA", unit: "USD" },
    ],
};

pub const EUROSTAT: Publisher = Publisher {
    id: "eurostat",
    label: "Eurostat",
    url: |flow, key| {
        format!("https://ec.europa.eu/eurostat/api/dissemination/sdmx/2.1/data/{flow}/{key}?format=SDMX-CSV")
    },
    accept: None,
    catalog: &[
        Entry { code: "namq_10_gdp/Q.CLV_PCH_PRE.SCA.B1GQ.EA21", title: "Euro area real GDP, q/q", category: "growth", country: "EA", unit: "%" },
        Entry { code: "une_rt_m/M.SA.TOTAL.PC_ACT.T.EA21", title: "Euro area unemployment rate", category: "labour", country: "EA", unit: "%" },
        Entry { code: "prc_hicp_minr/M.RCH_A.TOTAL.EA", title: "Euro area HICP (ECOICOP 2), y/y", category: "inflation", country: "EA", unit: "%" },
        Entry { code: "ei_bssi_m_r2/M.BS-ESI-I.SA.EA21", title: "Euro area economic sentiment indicator", category: "surveys", country: "EA", unit: "Index" },
        Entry { code: "une_rt_m/M.SA.TOTAL.PC_ACT.T.DE", title: "Germany unemployment rate", category: "labour", country: "DE", unit: "%" },
        Entry { code: "une_rt_m/M.SA.TOTAL.PC_ACT.T.FR", title: "France unemployment rate", category: "labour", country: "FR", unit: "%" },
    ],
};

pub const BIS: Publisher = Publisher {
    id: "bis",
    label: "BIS",
    url: |flow, key| format!("https://stats.bis.org/api/v1/data/{flow}/{key}?format=csv"),
    accept: None,
    catalog: &[
        Entry { code: "WS_CBPOL/M.US", title: "Fed policy rate (BIS)", category: "rates", country: "US", unit: "%" },
        Entry { code: "WS_CBPOL/M.XM", title: "ECB policy rate (BIS)", category: "rates", country: "EA", unit: "%" },
        Entry { code: "WS_CBPOL/M.GB", title: "Bank of England policy rate", category: "rates", country: "GB", unit: "%" },
        Entry { code: "WS_CBPOL/M.JP", title: "Bank of Japan policy rate", category: "rates", country: "JP", unit: "%" },
        Entry { code: "WS_CBPOL/M.CH", title: "Swiss National Bank policy rate", category: "rates", country: "CH", unit: "%" },
        Entry { code: "WS_EER/M.R.B.US", title: "US dollar real effective exchange rate (broad)", category: "other", country: "US", unit: "Index" },
        Entry { code: "WS_EER/M.R.B.XM", title: "Euro real effective exchange rate (broad)", category: "other", country: "EA", unit: "Index" },
    ],
};

pub const OECD: Publisher = Publisher {
    id: "oecd",
    label: "OECD",
    url: |flow, key| format!("https://sdmx.oecd.org/public/rest/data/{flow}/{key}?format=csvfile"),
    accept: None,
    catalog: &[
        Entry { code: "OECD.SDD.STES,DSD_STES@DF_CLI,/USA.M.LI...AA...H", title: "US composite leading indicator", category: "surveys", country: "US", unit: "Index" },
        Entry { code: "OECD.SDD.STES,DSD_STES@DF_CLI,/G7.M.LI...AA...H", title: "G7 composite leading indicator", category: "surveys", country: "G7", unit: "Index" },
        Entry { code: "OECD.SDD.STES,DSD_STES@DF_CLI,/CHN.M.LI...AA...H", title: "China composite leading indicator", category: "surveys", country: "CN", unit: "Index" },
    ],
};

pub const IMF: Publisher = Publisher {
    id: "imf",
    label: "IMF",
    url: |flow, key| format!("https://api.imf.org/external/sdmx/2.1/data/{flow}/{key}"),
    accept: Some("application/vnd.sdmx.data+csv;version=1.0.0"),
    catalog: &[
        Entry { code: "IMF.RES,WEO/G001.NGDP_RPCH.A", title: "World real GDP growth (WEO, incl. projections)", category: "growth", country: "World", unit: "%" },
        Entry { code: "IMF.RES,WEO/USA.NGDP_RPCH.A", title: "US real GDP growth (WEO, incl. projections)", category: "growth", country: "US", unit: "%" },
        Entry { code: "IMF.RES,WEO/CHN.NGDP_RPCH.A", title: "China real GDP growth (WEO, incl. projections)", category: "growth", country: "CN", unit: "%" },
        Entry { code: "IMF.RES,WEO/USA.PCPIPCH.A", title: "US inflation, average (WEO, incl. projections)", category: "inflation", country: "US", unit: "%" },
    ],
};

pub const ALL: &[&Publisher] = &[&ECB, &EUROSTAT, &BIS, &OECD, &IMF];

pub fn publisher(id: &str) -> Option<&'static Publisher> {
    ALL.iter().copied().find(|p| p.id == id)
}

fn split_code(code: &str) -> Result<(&str, &str)> {
    code.trim()
        .split_once('/')
        .filter(|(f, k)| !f.is_empty() && !k.is_empty())
        .ok_or_else(|| anyhow!("an SDMX series code is FLOW/KEY, as the publisher writes it"))
}

/// One CSV answer, as rows keyed by column name.
pub async fn rows(
    p: &Publisher,
    client: &reqwest::Client,
    flow: &str,
    key: &str,
    extra: &str,
) -> Result<Vec<HashMap<String, String>>> {
    let url = format!("{}{extra}", (p.url)(flow, key));
    let mut req = client.get(&url);
    if let Some(a) = p.accept {
        req = req.header(reqwest::header::ACCEPT, a);
    }
    let text = util::get_text(p.id, req).await?;
    let mut rdr = csv::Reader::from_reader(text.as_bytes());
    let headers: Vec<String> = rdr.headers()?.iter().map(str::to_string).collect();
    if !headers.iter().any(|h| h == "OBS_VALUE") {
        return Err(anyhow!("{}: no data for {flow}/{key}", p.label));
    }
    Ok(rdr
        .records()
        .flatten()
        .map(|r| headers.iter().cloned().zip(r.iter().map(str::to_string)).collect())
        .collect())
}

/// Columns that are not the series' identity.
const NOT_KEY: &[&str] = &[
    "DATAFLOW", "LAST UPDATE", "TIME_PERIOD", "OBS_VALUE", "OBS_STATUS", "OBS_CONF", "OBS_FLAG",
    "OBS_PRE_BREAK", "OBS_COM", "CONF_STATUS", "UNIT_MULT", "DECIMALS", "TITLE", "TITLE_COMPL",
];

/// The distinct series a set of rows covers: the dimension columns before TIME_PERIOD.
fn series_ids(rows: &[HashMap<String, String>], headers: &[String]) -> HashSet<String> {
    let dims: Vec<&String> = headers
        .iter()
        .take_while(|h| *h != "TIME_PERIOD")
        .filter(|h| !NOT_KEY.contains(&h.as_str()))
        .collect();
    rows.iter()
        .map(|r| dims.iter().map(|d| r.get(*d).map(String::as_str).unwrap_or("")).collect::<Vec<_>>().join("."))
        .collect()
}

pub async fn fetch(
    p: &Publisher,
    client: &reqwest::Client,
    code: &str,
) -> Result<(SeriesMeta, Vec<(Date, f64)>)> {
    let (flow, key) = split_code(code)?;
    let url = (p.url)(flow, key);
    let mut req = client.get(&url);
    if let Some(a) = p.accept {
        req = req.header(reqwest::header::ACCEPT, a);
    }
    let text = util::get_text(p.id, req).await?;
    let mut rdr = csv::Reader::from_reader(text.as_bytes());
    let headers: Vec<String> = rdr.headers()?.iter().map(str::to_string).collect();
    if !headers.iter().any(|h| h == "OBS_VALUE") {
        return Err(anyhow!("{} has no data for {code}", p.label));
    }
    let rows: Vec<HashMap<String, String>> = rdr
        .records()
        .flatten()
        .map(|r| headers.iter().cloned().zip(r.iter().map(str::to_string)).collect())
        .collect();
    let ids = series_ids(&rows, &headers);
    if ids.len() > 1 {
        return Err(anyhow!(
            "{code} matches {} series at {}: fill in every dimension of the key",
            ids.len(),
            p.label
        ));
    }
    let mut freq = "";
    let mut obs = Vec::new();
    for r in &rows {
        let (Some(period), Some(value)) = (
            r.get("TIME_PERIOD").and_then(|t| util::parse_period(t)),
            r.get("OBS_VALUE").and_then(|v| v.trim().parse::<f64>().ok()),
        ) else {
            continue;
        };
        freq = period.1;
        obs.push((period.0, value));
    }
    let first = rows.first();
    let pick = |k: &str| first.and_then(|r| r.get(k)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let curated = util::find_entry(p.catalog, code);
    let meta = SeriesMeta {
        provider: p.id.into(),
        code: code.trim().to_string(),
        title: curated
            .map(|e| e.title.to_string())
            .or_else(|| pick("TITLE_COMPL").or_else(|| pick("TITLE")))
            .unwrap_or_else(|| code.trim().to_string()),
        category: curated.map(|e| e.category).unwrap_or("other").into(),
        country: curated
            .map(|e| e.country.to_string())
            .or_else(|| pick("REF_AREA").or_else(|| pick("geo")).or_else(|| pick("COUNTRY")))
            .unwrap_or_default(),
        unit: curated
            .map(|e| e.unit.to_string())
            .or_else(|| pick("UNIT").or_else(|| pick("UNIT_MEASURE")).or_else(|| pick("unit")))
            .unwrap_or_default(),
        frequency: freq.into(),
        seasonal_adj: false,
        notes: String::new(),
    };
    Ok((meta, util::tidy(obs)))
}

pub fn search(p: &Publisher, q: &str) -> Vec<SeriesMeta> {
    util::search_catalog(p.id, p.catalog, q)
}

/// The SDMX connectors: keyless, no settings.
const fn cap(id: &'static str, label: &'static str, website: &'static str, docs: &'static str, rate: &'static str) -> Capability {
    super::fcap(id, label, website, docs, rate, &[], &["macro"])
}

pub static ECB_CAP: Capability = cap(
    "ecb",
    "ECB Data Portal",
    "https://data.ecb.europa.eu",
    "https://data.ecb.europa.eu/help/api/overview",
    "Keyless, no published cap. One request per series refresh.",
);
pub static EUROSTAT_CAP: Capability = cap(
    "eurostat",
    "Eurostat",
    "https://ec.europa.eu/eurostat",
    "https://wikis.ec.europa.eu/display/EUROSTATHELP/API+SDMX+2.1",
    "Keyless, no published cap. Euro-area codes moved to EA21 in 2026; EA20 series stop in 2025.",
);
pub static BIS_CAP: Capability = cap(
    "bis",
    "BIS",
    "https://data.bis.org",
    "https://stats.bis.org/api-doc/v1/",
    "Keyless, no published cap. Central-bank policy rates for every major bank.",
);
pub static OECD_CAP: Capability = cap(
    "oecd",
    "OECD",
    "https://data-explorer.oecd.org",
    "https://www.oecd.org/en/data/insights/data-explainers/2024/09/api.html",
    "Keyless, rate-limited per IP (about 20 requests per minute): add series one at a time.",
);
pub static IMF_CAP: Capability = cap(
    "imf",
    "IMF",
    "https://data.imf.org",
    "https://portal.api.imf.org",
    "Keyless for SDMX 2.1 data. WEO series include the IMF's projections a few years ahead.",
);

pub struct Ecb;
pub struct Eurostat;
pub struct Bis;
pub struct Oecd;
pub struct Imf;

super::fundamentals_connector!(Ecb, ECB_CAP);
super::fundamentals_connector!(Eurostat, EUROSTAT_CAP);
super::fundamentals_connector!(Bis, BIS_CAP);
super::fundamentals_connector!(Oecd, OECD_CAP);
super::fundamentals_connector!(Imf, IMF_CAP);

// ── Central banks ───────────────────────────────────────────────────────────

/// Banks in the policy-rate table: (BIS area, name).
const BANKS: &[(&str, &str)] = &[
    ("US", "Federal Reserve"),
    ("XM", "European Central Bank"),
    ("GB", "Bank of England"),
    ("JP", "Bank of Japan"),
    ("CH", "Swiss National Bank"),
    ("CA", "Bank of Canada"),
    ("AU", "Reserve Bank of Australia"),
    ("CN", "People's Bank of China"),
];

/// Current policy rate of each bank, the last move and when it happened, from BIS daily
/// series (one request for all banks).
pub async fn central_banks(client: &reqwest::Client) -> Result<serde_json::Value> {
    let areas: Vec<&str> = BANKS.iter().map(|(a, _)| *a).collect();
    let key = format!("D.{}", areas.join("+"));
    let from = time::OffsetDateTime::now_utc().date() - time::Duration::days(3 * 365);
    let rows = rows(&BIS, client, "WS_CBPOL", &key, &format!("&startPeriod={from}")).await?;
    let mut out = Vec::new();
    for (area, name) in BANKS {
        let mut obs: Vec<(Date, f64)> = rows
            .iter()
            .filter(|r| r.get("REF_AREA").map(String::as_str) == Some(*area))
            .filter_map(|r| {
                Some((
                    util::parse_period(r.get("TIME_PERIOD")?)?.0,
                    r.get("OBS_VALUE")?.trim().parse::<f64>().ok()?,
                ))
            })
            .collect();
        obs.sort_by_key(|(d, _)| *d);
        let Some(&(as_of, rate)) = obs.last() else { continue };
        // The last day the rate differed from today's: the move happened the day after.
        let change = obs.iter().rev().find(|(_, v)| (*v - rate).abs() > 1e-9);
        let (last_change, last_change_date) = match change {
            Some((d, prev)) => {
                let moved = obs.iter().find(|(dd, _)| dd > d).map(|(dd, _)| *dd);
                (Some(rate - prev), moved.map(|d| d.to_string()))
            }
            None => (None, None),
        };
        out.push(serde_json::json!({
            "bank": name,
            "area": area,
            "rate": rate,
            "as_of": as_of.to_string(),
            "last_change": last_change,
            "last_change_date": last_change_date,
        }));
    }
    Ok(serde_json::Value::Array(out))
}
