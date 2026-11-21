//! Demo fixtures for Fundamentals, loaded by `--seed-demo` (see [`crate::demo_seed`]).
//!
//! The sandbox grants Fundamentals no provider: every lookup would spend a key or reach a
//! public source on a visitor's behalf. Without this the module opens empty. Here each page
//! gets something to show: a few macro series, the yield curve and policy rates, two
//! companies (statements, filings, insiders, a call transcript and every aggregator
//! dataset), two ETFs, the market calendar and alternative data.
//!
//! The figures are made up: deterministic, shaped like the real thing, never fetched. The
//! snapshots carry the `demo` provider, so each data note says "Demo data", and the module
//! pages show a notice in demo mode. People and analyst firms are fictional. Dates are
//! relative to the seed run.

use anyhow::Result;
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use time::{Date, Duration, Month, OffsetDateTime, Weekday};

use crate::fundamentals::{datasets, date_ms};
use otw_store::fundamentals::{self as store, CompanyInput, DocumentInput, InsiderTrade, Segment, SeriesMeta, Statement};

pub async fn seed(pool: &PgPool) -> Result<()> {
    let today = OffsetDateTime::now_utc().date();
    series(pool, today).await?;
    market(pool, today).await?;
    for co in COMPANIES {
        company(pool, co, today).await?;
    }
    etfs(pool, today).await?;
    congress(pool, today).await?;
    println!("fundamentals: demo data seeded");
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Deterministic noise in [-1, 1] (splitmix64), so a reseed draws the same shapes.
fn noise(seed: u64, i: u64) -> f64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ i.wrapping_add(1).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

fn seed_of(s: &str) -> u64 {
    s.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
}

/// Fractional year, the x axis of every knot list.
fn yf(d: Date) -> f64 {
    d.year() as f64 + (d.ordinal() - 1) as f64 / 365.25
}

/// Piecewise-linear through `(year, value)` knots, flat beyond the ends.
fn interp(k: &[(f64, f64)], x: f64) -> f64 {
    if x <= k[0].0 {
        return k[0].1;
    }
    for w in k.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    k[k.len() - 1].1
}

fn round(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p).round() / p
}

fn ymd(y: i32, m: u8, d: u8) -> Date {
    Date::from_calendar_date(y, Month::try_from(m).expect("month"), d).expect("date")
}

/// First of the month `n` months after the month of `d`.
fn add_months(d: Date, n: i32) -> Date {
    let total = d.year() * 12 + d.month() as i32 - 1 + n;
    ymd(total.div_euclid(12), (total.rem_euclid(12) + 1) as u8, 1)
}

fn month_start(d: Date) -> Date {
    ymd(d.year(), d.month() as u8, 1)
}

fn month_end(y: i32, m: u8) -> Date {
    add_months(ymd(y, m, 1), 1) - Duration::days(1)
}

/// The day itself, or the Monday after a weekend.
fn biz(d: Date) -> Date {
    match d.weekday() {
        Weekday::Saturday => d + Duration::days(2),
        Weekday::Sunday => d + Duration::days(1),
        _ => d,
    }
}

/// The day itself, or the Friday before a weekend.
fn biz_back(d: Date) -> Date {
    match d.weekday() {
        Weekday::Saturday => d - Duration::days(1),
        Weekday::Sunday => d - Duration::days(2),
        _ => d,
    }
}

fn quarter_start(d: Date) -> Date {
    ymd(d.year(), (d.month() as u8 - 1) / 3 * 3 + 1, 1)
}

fn day(d: Date) -> String {
    d.to_string()
}

/// Quarterly `[[ms, value]]` over the last `n` quarters, oldest first.
fn quarterly(today: Date, n: i32, f: impl Fn(usize, Date) -> f64) -> Value {
    let last = add_months(quarter_start(today), -3);
    let pts: Vec<(i64, f64)> = (0..n)
        .map(|i| {
            let q = add_months(last, -3 * (n - 1 - i));
            (date_ms(q), f(i as usize, q))
        })
        .collect();
    json!(pts)
}

async fn snap(pool: &PgPool, subject: &str, dataset: &str, data: Value) -> Result<()> {
    store::put_snapshot(pool, subject, dataset, datasets::DEMO, &data).await
}

// ── Macro series ────────────────────────────────────────────────────────────

enum Gen {
    /// Knots are the level, noise is absolute.
    Level,
    /// Knots are the level, noise is relative.
    Rel,
    /// Knots are the level plus a mean-reverting walk (absolute step).
    Walk,
    /// Knots are the year-on-year rate in %, integrated into an index from this start.
    Index(f64),
}

struct SeriesSpec {
    provider: &'static str,
    code: &'static str,
    title: &'static str,
    category: &'static str,
    country: &'static str,
    unit: &'static str,
    freq: &'static str,
    sa: bool,
    from: (i32, u8),
    gen: Gen,
    noise: f64,
    digits: i32,
    knots: &'static [(f64, f64)],
}

const SERIES: &[SeriesSpec] = &[
    SeriesSpec {
        provider: "fred", code: "CPIAUCSL",
        title: "Consumer Price Index for All Urban Consumers: All Items in U.S. City Average",
        category: "inflation", country: "US", unit: "Index 1982-1984=100", freq: "M", sa: true,
        from: (2006, 1), gen: Gen::Index(198.3), noise: 0.0008, digits: 3,
        knots: &[(2006.0, 3.4), (2007.5, 2.4), (2008.5, 5.4), (2009.5, -1.6), (2011.7, 3.8), (2013.0, 1.6),
            (2015.2, -0.1), (2017.0, 2.4), (2019.0, 1.8), (2020.4, 0.2), (2021.5, 5.4), (2022.45, 9.0),
            (2023.5, 3.1), (2024.5, 2.9), (2025.5, 2.6), (2026.8, 2.9)],
    },
    SeriesSpec {
        provider: "fred", code: "FEDFUNDS", title: "Federal Funds Effective Rate",
        category: "rates", country: "US", unit: "Percent", freq: "M", sa: false,
        from: (2006, 1), gen: Gen::Level, noise: 0.01, digits: 2,
        knots: &[(2006.0, 4.3), (2006.5, 5.25), (2007.6, 5.25), (2008.0, 3.9), (2008.9, 0.5), (2009.0, 0.15),
            (2015.9, 0.12), (2016.0, 0.36), (2017.0, 0.66), (2018.0, 1.41), (2019.5, 2.4), (2019.9, 1.55),
            (2020.2, 0.65), (2020.3, 0.06), (2022.1, 0.08), (2022.3, 0.33), (2023.6, 5.33), (2024.7, 5.33),
            (2024.75, 5.13), (2025.0, 4.33), (2025.7, 4.33), (2025.8, 4.09), (2026.0, 3.88), (2026.8, 3.63)],
    },
    SeriesSpec {
        provider: "fred", code: "DGS10",
        title: "Market Yield on U.S. Treasury Securities at 10-Year Constant Maturity",
        category: "rates", country: "US", unit: "Percent", freq: "D", sa: false,
        from: (2016, 1), gen: Gen::Walk, noise: 0.035, digits: 2,
        knots: &[(2016.0, 2.2), (2016.5, 1.45), (2017.0, 2.45), (2018.8, 3.15), (2019.7, 1.6), (2020.2, 0.7),
            (2020.6, 0.55), (2021.2, 1.6), (2022.0, 1.7), (2022.8, 4.0), (2023.8, 4.9), (2024.0, 3.9),
            (2024.3, 4.6), (2024.7, 3.7), (2025.0, 4.6), (2025.5, 4.3), (2026.0, 4.1), (2026.8, 4.15)],
    },
    SeriesSpec {
        provider: "fred", code: "UNRATE", title: "Unemployment Rate",
        category: "labour", country: "US", unit: "Percent", freq: "M", sa: true,
        from: (2006, 1), gen: Gen::Level, noise: 0.06, digits: 1,
        knots: &[(2006.0, 4.7), (2007.0, 4.5), (2008.0, 5.0), (2009.0, 7.8), (2009.8, 10.0), (2011.0, 9.0),
            (2013.0, 7.5), (2015.0, 5.6), (2017.0, 4.6), (2019.0, 3.8), (2020.15, 3.5), (2020.25, 14.7),
            (2020.5, 10.2), (2021.0, 6.4), (2022.0, 3.9), (2023.2, 3.5), (2024.0, 3.8), (2024.6, 4.2),
            (2025.5, 4.1), (2026.8, 4.4)],
    },
    SeriesSpec {
        provider: "fred", code: "ICSA", title: "Initial Claims",
        category: "labour", country: "US", unit: "Number", freq: "W", sa: true,
        from: (2016, 1), gen: Gen::Rel, noise: 0.035, digits: 0,
        knots: &[(2016.0, 275e3), (2018.0, 220e3), (2019.0, 215e3), (2020.18, 220e3), (2020.22, 6.1e6),
            (2020.3, 3.0e6), (2020.5, 1.4e6), (2021.0, 850e3), (2021.5, 400e3), (2022.0, 230e3),
            (2022.2, 175e3), (2023.0, 210e3), (2024.0, 215e3), (2025.0, 225e3), (2026.8, 235e3)],
    },
    SeriesSpec {
        provider: "fred", code: "A191RL1Q225SBEA", title: "Real Gross Domestic Product",
        category: "growth", country: "US", unit: "Percent Change from Preceding Period, SAAR", freq: "Q", sa: true,
        from: (2006, 1), gen: Gen::Level, noise: 0.9, digits: 1,
        knots: &[(2006.0, 2.6), (2007.5, 2.3), (2008.5, -2.0), (2008.8, -8.5), (2009.25, -4.5), (2009.6, 1.5),
            (2010.0, 3.5), (2012.0, 1.8), (2015.0, 2.4), (2018.0, 2.8), (2019.9, 2.2), (2020.0, -5.5),
            (2020.25, -28.0), (2020.5, 34.8), (2020.75, 4.2), (2021.0, 6.3), (2022.0, 0.5), (2023.0, 3.0),
            (2024.0, 2.6), (2025.0, 1.8), (2026.0, 2.1)],
    },
    SeriesSpec {
        provider: "fred", code: "UMCSENT", title: "University of Michigan: Consumer Sentiment",
        category: "surveys", country: "US", unit: "Index 1966:Q1=100", freq: "M", sa: false,
        from: (2006, 1), gen: Gen::Level, noise: 1.8, digits: 1,
        knots: &[(2006.0, 88.0), (2008.4, 57.0), (2009.0, 62.0), (2011.6, 56.0), (2013.0, 76.0), (2015.0, 95.0),
            (2018.0, 99.0), (2020.0, 100.0), (2020.3, 72.0), (2021.0, 80.0), (2021.6, 70.0), (2022.45, 50.0),
            (2023.0, 64.0), (2024.0, 77.0), (2024.5, 68.0), (2025.3, 52.0), (2025.8, 55.0), (2026.8, 61.0)],
    },
    SeriesSpec {
        provider: "fred", code: "HOUST", title: "New Privately-Owned Housing Units Started: Total Units",
        category: "housing", country: "US", unit: "Thousands of Units, SAAR", freq: "M", sa: true,
        from: (2006, 1), gen: Gen::Rel, noise: 0.04, digits: 0,
        knots: &[(2006.0, 2200.0), (2007.0, 1400.0), (2009.3, 480.0), (2011.0, 600.0), (2013.0, 900.0),
            (2016.0, 1150.0), (2019.0, 1300.0), (2020.3, 950.0), (2021.0, 1600.0), (2022.3, 1800.0),
            (2023.0, 1400.0), (2024.0, 1350.0), (2025.0, 1300.0), (2026.8, 1330.0)],
    },
    SeriesSpec {
        provider: "ecb", code: "HICP/M.U2.N.000000.4D0.ANR",
        title: "HICP - Overall index, Euro area, annual rate of change",
        category: "inflation", country: "EA", unit: "Percent", freq: "M", sa: false,
        from: (2006, 1), gen: Gen::Level, noise: 0.08, digits: 1,
        knots: &[(2006.0, 2.2), (2008.5, 4.0), (2009.5, -0.6), (2011.8, 3.0), (2013.5, 1.4), (2015.0, -0.6),
            (2017.0, 1.8), (2019.0, 1.2), (2020.8, -0.3), (2021.9, 5.0), (2022.8, 10.6), (2023.5, 5.5),
            (2023.9, 2.4), (2024.7, 1.8), (2025.5, 2.0), (2026.8, 2.1)],
    },
];

/// Period starts of a series up to the last one a publisher would have out by `today`.
fn periods(freq: &str, from: Date, today: Date) -> Vec<Date> {
    let mut out = Vec::new();
    match freq {
        "M" | "Q" => {
            let step = if freq == "M" { 1 } else { 3 };
            let last = add_months(month_start(today), if freq == "M" { -1 } else { -4 });
            let mut d = from;
            while d <= last {
                out.push(d);
                d = add_months(d, step);
            }
        }
        "W" => {
            let mut d = from;
            while d.weekday() != Weekday::Saturday {
                d += Duration::days(1);
            }
            while d <= today - Duration::days(5) {
                out.push(d);
                d += Duration::days(7);
            }
        }
        _ => {
            let mut d = from;
            while d < today {
                if !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday) {
                    out.push(d);
                }
                d += Duration::days(1);
            }
        }
    }
    out
}

fn values(s: &SeriesSpec, dates: &[Date]) -> Vec<f64> {
    let seed = seed_of(s.code);
    let per_year = match s.freq {
        "D" => 252.0,
        "W" => 52.0,
        "M" => 12.0,
        "Q" => 4.0,
        _ => 1.0,
    };
    let mut level = if let Gen::Index(start) = s.gen { start } else { 0.0 };
    let mut walk = 0.0;
    dates
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let base = interp(s.knots, yf(*d));
            let n = noise(seed, i as u64);
            let v = match s.gen {
                Gen::Level => base + n * s.noise,
                Gen::Rel => base * (1.0 + n * s.noise),
                Gen::Walk => {
                    walk = 0.97 * walk + n * s.noise;
                    base + walk
                }
                Gen::Index(_) => {
                    level *= (1.0 + base / 100.0).powf(1.0 / per_year) * (1.0 + n * s.noise);
                    level
                }
            };
            round(v, s.digits)
        })
        .collect()
}

async fn series(pool: &PgPool, today: Date) -> Result<()> {
    for s in SERIES {
        let dates = periods(s.freq, ymd(s.from.0, s.from.1, 1), today);
        let obs: Vec<(Date, f64)> = dates.iter().copied().zip(values(s, &dates)).collect();
        let id = store::upsert_series(
            pool,
            &SeriesMeta {
                provider: s.provider.into(),
                code: s.code.into(),
                title: s.title.into(),
                category: s.category.into(),
                country: s.country.into(),
                unit: s.unit.into(),
                frequency: s.freq.into(),
                seasonal_adj: s.sa,
                notes: "Illustrative demo data, not the published series.".into(),
            },
        )
        .await?;
        store::replace_observations(pool, id, &obs).await?;
    }
    Ok(())
}

// ── Market-wide: yield curve, central banks, calendar ───────────────────────

async fn market(pool: &PgPool, today: Date) -> Result<()> {
    let last = biz_back(today - Duration::days(1));
    snap(
        pool,
        "_",
        "yield_curve",
        json!({
            "as_of": day(last),
            "tenors": ["1M", "3M", "6M", "1Y", "2Y", "3Y", "5Y", "7Y", "10Y", "20Y", "30Y"],
            "curves": [
                { "label": "today", "date": day(last),
                  "values": [3.92, 3.86, 3.74, 3.58, 3.49, 3.47, 3.55, 3.72, 4.08, 4.58, 4.62] },
                { "label": "1m", "date": day(biz_back(last - Duration::days(30))),
                  "values": [4.02, 3.97, 3.85, 3.66, 3.55, 3.52, 3.60, 3.78, 4.15, 4.66, 4.71] },
                { "label": "1y", "date": day(biz_back(last - Duration::days(365))),
                  "values": [4.85, 4.62, 4.41, 4.10, 3.92, 3.88, 3.90, 4.00, 4.18, 4.55, 4.48] },
            ],
        }),
    )
    .await?;

    // (bank, BIS area, rate, last move, days since the move)
    let banks: &[(&str, &str, f64, f64, i64)] = &[
        ("Federal Reserve", "US", 3.625, -0.25, 27),
        ("European Central Bank", "XM", 2.0, -0.25, 480),
        ("Bank of England", "GB", 3.75, -0.25, 62),
        ("Bank of Japan", "JP", 0.75, 0.25, 95),
        ("Swiss National Bank", "CH", 0.0, -0.25, 200),
        ("Bank of Canada", "CA", 2.25, -0.25, 40),
        ("Reserve Bank of Australia", "AU", 3.35, -0.25, 110),
        ("People's Bank of China", "CN", 3.0, -0.1, 140),
    ];
    let rows: Vec<Value> = banks
        .iter()
        .map(|(bank, area, rate, change, ago)| {
            json!({
                "bank": bank, "area": area, "rate": rate, "as_of": day(last),
                "last_change": change, "last_change_date": day(biz(today - Duration::days(*ago))),
            })
        })
        .collect();
    snap(pool, "_", "central_banks", Value::Array(rows)).await?;

    // (ticker, EPS estimate, revenue estimate, before the open)
    let reports: &[(&str, f64, f64, bool)] = &[
        ("JPM", 4.85, 42.1e9, true), ("WFC", 1.55, 21.0e9, true), ("C", 1.90, 21.3e9, true),
        ("GS", 10.95, 14.2e9, true), ("BAC", 0.95, 27.4e9, true), ("MS", 2.05, 16.8e9, true),
        ("JNJ", 2.80, 23.9e9, true), ("UNH", 6.10, 111.0e9, true), ("NFLX", 6.95, 11.5e9, false),
        ("TSLA", 0.55, 26.8e9, false), ("IBM", 2.45, 16.4e9, false), ("KO", 0.80, 12.5e9, true),
        ("PG", 1.95, 22.4e9, true), ("VZ", 1.20, 34.1e9, true), ("T", 0.58, 30.9e9, true),
        ("INTC", 0.12, 13.4e9, false), ("GE", 1.40, 11.0e9, true), ("LMT", 6.55, 18.6e9, true),
        ("ISRG", 2.10, 2.4e9, false), ("TXN", 1.50, 4.6e9, false),
    ];
    let mut d = biz(today + Duration::days(1));
    let earnings: Vec<Value> = reports
        .iter()
        .enumerate()
        .map(|(i, (t, eps, rev, bmo))| {
            if i > 0 && i % 2 == 0 {
                d = biz(d + Duration::days(1));
            }
            json!({ "date": day(d), "ticker": t, "time": if *bmo { "BMO" } else { "AMC" },
                    "eps_estimate": eps, "revenue_estimate": rev })
        })
        .collect();
    let in_days = |n: i64| day(biz(today + Duration::days(n)));
    snap(
        pool,
        "_",
        "calendar",
        json!({
            "earnings": earnings,
            "ipos": [
                { "date": in_days(6), "company": "Northwind Robotics Inc.", "ticker": "NWRB", "exchange": "NASDAQ", "range": "18.00-20.00", "size": 2.1e9 },
                { "date": in_days(11), "company": "Cobalt Ridge Energy Corp.", "ticker": "CBRG", "exchange": "NYSE", "range": "24.00-26.00", "size": 3.4e9 },
                { "date": in_days(19), "company": "Lumen Harbor Health Inc.", "ticker": "LMHB", "exchange": "NASDAQ", "range": "14.00-16.00", "size": 0.9e9 },
            ],
            "corporate": [
                { "date": in_days(8), "ticker": "NWTC", "type": "Stock split", "detail": "3:1" },
                { "date": in_days(15), "ticker": "PLXR", "type": "Stock split", "detail": "1:10" },
            ],
        }),
    )
    .await
}

// ── Companies ───────────────────────────────────────────────────────────────

struct Co {
    ticker: &'static str,
    cik: &'static str,
    name: &'static str,
    /// The name key the public alternative-data sources match on.
    registered: &'static str,
    exchange: &'static str,
    sector: &'static str,
    industry: &'static str,
    sic: &'static str,
    fye_month: u8,
    website: &'static str,
    about: &'static str,
    shares: f64,
    /// Yearly change of the share count (buybacks shrink it).
    share_drift: f64,
    /// Revenue of the latest reported fiscal year, and its yearly growth.
    revenue: f64,
    growth: f64,
    gross: f64,
    rnd: f64,
    sga: f64,
    tax: f64,
    dep: f64,
    capex: f64,
    buyback: f64,
    ppe: f64,
    goodwill: f64,
    debt: f64,
    /// Revenue weight of fiscal Q1..Q4.
    season: [f64; 4],
    /// Latest quarterly dividend per share.
    dps: f64,
    /// Close used when the seed downloaded no bars for it.
    price: f64,
    /// Average daily volume and short interest as % of shares.
    adv: f64,
    short_pct: f64,
    /// (name, weight, yearly drift of the weight)
    products: &'static [(&'static str, f64, f64)],
    regions: &'static [(&'static str, f64, f64)],
    /// (name, latest value, yearly growth)
    kpis: &'static [(&'static str, f64, f64)],
    peers: &'static [(&'static str, &'static str, f64)],
    splits: &'static [(&'static str, &'static str)],
    /// Fictional management: (name, title, pay).
    people: &'static [(&'static str, &'static str, f64)],
    esg: [f64; 4],
    institutions: f64,
    lobbying: f64,
    contracts: f64,
    patents: f64,
}

const COMPANIES: &[Co] = &[
    Co {
        ticker: "AAPL", cik: "0000320193", name: "Apple Inc.", registered: "APPLE INC",
        exchange: "Nasdaq", sector: "Technology", industry: "Consumer Electronics", sic: "3571",
        fye_month: 9, website: "https://www.apple.com",
        about: "Designs smartphones, personal computers, tablets, wearables and accessories, and sells \
                related services. Figures shown in this demo are illustrative, not the company's reported numbers.",
        shares: 14.75e9, share_drift: -0.025, revenue: 425e9, growth: 0.06,
        gross: 0.465, rnd: 0.08, sga: 0.062, tax: 0.16, dep: 0.027, capex: 0.03, buyback: 0.22,
        ppe: 0.12, goodwill: 0.0, debt: 0.2, season: [0.295, 0.235, 0.22, 0.25], dps: 0.27, price: 255.0,
        adv: 55e6, short_pct: 0.75,
        products: &[("iPhone", 0.50, -0.01), ("Services", 0.27, 0.06), ("Mac", 0.08, 0.0), ("Wearables, Home and Accessories", 0.08, -0.04), ("iPad", 0.07, 0.0)],
        regions: &[("Americas", 0.43, 0.0), ("Europe", 0.26, 0.01), ("Greater China", 0.16, -0.04), ("Rest of Asia Pacific", 0.08, 0.03), ("Japan", 0.07, 0.0)],
        kpis: &[("Paid subscriptions (M)", 1150.0, 0.09), ("Active installed base (B devices)", 2.45, 0.05), ("Services gross margin (%)", 75.2, 0.01)],
        peers: &[("MSFT", "Microsoft Corporation", 3.8e12), ("GOOGL", "Alphabet Inc.", 2.9e12), ("AMZN", "Amazon.com, Inc.", 2.4e12), ("META", "Meta Platforms, Inc.", 1.8e12), ("DELL", "Dell Technologies Inc.", 9.1e10), ("HPQ", "HP Inc.", 2.8e10)],
        splits: &[("2020-08-31", "4:1"), ("2014-06-09", "7:1"), ("2005-02-28", "2:1")],
        people: &[("Jordan Ellis", "Chief Executive Officer", 74.6e6), ("Priya Natarajan", "Chief Financial Officer", 27.1e6), ("Marcus Webb", "Chief Operating Officer", 26.9e6), ("Samuel Okafor", "General Counsel", 26.8e6), ("Helena Brandt", "SVP Retail", 25.4e6)],
        esg: [61.8, 58.9, 64.2, 62.3], institutions: 61.5,
        lobbying: 2.3e6, contracts: 14e6, patents: 640.0,
    },
    Co {
        ticker: "MSFT", cik: "0000789019", name: "Microsoft Corporation", registered: "MICROSOFT CORPORATION",
        exchange: "Nasdaq", sector: "Technology", industry: "Software - Infrastructure", sic: "7372",
        fye_month: 6, website: "https://www.microsoft.com",
        about: "Develops software, cloud infrastructure and services, devices and gaming. Figures shown \
                in this demo are illustrative, not the company's reported numbers.",
        shares: 7.43e9, share_drift: -0.004, revenue: 305e9, growth: 0.14,
        gross: 0.69, rnd: 0.105, sga: 0.12, tax: 0.18, dep: 0.11, capex: 0.27, buyback: 0.06,
        ppe: 0.75, goodwill: 0.4, debt: 0.13, season: [0.24, 0.245, 0.25, 0.265], dps: 0.91, price: 515.0,
        adv: 21e6, short_pct: 0.62,
        products: &[("Server products and cloud services", 0.36, 0.06), ("Microsoft 365 commercial", 0.27, 0.0), ("Gaming", 0.08, -0.02), ("LinkedIn", 0.06, -0.02), ("Windows and devices", 0.06, -0.06), ("Search and news advertising", 0.05, -0.02), ("Other", 0.12, -0.03)],
        regions: &[("United States", 0.51, 0.0), ("Other countries", 0.49, 0.0)],
        kpis: &[("Microsoft 365 consumer subscribers (M)", 89.0, 0.07), ("Commercial bookings growth (%)", 17.0, -0.06), ("LinkedIn members (M)", 1150.0, 0.06)],
        peers: &[("AAPL", "Apple Inc.", 3.7e12), ("GOOGL", "Alphabet Inc.", 2.9e12), ("AMZN", "Amazon.com, Inc.", 2.4e12), ("ORCL", "Oracle Corporation", 6.2e11), ("CRM", "Salesforce, Inc.", 2.4e11), ("ADBE", "Adobe Inc.", 1.5e11)],
        splits: &[("2003-02-18", "2:1"), ("1999-03-29", "2:1")],
        people: &[("Rafael Moreno", "Chief Executive Officer", 79.1e6), ("Claire Donovan", "Chief Financial Officer", 25.3e6), ("Victor Lindqvist", "President, Commercial Business", 23.9e6), ("Aisha Rahman", "Chief Legal Officer", 19.8e6), ("Tomasz Nowak", "EVP Cloud and AI", 18.7e6)],
        esg: [66.4, 69.1, 63.7, 66.5], institutions: 72.8,
        lobbying: 2.6e6, contracts: 95e6, patents: 610.0,
    },
];

/// Fiscal calendar: quarter `k` (1..4) of fiscal year `fy`, as (start, end).
fn fiscal_quarter(co: &Co, fy: i32, k: i32) -> (Date, Date) {
    let fy_start = add_months(month_end(fy - 1, co.fye_month), 1);
    let start = add_months(fy_start, 3 * (k - 1));
    (start, add_months(start, 3) - Duration::days(1))
}

/// When a quarter's results come out.
fn report_date(end: Date) -> Date {
    biz(end + Duration::days(30))
}

fn reported(end: Date, today: Date) -> bool {
    report_date(end) < today
}

/// The latest fiscal year whose fourth quarter is out.
fn latest_fy(co: &Co, today: Date) -> i32 {
    let mut fy = today.year() + 1;
    while !reported(fiscal_quarter(co, fy, 4).1, today) {
        fy -= 1;
    }
    fy
}

/// The model every figure of a company is drawn from, so statements, earnings, estimates
/// and segments agree with each other.
struct Model<'a> {
    co: &'a Co,
    latest: i32,
    seed: u64,
}

impl Model<'_> {
    fn annual_revenue(&self, fy: i32) -> f64 {
        self.co.revenue * (1.0 + self.co.growth).powi(fy - self.latest) * (1.0 + 0.015 * noise(self.seed, fy as u64))
    }

    fn shares(&self, fy: i32) -> f64 {
        self.co.shares * (1.0 + self.co.share_drift).powi(fy - self.latest)
    }

    fn dps(&self, fy: i32) -> f64 {
        self.co.dps / 1.045f64.powi(self.latest - fy)
    }

    fn quarter_revenue(&self, fy: i32, k: i32) -> f64 {
        let i = (fy * 4 + k) as u64;
        self.annual_revenue(fy) * self.co.season[(k - 1) as usize] * (1.0 + 0.01 * noise(self.seed ^ 7, i))
    }

    /// Income-statement lines of one fiscal quarter.
    fn income(&self, fy: i32, k: i32) -> Map<String, Value> {
        let co = self.co;
        let i = (fy * 4 + k) as u64;
        let r = self.quarter_revenue(fy, k);
        let gm = co.gross + 0.006 * noise(self.seed ^ 11, i);
        let cost = r * (1.0 - gm);
        let gp = r - cost;
        let rnd = r * co.rnd * (1.0 + 0.03 * noise(self.seed ^ 13, i));
        let sga = r * co.sga * (1.0 + 0.03 * noise(self.seed ^ 17, i));
        let op = gp - rnd - sga;
        let interest = r * 0.004;
        let pretax = op + r * 0.003;
        let tax = pretax * co.tax;
        let net = pretax - tax;
        let mut m = Map::new();
        for (k, v) in [
            ("revenue", r), ("cost_of_revenue", cost), ("gross_profit", gp), ("rnd", rnd), ("sga", sga),
            ("operating_income", op), ("interest_expense", interest), ("pretax_income", pretax),
            ("income_tax", tax), ("net_income", net), ("ebitda", op + r * co.dep),
        ] {
            m.insert(k.into(), json!(v.round()));
        }
        m.insert("eps_diluted".into(), json!(round(net / self.shares(fy), 2)));
        m
    }

    fn cashflow(&self, fy: i32, k: i32) -> Map<String, Value> {
        let co = self.co;
        let i = (fy * 4 + k) as u64;
        let r = self.quarter_revenue(fy, k);
        let net = self.income(fy, k)["net_income"].as_f64().unwrap_or(0.0);
        let dep = r * co.dep;
        let sbc = r * 0.03;
        let ocf = net + dep + sbc + r * 0.03 * noise(self.seed ^ 19, i);
        let capex = -r * co.capex;
        let acq = -r * 0.004;
        let icf = capex + acq - r * 0.01;
        let div = -self.dps(fy) * self.shares(fy);
        let buy = -r * co.buyback * (1.0 + 0.1 * noise(self.seed ^ 23, i));
        let mut m = Map::new();
        for (k, v) in [
            ("net_income", net), ("depreciation", dep), ("stock_comp", sbc), ("operating_cash_flow", ocf),
            ("capex", capex), ("acquisitions", acq), ("investing_cash_flow", icf), ("dividends_paid", div),
            ("buybacks", buy), ("financing_cash_flow", div + buy - r * 0.01), ("free_cash_flow", ocf + capex),
        ] {
            m.insert(k.into(), json!(v.round()));
        }
        m
    }

    /// Balance sheet at the end of a fiscal quarter.
    fn balance(&self, fy: i32, k: i32) -> Map<String, Value> {
        let co = self.co;
        let i = (fy * 4 + k) as u64;
        let a = self.annual_revenue(fy) * (1.0 + 0.02 * noise(self.seed ^ 29, i));
        let (cash, sti, rec, inv) = (a * 0.08, a * 0.07, a * 0.15, a * 0.02);
        let tca = cash + sti + rec + inv + a * 0.05;
        let ppe = a * co.ppe;
        let gw = a * co.goodwill;
        let ta = tca + ppe + gw + a * 0.25;
        let (ap, std) = (a * 0.17, a * 0.03);
        let tcl = ap + std + a * 0.12;
        let ltd = a * co.debt;
        let tl = tcl + ltd + a * 0.10;
        let mut m = Map::new();
        for (k, v) in [
            ("cash", cash), ("short_term_investments", sti), ("receivables", rec), ("inventory", inv),
            ("total_current_assets", tca), ("ppe_net", ppe), ("goodwill_intangibles", gw), ("total_assets", ta),
            ("accounts_payable", ap), ("short_term_debt", std), ("total_current_liabilities", tcl),
            ("long_term_debt", ltd), ("total_liabilities", tl), ("shareholders_equity", ta - tl),
        ] {
            m.insert(k.into(), json!(v.round()));
        }
        m
    }
}

/// Flow lines of four quarters summed into the fiscal year (EPS too, near enough).
fn sum_year(quarters: Vec<Map<String, Value>>) -> Map<String, Value> {
    let mut out = Map::new();
    for q in quarters {
        for (k, v) in q {
            let add = v.as_f64().unwrap_or(0.0);
            let cur = out.get(&k).and_then(Value::as_f64).unwrap_or(0.0);
            let sum = if k == "eps_diluted" { round(cur + add, 2) } else { cur + add };
            out.insert(k, json!(sum));
        }
    }
    out
}

/// Daily closes for the price snapshot: the seeded Yahoo bars when the seed downloaded
/// them, else a random walk ending at the company's reference close.
async fn closes(pool: &PgPool, co: &Co, today: Date) -> Result<Vec<(Date, f64)>> {
    let rows: Vec<(OffsetDateTime, f64)> = sqlx::query_as(
        "SELECT b.ts, b.close::float8 FROM histdata_bars b \
         JOIN histdata_datasets d ON d.id = b.dataset_id \
         WHERE d.provider = 'yahoo' AND d.ticker = $1 AND d.timeframe = '1d' \
           AND b.ts >= now() - interval '5 years' \
         ORDER BY b.ts",
    )
    .bind(co.ticker)
    .fetch_all(pool)
    .await?;
    if rows.len() >= 200 {
        return Ok(rows.into_iter().map(|(ts, c)| (ts.date(), c)).collect());
    }
    let seed = seed_of(co.ticker) ^ 31;
    let mut d = today - Duration::days(5 * 365);
    let mut p = 1.0;
    let mut out = Vec::new();
    let mut i = 0u64;
    while d < today {
        if !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday) {
            p *= 1.0 + 0.0006 + 0.016 * noise(seed, i);
            out.push((d, p));
            i += 1;
        }
        d += Duration::days(1);
    }
    let scale = co.price / p;
    Ok(out.into_iter().map(|(d, c)| (d, round(c * scale, 2))).collect())
}

fn close_on(px: &[(Date, f64)], d: Date) -> Option<f64> {
    px.iter().rev().find(|(x, _)| *x <= d).or(px.first()).map(|(_, c)| round(*c, 2))
}

/// One fake EDGAR accession number.
fn accession(co: &Co, filed: Date, seq: &mut u32) -> String {
    *seq += 1;
    format!("{}-{:02}-{:06}", co.cik, filed.year() % 100, *seq)
}

fn edgar_url(co: &Co, form: &str) -> String {
    format!(
        "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={}&type={}&dateb=&owner=include&count=40",
        co.cik,
        form.replace(' ', "+")
    )
}

fn filing(co: &Co, form: &str, title: &str, filed: Date, period: Option<Date>, seq: &mut u32) -> DocumentInput {
    DocumentInput {
        kind: "filing".into(),
        form: form.into(),
        accession: accession(co, filed, seq),
        filed_at: filed,
        period,
        title: title.into(),
        url: edgar_url(co, form),
        source: "SEC EDGAR".into(),
    }
}

async fn company(pool: &PgPool, co: &Co, today: Date) -> Result<()> {
    let latest = latest_fy(co, today);
    let m = Model { co, latest, seed: seed_of(co.ticker) };

    // Profile.
    let id = store::ensure_company(pool, co.ticker, co.cik, co.name).await?;
    store::update_company(
        pool,
        id,
        &CompanyInput {
            ticker: co.ticker.into(),
            cik: co.cik.into(),
            name: co.name.into(),
            exchange: co.exchange.into(),
            currency: "USD".into(),
            country: "US".into(),
            sector: co.sector.into(),
            industry: co.industry.into(),
            sic: co.sic.into(),
            fiscal_year_end: format!("{:02}{:02}", co.fye_month, month_end(2001, co.fye_month).day()),
            website: co.website.into(),
            description: co.about.into(),
            shares_out: Some(m.shares(latest + 1)),
        },
    )
    .await?;
    store::set_company_status(pool, id, "ok", None).await?;
    store::set_followed(pool, co.ticker, co.ticker == "AAPL").await?;

    // Statements: six fiscal years and every reported quarter in them.
    let mut rows = Vec::new();
    let mut quarters_out = Vec::new();
    for fy in latest - 5..=latest + 1 {
        for k in 1..=4 {
            let (start, end) = fiscal_quarter(co, fy, k);
            if !reported(end, today) {
                continue;
            }
            quarters_out.push((fy, k, start, end));
            for (kind, lines) in [("income", m.income(fy, k)), ("balance", m.balance(fy, k)), ("cashflow", m.cashflow(fy, k))] {
                rows.push(Statement {
                    kind: kind.into(),
                    fiscal_period: format!("Q{k}"),
                    fiscal_year: fy,
                    period_start: start,
                    period_end: end,
                    source: datasets::DEMO.into(),
                    lines: Value::Object(lines),
                    tags: json!({}),
                });
            }
        }
        if fy <= latest {
            let start = fiscal_quarter(co, fy, 1).0;
            let end = fiscal_quarter(co, fy, 4).1;
            let years = [
                ("income", sum_year((1..=4).map(|k| m.income(fy, k)).collect())),
                ("cashflow", sum_year((1..=4).map(|k| m.cashflow(fy, k)).collect())),
                ("balance", m.balance(fy, 4)),
            ];
            for (kind, lines) in years {
                rows.push(Statement {
                    kind: kind.into(),
                    fiscal_period: "FY".into(),
                    fiscal_year: fy,
                    period_start: start,
                    period_end: end,
                    source: datasets::DEMO.into(),
                    lines: Value::Object(lines),
                    tags: json!({}),
                });
            }
        }
    }
    store::replace_statements(pool, id, datasets::DEMO, &rows).await?;

    // Price first: earnings reactions, insider prices and targets read it.
    let px = closes(pool, co, today).await?;
    let last_px = px.last().map(|(_, c)| *c).unwrap_or(co.price);
    snap(
        pool,
        co.ticker,
        "price",
        json!({ "series": px.iter().map(|(d, c)| (date_ms(*d), round(*c, 2))).collect::<Vec<_>>(), "symbol": co.ticker }),
    )
    .await?;

    // Filings: the periodic reports of the last three years, one 8-K per results day and
    // the yearly proxy. Form 4s come with the insider trades below.
    let mut seq = 1000u32;
    let mut docs = Vec::new();
    let recent: Vec<_> = quarters_out.iter().rev().take(12).collect();
    for (_, k, _, end) in &recent {
        let out = report_date(*end);
        docs.push(filing(co, "8-K", "Current report", out, Some(*end), &mut seq));
        let (form, title) = if *k == 4 { ("10-K", "Annual report") } else { ("10-Q", "Quarterly report") };
        docs.push(filing(co, form, title, biz(out + Duration::days(1)), Some(*end), &mut seq));
        if *k == 4 {
            let proxy = biz(*end + Duration::days(115));
            if proxy < today {
                docs.push(filing(co, "DEF 14A", "Proxy statement", proxy, None, &mut seq));
            }
        }
    }

    // Insider trades over the last eighteen months, one Form 4 each.
    let insiders = &co.people;
    let mut trades = Vec::new();
    let mut owned: Vec<f64> = insiders.iter().enumerate().map(|(i, _)| 3.2e6 / (i as f64 + 1.0)).collect();
    for n in 0..14u64 {
        let who = (n as usize * 3) % insiders.len();
        let tx = biz_back(today - Duration::days(20 + n as i64 * 38));
        let filed = biz(tx + Duration::days(2));
        let (code, acquired) = match n % 7 {
            0 | 2 | 4 => ("S", false),
            1 => ("M", true),
            3 => ("F", false),
            5 => ("A", true),
            _ => ("P", true),
        };
        let shares = (8_000.0 + 90_000.0 * (0.5 + 0.5 * noise(m.seed ^ 37, n))).round();
        owned[who] = (owned[who] + if acquired { shares } else { -shares }).max(10_000.0);
        let price = match code {
            "A" => None,
            "M" => close_on(&px, tx).map(|p| round(p * 0.35, 2)),
            _ => close_on(&px, tx),
        };
        let acc = accession(co, filed, &mut seq);
        docs.push(DocumentInput {
            kind: "filing".into(),
            form: "4".into(),
            accession: acc.clone(),
            filed_at: filed,
            period: Some(tx),
            title: "Statement of changes in beneficial ownership".into(),
            url: edgar_url(co, "4"),
            source: "SEC EDGAR".into(),
        });
        trades.push(InsiderTrade {
            accession: acc,
            ordinal: 0,
            filed_at: filed,
            tx_date: tx,
            insider: insiders[who].0.into(),
            role: insiders[who].1.into(),
            code: code.into(),
            acquired,
            shares,
            price,
            owned_after: Some(owned[who]),
        });
    }
    store::upsert_documents(pool, id, &docs).await?;
    store::insert_insider_trades(pool, id, &trades).await?;

    // Earnings: twelve reported quarters and the next date.
    let history: Vec<Value> = quarters_out
        .iter()
        .rev()
        .take(12)
        .rev()
        .map(|(fy, k, _, end)| {
            let i = (fy * 4 + k) as u64;
            let inc = m.income(*fy, *k);
            let actual = inc["eps_diluted"].as_f64().unwrap_or(0.0);
            let est = round(actual / (1.0 + 0.025 + 0.045 * noise(m.seed ^ 41, i)), 2);
            let rev = inc["revenue"].as_f64().unwrap_or(0.0);
            json!({
                "period": format!("Q{k} FY{fy}"),
                "date": day(report_date(*end)),
                "time": "AMC",
                "eps_estimate": est,
                "eps_actual": actual,
                "surprise_pct": crate::fundamentals::fmp::surprise(Some(actual), Some(est)),
                "revenue_estimate": (rev / (1.01 + 0.012 * noise(m.seed ^ 43, i))).round(),
                "revenue_actual": rev,
                "move_next_day": null,
            })
        })
        .collect();
    let (nfy, nk) = match quarters_out.last() {
        Some((fy, 4, _, _)) => (fy + 1, 1),
        Some((fy, k, _, _)) => (*fy, k + 1),
        None => (latest + 1, 1),
    };
    let next_eps = |fy: i32, k: i32| round(m.income(fy, k)["eps_diluted"].as_f64().unwrap_or(0.0) * 0.98, 2);
    snap(
        pool,
        co.ticker,
        "earnings",
        json!({
            "history": history,
            "next": { "date": day(report_date(fiscal_quarter(co, nfy, nk).1)), "time": "AMC", "eps_estimate": next_eps(nfy, nk) },
        }),
    )
    .await?;

    // Estimates: the next two quarters and fiscal years, targets around the last close.
    let after = |fy: i32, k: i32| if k == 4 { (fy + 1, 1) } else { (fy, k + 1) };
    let (q2fy, q2k) = after(nfy, nk);
    let quarter_row = |fy: i32, k: i32, n: u64| {
        let eps = next_eps(fy, k);
        json!({
            "period": format!("Q{k} FY{fy}"), "date": day(fiscal_quarter(co, fy, k).1),
            "eps_mean": eps, "eps_low": round(eps * 0.93, 2), "eps_high": round(eps * 1.06, 2),
            "revenue_mean": m.quarter_revenue(fy, k).round(), "analysts": 26 + n,
            "revisions_up_30d": 6 + n * 2, "revisions_down_30d": 2 + n,
        })
    };
    let year_row = |fy: i32, n: u64| {
        let eps: f64 = (1..=4).map(|k| m.income(fy, k)["eps_diluted"].as_f64().unwrap_or(0.0)).sum();
        json!({
            "period": format!("FY{fy}"), "date": day(fiscal_quarter(co, fy, 4).1),
            "eps_mean": round(eps * 0.99, 2), "eps_low": round(eps * 0.92, 2), "eps_high": round(eps * 1.08, 2),
            "revenue_mean": m.annual_revenue(fy).round(), "analysts": 38 - n * 4,
            "revisions_up_30d": 9 - n * 3, "revisions_down_30d": 3,
        })
    };
    let firms = ["Northbridge Securities", "Halcyon Capital Markets", "Redwood & Pike", "Atlas Peak Research", "Granite Row Partners", "Meridian Equity Research"];
    let actions: Vec<Value> = (0..10u64)
        .map(|n| {
            let (action, rating) = match n % 5 {
                0 => ("maintain", "Buy"),
                1 => ("upgrade", "Outperform"),
                2 => ("maintain", "Overweight"),
                3 => ("downgrade", "Neutral"),
                _ => ("initiate", "Buy"),
            };
            json!({
                "date": day(biz_back(today - Duration::days(4 + n as i64 * 11))),
                "firm": firms[n as usize % firms.len()], "action": action, "rating": rating,
                "previous": if action == "initiate" { "" } else { "Hold" },
                "target": round(last_px * (1.12 + 0.1 * noise(m.seed ^ 47, n)), 0),
            })
        })
        .collect();
    let next_rev = m.quarter_revenue(nfy, nk);
    let issued = quarters_out.last().map(|(_, _, _, end)| day(report_date(*end))).unwrap_or_default();
    snap(
        pool,
        co.ticker,
        "estimates",
        json!({
            "consensus": [quarter_row(nfy, nk, 0), quarter_row(q2fy, q2k, 1), year_row(latest + 1, 0), year_row(latest + 2, 1)],
            "price_target": { "low": round(last_px * 0.78, 2), "mean": round(last_px * 1.09, 2), "median": round(last_px * 1.1, 2), "high": round(last_px * 1.32, 2), "current": null },
            "ratings": { "strong_buy": 14, "buy": 18, "hold": 9, "sell": 2, "strong_sell": 1 },
            "actions": actions,
            "guidance": [
                { "period": format!("Q{nk} FY{nfy}"), "metric": "Revenue", "low": (next_rev * 0.97).round(), "high": (next_rev * 1.02).round(), "issued": issued },
                { "period": format!("Q{nk} FY{nfy}"), "metric": "Gross margin %", "low": round(co.gross * 100.0 - 0.5, 1), "high": round(co.gross * 100.0 + 0.5, 1), "issued": issued },
                { "period": format!("Q{nk} FY{nfy}"), "metric": "Operating expenses", "low": (next_rev * (co.rnd + co.sga) * 0.98).round(), "high": (next_rev * (co.rnd + co.sga) * 1.01).round(), "issued": issued },
            ],
        }),
    )
    .await?;

    // Segments: five fiscal years.
    let years: Vec<i32> = (latest - 4..=latest).collect();
    let split = |parts: &[(&str, f64, f64)]| -> Vec<Value> {
        let mut out: Vec<Value> = parts
            .iter()
            .map(|(name, w, drift)| {
                let values: Vec<f64> = years
                    .iter()
                    .map(|fy| {
                        let total: f64 = parts.iter().map(|(_, w2, d2)| w2 * (1.0 + d2 * (fy - latest) as f64)).sum();
                        (m.annual_revenue(*fy) * w * (1.0 + drift * (fy - latest) as f64) / total).round()
                    })
                    .collect();
                json!({ "name": name, "values": values })
            })
            .collect();
        out.sort_by(|a, b| {
            let last = |v: &Value| v["values"].as_array().and_then(|a| a.last()).and_then(Value::as_f64).unwrap_or(0.0);
            last(b).total_cmp(&last(a))
        });
        out
    };
    let kpis: Vec<Value> = co
        .kpis
        .iter()
        .map(|(name, v, g)| {
            json!({ "name": name, "values": years.iter().map(|fy| round(v / (1.0 + g).powi(latest - fy), 1)).collect::<Vec<_>>() })
        })
        .collect();
    snap(
        pool,
        co.ticker,
        "segments",
        json!({
            "years": years.iter().map(|y| format!("FY{y}")).collect::<Vec<_>>(),
            "product": split(co.products),
            "region": split(co.regions),
            "kpis": kpis,
        }),
    )
    .await?;

    // Dividends: six years of quarterly ex-dates, raised once a year.
    let mut dividends = Vec::new();
    let mut q = add_months(quarter_start(today), -72);
    while q <= today {
        let ex = biz(ymd(q.year(), q.month() as u8 + 1, 12));
        if ex < today {
            let years_back = (yf(today) - yf(ex)).floor() as i32;
            dividends.push(json!({
                "period": format!("{}-{:02}", ex.year(), ex.month() as u8),
                "ex_date": day(ex),
                "pay_date": day(biz(ex + Duration::days(4))),
                "amount": round(co.dps / 1.045f64.powi(years_back), 3),
            }));
        }
        q = add_months(q, 3);
    }
    snap(
        pool,
        co.ticker,
        "dividends",
        json!({
            "dividends": dividends,
            "splits": co.splits.iter().map(|(d, r)| json!({ "date": d, "ratio": r })).collect::<Vec<_>>(),
        }),
    )
    .await?;

    // 13F holders of the last complete quarter.
    let q13 = add_months(quarter_start(today), -6);
    let q13_end = add_months(q13, 3) - Duration::days(1);
    let holders: &[(&str, f64)] = &[
        ("Vanguard Group Inc", 9.4), ("BlackRock Inc", 7.3), ("State Street Corp", 4.0), ("Geode Capital Management", 2.3),
        ("FMR LLC", 2.1), ("Morgan Stanley", 1.5), ("JPMorgan Chase & Co", 1.3), ("Northern Trust Corp", 1.0),
        ("T. Rowe Price Associates", 0.9), ("Norges Bank", 0.9), ("Capital Research Global Investors", 0.8),
        ("Bank of America Corp", 0.8), ("Goldman Sachs Group Inc", 0.6), ("Invesco Ltd", 0.5), ("UBS Asset Management", 0.5),
    ];
    let shares_out = m.shares(latest + 1);
    snap(
        pool,
        co.ticker,
        "holders",
        json!({
            "quarter": format!("{}-Q{}", q13.year(), (q13.month() as u8 - 1) / 3 + 1),
            "holders": holders.iter().enumerate().map(|(i, (h, pct))| {
                let pct = round(pct * (1.0 + 0.08 * noise(m.seed ^ 53, i as u64)), 2);
                json!({
                    "holder": h, "pct": pct, "shares": (shares_out * pct / 100.0).round(),
                    "change_pct": round(3.0 * noise(m.seed ^ 59, i as u64), 2),
                    "filed": day(biz(q13_end + Duration::days(40 + i as i64 % 5))),
                })
            }).collect::<Vec<_>>(),
            "institutions_pct": co.institutions,
            "investors": 6100,
        }),
    )
    .await?;

    // Short interest: twice-monthly settlements over two years.
    let mut si = Vec::new();
    let mut mo = add_months(month_start(today), -24);
    let mut walk = 0.0;
    let mut i = 0u64;
    while mo <= today {
        for settle in [ymd(mo.year(), mo.month() as u8, 15), month_end(mo.year(), mo.month() as u8)] {
            let settle = biz_back(settle);
            if settle > today - Duration::days(10) {
                continue;
            }
            walk = 0.85 * walk + 0.08 * noise(m.seed ^ 61, i);
            let shares = (shares_out * co.short_pct / 100.0 * (1.0 + walk)).round();
            let adv = (co.adv * (1.0 + 0.2 * noise(m.seed ^ 67, i))).round();
            si.push(json!({ "date": day(settle), "shares": shares, "days_to_cover": round(shares / adv, 2), "avg_volume": adv }));
            i += 1;
        }
        mo = add_months(mo, 1);
    }
    snap(pool, co.ticker, "short_interest", json!({ "series": si })).await?;

    snap(
        pool,
        co.ticker,
        "peers",
        Value::Array(co.peers.iter().map(|(t, n, cap)| json!({ "ticker": t, "name": n, "market_cap": cap })).collect()),
    )
    .await?;

    snap(
        pool,
        co.ticker,
        "esg",
        json!({
            "esg": {
                "total": co.esg[0], "environment": co.esg[1], "social": co.esg[2], "governance": co.esg[3],
                "controversy": null, "date": day(today - Duration::days(60)), "provider": "Demo data", "higher_is_better": true,
            },
            "management": co.people.iter().map(|(n, t, pay)| json!({ "name": format!("{n}, {t}"), "title": "", "since": latest, "pay": pay })).collect::<Vec<_>>(),
        }),
    )
    .await?;

    // The last two calls, already split into turns.
    for (fy, k, _, end) in quarters_out.iter().rev().take(2) {
        let acc = format!("demo:{}:{fy}:{k}", co.ticker);
        let doc = DocumentInput {
            kind: "transcript".into(),
            form: "transcript".into(),
            accession: acc.clone(),
            filed_at: report_date(*end),
            period: Some(*end),
            title: format!("{} Q{k} FY{fy} earnings call", co.name),
            url: String::new(),
            source: "Demo data".into(),
        };
        store::upsert_documents(pool, id, &[doc]).await?;
        if let Some(doc_id) = store::document_id(pool, "Demo data", &acc).await? {
            store::store_transcript(pool, doc_id, &call(co, &m, *fy, *k)).await?;
        }
    }

    alt(pool, co, &m, today).await
}

fn call(co: &Co, m: &Model, fy: i32, k: i32) -> Vec<Segment> {
    let inc = m.income(fy, k);
    let rev = inc["revenue"].as_f64().unwrap_or(0.0) / 1e9;
    let prev = m.income(fy - 1, k)["revenue"].as_f64().unwrap_or(1.0) / 1e9;
    let growth = (rev / prev - 1.0) * 100.0;
    let gm = inc["gross_profit"].as_f64().unwrap_or(0.0) / 1e9 / rev * 100.0;
    let eps = inc["eps_diluted"].as_f64().unwrap_or(0.0);
    let cf = m.cashflow(fy, k);
    let ocf = cf["operating_cash_flow"].as_f64().unwrap_or(0.0) / 1e9;
    let returned = (cf["dividends_paid"].as_f64().unwrap_or(0.0) + cf["buybacks"].as_f64().unwrap_or(0.0)).abs() / 1e9;
    let top = co.products[0].0;
    let second = co.products[1].0;
    let (ceo, cfo) = (co.people[0].0, co.people[1].0);
    let turns: Vec<(&str, &str, &str, String)> = vec![
        ("Operator", "operator", "prepared", format!(
            "Good afternoon, and welcome to the {} fiscal Q{k} {fy} earnings conference call. Today's call is being recorded. \
             This is an illustrative transcript written for the OpenTraderWorld demo.", co.name)),
        (ceo, "exec", "prepared", format!(
            "Thank you, and good afternoon, everyone. We delivered revenue of ${rev:.1} billion this quarter, up {growth:.0}% \
             from a year ago. {top} led the quarter, and {second} reached a new record. Customer demand stayed healthy \
             across every region, and we continued to invest in the long-term roadmap.")),
        (cfo, "exec", "prepared", format!(
            "Gross margin was {gm:.1}%, within the range we guided to. Diluted earnings per share were ${eps:.2}. \
             Operating cash flow was ${ocf:.1} billion, and we returned ${returned:.1} billion to shareholders through \
             dividends and buybacks. For next quarter we expect revenue growth in the mid single digits and gross \
             margin roughly in line with this quarter.")),
        ("Operator", "operator", "qa", "We will now begin the question-and-answer session. The first question comes from Dana Kowalski with Northbridge Securities.".into()),
        ("Dana Kowalski", "analyst", "qa", format!("Thanks for taking the question. Could you talk about the drivers behind {second} and how durable that growth is into next year?")),
        (ceo, "exec", "qa", format!(
            "Sure. {second} keeps growing because the installed base keeps growing, and engagement per customer is up. \
             We see a long runway there.")),
        ("Operator", "operator", "qa", "The next question comes from Felix Haraldsen with Halcyon Capital Markets.".into()),
        ("Felix Haraldsen", "analyst", "qa", "On margins: how should we think about input costs and the mix shift over the next few quarters?".into()),
        (cfo, "exec", "qa",
            "Mix is a tailwind as the higher-margin businesses grow faster. Input costs are stable. We are comfortable with the guidance range.".into()),
        ("Operator", "operator", "qa", "That concludes today's question-and-answer session. Thank you for joining.".into()),
    ];
    turns
        .into_iter()
        .enumerate()
        .map(|(i, (speaker, role, section, text))| Segment {
            ordinal: i as i32,
            speaker: speaker.into(),
            role: role.into(),
            section: section.into(),
            text,
        })
        .collect()
}

// ── Alternative data ────────────────────────────────────────────────────────

async fn alt(pool: &PgPool, co: &Co, m: &Model<'_>, today: Date) -> Result<()> {
    let seed = m.seed;
    let lob_series = quarterly(today, 12, |i, _| (co.lobbying * (1.0 + 0.15 * noise(seed ^ 71, i as u64))).round());
    let filings: Vec<Value> = lob_series
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .take(8)
        .flat_map(|p| {
            let ms = p[0].as_i64().unwrap_or(0);
            let total = p[1].as_f64().unwrap_or(0.0);
            let d = OffsetDateTime::from_unix_timestamp(ms / 1000).map(|t| t.date()).unwrap_or(today);
            let period = format!("{} Q{}", d.year(), (d.month() as u8 - 1) / 3 + 1);
            [
                json!({ "period": period, "registrant": co.name, "amount": (total * 0.86).round(), "issues": "Taxation, Trade, Copyright/Patent/Trademark, Telecommunications, Consumer Issues/Safety/Protection", "url": "" }),
                json!({ "period": period, "registrant": "Capitol Bridge Strategies LLC", "amount": (total * 0.08).round(), "issues": "Trade, Taxation", "url": "" }),
                json!({ "period": period, "registrant": "Meridian Public Affairs", "amount": (total * 0.06).round(), "issues": "Computer Industry, Science/Technology", "url": "" }),
            ]
        })
        .collect();
    snap(pool, co.ticker, "lobbying", json!({ "match": co.registered, "filings": filings, "series": lob_series })).await?;

    let agencies = [
        ("Department of Defense", "Enterprise devices and software licences"),
        ("Department of Veterans Affairs", "Clinical mobility devices"),
        ("General Services Administration", "Schedule purchase, end-user computing"),
        ("Department of Homeland Security", "Secure mobile platform support"),
        ("Department of Energy", "Cloud services and support"),
    ];
    let awards: Vec<Value> = (0..8u64)
        .map(|n| {
            let (agency, desc) = agencies[n as usize % agencies.len()];
            json!({
                "date": day(biz_back(today - Duration::days(9 + n as i64 * 41))),
                "agency": agency,
                "amount": (co.contracts * 0.12 * (1.0 + 0.6 * noise(seed ^ 73, n))).round(),
                "description": desc,
                "url": "",
            })
        })
        .collect();
    snap(
        pool,
        co.ticker,
        "contracts",
        json!({
            "match": co.registered,
            "awards": awards,
            "series": quarterly(today, 20, |i, _| (co.contracts * (1.0 + 0.35 * noise(seed ^ 79, i as u64))).round()),
        }),
    )
    .await?;

    let patents = quarterly(today, 20, |i, _| (co.patents * (1.0 + 0.12 * noise(seed ^ 83, i as u64))).round());
    let total: f64 = patents.as_array().into_iter().flatten().filter_map(|p| p[1].as_f64()).sum();
    snap(pool, co.ticker, "patents", json!({ "match": co.registered, "total": total, "series": patents })).await
}

/// Fictional members: (name, chamber, party).
const MEMBERS: &[(&str, &str, &str)] = &[
    ("Dana Whitfield", "house", "D"),
    ("Robert Castellano", "house", "R"),
    ("Evelyn Marsh", "senate", "D"),
    ("Grant Holloway", "senate", "R"),
    ("Theresa Quinn", "house", "D"),
    ("Malcolm Avery", "house", "R"),
];
const RANGES: &[&str] = &["$1,001 - $15,000", "$15,001 - $50,000", "$50,001 - $100,000", "$100,001 - $250,000"];

fn congress_trades(tickers: &[&str], n: u64, seed: u64, today: Date) -> Vec<Value> {
    (0..n)
        .map(|i| {
            let (member, chamber, party) = MEMBERS[(i as usize * 5 + seed as usize) % MEMBERS.len()];
            // Disclosed up to 34 days after the trade, so always before today.
            let traded = biz_back(today - Duration::days(45 + i as i64 * 9));
            let kind = match i % 5 {
                0 | 2 => "purchase",
                1 | 3 => "sale",
                _ => "exchange",
            };
            json!({
                "member": member, "chamber": chamber, "party": party,
                "ticker": tickers[i as usize % tickers.len()], "type": kind,
                "amount": RANGES[(i as usize + seed as usize) % RANGES.len()],
                "traded": day(traded), "disclosed": day(biz_back(traded + Duration::days(20 + (i as i64 % 15)))),
            })
        })
        .collect()
}

async fn congress(pool: &PgPool, today: Date) -> Result<()> {
    let all = ["NVDA", "AAPL", "MSFT", "AMZN", "LMT", "JPM", "XOM", "GOOGL", "UNH", "TSLA"];
    snap(pool, "_", "congress", json!({ "trades": congress_trades(&all, 18, 1, today) })).await?;
    for co in COMPANIES {
        snap(pool, co.ticker, "congress", json!({ "trades": congress_trades(&[co.ticker], 6, seed_of(co.ticker) % 7, today) })).await?;
    }
    Ok(())
}

// ── ETFs ────────────────────────────────────────────────────────────────────

async fn etfs(pool: &PgPool, today: Date) -> Result<()> {
    let flows = |seed: u64, base: f64| -> Value {
        let mut cum = 0.0;
        let start = add_months(month_start(today), -24);
        let rows: Vec<Value> = (0..24)
            .map(|i| {
                let f = (base * (0.4 + 1.2 * noise(seed, i as u64))).round();
                cum += f;
                json!([date_ms(add_months(start, i)), f, cum])
            })
            .collect();
        Value::Array(rows)
    };
    let funds = [
        json!({
            "ticker": "SPY", "name": "SPDR S&P 500 ETF Trust", "issuer": "State Street Global Advisors",
            "aum": 6.4e11, "expense": 0.0945, "inception": "1993-01-22", "holdings": 503, "index": "S&P 500",
            "description": "Tracks the S&P 500 index of large US companies. Figures shown in this demo are illustrative.",
            "top_holdings": [
                { "name": "NVDA", "label": "NVIDIA Corp", "weight": 7.9 }, { "name": "MSFT", "label": "Microsoft Corp", "weight": 6.8 },
                { "name": "AAPL", "label": "Apple Inc", "weight": 6.3 }, { "name": "AMZN", "label": "Amazon.com Inc", "weight": 3.9 },
                { "name": "META", "label": "Meta Platforms Inc", "weight": 2.8 }, { "name": "AVGO", "label": "Broadcom Inc", "weight": 2.6 },
                { "name": "GOOGL", "label": "Alphabet Inc Class A", "weight": 2.3 }, { "name": "GOOG", "label": "Alphabet Inc Class C", "weight": 1.9 },
                { "name": "TSLA", "label": "Tesla Inc", "weight": 1.8 }, { "name": "BRK.B", "label": "Berkshire Hathaway Inc", "weight": 1.6 },
                { "name": "JPM", "label": "JPMorgan Chase & Co", "weight": 1.5 }, { "name": "LLY", "label": "Eli Lilly and Co", "weight": 1.2 },
            ],
            "sectors": [
                { "name": "Information Technology", "weight": 33.8 }, { "name": "Financials", "weight": 13.4 },
                { "name": "Communication Services", "weight": 9.9 }, { "name": "Consumer Discretionary", "weight": 10.4 },
                { "name": "Health Care", "weight": 8.9 }, { "name": "Industrials", "weight": 8.4 },
                { "name": "Consumer Staples", "weight": 5.2 }, { "name": "Energy", "weight": 3.0 },
                { "name": "Utilities", "weight": 2.4 }, { "name": "Real Estate", "weight": 2.0 }, { "name": "Materials", "weight": 1.8 },
            ],
            "countries": [
                { "name": "United States", "weight": 99.4 }, { "name": "Ireland", "weight": 0.3 },
                { "name": "Netherlands", "weight": 0.1 }, { "name": "Switzerland", "weight": 0.1 }, { "name": "Other", "weight": 0.1 },
            ],
            "flows": flows(101, 4.5e9),
        }),
        json!({
            "ticker": "QQQ", "name": "Invesco QQQ Trust, Series 1", "issuer": "Invesco",
            "aum": 3.6e11, "expense": 0.2, "inception": "1999-03-10", "holdings": 101, "index": "Nasdaq-100",
            "description": "Tracks the Nasdaq-100 index of large non-financial companies listed on Nasdaq. Figures shown in this demo are illustrative.",
            "top_holdings": [
                { "name": "NVDA", "label": "NVIDIA Corp", "weight": 9.6 }, { "name": "MSFT", "label": "Microsoft Corp", "weight": 8.5 },
                { "name": "AAPL", "label": "Apple Inc", "weight": 7.7 }, { "name": "AMZN", "label": "Amazon.com Inc", "weight": 5.4 },
                { "name": "AVGO", "label": "Broadcom Inc", "weight": 5.2 }, { "name": "META", "label": "Meta Platforms Inc", "weight": 3.6 },
                { "name": "NFLX", "label": "Netflix Inc", "weight": 2.9 }, { "name": "TSLA", "label": "Tesla Inc", "weight": 2.8 },
                { "name": "GOOGL", "label": "Alphabet Inc Class A", "weight": 2.6 }, { "name": "COST", "label": "Costco Wholesale Corp", "weight": 2.5 },
            ],
            "sectors": [
                { "name": "Information Technology", "weight": 52.1 }, { "name": "Communication Services", "weight": 15.6 },
                { "name": "Consumer Discretionary", "weight": 13.2 }, { "name": "Consumer Staples", "weight": 5.1 },
                { "name": "Health Care", "weight": 5.0 }, { "name": "Industrials", "weight": 4.6 }, { "name": "Other", "weight": 4.4 },
            ],
            "countries": [
                { "name": "United States", "weight": 97.2 }, { "name": "Netherlands", "weight": 1.0 },
                { "name": "United Kingdom", "weight": 0.9 }, { "name": "China", "weight": 0.5 }, { "name": "Other", "weight": 0.4 },
            ],
            "flows": flows(103, 1.6e9),
        }),
    ];
    for f in funds {
        let ticker = f["ticker"].as_str().unwrap_or_default().to_string();
        snap(pool, &ticker, "etf", f).await?;
        sqlx::query("INSERT INTO fund_etfs (ticker, followed) VALUES ($1, $2) ON CONFLICT (ticker) DO NOTHING")
            .bind(&ticker)
            .bind(ticker == "SPY")
            .execute(pool)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fiscal_quarters_follow_the_year_end() {
        let aapl = &COMPANIES[0];
        assert_eq!(fiscal_quarter(aapl, 2026, 1), (ymd(2025, 10, 1), ymd(2025, 12, 31)));
        assert_eq!(fiscal_quarter(aapl, 2026, 4), (ymd(2026, 7, 1), ymd(2026, 9, 30)));
        let msft = &COMPANIES[1];
        assert_eq!(fiscal_quarter(msft, 2026, 4), (ymd(2026, 4, 1), ymd(2026, 6, 30)));
        // Early October: Apple's FY2026 is not out yet, Microsoft's is.
        assert_eq!(latest_fy(aapl, ymd(2026, 10, 5)), 2025);
        assert_eq!(latest_fy(msft, ymd(2026, 10, 5)), 2026);
    }

    #[test]
    fn the_series_stay_in_plausible_ranges() {
        let today = ymd(2026, 10, 5);
        for s in SERIES {
            let dates = periods(s.freq, ymd(s.from.0, s.from.1, 1), today);
            assert!(dates.len() > 30, "{}", s.code);
            assert!(dates.windows(2).all(|w| w[0] < w[1]), "{}", s.code);
            assert!(values(s, &dates).iter().all(|v| v.is_finite()), "{}", s.code);
        }
        let cpi = &SERIES[0];
        let dates = periods(cpi.freq, ymd(2006, 1, 1), today);
        let v = values(cpi, &dates);
        assert!(v[v.len() - 1] > 300.0 && v[v.len() - 1] < 360.0, "CPI ends at {}", v[v.len() - 1]);
    }

    #[test]
    fn noise_is_bounded_and_repeatable() {
        for i in 0..1000 {
            let n = noise(42, i);
            assert!((-1.0..=1.0).contains(&n));
            assert_eq!(n, noise(42, i));
        }
    }
}
