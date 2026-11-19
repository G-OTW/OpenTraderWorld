//! Futures contract specifications: tick size, point value and quote currency per contract
//! root, from QuantConnect Lean's symbol properties database (Apache 2.0, `LICENSE-LEAN`).
//!
//! Reference data, not the user's. `futures.csv` is Lean's file cut down to its future rows and
//! embedded in the binary, so a fresh install knows the common contracts offline. Once a week
//! the server reads Lean's current file and adopts it when it parses, the way the exchange
//! calendars refresh; the adopted copy is kept in `app_settings` so a restart does not fall
//! back to the older embedded one. Margins are not taken: they change by the day and every
//! broker sets its own.
//!
//! A row is keyed by Lean's market (`cme`, `cbot`, `eurex`…) and the root (`ES`, `ZN`, `FDAX`).
//! A chart's ticker is reduced to that root ([`root_of`]) and its exchange, when it names one,
//! to that market. A root listed on several markets is never picked by guess: without an
//! exchange it is refused with the candidates.
//!
//! The facts are stated in the units the exchange quotes. Lean divides some prices by a
//! `price_magnifier` (grains and livestock are quoted in cents, Lean keeps dollars), so its
//! tick is multiplied back and its multiplier divided: corn trades in 0.25 cent ticks worth
//! 50 USD a cent, as it prints on the chart.

use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use sqlx::PgPool;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const EMBEDDED: &str = include_str!("futures.csv");
/// Lean's own file, read once a week.
const REMOTE: &str = "https://raw.githubusercontent.com/QuantConnect/Lean/master/Data/symbol-properties/symbol-properties-database.csv";
const KEY_BODY: &str = "futures_specs";
const KEY_CHECKED: &str = "futures_specs_checked_at";
const SYNC_EVERY: i64 = 7 * 24 * 3600;
const TICK: Duration = Duration::from_secs(3600);
const WARMUP: Duration = Duration::from_secs(45);
/// Ceiling on the remote file, which is ~0.7 MB today.
const MAX_BYTES: usize = 8 * 1024 * 1024;
/// A file with fewer futures than this is a broken download, not Lean's database.
const MIN_ROWS: usize = 100;
/// Credited wherever a fact is shown as coming from here.
pub const SOURCE: &str = "QuantConnect Lean";

/// One contract root, in the units the exchange quotes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Spec {
    /// Lean's market id: `cme`, `cbot`, `nymex`, `comex`, `ice`, `eurex`…
    pub market: String,
    pub root: String,
    pub description: String,
    pub currency: String,
    /// What one point of price is worth, in `currency`.
    pub pointvalue: f64,
    /// The smallest price step.
    pub mintick: f64,
}

/// The future rows of a symbol properties file (Lean's whole file, or the embedded cut).
fn parse(text: &str) -> Result<Vec<Spec>> {
    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.context("reading the futures specifications")?;
        if rec.get(2) != Some("future") {
            continue;
        }
        let num = |i: usize| rec.get(i).filter(|s| !s.is_empty()).and_then(|s| s.parse::<f64>().ok());
        let (Some(multiplier), Some(mpv)) = (num(5), num(6)) else { continue };
        let magnifier = num(10).filter(|m| *m > 0.0).unwrap_or(1.0);
        let (pointvalue, mintick) = (multiplier / magnifier, mpv * magnifier);
        if !(pointvalue > 0.0 && mintick > 0.0 && pointvalue.is_finite() && mintick.is_finite()) {
            continue;
        }
        let field = |i: usize| rec.get(i).unwrap_or_default().to_string();
        out.push(Spec {
            market: field(0).to_lowercase(),
            root: field(1).to_uppercase(),
            description: field(3),
            currency: field(4).to_uppercase(),
            pointvalue,
            mintick: round(mintick),
        });
    }
    if out.len() < MIN_ROWS {
        bail!("only {} futures in the file", out.len());
    }
    Ok(out)
}

/// A tick computed from a product of floats, back to the decimal it is (0.0025 × 100 = 0.25).
fn round(x: f64) -> f64 {
    let f = 1e10;
    (x * f).round() / f
}

fn slot() -> &'static RwLock<Arc<Vec<Spec>>> {
    static S: OnceLock<RwLock<Arc<Vec<Spec>>>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(Arc::new(parse(EMBEDDED).expect("the embedded futures specifications are valid"))))
}

fn current() -> Arc<Vec<Spec>> {
    slot().read().map(|s| s.clone()).unwrap_or_default()
}

fn adopt(specs: Vec<Spec>) {
    if let Ok(mut s) = slot().write() {
        *s = Arc::new(specs);
    }
}

/// The exchange names the chart's providers write (IB's first), as Lean's markets.
fn market_of(exchange: &str) -> Option<&'static str> {
    Some(match exchange.trim().to_uppercase().as_str() {
        "CME" | "GLOBEX" | "XCME" => "cme",
        "CBOT" | "ECBOT" | "XCBT" => "cbot",
        "NYMEX" | "XNYM" => "nymex",
        "COMEX" | "XCEC" => "comex",
        "ICE" | "ICEUS" | "ICEEU" | "ICEEUSOFT" | "IPE" | "NYBOT" | "IFUS" | "IFEU" => "ice",
        "EUREX" | "DTB" | "XEUR" => "eurex",
        "CFE" | "XCBF" => "cfe",
        "SGX" | "XSES" => "sgx",
        "HKFE" | "XHKF" => "hkfe",
        "NSE" | "XNSE" => "india",
        "KRX" | "KSE" | "XKRX" => "krx",
        "LIFFE" | "NYSELIFFE" => "nyseliffe",
        _ => return None,
    })
}

const MONTHS: &[char] = &['F', 'G', 'H', 'J', 'K', 'M', 'N', 'Q', 'U', 'V', 'X', 'Z'];

/// The root a chart's futures ticker names, and the exchange it names when it names one.
///
/// IB writes `MNQ.202612@CME:USD` or its local symbol `MNQZ6`; TradeStation `@ES` (continuous)
/// or `ESZ25`; Yahoo `ES=F`; a thinkorswim habit `/ES`. The contract month is dropped: every
/// month of a root has the same tick and point value. A letter-and-digits tail is read as a
/// month only when what it leaves is a known root, so a root that ends that way stays whole.
pub fn root_of(ticker: &str, known: impl Fn(&str) -> bool) -> (String, Option<String>) {
    let mut t = ticker.trim().to_uppercase();
    if let Some((head, cur)) = t.rsplit_once(':') {
        if cur.len() == 3 && cur.chars().all(|c| c.is_ascii_alphabetic()) {
            t = head.to_string();
        }
    }
    let mut exchange = None;
    if let Some(rest) = t.strip_prefix('@') {
        t = rest.to_string();
    } else if let Some((head, ex)) = t.split_once('@') {
        exchange = Some(ex.to_string()).filter(|e| !e.is_empty());
        t = head.to_string();
    }
    let t = t.trim_start_matches('/').trim_end_matches("=F").to_string();
    if let Some((head, month)) = t.split_once('.') {
        if !head.is_empty() && month.chars().all(|c| c.is_ascii_digit()) {
            return (head.to_string(), exchange);
        }
    }
    if known(&t) {
        return (t, exchange);
    }
    let digits = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if (1..=2).contains(&digits) && t.len() > digits + 1 {
        let cut = t.len() - digits - 1;
        let month = t[cut..].chars().next().unwrap_or(' ');
        let head = &t[..cut];
        if MONTHS.contains(&month) && known(head) {
            return (head.to_string(), exchange);
        }
    }
    (t, exchange)
}

/// The specification of a futures ticker: `None` when its root is not listed (or its exchange
/// is not one listed), an error naming the candidates when the root is on several markets
/// and the ticker does not say which.
pub fn lookup(ticker: &str) -> Result<Option<Spec>> {
    lookup_in(&current(), ticker)
}

fn lookup_in(specs: &[Spec], ticker: &str) -> Result<Option<Spec>> {
    let (root, exchange) = root_of(ticker, |r| specs.iter().any(|s| s.root == r));
    let wanted = match &exchange {
        Some(e) => match market_of(e) {
            Some(m) => Some(m),
            None => return Ok(None),
        },
        None => None,
    };
    let hits: Vec<&Spec> = specs.iter().filter(|s| s.root == root && wanted.is_none_or(|m| s.market == m)).collect();
    match hits.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        many => bail!(
            "{root} is listed on several exchanges ({}): name the exchange in the ticker ({root}@EXCHANGE) or set the instrument info",
            many.iter().map(|s| s.market.to_uppercase()).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// The instrument facts a future has, with what its stored info leaves empty filled from its
/// contract specification. Stored values always win; anything that is not a future, or a
/// root that is not listed, comes back as stored.
pub fn complete(
    provider: &str,
    asset_type: &str,
    ticker: &str,
    meta: Option<otw_store::instrument_meta::InstrumentMeta>,
) -> Result<Option<otw_store::instrument_meta::InstrumentMeta>> {
    if !matches!(asset_type, "future" | "futures") {
        return Ok(meta);
    }
    let missing = meta.as_ref().is_none_or(|m| m.mintick.is_none() || m.pointvalue.is_none() || m.currency.is_none());
    if !missing {
        return Ok(meta);
    }
    let Some(spec) = lookup(ticker)? else { return Ok(meta) };
    let mut m = meta.unwrap_or_else(|| otw_store::instrument_meta::InstrumentMeta {
        provider: provider.to_string(),
        asset_type: asset_type.to_string(),
        ticker: ticker.to_string(),
        mintick: None,
        pointvalue: None,
        timezone: None,
        session: None,
        kind: None,
        currency: None,
        basecurrency: None,
        description: None,
        margin: None,
        margin_notional: None,
    });
    m.mintick.get_or_insert(spec.mintick);
    m.pointvalue.get_or_insert(spec.pointvalue);
    m.currency.get_or_insert(spec.currency);
    m.kind.get_or_insert_with(|| "futures".into());
    m.description.get_or_insert(spec.description);
    Ok(Some(m))
}

/// Adopt the copy the last weekly read stored, when it parses.
pub async fn load(pool: &PgPool) {
    let stored = otw_store::settings::get(pool, KEY_BODY).await.ok().flatten();
    if let Some(specs) = stored.and_then(|body| parse(&body).ok()) {
        adopt(specs);
    }
}

/// Weekly: read Lean's file and adopt it.
pub fn spawn(pool: PgPool, http: reqwest::Client) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        loop {
            if due(&pool).await {
                if let Err(e) = sync(&pool, &http).await {
                    tracing::warn!("futures specifications: {e:#}");
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn due(pool: &PgPool) -> bool {
    let last = otw_store::settings::get(pool, KEY_CHECKED)
        .await
        .ok()
        .flatten()
        .and_then(|s| OffsetDateTime::parse(&s, &Rfc3339).ok());
    last.is_none_or(|t| (OffsetDateTime::now_utc() - t).whole_seconds() >= SYNC_EVERY)
}

pub async fn sync(pool: &PgPool, http: &reqwest::Client) -> Result<usize> {
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    // Written first: a remote that keeps failing is retried next week, not every hour.
    otw_store::settings::set(pool, KEY_CHECKED, &now).await?;
    let res = http.get(REMOTE).timeout(Duration::from_secs(60)).send().await.context("reading Lean's symbol properties")?;
    if !res.status().is_success() {
        bail!("Lean's symbol properties answered {}", res.status());
    }
    let body = res.bytes().await?;
    if body.len() > MAX_BYTES {
        bail!("Lean's symbol properties are {} bytes, past the {MAX_BYTES} ceiling", body.len());
    }
    let text = String::from_utf8(body.to_vec()).context("Lean's symbol properties are not UTF-8")?;
    let specs = parse(&text)?;
    // Only the futures are kept: the rest of the file is equities, options and currencies.
    let mut cut = String::from("market,symbol,type,description,quote_currency,contract_multiplier,minimum_price_variation,lot_size,market_ticker,minimum_order_size,price_magnifier\n");
    for line in text.lines().filter(|l| l.split(',').nth(2).map(str::trim) == Some("future")) {
        cut.push_str(line);
        cut.push('\n');
    }
    otw_store::settings::set(pool, KEY_BODY, &cut).await?;
    let n = specs.len();
    adopt(specs);
    tracing::info!("futures specifications updated from {SOURCE}: {n} contracts");
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(ticker: &str) -> Spec {
        lookup(ticker).unwrap_or_else(|e| panic!("{e}")).unwrap_or_else(|| panic!("{ticker}: not found"))
    }

    #[test]
    fn the_embedded_file_reads_and_states_facts_in_exchange_units() {
        let es = spec("ES");
        assert_eq!((es.market.as_str(), es.currency.as_str(), es.pointvalue, es.mintick), ("cme", "USD", 50.0, 0.25));
        // Corn is quoted in cents: a 0.25 cent tick worth 12.50 USD.
        let zc = spec("ZC");
        assert_eq!((zc.mintick, zc.pointvalue), (0.25, 50.0));
        assert!((zc.mintick * zc.pointvalue - 12.5).abs() < 1e-9);
        let zn = spec("ZN");
        assert_eq!((zn.mintick, zn.pointvalue), (0.015625, 1000.0));
        assert_eq!(spec("FDAX").currency, "EUR");
    }

    #[test]
    fn every_provider_spelling_reduces_to_its_root() {
        let known = |r: &str| ["ES", "MNQ", "6E", "ZN"].contains(&r);
        assert_eq!(root_of("MNQ.202612@CME:USD", known), ("MNQ".into(), Some("CME".into())));
        assert_eq!(root_of("MNQ.20261218@CME", known), ("MNQ".into(), Some("CME".into())));
        assert_eq!(root_of("MNQZ6", known), ("MNQ".into(), None));
        assert_eq!(root_of("esz25", known), ("ES".into(), None));
        assert_eq!(root_of("@ES", known), ("ES".into(), None));
        assert_eq!(root_of("ES=F", known), ("ES".into(), None));
        assert_eq!(root_of("/ZN", known), ("ZN".into(), None));
        assert_eq!(root_of("6EH7", known), ("6E".into(), None));
        assert_eq!(root_of("MNQ@CME", known), ("MNQ".into(), Some("CME".into())));
        // A tail that leaves no known root is not a month.
        assert_eq!(root_of("XYZ5", known), ("XYZ5".into(), None));
    }

    #[test]
    fn an_exchange_narrows_the_root_and_an_unlisted_one_finds_nothing() {
        assert_eq!(spec("ES@GLOBEX").market, "cme");
        assert_eq!(lookup("ES@XYZ").unwrap(), None);
        assert_eq!(lookup("NOPE").unwrap(), None);
    }

    #[test]
    fn a_root_listed_on_several_markets_is_refused_with_its_candidates() {
        let row = |market: &str| Spec {
            market: market.into(),
            root: "XX".into(),
            description: String::new(),
            currency: "USD".into(),
            pointvalue: 10.0,
            mintick: 0.5,
        };
        let specs = [row("cme"), row("eurex")];
        let e = lookup_in(&specs, "XXZ6").unwrap_err().to_string();
        assert!(e.contains("CME") && e.contains("EUREX"), "{e}");
        // The exchange settles it.
        assert_eq!(lookup_in(&specs, "XX.202612@EUREX").unwrap().map(|s| s.market), Some("eurex".into()));
    }

    #[test]
    fn stored_facts_win_and_only_the_empty_ones_are_filled() {
        let mut m = complete("ibkr", "future", "ESZ6", None).unwrap().unwrap();
        assert_eq!((m.mintick, m.pointvalue, m.currency.as_deref()), (Some(0.25), Some(50.0), Some("USD")));
        m.pointvalue = Some(5.0);
        m.mintick = None;
        let again = complete("ibkr", "future", "ESZ6", Some(m)).unwrap().unwrap();
        assert_eq!((again.mintick, again.pointvalue), (Some(0.25), Some(5.0)));
        // Not a future: untouched.
        assert!(complete("ibkr", "equity", "ES", None).unwrap().is_none());
    }

    #[test]
    fn a_short_download_is_not_adopted() {
        assert!(parse("market,symbol,type\ncme,ES,future,x,USD,50,0.25,1\n").is_err());
    }
}
