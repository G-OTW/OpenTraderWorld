//! Venues ported to the connector framework (`core/connectors`), served through the same
//! [`Connector`] trait as every other provider, so no module can tell the difference.
//!
//! The bridge is thin on purpose: it turns OTW's `(ticker, timeframe, from, to)` into a
//! canonical command, runs it through the framework's driver, and turns the canonical
//! answer back into OTW's `Bar`. Prices cross from `Decimal` to `f64` here, through their
//! text, so a ported venue stores exactly what its legacy connector stored.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Result};
use otw_connector_model::{
    AssetClass, Candle, Command, ConnectorError, CorrId, DataType, Event, HistoryItems, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::{Driver, DriverConfig, Mapper, Observer};
use time::OffsetDateTime;

use otw_store::histdata::{Bar, Quote, QuoteBar};

use super::{Capability, Chunk, Connector, SymbolHit};

/// Feeds the framework's HTTP exchanges to OTW's API-rate dashboard and Retry-After memory.
struct RateObserver(&'static str);

impl Observer for RateObserver {
    fn on_http(&self, url: &str, status: Option<u16>, retry_after: Option<&str>) {
        crate::rate::record_http(self.0, url, status, retry_after);
    }
}

/// A framework error, kept whole inside the `anyhow` chain so the download worker can read
/// its kind (`histdata_job::is_transient`).
fn err(e: ConnectorError) -> anyhow::Error {
    anyhow::Error::new(e)
}

/// A window crosses into the framework to the millisecond, so `[from, to)` is exact: a bar
/// that opened a fraction of a second before `from` is outside it.
fn to_ts(t: OffsetDateTime) -> Result<Timestamp> {
    let ms = i64::try_from(t.unix_timestamp_nanos() / 1_000_000).map_err(|_| anyhow!("timestamp {t} out of range"))?;
    Timestamp::from_millis(ms).ok_or_else(|| anyhow!("timestamp {t} out of range"))
}

/// `Decimal` to `f64` through the text, which is what every legacy connector parsed.
fn f(d: otw_connector_model::Decimal) -> Result<f64> {
    d.to_string().parse::<f64>().map_err(|e| anyhow!("number {d}: {e}"))
}

fn bar(c: &Candle) -> Result<Bar> {
    let ts = c.open_ts.to_offset_datetime().ok_or_else(|| anyhow!("bar time {} out of range", c.open_ts))?;
    Ok(Bar {
        ts,
        open: f(c.open.0)?,
        high: f(c.high.0)?,
        low: f(c.low.0)?,
        close: f(c.close.0)?,
        volume: f(c.volume.0)?,
        adj_open: adj(c, |a| a.open)?,
        adj_high: adj(c, |a| a.high)?,
        adj_low: adj(c, |a| a.low)?,
        adj_close: adj(c, |a| a.close)?,
    })
}

/// A candle's bid and ask, as OTW stores them next to the mid.
fn sides(s: &otw_connector_model::Sides) -> Result<Quote> {
    Ok(Quote {
        bid_open: f(s.bid.open.0)?,
        bid_high: f(s.bid.high.0)?,
        bid_low: f(s.bid.low.0)?,
        bid_close: f(s.bid.close.0)?,
        ask_open: f(s.ask.open.0)?,
        ask_high: f(s.ask.high.0)?,
        ask_low: f(s.ask.low.0)?,
        ask_close: f(s.ask.close.0)?,
    })
}

fn adj(c: &Candle, pick: impl Fn(&otw_connector_model::Adjusted) -> Option<otw_connector_model::Price>) -> Result<Option<f64>> {
    c.adjusted.as_ref().and_then(pick).map(|p| f(p.0)).transpose()
}

/// When a candle's period is over: the end of its bucket, and never less than one full
/// period after its stamp (a session stamped at its own local midnight, east of UTC, would
/// otherwise end while it still trades). The rule OTW's worker applies to every bar.
fn closes_at(c: &Candle) -> Option<Timestamp> {
    use otw_connector_engine::candles::{bucket_end, bucket_start};
    let end = bucket_end(c.tf, bucket_start(c.tf, c.open_ts)?)?;
    Some(match c.tf.fixed_secs() {
        Some(s) => end.max(c.open_ts.plus_secs(s)),
        None => end,
    })
}

fn now_ts() -> Timestamp {
    Timestamp::from_nanos(i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos()).unwrap_or(i64::MAX))
}

/// OTW's usual spelling of a market, for a search line.
fn plain_type(a: AssetClass) -> &'static str {
    match a {
        AssetClass::Equity => "equity",
        AssetClass::Etf => "etf",
        AssetClass::Spot | AssetClass::Perp => "crypto",
        AssetClass::Forex => "fx",
        AssetClass::Index => "index",
        AssetClass::Option => "option",
        AssetClass::Future => "future",
        AssetClass::Cfd => "cfd",
        AssetClass::Bond => "bond",
        AssetClass::Commodity => "commodity",
        AssetClass::Other => "other",
    }
}

/// The venue's id for an OTW ticker and asset type. An option is first written as the OCC
/// symbol OTW's own parser reads it as (refused when it is not one), so every venue receives
/// the same canonical contract whatever spelling the form submitted.
pub(crate) fn instrument_of<M: Mapper>(mapper: &M, ticker: &str, asset_type: &str) -> Result<otw_connector_model::InstrumentId> {
    let m = market(asset_type);
    let symbol = match m {
        Some(AssetClass::Option) => match super::parse_option_symbol(ticker) {
            Ok(c) => c.occ(),
            // A venue that reads its own option notation gets the symbol as typed.
            Err(_) if mapper.own_option_symbols() => ticker.to_string(),
            Err(e) => return Err(e),
        },
        _ => ticker.to_string(),
    };
    mapper.instrument_for(&symbol, m).map_err(err)
}

/// The market an OTW asset type names, for venues that route by it.
pub(crate) fn market(asset_type: &str) -> Option<AssetClass> {
    Some(match asset_type.trim().to_ascii_lowercase().as_str() {
        "equity" | "stock" | "stocks" => AssetClass::Equity,
        "etf" => AssetClass::Etf,
        "crypto" => AssetClass::Spot,
        "option" | "options" => AssetClass::Option,
        "future" | "futures" => AssetClass::Future,
        "fx" | "forex" => AssetClass::Forex,
        "index" => AssetClass::Index,
        "cfd" => AssetClass::Cfd,
        "bond" | "bonds" => AssetClass::Bond,
        "commodity" | "commodities" => AssetClass::Commodity,
        _ => return None,
    })
}

/// OTW's asset type for a canonical asset class.
fn asset_type(a: AssetClass) -> &'static str {
    match a {
        AssetClass::Spot | AssetClass::Perp => "crypto",
        AssetClass::Future => "futures",
        AssetClass::Option => "options",
        AssetClass::Equity => "stocks",
        AssetClass::Etf => "etf",
        AssetClass::Index => "index",
        AssetClass::Forex => "forex",
        AssetClass::Cfd => "cfd",
        AssetClass::Bond => "bonds",
        AssetClass::Commodity => "commodities",
        AssetClass::Other => "other",
    }
}

/// One of the books a connector setting chooses between (Bitget's `market`: spot or
/// USDT-M futures). Most venues have one, unnamed.
pub struct Variant<M: Mapper> {
    /// Accepted values of the setting, the first being the one shown.
    pub names: &'static [&'static str],
    /// Key of this book's instrument list in the in-process symbol cache.
    pub cache_key: &'static str,
    pub driver: Arc<Driver<M>>,
}

pub struct NativeHist<M: Mapper> {
    variants: Vec<Variant<M>>,
    /// The connector setting that picks a variant, when there are several.
    setting: Option<&'static str>,
    cap: &'static Capability,
}

/// Pick a variant by the connector's setting: empty = the first, otherwise one of its names.
pub(crate) fn pick<'a, M: Mapper>(
    variants: &'a [Variant<M>],
    setting: Option<&'static str>,
    label: &str,
    config: &HashMap<String, String>,
) -> Result<&'a Variant<M>> {
    let first = variants.first().ok_or_else(|| anyhow!("{label}: no adapter registered"))?;
    let Some(key) = setting else { return Ok(first) };
    let value = config.get(key).map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
    if value.is_empty() {
        return Ok(first);
    }
    variants.iter().find(|v| v.names.contains(&value.as_str())).ok_or_else(|| {
        let shown: Vec<String> = variants.iter().filter_map(|v| v.names.first()).map(|n| format!("\"{n}\"")).collect();
        anyhow!("the {label} {key} is {}, not \"{value}\"", shown.join(" or "))
    })
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> CorrId {
    CorrId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

impl<M: Mapper + Clone> NativeHist<M> {
    /// A timeframe the venue does not serve, folded by the engine from the one the planner
    /// picks (Alpaca's week from its days). The first bucket is read from its own start, so
    /// a chunk boundary never splits a period; each bucket is stamped as the venue's
    /// capabilities say.
    async fn folded(
        &self,
        driver: &Arc<Driver<M>>,
        signer: Option<&dyn otw_connector_sdk::Signer>,
        instrument: otw_connector_model::InstrumentId,
        tf: Timeframe,
        range: TimeRange,
    ) -> Result<Vec<Candle>> {
        use otw_connector_engine::candles::{aggregate_stamped, bucket_end, bucket_start};
        let caps = driver.mapper().capabilities();
        let plan = otw_connector_engine::plan(&otw_connector_engine::Request::HistoryCandles { tf, range: Some(range) }, caps, &otw_connector_engine::Prefs::default())
            .map_err(|u| anyhow!("{}", u.0))?;
        let Some(src) = plan.steps.iter().find_map(|s| match s {
            otw_connector_engine::Step::History(DataType::Candles { tf }) => Some(*tf),
            _ => None,
        }) else {
            return Err(anyhow!("{}: {tf} history would need {:?}, which this download does not run", caps.label, plan.steps));
        };
        let start = bucket_start(tf, range.from).unwrap_or(range.from);
        let pages = routed(driver)
            .history_as(next_id(), &instrument, &DataType::Candles { tf: src }, TimeRange::new(start, range.to), signer)
            .await
            .map_err(|e| self.fail(driver, e))?;
        let mut source: Vec<Candle> = pages
            .into_iter()
            .flat_map(|p| match p {
                HistoryItems::Candles(c) => c,
                HistoryItems::Trades(_) => Vec::new(),
            })
            .collect();
        source.sort_by_key(|c| c.open_ts);
        let stamp = caps.history.candles.as_ref().map(|h| h.aggregate_stamp).unwrap_or_default();
        // A bucket a past window end cuts holds part of its period only. A venue with no
        // closed flag cannot say so, and a past period passes the period-end test: drop it
        // here, the window that reaches its end reads it whole. A bucket still running is
        // the forming bar, left to the caller's closed test.
        let now = now_ts();
        let whole = |c: &Candle| {
            bucket_start(tf, c.open_ts).and_then(|s| bucket_end(tf, s)).is_some_and(|end| end <= range.to || end > now)
        };
        Ok(aggregate_stamped(&source, tf, stamp).into_iter().filter(|c| c.open_ts < range.to && whole(c)).collect())
    }

    /// One page of bars for `[from, to)`, closed ones only unless `forming`.
    #[allow(clippy::too_many_arguments)]
    async fn bars(
        &self,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        forming: bool,
    ) -> Result<Chunk> {
        let driver = &self.pick(secrets)?.driver;
        let instrument = instrument_of(driver.mapper(), ticker, asset_type)?;
        let tf: Timeframe = timeframe.parse().map_err(|e| anyhow!("{e}"))?;
        let range = TimeRange::new(to_ts(from)?, to_ts(to)?);
        let caps = driver.mapper().capabilities();
        let signer = self.data_signer(driver, secrets)?;
        let candles = if caps.history_timeframes().contains(&tf) {
            // One page per call: the worker advances `from` itself and calls again.
            let cmd = Command::RequestHistory { req_id: next_id(), instrument, data_type: DataType::Candles { tf }, range, cursor: None };
            let mut cs = Vec::new();
            for env in routed(driver).execute_as(&cmd, signer.as_deref()).await.map_err(|e| self.fail(driver, e))? {
                match env.event {
                    Event::HistoryChunk { items: HistoryItems::Candles(c), .. } => cs.extend(c),
                    Event::Error(e) => return Err(err(e)),
                    _ => {}
                }
            }
            cs
        } else {
            self.folded(driver, signer.as_deref(), instrument, tf, range).await?
        };
        // A bar still forming is not a period yet. A venue with no closed flag sends every
        // bar forming: the period's end decides. The live seed asks for it anyway, as the
        // period in progress.
        let now = now_ts();
        let flagged = caps.history.candles.as_ref().is_none_or(|h| h.final_flag);
        let closed = |c: &Candle| forming || c.is_final || !flagged && closes_at(c).is_some_and(|end| end <= now);
        let mut bars = Vec::new();
        let mut quotes = Vec::new();
        for c in candles.iter().filter(|c| closed(c)) {
            let b = bar(c)?;
            if let Some(s) = &c.sides {
                quotes.push(QuoteBar { ts: b.ts, quote: sides(s)? });
            }
            bars.push(b);
        }
        Ok(Chunk { bars, quotes })
    }

    /// The credentials a keyed venue's market data needs; none for a keyless one.
    fn data_signer(&self, driver: &Driver<M>, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn otw_connector_sdk::Signer>>> {
        if driver.mapper().capabilities().auth == otw_connector_model::AuthKind::None {
            return Ok(None);
        }
        driver.mapper().signer(secrets).map_err(err)
    }

    /// A framework error for OTW. A rate limit the venue wrote in an answer's body (no 429
    /// for the HTTP observer to see) is noted on the API-rate dashboard here.
    fn fail(&self, driver: &Driver<M>, e: ConnectorError) -> anyhow::Error {
        if e.kind == otw_connector_model::ErrorKind::RateLimited && e.provider_code.as_deref() != Some("429") {
            let url = driver.base_url("rest").unwrap_or_default();
            crate::rate::note_limited(self.cap.provider, &url, &e.message);
        }
        err(e)
    }

    pub fn new(driver: Arc<Driver<M>>, cap: &'static Capability) -> Self {
        NativeHist { variants: vec![Variant { names: &[], cache_key: cap.provider, driver }], setting: None, cap }
    }

    /// A connector whose setting `setting` picks one of these books.
    pub fn with_variants(setting: &'static str, variants: Vec<Variant<M>>, cap: &'static Capability) -> Self {
        NativeHist { variants, setting: Some(setting), cap }
    }

    fn pick(&self, config: &HashMap<String, String>) -> Result<&Variant<M>> {
        pick(&self.variants, self.setting, self.cap.label, config)
    }

    /// The asset type OTW files this instrument under: the canonical class's, when the
    /// connector serves that type, else the connector's own (a futures contract on a venue
    /// OTW lists as `crypto` is filed as crypto).
    fn otw_asset_type(&self, a: AssetClass) -> &'static str {
        // OTW spells some types two ways across connectors (`equity` / `stocks`).
        let spellings: &[&'static str] = match a {
            AssetClass::Equity => &["equity", "stocks"],
            AssetClass::Forex => &["fx", "forex"],
            AssetClass::Option => &["option", "options"],
            AssetClass::Future => &["future", "futures"],
            _ => &[],
        };
        let t = asset_type(a);
        if let Some(s) = spellings.iter().chain(std::iter::once(&t)).find(|s| self.cap.asset_types.contains(s)) {
            return s;
        }
        self.cap.asset_types.first().copied().unwrap_or(t)
    }
}

/// The driver to use for this call. Outside tests, the shared one. In tests, when the
/// thread is pointed at a mock server (`brokers::serve_from`), a driver on that server, so
/// the mock tests drive the venues' adapters against it unchanged.
pub(crate) fn routed<M: Mapper + Clone>(d: &Arc<Driver<M>>) -> Arc<Driver<M>> {
    #[cfg(test)]
    {
        let probe = crate::brokers::at("probe:");
        if probe != "probe:" {
            let mut cfg = DriverConfig::new(&d.connector().0);
            for (k, _) in d.mapper().base_urls() {
                cfg = cfg.base(k, &probe);
            }
            return Arc::new(Driver::new(d.mapper().clone(), cfg, reqwest::Client::new()));
        }
    }
    d.clone()
}

/// The process-wide driver for a venue: one instrument cache and one session, shared by
/// the worker, the chart and the symbol search.
pub fn shared_driver<M: Mapper>(
    cell: &'static OnceLock<Option<Arc<Driver<M>>>>,
    provider: &'static str,
    make: impl FnOnce() -> Option<M>,
) -> Option<Arc<Driver<M>>> {
    cell.get_or_init(|| {
        let mapper = make()?;
        let http = super::client().ok()?;
        Some(Arc::new(
            Driver::new(mapper, DriverConfig::new(provider), http).with_observer(Arc::new(RateObserver(provider))),
        ))
    })
    .clone()
}

#[async_trait::async_trait]
impl<M: Mapper + Clone> Connector for NativeHist<M> {
    fn capability(&self) -> &'static Capability {
        self.cap
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        self.pick(config)?.driver.mapper().check_settings(config).map_err(err)
    }

    /// The key against the venue: its instrument list, read with it.
    async fn test(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<String> {
        let driver = &self.pick(secrets)?.driver;
        let signer = self.data_signer(driver, secrets)?;
        if let Some(probe) = driver.mapper().probe_symbol() {
            let id = driver.mapper().instrument_id(probe).map_err(err)?;
            let found = routed(driver).resolve_instruments(next_id(), &[id], signer.as_deref()).await.map_err(|e| self.fail(driver, e))?;
            let spec = found.first().ok_or_else(|| anyhow!("the key reached {} but market data answered nothing. Check the sign-in granted market data.", self.cap.label))?;
            return Ok(format!("market data reached, {} priced on {} in {}", spec.id.native, spec.exchange.clone().unwrap_or_default(), spec.quote));
        }
        let specs = routed(driver).load_instruments_as(next_id(), signer.as_deref()).await.map_err(|e| self.fail(driver, e))?;
        Ok(format!("key accepted, {} instrument(s) listed", specs.len()))
    }

    async fn fetch_chunk(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Chunk> {
        self.bars(secrets, ticker, asset_type, timeframe, from, to, false).await
    }

    async fn fetch_seed(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Chunk> {
        self.bars(secrets, ticker, asset_type, timeframe, from, to, true).await
    }

    async fn best_quote(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
    ) -> Result<(f64, f64)> {
        let driver = &self.pick(secrets)?.driver;
        let instrument = instrument_of(driver.mapper(), ticker, asset_type)?;
        let cmd = Command::RequestSnapshot { req_id: next_id(), instrument, data_type: DataType::Quote };
        let signer = self.data_signer(driver, secrets)?;
        for env in routed(driver).execute_as(&cmd, signer.as_deref()).await.map_err(|e| self.fail(driver, e))? {
            match env.event {
                Event::Quote(q) if q.bid.0.is_sign_positive() && !q.bid.is_zero() && q.ask.0.is_sign_positive() && !q.ask.is_zero() => {
                    return Ok((f(q.bid.0)?, f(q.ask.0)?));
                }
                Event::Error(e) => return Err(err(e)),
                _ => {}
            }
        }
        Err(anyhow!("{}: no two-sided price right now", self.cap.provider))
    }

    /// The venue's whole list, fetched once per process window and filtered locally.
    async fn search_symbols(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        query: &str,
        asset_type: &str,
        limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        let variant = self.pick(secrets)?;
        let driver = &variant.driver;
        // A venue with no list to load answers each query itself, in its own order.
        if driver.mapper().capabilities().instrument_discovery == otw_connector_model::Discovery::SearchOnly {
            let q = query.trim();
            if q.is_empty() {
                return Ok(Vec::new());
            }
            let cmd = Command::SearchInstruments { req_id: next_id(), query: q.to_string(), asset_class: market(asset_type) };
            let mut hits = Vec::new();
            let signer = self.data_signer(driver, secrets)?;
            for env in routed(driver).execute_as(&cmd, signer.as_deref()).await.map_err(|e| self.fail(driver, e))? {
                match env.event {
                    Event::Instrument { spec } => hits.push(SymbolHit {
                        symbol: spec.id.native.clone(),
                        name: spec.symbol.0.clone(),
                        // As the venue classed it, whatever the connector downloads.
                        asset_type: plain_type(spec.asset_class).into(),
                        exchange: driver.mapper().search_exchange(&spec),
                    }),
                    Event::Error(e) => return Err(err(e)),
                    _ => {}
                }
            }
            hits.truncate(limit);
            return Ok(hits);
        }
        // A venue listing several markets answers the one asked for.
        let caps = driver.mapper().capabilities();
        let mut markets: Vec<&'static str> = caps.asset_classes.iter().map(|a| self.otw_asset_type(*a)).collect();
        markets.sort_unstable();
        markets.dedup();
        let want = (markets.len() > 1).then(|| market(asset_type).map(|m| self.otw_asset_type(m)).unwrap_or_else(|| {
            // None named: the stock market when the venue has one, as OTW's search form means.
            let first = if caps.asset_classes.contains(&AssetClass::Equity) { AssetClass::Equity } else { caps.asset_classes.iter().next().copied().unwrap_or(AssetClass::Equity) };
            self.otw_asset_type(first)
        }));
        let narrow = |all: Vec<SymbolHit>| -> Vec<SymbolHit> {
            match want {
                Some(w) => all.into_iter().filter(|h| h.asset_type == w).collect(),
                None => all,
            }
        };
        // A venue that serves instruments per account caches one list per account.
        let cache_key = match secrets.get("account_id").map(|a| a.trim()).filter(|a| !a.is_empty()) {
            Some(a) => format!("{}:{a}", variant.cache_key),
            None => variant.cache_key.to_string(),
        };
        if let Some(all) = super::cached_symbols(&cache_key) {
            return Ok(super::filter_symbols(&narrow(all), query, limit));
        }
        let signer = self.data_signer(driver, secrets)?;
        let specs = routed(driver).load_instruments_as(next_id(), signer.as_deref()).await.map_err(|e| self.fail(driver, e))?;
        let all: Vec<SymbolHit> = specs
            .iter()
            .filter(|s| s.active)
            .map(|s| SymbolHit {
                symbol: s.id.native.clone(),
                name: s.symbol.0.clone(),
                asset_type: self.otw_asset_type(s.asset_class).into(),
                exchange: driver.mapper().search_exchange(s),
            })
            .collect();
        super::cache_symbols(&cache_key, &all);
        Ok(super::filter_symbols(&narrow(all), query, limit))
    }

    /// The candles' own bid and ask: the bars' request carries them.
    #[allow(clippy::too_many_arguments)]
    async fn fetch_quote_bars(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Vec<QuoteBar>> {
        if self.quote_history(asset_type) != super::QuoteHistory::Candles {
            return Err(anyhow!("{} publishes no historical bid/ask candles", self.cap.label));
        }
        Ok(self.fetch_chunk(client, secrets, ticker, asset_type, timeframe, from, to).await?.quotes)
    }

    /// Past bid/ask: on every candle when the venue's candles carry both sides, otherwise
    /// one quote per moment for the markets the venue declares.
    fn quote_history(&self, asset_type: &str) -> super::QuoteHistory {
        if self.variants.first().is_some_and(|v| v.driver.mapper().capabilities().history.candles.as_ref().is_some_and(|h| h.sides)) {
            return super::QuoteHistory::Candles;
        }
        let declared = self.variants.first().is_some_and(|v| market(asset_type).is_some_and(|m| v.driver.mapper().capabilities().history.quotes_at.contains(&m)));
        if declared {
            super::QuoteHistory::Ticks
        } else {
            super::QuoteHistory::None
        }
    }

    async fn quote_at(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        at: OffsetDateTime,
        before: bool,
    ) -> Result<Option<(f64, f64)>> {
        let driver = &self.pick(secrets)?.driver;
        let instrument = instrument_of(driver.mapper(), ticker, asset_type)?;
        let signer = self.data_signer(driver, secrets)?;
        let q = routed(driver).quote_at(next_id(), &instrument, to_ts(at)?, before, signer.as_deref()).await.map_err(|e| self.fail(driver, e))?;
        q.map(|q| Ok((f(q.bid.0)?, f(q.ask.0)?))).transpose()
    }
}

// ── Registered venues ─────────────────────────────────────────────────────────

use otw_connector_providers::binance::Binance;

static BINANCE: OnceLock<Option<Arc<Driver<Binance>>>> = OnceLock::new();
static BINANCE_USDM: OnceLock<Option<Arc<Driver<Binance>>>> = OnceLock::new();

pub(crate) fn binance_driver() -> Option<Arc<Driver<Binance>>> {
    shared_driver(&BINANCE, "binance", || Binance::spot().ok())
}

pub(crate) fn binance_futures_driver() -> Option<Arc<Driver<Binance>>> {
    shared_driver(&BINANCE_USDM, "binance_futures", || Binance::usdm().ok())
}

static BINANCE_USDM_TESTNET: OnceLock<Option<Arc<Driver<Binance>>>> = OnceLock::new();

/// The futures testnet: the same adapter on the host its capabilities declare.
pub(crate) fn binance_futures_testnet_driver() -> Option<Arc<Driver<Binance>>> {
    BINANCE_USDM_TESTNET
        .get_or_init(|| {
            let mapper = Binance::usdm().ok()?;
            let url = mapper.capabilities().regions.sandbox_url.clone()?;
            let cfg = DriverConfig::new("binance_futures").base("rest", &url);
            Some(Arc::new(Driver::new(mapper, cfg, super::client().ok()?).with_observer(Arc::new(RateObserver("binance_futures")))))
        })
        .clone()
}

/// Binance spot through the framework, with its connector's UI capability.
pub fn binance() -> Option<NativeHist<Binance>> {
    Some(NativeHist::new(binance_driver()?, &super::caps::BINANCE))
}

use otw_connector_providers::okx::Okx;

static OKX: OnceLock<Option<Arc<Driver<Okx>>>> = OnceLock::new();

pub(crate) fn okx_driver() -> Option<Arc<Driver<Okx>>> {
    shared_driver(&OKX, "okx", || Okx::new().ok())
}

/// OKX through the framework.
pub fn okx() -> Option<NativeHist<Okx>> {
    Some(NativeHist::new(okx_driver()?, &super::caps::OKX))
}

use otw_connector_providers::bitget::Bitget;

static BITGET_SPOT: OnceLock<Option<Arc<Driver<Bitget>>>> = OnceLock::new();
static BITGET_FUTURES: OnceLock<Option<Arc<Driver<Bitget>>>> = OnceLock::new();

pub(crate) fn bitget_drivers() -> Option<(Arc<Driver<Bitget>>, Arc<Driver<Bitget>>)> {
    Some((
        shared_driver(&BITGET_SPOT, "bitget", || Bitget::spot().ok())?,
        shared_driver(&BITGET_FUTURES, "bitget", || Bitget::usdt_futures().ok())?,
    ))
}

/// The two Bitget books a connector's `market` setting chooses between.
pub(crate) fn bitget_variants() -> Option<Vec<Variant<Bitget>>> {
    let (spot, futures) = bitget_drivers()?;
    Some(vec![
        Variant { names: &["spot"], cache_key: "bitget:spot", driver: spot },
        Variant { names: &["usdt-futures", "futures"], cache_key: "bitget:usdt-futures", driver: futures },
    ])
}

/// Bitget through the framework: spot or USDT-M futures by the connector's `market`.
pub fn bitget() -> Option<NativeHist<Bitget>> {
    Some(NativeHist::with_variants("market", bitget_variants()?, &super::caps::BITGET))
}

use otw_connector_providers::bybit::Bybit;

static BYBIT_SPOT: OnceLock<Option<Arc<Driver<Bybit>>>> = OnceLock::new();
static BYBIT_LINEAR: OnceLock<Option<Arc<Driver<Bybit>>>> = OnceLock::new();
static BYBIT_INVERSE: OnceLock<Option<Arc<Driver<Bybit>>>> = OnceLock::new();
static BYBIT_OPTION: OnceLock<Option<Arc<Driver<Bybit>>>> = OnceLock::new();
static BYBIT_EU: OnceLock<Option<Arc<Driver<Bybit>>>> = OnceLock::new();

/// The books of bybit.com, one adapter each.
pub(crate) struct BybitDrivers {
    pub spot: Arc<Driver<Bybit>>,
    pub linear: Arc<Driver<Bybit>>,
    pub inverse: Arc<Driver<Bybit>>,
    pub option: Arc<Driver<Bybit>>,
}

pub(crate) fn bybit_drivers() -> Option<BybitDrivers> {
    Some(BybitDrivers {
        spot: shared_driver(&BYBIT_SPOT, "bybit", || Bybit::spot().ok())?,
        linear: shared_driver(&BYBIT_LINEAR, "bybit", || Bybit::linear().ok())?,
        inverse: shared_driver(&BYBIT_INVERSE, "bybit", || Bybit::inverse().ok())?,
        option: shared_driver(&BYBIT_OPTION, "bybit", || Bybit::option().ok())?,
    })
}

pub(crate) fn bybit_eu_driver() -> Option<Arc<Driver<Bybit>>> {
    shared_driver(&BYBIT_EU, "bybit_eu", || Bybit::eu_spot().ok())
}

/// The Bybit books with candles a connector's `market` setting chooses between (options
/// publish none).
pub(crate) fn bybit_variants() -> Option<Vec<Variant<Bybit>>> {
    let d = bybit_drivers()?;
    Some(vec![
        Variant { names: &["spot"], cache_key: "bybit:spot", driver: d.spot },
        Variant { names: &["linear"], cache_key: "bybit:linear", driver: d.linear },
        Variant { names: &["inverse"], cache_key: "bybit:inverse", driver: d.inverse },
    ])
}

/// Bybit through the framework: spot, linear or inverse by the connector's `market`.
pub fn bybit() -> Option<NativeHist<Bybit>> {
    Some(NativeHist::with_variants("market", bybit_variants()?, &super::caps::BYBIT))
}

/// Bybit EU's spot book, its own venue.
pub fn bybit_eu() -> Option<NativeHist<Bybit>> {
    Some(NativeHist::new(bybit_eu_driver()?, &super::caps::BYBIT_EU))
}

use otw_connector_providers::gateio::Gateio;

static GATEIO_SPOT: OnceLock<Option<Arc<Driver<Gateio>>>> = OnceLock::new();
static GATEIO_FUTURES: OnceLock<Option<Arc<Driver<Gateio>>>> = OnceLock::new();

pub(crate) fn gateio_drivers() -> Option<(Arc<Driver<Gateio>>, Arc<Driver<Gateio>>)> {
    Some((shared_driver(&GATEIO_SPOT, "gateio", || Gateio::spot().ok())?, shared_driver(&GATEIO_FUTURES, "gateio", || Gateio::futures().ok())?))
}

/// The two Gate.io books a connector's `market` setting chooses between.
pub(crate) fn gateio_variants() -> Option<Vec<Variant<Gateio>>> {
    let (spot, futures) = gateio_drivers()?;
    Some(vec![
        Variant { names: &["spot"], cache_key: "gateio:spot", driver: spot },
        Variant { names: &["futures", "usdt-futures"], cache_key: "gateio:futures", driver: futures },
    ])
}

/// Gate.io through the framework: spot or USDT perpetuals by the connector's `market`.
pub fn gateio() -> Option<NativeHist<Gateio>> {
    Some(NativeHist::with_variants("market", gateio_variants()?, &super::caps::GATEIO))
}

use otw_connector_providers::mexc::Mexc;

static MEXC_SPOT: OnceLock<Option<Arc<Driver<Mexc>>>> = OnceLock::new();
static MEXC_FUTURES: OnceLock<Option<Arc<Driver<Mexc>>>> = OnceLock::new();

pub(crate) fn mexc_drivers() -> Option<(Arc<Driver<Mexc>>, Arc<Driver<Mexc>>)> {
    Some((shared_driver(&MEXC_SPOT, "mexc", || Mexc::spot().ok())?, shared_driver(&MEXC_FUTURES, "mexc", || Mexc::futures().ok())?))
}

/// The two MEXC books a connector's `market` setting chooses between.
pub(crate) fn mexc_variants() -> Option<Vec<Variant<Mexc>>> {
    let (spot, futures) = mexc_drivers()?;
    Some(vec![
        Variant { names: &["spot"], cache_key: "mexc:spot", driver: spot },
        Variant { names: &["futures"], cache_key: "mexc:futures", driver: futures },
    ])
}

/// MEXC through the framework: spot or the perpetuals by the connector's `market`.
pub fn mexc() -> Option<NativeHist<Mexc>> {
    Some(NativeHist::with_variants("market", mexc_variants()?, &super::caps::MEXC))
}

use otw_connector_providers::bitfinex::Bitfinex;

static BITFINEX_SPOT: OnceLock<Option<Arc<Driver<Bitfinex>>>> = OnceLock::new();
static BITFINEX_PERP: OnceLock<Option<Arc<Driver<Bitfinex>>>> = OnceLock::new();

/// The spot adapter also reads the whole account: one key sees both books.
pub(crate) fn bitfinex_driver() -> Option<Arc<Driver<Bitfinex>>> {
    shared_driver(&BITFINEX_SPOT, "bitfinex", || Bitfinex::spot().ok())
}

/// The two Bitfinex books a connector's `market` setting chooses between.
pub(crate) fn bitfinex_variants() -> Option<Vec<Variant<Bitfinex>>> {
    let perp = shared_driver(&BITFINEX_PERP, "bitfinex", || Bitfinex::derivatives().ok())?;
    Some(vec![
        Variant { names: &["spot"], cache_key: "bitfinex:spot", driver: bitfinex_driver()? },
        Variant { names: &["derivatives", "perp", "futures"], cache_key: "bitfinex:derivatives", driver: perp },
    ])
}

/// Bitfinex through the framework: spot pairs or the perpetuals by the connector's `market`.
pub fn bitfinex() -> Option<NativeHist<Bitfinex>> {
    Some(NativeHist::with_variants("market", bitfinex_variants()?, &super::caps::BITFINEX))
}

use otw_connector_providers::deribit::Deribit;

static DERIBIT: OnceLock<Option<Arc<Driver<Deribit>>>> = OnceLock::new();

pub(crate) fn deribit_driver() -> Option<Arc<Driver<Deribit>>> {
    shared_driver(&DERIBIT, "deribit", || Deribit::new().ok())
}

/// Deribit through the framework: every kind on one book, the name saying which.
pub fn deribit() -> Option<NativeHist<Deribit>> {
    Some(NativeHist::new(deribit_driver()?, &super::caps::DERIBIT))
}

use otw_connector_providers::kucoin::Kucoin;

static KUCOIN: OnceLock<Option<Arc<Driver<Kucoin>>>> = OnceLock::new();
static KUCOIN_FUTURES: OnceLock<Option<Arc<Driver<Kucoin>>>> = OnceLock::new();

pub(crate) fn kucoin_driver() -> Option<Arc<Driver<Kucoin>>> {
    shared_driver(&KUCOIN, "kucoin", || Kucoin::spot().ok())
}

pub(crate) fn kucoin_futures_driver() -> Option<Arc<Driver<Kucoin>>> {
    shared_driver(&KUCOIN_FUTURES, "kucoin_futures", || Kucoin::futures().ok())
}

/// KuCoin spot through the framework.
pub fn kucoin() -> Option<NativeHist<Kucoin>> {
    Some(NativeHist::new(kucoin_driver()?, &super::caps::KUCOIN))
}

/// KuCoin Futures through the framework: its own platform, host and symbols.
pub fn kucoin_futures() -> Option<NativeHist<Kucoin>> {
    Some(NativeHist::new(kucoin_futures_driver()?, &super::caps::KUCOIN_FUTURES))
}

use otw_connector_providers::alpaca::{Alpaca, Feed};

static ALPACA_IEX: OnceLock<Option<Arc<Driver<Alpaca>>>> = OnceLock::new();
static ALPACA_SIP: OnceLock<Option<Arc<Driver<Alpaca>>>> = OnceLock::new();
static ALPACA_PAPER: OnceLock<Option<Arc<Driver<Alpaca>>>> = OnceLock::new();

/// Alpaca's two entitlements (the connector's `feed`): they differ by the live socket only.
pub(crate) fn alpaca_variants() -> Option<Vec<Variant<Alpaca>>> {
    Some(vec![
        Variant { names: &["iex"], cache_key: "alpaca", driver: shared_driver(&ALPACA_IEX, "alpaca", || Alpaca::new(Feed::Iex).ok())? },
        Variant { names: &["sip"], cache_key: "alpaca", driver: shared_driver(&ALPACA_SIP, "alpaca", || Alpaca::new(Feed::Sip).ok())? },
    ])
}

/// A venue's test environment: the same adapter with the account's host moved to the one
/// its capabilities declare.
pub(crate) fn sandbox_driver<M: Mapper>(cell: &'static OnceLock<Option<Arc<Driver<M>>>>, provider: &'static str, make: impl FnOnce() -> Option<M>) -> Option<Arc<Driver<M>>> {
    cell.get_or_init(|| {
        let mapper = make()?;
        let url = mapper.capabilities().regions.sandbox_url.clone()?;
        let key = mapper.capabilities().regions.sandbox_base.clone().unwrap_or_else(|| "rest".into());
        let cfg = DriverConfig::new(provider).base(&key, &url);
        Some(Arc::new(Driver::new(mapper, cfg, super::client().ok()?).with_observer(Arc::new(RateObserver(provider)))))
    })
    .clone()
}

pub(crate) fn alpaca_paper_driver() -> Option<Arc<Driver<Alpaca>>> {
    sandbox_driver(&ALPACA_PAPER, "alpaca", || Alpaca::new(Feed::Iex).ok())
}

/// Alpaca through the framework: the `feed` setting picks the entitlement.
pub fn alpaca() -> Option<NativeHist<Alpaca>> {
    Some(NativeHist::with_variants("feed", alpaca_variants()?, &super::caps::ALPACA))
}

use otw_connector_providers::forexcom::ForexCom;

static FOREXCOM: OnceLock<Option<Arc<Driver<ForexCom>>>> = OnceLock::new();

pub(crate) fn forexcom_driver() -> Option<Arc<Driver<ForexCom>>> {
    shared_driver(&FOREXCOM, "forexcom", || ForexCom::new().ok())
}

/// FOREX.com through the framework.
pub fn forexcom() -> Option<NativeHist<ForexCom>> {
    Some(NativeHist::new(forexcom_driver()?, &super::caps::FOREXCOM))
}

use otw_connector_providers::capitalcom::CapitalCom;

static CAPITALCOM_LIVE: OnceLock<Option<Arc<Driver<CapitalCom>>>> = OnceLock::new();
static CAPITALCOM_DEMO: OnceLock<Option<Arc<Driver<CapitalCom>>>> = OnceLock::new();

/// Capital.com's two platforms (the connector's `env`), each its own API host; the socket
/// host is shared.
pub(crate) fn capitalcom_variants() -> Option<Vec<Variant<CapitalCom>>> {
    Some(vec![
        Variant { names: &["live"], cache_key: "capitalcom", driver: shared_driver(&CAPITALCOM_LIVE, "capitalcom", || CapitalCom::new().ok())? },
        Variant { names: &["demo"], cache_key: "capitalcom-demo", driver: capitalcom_demo_driver()? },
    ])
}

pub(crate) fn capitalcom_demo_driver() -> Option<Arc<Driver<CapitalCom>>> {
    sandbox_driver(&CAPITALCOM_DEMO, "capitalcom", || CapitalCom::new().ok())
}

/// Capital.com through the framework: the `env` setting picks the host.
pub fn capitalcom() -> Option<NativeHist<CapitalCom>> {
    Some(NativeHist::with_variants("env", capitalcom_variants()?, &super::caps::CAPITALCOM))
}

use otw_connector_providers::tradestation::TradeStation;

static TRADESTATION_LIVE: OnceLock<Option<Arc<Driver<TradeStation>>>> = OnceLock::new();
static TRADESTATION_SIM: OnceLock<Option<Arc<Driver<TradeStation>>>> = OnceLock::new();

/// TradeStation's two environments (the connector's `env`), each its own API host; the
/// sign-in host is shared.
pub(crate) fn tradestation_variants() -> Option<Vec<Variant<TradeStation>>> {
    Some(vec![
        Variant { names: &["live"], cache_key: "tradestation", driver: shared_driver(&TRADESTATION_LIVE, "tradestation", || TradeStation::new().ok())? },
        Variant { names: &["sim"], cache_key: "tradestation-sim", driver: tradestation_sim_driver()? },
    ])
}

pub(crate) fn tradestation_sim_driver() -> Option<Arc<Driver<TradeStation>>> {
    sandbox_driver(&TRADESTATION_SIM, "tradestation", || TradeStation::new().ok())
}

/// TradeStation through the framework: the `env` setting picks the host.
pub fn tradestation() -> Option<NativeHist<TradeStation>> {
    Some(NativeHist::with_variants("env", tradestation_variants()?, &super::caps::TRADESTATION))
}

use otw_connector_providers::oanda::Oanda;

static OANDA_LIVE: OnceLock<Option<Arc<Driver<Oanda>>>> = OnceLock::new();
static OANDA_PRACTICE: OnceLock<Option<Arc<Driver<Oanda>>>> = OnceLock::new();

/// OANDA's two environments (the connector's `env`), each its own host.
pub(crate) fn oanda_variants() -> Option<Vec<Variant<Oanda>>> {
    Some(vec![
        Variant { names: &["live"], cache_key: "oanda", driver: shared_driver(&OANDA_LIVE, "oanda", || Oanda::new().ok())? },
        Variant { names: &["practice"], cache_key: "oanda-practice", driver: oanda_practice_driver()? },
    ])
}

pub(crate) fn oanda_practice_driver() -> Option<Arc<Driver<Oanda>>> {
    sandbox_driver(&OANDA_PRACTICE, "oanda", || Oanda::new().ok())
}

/// OANDA through the framework: the `env` setting picks the host.
pub fn oanda() -> Option<NativeHist<Oanda>> {
    Some(NativeHist::with_variants("env", oanda_variants()?, &super::caps::OANDA))
}

use otw_connector_providers::alphavantage::AlphaVantage;

static ALPHAVANTAGE: OnceLock<Option<Arc<Driver<AlphaVantage>>>> = OnceLock::new();

/// Alpha Vantage through the framework.
pub fn alphavantage() -> Option<NativeHist<AlphaVantage>> {
    Some(NativeHist::new(shared_driver(&ALPHAVANTAGE, "alphavantage", || AlphaVantage::new().ok())?, &super::caps::ALPHAVANTAGE))
}

use otw_connector_providers::eodhd::Eodhd;

static EODHD: OnceLock<Option<Arc<Driver<Eodhd>>>> = OnceLock::new();

/// EODHD through the framework.
pub fn eodhd() -> Option<NativeHist<Eodhd>> {
    Some(NativeHist::new(shared_driver(&EODHD, "eodhd", || Eodhd::new().ok())?, &super::caps::EODHD))
}

use otw_connector_providers::massive::Massive;

static MASSIVE: OnceLock<Option<Arc<Driver<Massive>>>> = OnceLock::new();

pub(crate) fn massive_driver() -> Option<Arc<Driver<Massive>>> {
    shared_driver(&MASSIVE, "massive", || Massive::new().ok())
}

/// Massive through the framework.
pub fn massive() -> Option<NativeHist<Massive>> {
    Some(NativeHist::new(massive_driver()?, &super::caps::MASSIVE))
}

use otw_connector_providers::yahoo::Yahoo;

static YAHOO: OnceLock<Option<Arc<Driver<Yahoo>>>> = OnceLock::new();

/// Yahoo through the framework.
pub fn yahoo() -> Option<NativeHist<Yahoo>> {
    Some(NativeHist::new(shared_driver(&YAHOO, "yahoo", || Yahoo::new().ok())?, &super::caps::YAHOO))
}

use otw_connector_providers::coinbase::Coinbase;

static COINBASE: OnceLock<Option<Arc<Driver<Coinbase>>>> = OnceLock::new();

pub(crate) fn coinbase_driver() -> Option<Arc<Driver<Coinbase>>> {
    shared_driver(&COINBASE, "coinbase", || Coinbase::new().ok())
}

/// Coinbase through the framework.
pub fn coinbase() -> Option<NativeHist<Coinbase>> {
    Some(NativeHist::new(coinbase_driver()?, &super::caps::COINBASE))
}

use otw_connector_providers::ibkr::flex::IbkrFlex;

static IBKR_FLEX: OnceLock<Option<Arc<Driver<IbkrFlex>>>> = OnceLock::new();

/// IBKR Flex: a statement is collected again every five seconds while IBKR builds it, paced
/// by the declared limit; a statement takes longer to answer than a market-data call.
pub(crate) fn ibkr_flex_driver() -> Option<Arc<Driver<IbkrFlex>>> {
    IBKR_FLEX
        .get_or_init(|| {
            let mapper = IbkrFlex::new().ok()?;
            let limiter = Arc::new(otw_connector_engine::limiter::TokenBucketLimiter::new(&mapper.capabilities().rate_limits));
            let mut cfg = DriverConfig::new("ibkr");
            cfg.timeout = std::time::Duration::from_secs(45);
            Some(Arc::new(Driver::new(mapper, cfg, crate::brokers::client().ok()?).with_observer(Arc::new(RateObserver("ibkr"))).with_limiter(limiter)))
        })
        .clone()
}

use otw_connector_providers::kraken::Kraken;

static KRAKEN: OnceLock<Option<Arc<Driver<Kraken>>>> = OnceLock::new();

/// Kraken's driver paces the account's fill walk by its declared limits (the private-call
/// counter decays slowly), as the legacy connector did by hand.
pub(crate) fn kraken_driver() -> Option<Arc<Driver<Kraken>>> {
    KRAKEN
        .get_or_init(|| {
            let mapper = Kraken::new().ok()?;
            let limiter = Arc::new(otw_connector_engine::limiter::TokenBucketLimiter::new(&mapper.capabilities().rate_limits));
            Some(Arc::new(
                Driver::new(mapper, DriverConfig::new("kraken"), super::client().ok()?)
                    .with_observer(Arc::new(RateObserver("kraken")))
                    .with_limiter(limiter),
            ))
        })
        .clone()
}

/// Kraken through the framework.
pub fn kraken() -> Option<NativeHist<Kraken>> {
    Some(NativeHist::new(kraken_driver()?, &super::caps::KRAKEN))
}

/// Binance USDⓈ-M futures through the framework.
pub fn binance_futures() -> Option<NativeHist<Binance>> {
    Some(NativeHist::new(binance_futures_driver()?, &super::caps::BINANCE_FUTURES))
}

pub(crate) mod ibkr;

/// Interactive Brokers through the framework's gateway adapter.
pub fn ibkr() -> Option<ibkr::NativeIbkr> {
    Some(ibkr::NativeIbkr::new(otw_connector_providers::ibkr::gateway::IbkrGateway::new().ok()?, &super::caps::IBKR))
}

#[cfg(test)]
pub(crate) mod golden;
#[cfg(test)]
mod parity_tests;
