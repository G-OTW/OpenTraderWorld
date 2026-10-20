//! Statistics offices with their own APIs: BLS, BEA, EIA, US Census (free keys), and the
//! keyless World Bank and CFTC. Each one takes the series code its own site shows.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::Date;

use super::util::{self, Entry};
use crate::histdata::{enc, Capability};
use otw_store::fundamentals::SeriesMeta;

fn meta(provider: &str, code: &str, catalog: &[Entry], title: String, country: &str, unit: String, freq: &str) -> SeriesMeta {
    let curated = util::find_entry(catalog, code);
    SeriesMeta {
        provider: provider.into(),
        code: code.into(),
        title: curated.map(|e| e.title.to_string()).unwrap_or(title),
        category: curated.map(|e| e.category).unwrap_or("other").into(),
        country: curated.map(|e| e.country).unwrap_or(country).into(),
        unit: curated.map(|e| e.unit.to_string()).unwrap_or(unit),
        frequency: freq.into(),
        seasonal_adj: false,
        notes: String::new(),
    }
}

// ── BLS ─────────────────────────────────────────────────────────────────────

pub mod bls {
    use super::*;

    pub const PROVIDER: &str = "bls";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "BLS",
        "https://www.bls.gov",
        "https://www.bls.gov/developers/",
        "Free key (data.bls.gov/registrationEngine): 500 queries per day, 20 years per query. \
         Series ids as bls.gov shows them (CUUR0000SA0).",
        &["api_key"],
        &["macro"],
    );
    pub struct Bls;
    super::super::fundamentals_connector!(Bls, CAP);

    pub const CATALOG: &[Entry] = &[
        Entry { code: "CUUR0000SA0", title: "CPI-U, all items (index, NSA)", category: "inflation", country: "US", unit: "Index 1982-84=100" },
        Entry { code: "CUSR0000SA0L1E", title: "Core CPI, all items less food and energy (SA)", category: "inflation", country: "US", unit: "Index 1982-84=100" },
        Entry { code: "LNS14000000", title: "Unemployment rate (SA)", category: "labour", country: "US", unit: "%" },
        Entry { code: "CES0000000001", title: "Nonfarm payrolls, level (SA)", category: "labour", country: "US", unit: "k" },
        Entry { code: "CES0500000003", title: "Average hourly earnings, private (SA)", category: "labour", country: "US", unit: "USD" },
        Entry { code: "JTS000000000000000JOR", title: "JOLTS job openings rate (SA)", category: "labour", country: "US", unit: "%" },
    ];

    pub async fn fetch(client: &reqwest::Client, secrets: &HashMap<String, String>, code: &str) -> Result<(SeriesMeta, Vec<(Date, f64)>)> {
        let key = util::need_key(secrets, "BLS", "data.bls.gov/registrationEngine")?;
        let year = time::OffsetDateTime::now_utc().year();
        let body = json!({
            "seriesid": [code],
            "startyear": (year - 19).to_string(),
            "endyear": year.to_string(),
            "registrationkey": key,
            "catalog": true,
        });
        let v = util::get_json(
            PROVIDER,
            client.post("https://api.bls.gov/publicAPI/v2/timeseries/data/").json(&body),
        )
        .await?;
        if v.get("status").and_then(Value::as_str) != Some("REQUEST_SUCCEEDED") {
            let msg = v.get("message").map(|m| m.to_string()).unwrap_or_default();
            return Err(anyhow!("BLS refused the request: {msg}"));
        }
        let series = v.pointer("/Results/series/0").ok_or_else(|| anyhow!("BLS has no series {code}"))?;
        let mut freq = "";
        let obs: Vec<(Date, f64)> = series
            .get("data")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        let year = util::text(r.get("year"));
                        let period = util::text(r.get("period"));
                        // M13 / Q05 are annual averages, not periods of the series.
                        let p = match period.as_bytes().first() {
                            Some(b'M') if period != "M13" => format!("{year}-{}", &period[1..]),
                            Some(b'Q') if period != "Q05" => format!("{year}-Q{}", period[1..].trim_start_matches('0')),
                            Some(b'A') => year.clone(),
                            _ => return None,
                        };
                        let (d, f) = util::parse_period(&p)?;
                        freq = f;
                        Some((d, util::num(r.get("value"))?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if obs.is_empty() {
            return Err(anyhow!("BLS has no data for {code}: check the series id"));
        }
        let title = util::text(series.pointer("/catalog/series_title"));
        Ok((meta(PROVIDER, code, CATALOG, if title.is_empty() { code.into() } else { title }, "US", String::new(), freq), util::tidy(obs)))
    }
}

// ── BEA ─────────────────────────────────────────────────────────────────────

pub mod bea {
    use super::*;

    pub const PROVIDER: &str = "bea";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "BEA",
        "https://www.bea.gov",
        "https://apps.bea.gov/api/signup/",
        "Free key (apps.bea.gov/api/signup): 100 requests and 100 MB per minute. Codes are \
         DATASET/TABLE/LINE/FREQ (NIPA/T10101/1/Q).",
        &["api_key"],
        &["macro"],
    );
    pub struct Bea;
    super::super::fundamentals_connector!(Bea, CAP);

    pub const CATALOG: &[Entry] = &[
        Entry { code: "NIPA/T10101/1/Q", title: "Real GDP, % change annualised", category: "growth", country: "US", unit: "%" },
        Entry { code: "NIPA/T10101/2/Q", title: "Real personal consumption, % change annualised", category: "growth", country: "US", unit: "%" },
        Entry { code: "NIPA/T20807/1/M", title: "PCE price index, % change m/m", category: "inflation", country: "US", unit: "%" },
    ];

    pub async fn fetch(client: &reqwest::Client, secrets: &HashMap<String, String>, code: &str) -> Result<(SeriesMeta, Vec<(Date, f64)>)> {
        let key = util::need_key(secrets, "BEA", "apps.bea.gov/api/signup")?;
        let parts: Vec<&str> = code.split('/').collect();
        let [dataset, table, line, freq] = parts.as_slice() else {
            return Err(anyhow!("a BEA code is DATASET/TABLE/LINE/FREQ, e.g. NIPA/T10101/1/Q"));
        };
        let url = format!(
            "https://apps.bea.gov/api/data/?UserID={key}&method=GetData&DataSetName={}&TableName={}\
             &Frequency={}&Year=ALL&ResultFormat=JSON",
            enc(dataset), enc(table), enc(freq)
        );
        let v = util::get_json(PROVIDER, client.get(&url)).await?;
        if let Some(err) = v.pointer("/BEAAPI/Results/Error").or_else(|| v.pointer("/BEAAPI/Error")) {
            let msg = util::text(err.get("APIErrorDescription"));
            return Err(anyhow!("BEA: {}", if msg.is_empty() { err.to_string() } else { msg }));
        }
        let rows = v.pointer("/BEAAPI/Results/Data").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut title = String::new();
        let mut unit = String::new();
        let mut f = "";
        let obs: Vec<(Date, f64)> = rows
            .iter()
            .filter(|r| util::text(r.get("LineNumber")) == *line)
            .filter_map(|r| {
                if title.is_empty() {
                    title = util::text(r.get("LineDescription"));
                    unit = util::text(r.get("CL_UNIT"));
                }
                let (d, fr) = util::parse_period(&util::text(r.get("TimePeriod")))?;
                f = fr;
                Some((d, util::num(r.get("DataValue"))?))
            })
            .collect();
        if obs.is_empty() {
            return Err(anyhow!("BEA has no line {line} in {dataset}/{table} at frequency {freq}"));
        }
        Ok((meta(PROVIDER, code, CATALOG, format!("{title} ({table})"), "US", unit, f), util::tidy(obs)))
    }
}

// ── EIA ─────────────────────────────────────────────────────────────────────

pub mod eia {
    use super::*;

    pub const PROVIDER: &str = "eia";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "EIA",
        "https://www.eia.gov",
        "https://www.eia.gov/opendata/",
        "Free key (eia.gov/opendata/register.php). Codes are the legacy series ids \
         (PET.WCESTUS1.W), served by the v2 API.",
        &["api_key"],
        &["macro"],
    );
    pub struct Eia;
    super::super::fundamentals_connector!(Eia, CAP);

    pub const CATALOG: &[Entry] = &[
        Entry { code: "PET.WCESTUS1.W", title: "US crude oil inventories, ex SPR", category: "energy", country: "US", unit: "k bbl" },
        Entry { code: "PET.WGTSTUS1.W", title: "US gasoline inventories", category: "energy", country: "US", unit: "k bbl" },
        Entry { code: "PET.RWTC.D", title: "WTI crude spot price", category: "energy", country: "US", unit: "USD/bbl" },
        Entry { code: "NG.RNGWHHD.D", title: "Henry Hub natural gas spot price", category: "energy", country: "US", unit: "USD/MMBtu" },
    ];

    pub async fn fetch(client: &reqwest::Client, secrets: &HashMap<String, String>, code: &str) -> Result<(SeriesMeta, Vec<(Date, f64)>)> {
        let key = util::need_key(secrets, "EIA", "eia.gov/opendata/register.php")?;
        let url = format!("https://api.eia.gov/v2/seriesid/{}?api_key={key}", enc(code));
        let v = util::get_json(PROVIDER, client.get(&url)).await?;
        if let Some(e) = v.get("error") {
            return Err(anyhow!("EIA: {}", util::text(Some(e))));
        }
        let rows = v.pointer("/response/data").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut f = "";
        let obs: Vec<(Date, f64)> = rows
            .iter()
            .filter_map(|r| {
                let (d, fr) = util::parse_period(&util::text(r.get("period")))?;
                f = fr;
                Some((d, util::num(r.get("value"))?))
            })
            .collect();
        if obs.is_empty() {
            return Err(anyhow!("EIA has no data for {code}"));
        }
        let first = rows.first();
        let title = util::text(first.and_then(|r| r.get("series-description")));
        let unit = util::text(first.and_then(|r| r.get("units")));
        Ok((meta(PROVIDER, code, CATALOG, if title.is_empty() { code.into() } else { title }, "US", unit, f), util::tidy(obs)))
    }
}

// ── US Census ───────────────────────────────────────────────────────────────

pub mod census {
    use super::*;

    pub const PROVIDER: &str = "census";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "US Census",
        "https://www.census.gov",
        "https://www.census.gov/data/developers/data-sets/economic-indicators.html",
        "Free key (api.census.gov/data/key_signup.html). Economic indicators: codes are \
         PROGRAM/CATEGORY/DATA_TYPE/SA (resconst/APERMITS/TOTAL/yes).",
        &["api_key"],
        &["macro"],
    );
    pub struct Census;
    super::super::fundamentals_connector!(Census, CAP);

    pub const CATALOG: &[Entry] = &[
        // Seasonally adjusted starts (STARTS/TOTAL/yes) answer empty or drop the connection
        // on the Census API (October 2026); FRED's HOUST carries them.
        Entry { code: "resconst/UNDERCONST/TOTAL/yes", title: "Housing units under construction (SA)", category: "housing", country: "US", unit: "k" },
        Entry { code: "resconst/APERMITS/TOTAL/yes", title: "Building permits (SAAR)", category: "housing", country: "US", unit: "k" },
        Entry { code: "marts/44X72/SM/yes", title: "Retail and food services sales (SA)", category: "growth", country: "US", unit: "USD m" },
    ];

    pub async fn fetch(client: &reqwest::Client, secrets: &HashMap<String, String>, code: &str) -> Result<(SeriesMeta, Vec<(Date, f64)>)> {
        let key = util::need_key(secrets, "US Census", "api.census.gov/data/key_signup.html")?;
        let parts: Vec<&str> = code.split('/').collect();
        let [program, category, data_type, sa] = parts.as_slice() else {
            return Err(anyhow!("a Census code is PROGRAM/CATEGORY/DATA_TYPE/SA, e.g. resconst/APERMITS/TOTAL/yes"));
        };
        let url = format!(
            "https://api.census.gov/data/timeseries/eits/{}?get=cell_value,time_slot_id\
             &for=us:*&time=from+2000&category_code={}&data_type_code={}&seasonally_adj={}&key={key}",
            enc(program), enc(category), enc(data_type), enc(sa)
        );
        let v = util::get_json(PROVIDER, client.get(&url)).await?;
        let rows = v.as_array().cloned().unwrap_or_default();
        let header: Vec<String> = rows.first().and_then(Value::as_array).map(|h| h.iter().map(|x| util::text(Some(x))).collect()).unwrap_or_default();
        let col = |n: &str| header.iter().position(|h| h == n);
        let (Some(vi), Some(ti)) = (col("cell_value"), col("time")) else {
            return Err(anyhow!("US Census has no data for {code}"));
        };
        let slot = col("time_slot_id");
        let mut f = "";
        let obs: Vec<(Date, f64)> = rows
            .iter()
            .skip(1)
            .filter_map(Value::as_array)
            // Several time slots can share a month; slot 0 is the headline figure.
            .filter(|r| slot.map_or(true, |s| matches!(util::text(r.get(s)).as_str(), "0" | "")))
            .filter_map(|r| {
                let (d, fr) = util::parse_period(&util::text(r.get(ti)))?;
                f = fr;
                Some((d, util::num(r.get(vi))?))
            })
            .collect();
        if obs.is_empty() {
            return Err(anyhow!("US Census has no data for {code}"));
        }
        Ok((meta(PROVIDER, code, CATALOG, code.into(), "US", String::new(), f), util::tidy(obs)))
    }
}

// ── World Bank ──────────────────────────────────────────────────────────────

pub mod worldbank {
    use super::*;

    pub const PROVIDER: &str = "worldbank";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "World Bank",
        "https://data.worldbank.org",
        "https://datahelpdesk.worldbank.org/knowledgebase/articles/889392",
        "Keyless, no published cap. Annual data; codes are COUNTRY/INDICATOR (USA/NY.GDP.MKTP.KD.ZG).",
        &[],
        &["macro"],
    );
    pub struct WorldBank;
    super::super::fundamentals_connector!(WorldBank, CAP);

    pub const CATALOG: &[Entry] = &[
        Entry { code: "WLD/NY.GDP.MKTP.KD.ZG", title: "World GDP growth", category: "growth", country: "World", unit: "%" },
        Entry { code: "USA/NY.GDP.MKTP.KD.ZG", title: "US GDP growth", category: "growth", country: "US", unit: "%" },
        Entry { code: "CHN/NY.GDP.MKTP.KD.ZG", title: "China GDP growth", category: "growth", country: "CN", unit: "%" },
        Entry { code: "EMU/FP.CPI.TOTL.ZG", title: "Euro area inflation, consumer prices", category: "inflation", country: "EA", unit: "%" },
        Entry { code: "USA/GC.DOD.TOTL.GD.ZS", title: "US central government debt, % of GDP", category: "fiscal", country: "US", unit: "%" },
    ];

    pub async fn fetch(client: &reqwest::Client, code: &str) -> Result<(SeriesMeta, Vec<(Date, f64)>)> {
        let (country, indicator) = code
            .split_once('/')
            .ok_or_else(|| anyhow!("a World Bank code is COUNTRY/INDICATOR, e.g. USA/NY.GDP.MKTP.KD.ZG"))?;
        let url = format!(
            "https://api.worldbank.org/v2/country/{}/indicator/{}?format=json&per_page=20000",
            enc(country), enc(indicator)
        );
        let v = util::get_json(PROVIDER, client.get(&url)).await?;
        if let Some(msg) = v.pointer("/0/message/0/value") {
            return Err(anyhow!("World Bank: {}", util::text(Some(msg))));
        }
        let rows = v.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
        let obs: Vec<(Date, f64)> = rows
            .iter()
            .filter_map(|r| Some((util::parse_period(&util::text(r.get("date")))?.0, util::num(r.get("value"))?)))
            .collect();
        if obs.is_empty() {
            return Err(anyhow!("World Bank has no data for {code}"));
        }
        let first = rows.first();
        let title = format!(
            "{}, {}",
            util::text(first.and_then(|r| r.pointer("/indicator/value"))),
            util::text(first.and_then(|r| r.pointer("/country/value")))
        );
        Ok((meta(PROVIDER, code, CATALOG, title, country, String::new(), "A"), util::tidy(obs)))
    }
}

// ── CFTC ────────────────────────────────────────────────────────────────────

pub mod cftc {
    use super::*;

    pub const PROVIDER: &str = "cftc";
    const LEGACY: &str = "https://publicreporting.cftc.gov/resource/6dca-aqww.json";
    pub static CAP: Capability = super::super::fcap(
        PROVIDER,
        "CFTC",
        "https://publicreporting.cftc.gov",
        "https://publicreporting.cftc.gov/stories/s/r4w3-av2u",
        "Keyless (Socrata), weekly release on Friday for Tuesday positions. A code is the \
         contract market code (13874A = E-mini S&P 500); the series is the non-commercial net.",
        &[],
        &["cot"],
    );
    pub struct Cftc;
    super::super::fundamentals_connector!(Cftc, CAP);

    pub const CATALOG: &[Entry] = &[
        Entry { code: "13874A", title: "COT: E-mini S&P 500, non-commercial net", category: "positioning", country: "US", unit: "contracts" },
        Entry { code: "209742", title: "COT: Nasdaq mini, non-commercial net", category: "positioning", country: "US", unit: "contracts" },
        Entry { code: "043602", title: "COT: 10-year T-note, non-commercial net", category: "positioning", country: "US", unit: "contracts" },
        Entry { code: "067651", title: "COT: WTI crude, non-commercial net", category: "positioning", country: "US", unit: "contracts" },
        Entry { code: "088691", title: "COT: Gold, non-commercial net", category: "positioning", country: "US", unit: "contracts" },
        Entry { code: "099741", title: "COT: Euro FX, non-commercial net", category: "positioning", country: "US", unit: "contracts" },
        Entry { code: "133741", title: "COT: Bitcoin (CME), non-commercial net", category: "positioning", country: "US", unit: "contracts" },
    ];

    fn soql(params: &[(&str, String)]) -> String {
        params.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&")
    }

    /// Contract markets whose name contains `q`, most recently reported first.
    pub async fn search(client: &reqwest::Client, q: &str) -> Result<Vec<SeriesMeta>> {
        if q.trim().len() < 2 {
            return Ok(util::search_catalog(PROVIDER, CATALOG, q));
        }
        let needle = q.trim().to_uppercase().replace('\'', "''");
        let url = format!(
            "{LEGACY}?{}",
            soql(&[
                ("$select", "cftc_contract_market_code,market_and_exchange_names,max(report_date_as_yyyy_mm_dd) as last".into()),
                ("$where", format!("upper(market_and_exchange_names) like '%{needle}%'")),
                ("$group", "cftc_contract_market_code,market_and_exchange_names".into()),
                ("$order", "last DESC".into()),
                ("$limit", "25".into()),
            ])
        );
        let v = util::get_json(PROVIDER, client.get(&url)).await?;
        Ok(v.as_array()
            .map(|rows| {
                rows.iter()
                    .map(|r| SeriesMeta {
                        provider: PROVIDER.into(),
                        code: util::text(r.get("cftc_contract_market_code")),
                        title: format!("COT: {}, non-commercial net", util::text(r.get("market_and_exchange_names"))),
                        category: "positioning".into(),
                        country: "US".into(),
                        unit: "contracts".into(),
                        frequency: "W".into(),
                        seasonal_adj: false,
                        notes: format!("last report {}", util::text(r.get("last")).get(..10).unwrap_or("")),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn fetch(client: &reqwest::Client, code: &str) -> Result<(SeriesMeta, Vec<(Date, f64)>)> {
        let code = code.trim();
        let url = format!(
            "{LEGACY}?{}",
            soql(&[
                ("$select", "report_date_as_yyyy_mm_dd,market_and_exchange_names,noncomm_positions_long_all,noncomm_positions_short_all".into()),
                ("cftc_contract_market_code", code.into()),
                ("$order", "report_date_as_yyyy_mm_dd ASC".into()),
                ("$limit", "5000".into()),
            ])
        );
        let v = util::get_json(PROVIDER, client.get(&url)).await?;
        let rows = v.as_array().cloned().unwrap_or_default();
        let obs: Vec<(Date, f64)> = rows
            .iter()
            .filter_map(|r| {
                Some((
                    util::date(r.get("report_date_as_yyyy_mm_dd"))?,
                    util::num(r.get("noncomm_positions_long_all"))? - util::num(r.get("noncomm_positions_short_all"))?,
                ))
            })
            .collect();
        if obs.is_empty() {
            return Err(anyhow!("CFTC has no legacy COT report for contract market code {code}"));
        }
        let name = util::text(rows.last().and_then(|r| r.get("market_and_exchange_names")));
        let m = meta(PROVIDER, code, CATALOG, format!("COT: {name}, non-commercial net"), "US", "contracts".into(), "W");
        Ok((SeriesMeta { category: "positioning".into(), ..m }, util::tidy(obs)))
    }
}

