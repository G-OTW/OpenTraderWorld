//! Broker accounts of venues ported to the connector framework, served through the same
//! [`Broker`] trait as every other account, so the journal import, the portfolio and the
//! settings page cannot tell the difference.
//!
//! The venue's mapper reads the account in canonical terms (fills, balances, positions,
//! orders); this bridge turns them into OTW's rows. What is the same for every venue lives
//! here once:
//!   - what a contract is worth and settles in comes from the venue's instrument list
//!     (`multiplier`, settlement currency, else the quote currency);
//!   - a holding is valued on the first market the venue lists in its declared order of quote
//!     assets (reversed too where it allows it, the contract itself where it says so);
//!   - the symbol suggestions are what the account is positioned in or working (a margin
//!     account), or the pairs of what it holds plus what it is working (a cash account);
//!   - a venue's test environment is chosen by the account's `env` setting, on another host
//!     when the venue declares one.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use otw_connector_model::{AccountItems, AccountKind, Command, ConnectorError, CorrId, Event, Mapped, Side, TimeRange, Timestamp};
use otw_connector_sdk::{Driver, Mapper, Signer};
use time::OffsetDateTime;

use super::{Broker, BrokerCapability, ExecQuery, Execution, Holding, OpenOrder, Position, Quote};

static NEXT_ID: AtomicU64 = AtomicU64::new(1_000_000_000);

pub(super) fn next_id() -> CorrId {
    CorrId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn err(e: ConnectorError) -> anyhow::Error {
    anyhow::Error::new(e)
}

fn f(d: otw_connector_model::Decimal) -> Result<f64> {
    d.to_string().parse::<f64>().map_err(|e| anyhow!("number {d}: {e}"))
}

fn side(s: &Mapped<Side>) -> String {
    match s {
        Mapped::Known(Side::Buy) => "buy".into(),
        Mapped::Known(Side::Sell) => "sell".into(),
        Mapped::Unknown(raw) => raw.to_ascii_lowercase(),
    }
}

fn at_ms(t: Timestamp) -> Result<OffsetDateTime> {
    // Kept to the millisecond: two fills of one order land in the same second.
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(t.millis()) * 1_000_000).map_err(|_| anyhow!("impossible timestamp {t}"))
}

/// An account window, to the millisecond (what the venues stamp fills with).
fn ms(t: OffsetDateTime) -> Result<Timestamp> {
    let ms = i64::try_from(t.unix_timestamp_nanos() / 1_000_000).map_err(|_| anyhow!("timestamp {t} out of range"))?;
    Timestamp::from_millis(ms).ok_or_else(|| anyhow!("timestamp {t} out of range"))
}

fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// What the instrument list says a symbol is worth and settles in.
#[derive(Debug, Clone)]
struct Contract {
    /// Settlement currency when the venue names one, else the quote currency.
    currency: String,
    /// Base units per contract; one for anything sized in the coin itself.
    multiplier: f64,
    /// What the venue says the instrument is.
    class: otw_connector_model::AssetClass,
    /// The exchange it is listed on, when the venue says.
    exchange: Option<String>,
    /// The venue's own name for it.
    name: String,
}

/// An instrument's market in the journal's vocabulary.
fn journal_class(a: otw_connector_model::AssetClass) -> &'static str {
    use otw_connector_model::AssetClass as A;
    match a {
        A::Equity => "stock",
        A::Etf => "etf",
        A::Option => "option",
        A::Spot | A::Perp => "crypto",
        A::Future => "future",
        A::Forex => "forex",
        // The journal has no class of its own for these: filed as what they are not would
        // be a guess.
        A::Cfd | A::Index | A::Bond | A::Commodity | A::Other => "other",
    }
}

pub struct NativeBroker<M: Mapper> {
    driver: Arc<Driver<M>>,
    /// The venue's test environment on another host, for accounts set to it.
    sandbox: Option<Arc<Driver<M>>>,
    cap: &'static BrokerCapability,
}

impl<M: Mapper + Clone> NativeBroker<M> {
    pub fn new(driver: Arc<Driver<M>>, cap: &'static BrokerCapability) -> Self {
        NativeBroker { driver, sandbox: None, cap }
    }

    pub fn with_sandbox(mut self, sandbox: Option<Arc<Driver<M>>>) -> Self {
        self.sandbox = sandbox;
        self
    }

    pub(super) fn caps(&self) -> &otw_connector_model::Capabilities {
        self.driver.mapper().capabilities()
    }

    /// The account's `env` names the venue's test environment.
    pub(super) fn wants_sandbox(&self, secrets: &HashMap<String, String>) -> bool {
        match (&self.caps().account.sandbox_env, secrets.get("env")) {
            (Some(sandbox), Some(v)) => v.trim().eq_ignore_ascii_case(sandbox),
            _ => false,
        }
    }

    fn env_label(&self, secrets: &HashMap<String, String>) -> String {
        match &self.caps().account.sandbox_env {
            Some(s) if self.wants_sandbox(secrets) => capitalized(s),
            _ => "Live".into(),
        }
    }

    /// The driver this account talks to: live, or the venue's test host. A test environment
    /// on the same host (a header, not a URL) is the signer's business.
    pub(super) fn on(&self, secrets: &HashMap<String, String>) -> Result<Arc<Driver<M>>> {
        let d = if self.wants_sandbox(secrets) && self.caps().regions.sandbox_url.is_some() {
            self.sandbox.as_ref().ok_or_else(|| anyhow!("{} has no test environment configured", self.cap.label))?
        } else {
            &self.driver
        };
        Ok(crate::histdata::native::routed(d))
    }

    fn venue(&self) -> String {
        self.caps().account.venue_label.clone().unwrap_or_else(|| self.cap.label.to_string())
    }

    /// A refused key on a venue with two environments usually belongs to the other one.
    pub(super) fn hint(&self, secrets: &HashMap<String, String>, e: ConnectorError) -> anyhow::Error {
        if matches!(e.kind, otw_connector_model::ErrorKind::Auth | otw_connector_model::ErrorKind::Permission) && self.caps().account.sandbox_env.is_some() {
            let env = self.env_label(secrets).to_ascii_lowercase();
            return anyhow!(
                "{}. Check the key belongs to the {env} environment this account is set to and that its access is enabled on it.",
                e.message
            );
        }
        err(e)
    }

    /// The account of the login this connector is set to read, by the venue's setting.
    fn account_of(&self, secrets: &HashMap<String, String>) -> Option<String> {
        self.driver.mapper().account_selector(secrets)
    }

    /// A row's account: the one the venue names, else the account this connector is set to
    /// (a venue read one account at a time, by its `account_id` setting).
    fn row_account(&self, row: Option<String>, secrets: &HashMap<String, String>) -> String {
        row.filter(|a| !a.is_empty())
            .or_else(|| secrets.get("account_id").map(|a| a.trim().to_string()))
            .unwrap_or_default()
    }

    pub(super) fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>> {
        self.driver.mapper().signer(secrets).map_err(err)
    }

    fn asset_class(&self) -> &'static str {
        self.cap.asset_classes.first().copied().unwrap_or("")
    }

    /// A row's class: what the venue said the instrument is, for a venue asked per id (an
    /// instrument nobody could identify is "other", never a guess); the connector's own class
    /// otherwise.
    fn row_class(&self, row: Option<otw_connector_model::AssetClass>, c: Option<&Contract>) -> String {
        if let Some(r) = row {
            return journal_class(r).into();
        }
        if self.caps().account.resolve_by_id {
            c.map(|c| journal_class(c.class)).unwrap_or("other").into()
        } else {
            self.asset_class().into()
        }
    }

    /// A row's venue: the exchange the row or its instrument names, for a venue asked per id
    /// or whose rows describe themselves; the venue's own name otherwise.
    fn row_venue(&self, row: Option<String>, c: Option<&Contract>) -> String {
        if self.caps().account.resolve_by_id || self.caps().account.rows_carry_terms {
            row.filter(|v| !v.is_empty()).or_else(|| c.and_then(|c| c.exchange.clone())).unwrap_or_default()
        } else {
            self.venue()
        }
    }

    /// Positions with their contract terms, read once for holdings, quotes and positions.
    async fn positions_with_terms(&self, secrets: &HashMap<String, String>) -> Result<(Vec<otw_connector_model::Position>, HashMap<String, Contract>)> {
        let signer = self.begin(secrets).await?;
        let mut rows = Vec::new();
        for page in self.query(secrets, AccountKind::Positions, signer.as_deref()).await? {
            if let AccountItems::Positions(p) = page {
                rows.extend(p);
            }
        }
        let contracts = self.contracts_lenient(secrets, rows.iter().map(|p| p.instrument.clone()).collect()).await;
        Ok((rows, contracts))
    }

    async fn query(&self, secrets: &HashMap<String, String>, kind: AccountKind, signer: Option<&dyn Signer>) -> Result<Vec<AccountItems>> {
        self.on(secrets)?.account(next_id(), kind, &[], None, self.account_of(secrets).as_deref(), signer).await.map_err(|e| self.hint(secrets, e))
    }

    /// Signer and venue clock, in that order: what every signed read starts with.
    async fn begin(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>> {
        let signer = self.signer(secrets)?;
        self.on(secrets)?.sync_time(next_id()).await.map_err(err)?;
        Ok(signer)
    }

    /// Contract terms per native symbol, from the instrument list (loaded once, reloaded
    /// when a symbol is missing from it). `strict`: a symbol the venue does not list is an
    /// error naming it; otherwise it is simply left out.
    async fn contracts(&self, secrets: &HashMap<String, String>, ids: &[otw_connector_model::InstrumentId], strict: bool) -> Result<HashMap<String, Contract>> {
        // Rows that describe themselves need no instrument behind them.
        if self.caps().account.rows_carry_terms {
            return Ok(HashMap::new());
        }
        let driver = self.on(secrets)?;
        let display = self.caps().account.display_symbols;
        let lookup = |ids: &[otw_connector_model::InstrumentId]| -> Result<(HashMap<String, Contract>, Vec<otw_connector_model::InstrumentId>)> {
            let cache = driver.instruments().map_err(err)?;
            let mut found = HashMap::new();
            let mut missing = Vec::new();
            for id in ids {
                // A venue whose account rows name pairs by display symbol is looked up so.
                let spec = if display { cache.by_symbol(&id.native).and_then(|i| cache.get(i)) } else { cache.get(id) };
                match spec {
                    Some(spec) => {
                        let currency = spec.settlement_ccy.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| spec.quote.clone());
                        found.insert(
                            id.native.clone(),
                            Contract {
                                currency,
                                multiplier: f(spec.multiplier)?,
                                class: spec.asset_class,
                                exchange: spec.exchange.clone(),
                                name: spec.symbol.0.clone(),
                            },
                        );
                    }
                    None => missing.push(id.clone()),
                }
            }
            Ok((found, missing))
        };
        let (mut found, missing) = lookup(ids)?;
        if missing.is_empty() {
            return Ok(found);
        }
        // A venue that answers per instrument is asked for these; otherwise its whole list.
        let loaded = if self.caps().account.resolve_by_id {
            let signer = self.signer(secrets)?;
            let mut r = Ok(());
            for batch in missing.chunks(driver.mapper().resolve_batch().max(1)) {
                if let Err(e) = driver.resolve_instruments(next_id(), batch, signer.as_deref()).await {
                    r = Err(e);
                    break;
                }
            }
            r.map(|_| ())
        } else {
            driver.load_instruments(next_id()).await.map(|_| ())
        };
        match loaded {
            Ok(()) => {}
            Err(e) if strict => return Err(err(e)),
            Err(_) => return Ok(found),
        }
        let (more, still) = lookup(&missing)?;
        found.extend(more);
        if strict {
            if let Some(m) = still.first() {
                return Err(anyhow!("{} does not list a pair named {}, check its spelling", self.cap.label, m.native));
            }
        }
        Ok(found)
    }

    /// Each asset at the mid of the venue's own bid and ask (or its last trade, where the
    /// venue says so), asked with the account's key. An instrument with no price is left out.
    async fn quotes_from_snapshots(&self, secrets: &HashMap<String, String>, symbols: &[String]) -> Result<Vec<Quote>> {
        let mut ids = Vec::new();
        for s in symbols.iter().filter(|s| !s.trim().is_empty()) {
            ids.push((s.clone(), self.driver.mapper().instrument_id(s).map_err(err)?));
        }
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let signer = self.begin(secrets).await?;
        let driver = self.on(secrets)?;
        let mut priced = Vec::new();
        let last = self.caps().account.quotes_last_trade;
        let data_type = if last { otw_connector_model::DataType::Trades } else { otw_connector_model::DataType::Quote };
        for (asked, id) in ids {
            let cmd = Command::RequestSnapshot { req_id: next_id(), instrument: id.clone(), data_type: data_type.clone() };
            for env in driver.execute_as(&cmd, signer.as_deref()).await.map_err(|e| self.hint(secrets, e))? {
                match env.event {
                    Event::Quote(q) if !last => {
                        let (bid, ask) = (f(q.bid.0)?, f(q.ask.0)?);
                        if bid > 0.0 && ask > 0.0 {
                            priced.push((asked.clone(), id.clone(), (bid + ask) / 2.0));
                        }
                    }
                    Event::Trade(t) if last => {
                        let p = f(t.price.0)?;
                        if p > 0.0 {
                            priced.push((asked.clone(), id.clone(), p));
                        }
                    }
                    Event::Error(e) => return Err(err(e)),
                    _ => {}
                }
            }
        }
        let contracts = self.contracts_lenient(secrets, priced.iter().map(|(_, id, _)| id.clone()).collect()).await;
        Ok(priced
            .into_iter()
            .map(|(symbol, id, price)| Quote {
                symbol,
                price,
                currency: contracts.get(&id.native).map(|c| c.currency.clone()).unwrap_or_default(),
                pair: id.native,
            })
            .collect())
    }

    async fn contracts_lenient(&self, secrets: &HashMap<String, String>, ids: Vec<otw_connector_model::InstrumentId>) -> HashMap<String, Contract> {
        let mut uniq: Vec<otw_connector_model::InstrumentId> = Vec::new();
        for n in ids {
            if !uniq.contains(&n) {
                uniq.push(n);
            }
        }
        if uniq.is_empty() {
            return HashMap::new();
        }
        self.contracts(secrets, &uniq, false).await.unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl<M: Mapper + Clone> Broker for NativeBroker<M> {
    fn capability(&self) -> &'static BrokerCapability {
        self.cap
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        self.driver.mapper().check_settings(config).map_err(err)?;
        let Some(sandbox) = self.caps().account.sandbox_env.clone() else {
            return Ok(());
        };
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => Ok(()),
            Some(v) if v.is_empty() || v == "live" || v == sandbox => Ok(()),
            Some(other) => Err(anyhow!("the {} environment is \"live\" or \"{sandbox}\", not \"{other}\"", self.cap.label)),
        }
    }

    async fn test(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<String> {
        let signer = self.begin(secrets).await?;
        let cmd = Command::QueryAccount { req_id: next_id(), kind: AccountKind::Summary, symbols: vec![], range: None, cursor: None, account: self.account_of(secrets) };
        for env in self.on(secrets)?.execute_as(&cmd, signer.as_deref()).await.map_err(|e| self.hint(secrets, e))? {
            match env.event {
                Event::AccountSummary(a) => {
                    // A broker that takes orders needs a trading key; one that only reads does not.
                    let note = match (a.can_trade, self.caps().orders.any()) {
                        (Some(true), true) => ". The key can trade, as orders need: keep withdrawals off on it",
                        (Some(false), true) => ". The key cannot trade: enough to read, but orders need trading enabled on it (withdrawals stay off)",
                        (Some(true), false) => ". The key can trade; a read-only key is enough here",
                        _ => "",
                    };
                    if let Some(detail) = a.detail {
                        return Ok(match self.caps().account.sandbox_env {
                            Some(_) => format!("{} {detail}{note}", self.env_label(secrets)),
                            None => format!("{detail}{note}"),
                        });
                    }
                    if let Some(open) = a.open_positions {
                        let balance = a.wallet_balance.unwrap_or_default();
                        return Ok(format!("{} futures account, wallet balance {balance}, {open} open position(s){note}", self.env_label(secrets)));
                    }
                    let kind = a.account_type.unwrap_or_else(|| "SPOT".into());
                    let env = if self.wants_sandbox(secrets) { format!("{} ", self.env_label(secrets)) } else { String::new() };
                    return Ok(format!("{env}{kind} account, {} asset(s) held, permissions [{}]{note}", a.assets_held, a.permissions.join(", ")));
                }
                Event::Error(e) => return Err(err(e)),
                _ => {}
            }
        }
        Err(anyhow!("{} answered no account summary", self.cap.label))
    }

    async fn executions(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>, q: &ExecQuery) -> Result<Vec<Execution>> {
        let account = self.caps().account.clone();
        if account.fills_need_symbols && q.symbols.is_empty() {
            return Err(anyhow!(
                "{} answers its trade history one instrument at a time. Name the symbols to pull (BTCUSDT, ETHEUR…)",
                self.cap.label
            ));
        }
        let mut ids = Vec::with_capacity(q.symbols.len());
        for s in q.symbols.iter().filter(|s| !s.trim().is_empty()) {
            ids.push(self.driver.mapper().instrument_id(s).map_err(err)?);
        }
        let wanted: HashSet<String> = ids.iter().map(|i| i.native.clone()).collect();
        if account.require_listed_pairs {
            self.contracts(secrets, &ids, true).await?;
        }
        let signer = self.begin(secrets).await?;
        let range = TimeRange::new(ms(q.from)?, ms(q.to)?);
        // The venue is told what was named (one that filters server-side pages less); the
        // rows are filtered here whatever it did with them.
        let pages = self
            .on(secrets)?
            .account(next_id(), AccountKind::Fills, &ids, Some(range), self.account_of(secrets).as_deref(), signer.as_deref())
            .await
            .map_err(|e| self.hint(secrets, e))?;
        let mut fills = Vec::new();
        for page in pages {
            if let AccountItems::Fills(f) = page {
                fills.extend(f.into_iter().filter(|x| wanted.is_empty() || wanted.contains(&x.instrument.native)));
            }
        }
        let contracts = self.contracts_lenient(secrets, fills.iter().map(|x| x.instrument.clone()).collect()).await;
        let mut out = Vec::with_capacity(fills.len());
        for x in fills {
            let c = contracts.get(&x.instrument.native);
            out.push(Execution {
                id: x.id,
                order_id: x.order_id.unwrap_or_default(),
                at: at_ms(x.ts)?,
                currency: c.map(|c| c.currency.clone()).filter(|c| !c.is_empty()).or(x.quote_ccy).unwrap_or_default(),
                // The row's own point value first (a contract the venue answers per id),
                // else the instrument's.
                multiplier: match x.multiplier {
                    Some(m) => f(m)?,
                    None => c.map(|c| c.multiplier).unwrap_or(1.0),
                },
                symbol: x.instrument.native,
                side: side(&x.side),
                qty: f(x.qty.0)?,
                price: f(x.price.0)?,
                fee: f(x.fee.0)?,
                fee_currency: x.fee_asset.unwrap_or_default(),
                asset_class: self.row_class(x.class, c),
                venue: self.row_venue(x.venue, c),
                account: self.row_account(x.account, secrets),
            });
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    /// Instruments worth offering. A margin account: what it is positioned in or working.
    /// A cash account: every listed pair whose base asset it holds, plus whatever has a
    /// working order. A suggestion, not a filter.
    async fn symbols(&self, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<Vec<String>> {
        if self.caps().account.positions {
            let mut out: Vec<String> = self.positions(client, secrets).await.unwrap_or_default().into_iter().map(|p| p.symbol).collect();
            out.extend(self.orders(client, secrets).await.unwrap_or_default().into_iter().map(|o| o.symbol));
            out.retain(|s| !s.is_empty());
            out.sort();
            out.dedup();
            return Ok(out);
        }
        let signer = self.begin(secrets).await?;
        let mut held: HashSet<String> = HashSet::new();
        for page in self.query(secrets, AccountKind::Balances, signer.as_deref()).await? {
            if let AccountItems::Balances(b) = page {
                held.extend(b.into_iter().map(|b| b.asset));
            }
        }
        let mut out: HashSet<String> = HashSet::new();
        if !held.is_empty() {
            for spec in self.on(secrets)?.load_instruments(next_id()).await.map_err(err)? {
                if spec.base.as_ref().is_some_and(|b| held.contains(b)) {
                    out.insert(if self.caps().account.display_symbols { spec.symbol.0 } else { spec.id.native });
                }
            }
        }
        for page in self.query(secrets, AccountKind::OpenOrders, signer.as_deref()).await? {
            if let AccountItems::Orders(o) = page {
                out.extend(o.into_iter().map(|o| o.instrument.native));
            }
        }
        let mut list: Vec<String> = out.into_iter().collect();
        list.sort();
        Ok(list)
    }

    /// What the account owns, asset by asset. A balance carries no cost basis.
    async fn holdings(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<Vec<Holding>> {
        if self.caps().account.holdings_from_positions {
            // A securities account: what it holds is its positions, priced at cost.
            let (rows, contracts) = self.positions_with_terms(secrets).await?;
            let mut out = Vec::new();
            for p in rows {
                let qty = f(p.qty.0)?;
                let c = contracts.get(&p.instrument.native);
                out.push(Holding {
                    name: p.name.clone().filter(|n| !n.is_empty()).or_else(|| c.map(|c| c.name.clone())).unwrap_or_default(),
                    qty: qty.abs(),
                    side: if qty < 0.0 { "short".into() } else { "long".into() },
                    asset_class: self.row_class(p.class, c),
                    avg_price: p.avg_price.map(|x| f(x.0)).transpose()?,
                    currency: c.map(|c| c.currency.clone()).filter(|c| !c.is_empty()).or(p.currency).unwrap_or_default(),
                    venue: self.row_venue(p.venue, c),
                    account: self.row_account(p.account, secrets),
                    symbol: p.instrument.native,
                });
            }
            out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
            return Ok(out);
        }
        let signer = self.begin(secrets).await?;
        let mut out = Vec::new();
        for page in self.query(secrets, AccountKind::Balances, signer.as_deref()).await? {
            let AccountItems::Balances(b) = page else { continue };
            for b in b {
                out.push(Holding {
                    symbol: b.asset,
                    name: b.name.unwrap_or_default(),
                    qty: f(b.total.0)?,
                    side: "long".into(),
                    // A fiat wallet is spelled as the currency it is.
                    asset_class: if b.fiat { "forex".into() } else { self.asset_class().into() },
                    avg_price: None,
                    currency: String::new(),
                    venue: self.venue(),
                    account: self.row_account(b.account, secrets),
                });
            }
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    /// What the venue trades these assets at right now: the asset itself when it is a
    /// listed instrument (and the venue says so), else the first market in its declared order
    /// of quote assets, reversed too where the venue allows it. An asset with no market is
    /// left out.
    async fn quotes(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>, symbols: &[String]) -> Result<Vec<Quote>> {
        if self.caps().account.quotes_from_positions {
            // The venue marks every open position itself: no market-data plan needed. An
            // asset the account no longer holds is left out.
            let (rows, contracts) = self.positions_with_terms(secrets).await?;
            let mut out = Vec::new();
            for p in rows {
                let Some(wanted) = symbols.iter().find(|s| s.trim().eq_ignore_ascii_case(&p.instrument.native)) else { continue };
                let Some(mark) = p.mark_price.map(|m| f(m.0)).transpose()?.filter(|m| *m > 0.0) else { continue };
                let c = contracts.get(&p.instrument.native);
                out.push(Quote {
                    symbol: wanted.clone(),
                    price: mark,
                    currency: c.map(|c| c.currency.clone()).filter(|c| !c.is_empty()).or(p.currency).unwrap_or_default(),
                    pair: p.instrument.native,
                });
            }
            return Ok(out);
        }
        if self.caps().account.quotes_from_snapshots {
            return self.quotes_from_snapshots(secrets, symbols).await;
        }
        let mut board: HashMap<String, f64> = HashMap::new();
        for p in self.on(secrets)?.board(next_id()).await.map_err(err)? {
            board.insert(p.instrument.native, f(p.price.0)?);
        }
        let mapper = self.driver.mapper();
        let account = mapper.capabilities().account.clone();
        let mut out = Vec::new();
        for s in symbols {
            let asset = s.trim().to_ascii_uppercase();
            if asset.is_empty() {
                continue;
            }
            if !account.quote_reverse {
                // The first market that exists decides; a dead one is not replaced by another.
                let pair = if account.quote_self && board.contains_key(&asset) {
                    Some((asset.clone(), None))
                } else {
                    account
                        .quote_preference
                        .iter()
                        .filter_map(|q| mapper.pair_symbol(&asset, q).map(|p| (p, Some(q.clone()))))
                        .find(|(p, _)| board.contains_key(p))
                };
                let Some((pair, quote)) = pair else { continue };
                let Some(price) = board.get(&pair).copied().filter(|p| *p > 0.0) else { continue };
                let currency = match quote {
                    Some(q) => q,
                    None => self
                        .contracts_lenient(secrets, vec![otw_connector_model::InstrumentId::new(&mapper.capabilities().venue, &pair)])
                        .await
                        .remove(&pair)
                        .map(|c| c.currency)
                        .unwrap_or_default(),
                };
                out.push(Quote { symbol: s.clone(), price, currency, pair });
                continue;
            }
            for q in &account.quote_preference {
                if *q == asset {
                    continue;
                }
                if let Some(direct) = mapper.pair_symbol(&asset, q) {
                    if let Some(p) = board.get(&direct).copied().filter(|p| *p > 0.0) {
                        out.push(Quote { symbol: s.clone(), price: p, currency: q.clone(), pair: direct });
                        break;
                    }
                }
                // The other way round: USDT has no USDT market, but USDCUSDT prices it.
                if let Some(reverse) = mapper.pair_symbol(q, &asset) {
                    if let Some(p) = board.get(&reverse).copied().filter(|p| *p > 0.0) {
                        out.push(Quote { symbol: s.clone(), price: 1.0 / p, currency: q.clone(), pair: reverse });
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    async fn positions(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<Vec<Position>> {
        if !self.caps().account.positions {
            return Err(anyhow!("{} does not report positions", self.cap.label));
        }
        let (rows, contracts) = self.positions_with_terms(secrets).await?;
        let mut out = Vec::new();
        for p in rows {
            let qty = f(p.qty.0)?;
            let c = contracts.get(&p.instrument.native);
            out.push(Position {
                side: if qty < 0.0 { "short".into() } else { "long".into() },
                qty: qty.abs(),
                avg_price: p.avg_price.map(|x| f(x.0)).transpose()?,
                currency: c.map(|c| c.currency.clone()).filter(|c| !c.is_empty()).or(p.currency).unwrap_or_default(),
                asset_class: self.row_class(p.class, c),
                // The row's own contract size first (a CFD deal), else the instrument's.
                multiplier: match p.multiplier {
                    Some(m) => f(m)?,
                    None => c.map(|c| c.multiplier).unwrap_or(1.0),
                },
                venue: self.row_venue(p.venue, c),
                account: self.row_account(p.account, secrets),
                native_id: None,
                market_price: p.mark_price.map(|x| f(x.0)).transpose()?,
                unrealized_pnl: p.unrealized_pnl.map(|x| f(x.0)).transpose()?,
                opened_at: None,
                entry_fee: None,
                fee_currency: String::new(),
                leverage: None,
                margin_mode: None,
                symbol: p.instrument.native,
            });
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    async fn orders(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<Vec<OpenOrder>> {
        let signer = self.begin(secrets).await?;
        let mut rows = Vec::new();
        for page in self.query(secrets, AccountKind::OpenOrders, signer.as_deref()).await? {
            if let AccountItems::Orders(o) = page {
                rows.extend(o);
            }
        }
        // Best effort: an order is still an order without its currency.
        let contracts = if rows.iter().all(|o| o.quote_ccy.is_some()) && !self.caps().account.resolve_by_id {
            HashMap::new()
        } else {
            self.contracts_lenient(secrets, rows.iter().map(|o| o.instrument.clone()).collect()).await
        };
        let mut out = Vec::new();
        for o in rows {
            out.push(OpenOrder {
                currency: o.quote_ccy.clone().or_else(|| contracts.get(&o.instrument.native).map(|c| c.currency.clone())).unwrap_or_default(),
                id: o.id,
                side: side(&o.side),
                order_type: o.order_type,
                qty: f(o.qty.0)?,
                filled_qty: f(o.filled_qty.0)?,
                limit_price: o.limit_price.map(|p| f(p.0)).transpose()?,
                stop_price: o.stop_price.map(|p| f(p.0)).transpose()?,
                trail_start: None,
                trail_rate: None,
                asset_class: self.row_class(o.class, contracts.get(&o.instrument.native)),
                // To the millisecond, as the venue stamps it.
                placed_at: o.placed_at.and_then(|t| at_ms(t).ok()),
                venue: self.row_venue(o.venue, contracts.get(&o.instrument.native)),
                account: self.row_account(o.account, secrets),
                parent_id: None,
                reduce_only: false,
                symbol: o.instrument.native,
            });
        }
        Ok(out)
    }
}

// ── Accounts over several books ──────────────────────────────────────────────

/// One book of a multi-book account (Bitget: spot, USDT-M futures), with the names its
/// `books` setting accepts, the first being the one shown.
pub struct BookBroker<M: Mapper> {
    pub names: &'static [&'static str],
    pub broker: NativeBroker<M>,
}

/// An account read over the books its setting names (all of them when empty). Rows from
/// every book are merged; each keeps its own venue label. Prices come from the first
/// registered book's board, whatever the selection.
pub struct NativeMultiBroker<M: Mapper> {
    books: Vec<BookBroker<M>>,
    setting: &'static str,
    cap: &'static BrokerCapability,
}

impl<M: Mapper + Clone> NativeMultiBroker<M> {
    pub fn new(setting: &'static str, books: Vec<BookBroker<M>>, cap: &'static BrokerCapability) -> Self {
        NativeMultiBroker { books, setting, cap }
    }

    fn shown(&self) -> Vec<&'static str> {
        self.books.iter().filter_map(|b| b.names.first().copied()).collect()
    }

    fn selected(&self, config: &HashMap<String, String>) -> Result<Vec<&BookBroker<M>>> {
        let raw = config.get(self.setting).map(|s| s.trim()).unwrap_or_default();
        if raw.is_empty() {
            return Ok(self.books.iter().collect());
        }
        let mut chosen = vec![false; self.books.len()];
        for part in raw.split(',') {
            let p = part.trim().to_ascii_lowercase();
            if p.is_empty() {
                continue;
            }
            match self.books.iter().position(|b| b.names.contains(&p.as_str())) {
                Some(i) => chosen[i] = true,
                None => {
                    return Err(anyhow!(
                        "\"{p}\" is not a {} book this connector reads. Use {}, or both separated by a comma.",
                        self.cap.label,
                        self.shown().join(", ")
                    ))
                }
            }
        }
        let out: Vec<&BookBroker<M>> = self.books.iter().zip(chosen).filter(|(_, c)| *c).map(|(b, _)| b).collect();
        if out.is_empty() {
            return Err(anyhow!("name at least one {} book to read: {}", self.cap.label, self.shown().join(" or ")));
        }
        Ok(out)
    }
}

#[async_trait::async_trait]
impl<M: Mapper + Clone> Broker for NativeMultiBroker<M> {
    fn capability(&self) -> &'static BrokerCapability {
        self.cap
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        for b in self.selected(config)? {
            b.broker.validate_config(config)?;
        }
        Ok(())
    }

    async fn test(&self, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<String> {
        let mut said = Vec::new();
        for b in self.selected(secrets)? {
            said.push(b.broker.test(client, secrets).await?);
        }
        Ok(format!("key accepted. {}", said.join("; ")))
    }

    async fn executions(&self, client: &reqwest::Client, secrets: &HashMap<String, String>, q: &ExecQuery) -> Result<Vec<Execution>> {
        if q.to <= q.from {
            return Err(anyhow!("the period ends before it starts"));
        }
        // A book that serves no fills (Bybit inverse) is left out of a pull, by its declaration.
        let books: Vec<&BookBroker<M>> = self.selected(secrets)?.into_iter().filter(|b| b.broker.caps().account.fills_history).collect();
        if books.is_empty() {
            return Err(anyhow!(
                "this {} account reads no book whose fills are imported. Add {} to its books.",
                self.cap.label,
                self.books.iter().filter(|b| b.broker.caps().account.fills_history).filter_map(|b| b.names.first()).copied().collect::<Vec<_>>().join(" or ")
            ));
        }
        let mut out = Vec::new();
        for b in books {
            out.extend(b.broker.executions(client, secrets, q).await?);
        }
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn positions(&self, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<Vec<Position>> {
        let books: Vec<&BookBroker<M>> = self.selected(secrets)?.into_iter().filter(|b| b.broker.caps().account.positions).collect();
        if books.is_empty() {
            return Err(anyhow!(
                "this {} account reads no book with positions (a balance is a holding, not a position). Add {} to its books to read them.",
                self.cap.label,
                self.books.iter().filter(|b| b.broker.caps().account.positions).filter_map(|b| b.names.first()).copied().collect::<Vec<_>>().join(" or ")
            ));
        }
        let mut out = Vec::new();
        for b in books {
            out.extend(b.broker.positions(client, secrets).await?);
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    async fn orders(&self, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<Vec<OpenOrder>> {
        let mut out = Vec::new();
        for b in self.selected(secrets)? {
            out.extend(b.broker.orders(client, secrets).await?);
        }
        Ok(out)
    }

    async fn holdings(&self, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<Vec<Holding>> {
        let books: Vec<&BookBroker<M>> = self.selected(secrets)?.into_iter().filter(|b| b.broker.caps().account.balances).collect();
        if books.is_empty() {
            return Err(anyhow!(
                "this {} account reads no book that holds assets. Add {} to its books to read balances.",
                self.cap.label,
                self.books.iter().filter(|b| b.broker.caps().account.balances).filter_map(|b| b.names.first()).copied().collect::<Vec<_>>().join(" or ")
            ));
        }
        let mut out = Vec::new();
        for b in books {
            out.extend(b.broker.holdings(client, secrets).await?);
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    async fn quotes(&self, client: &reqwest::Client, secrets: &HashMap<String, String>, symbols: &[String]) -> Result<Vec<Quote>> {
        let first = self.books.first().ok_or_else(|| anyhow!("{}: no book registered", self.cap.label))?;
        first.broker.quotes(client, secrets, symbols).await
    }

    /// Whatever has a working order, plus each held coin against the first quote the price
    /// board lists it with. A suggestion, not a filter.
    async fn symbols(&self, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<Vec<String>> {
        let mut out: Vec<String> = self.orders(client, secrets).await.unwrap_or_default().into_iter().map(|o| o.symbol).collect();
        let holds_assets = self.selected(secrets)?.iter().any(|b| b.broker.caps().account.balances);
        if holds_assets {
            if let Some(first) = self.books.first() {
                let board: HashSet<String> = match first.broker.on(secrets) {
                    Ok(d) => d.board(next_id()).await.unwrap_or_default().into_iter().map(|p| p.instrument.native).collect(),
                    Err(_) => HashSet::new(),
                };
                let mapper = first.broker.driver.mapper();
                let preference = mapper.capabilities().account.quote_preference.clone();
                for h in self.holdings(client, secrets).await.unwrap_or_default() {
                    if let Some(pair) = preference.iter().filter_map(|q| mapper.pair_symbol(&h.symbol, q)).find(|p| board.contains(p)) {
                        out.push(pair);
                    }
                }
            }
        }
        out.retain(|s| !s.is_empty());
        out.sort();
        out.dedup();
        Ok(out)
    }
}

// ── Registered venues ─────────────────────────────────────────────────────────

pub(super) fn binance_native() -> Option<NativeBroker<otw_connector_providers::binance::Binance>> {
    Some(NativeBroker::new(crate::histdata::native::binance_driver()?, &super::caps::BINANCE).with_sandbox(crate::histdata::native::binance_testnet_driver()))
}

pub fn bitget() -> Option<NativeMultiBroker<otw_connector_providers::bitget::Bitget>> {
    let (spot, futures) = crate::histdata::native::bitget_drivers()?;
    Some(NativeMultiBroker::new(
        "books",
        vec![
            BookBroker { names: &["spot"], broker: NativeBroker::new(spot, &super::caps::BITGET) },
            BookBroker { names: &["usdt-futures", "futures"], broker: NativeBroker::new(futures, &super::caps::BITGET) },
        ],
        &super::caps::BITGET,
    ))
}

pub fn bybit() -> Option<NativeMultiBroker<otw_connector_providers::bybit::Bybit>> {
    let d = crate::histdata::native::bybit_drivers()?;
    Some(NativeMultiBroker::new(
        "books",
        vec![
            BookBroker { names: &["spot"], broker: NativeBroker::new(d.spot, &super::caps::BYBIT) },
            BookBroker { names: &["linear"], broker: NativeBroker::new(d.linear, &super::caps::BYBIT) },
            BookBroker { names: &["inverse"], broker: NativeBroker::new(d.inverse, &super::caps::BYBIT) },
            BookBroker { names: &["option", "options"], broker: NativeBroker::new(d.option, &super::caps::BYBIT) },
        ],
        &super::caps::BYBIT,
    ))
}

/// Bybit EU: a separate platform, its spot book only, read with a bybit.eu key.
pub fn bybit_eu() -> Option<NativeBroker<otw_connector_providers::bybit::Bybit>> {
    Some(NativeBroker::new(crate::histdata::native::bybit_eu_driver()?, &super::caps::BYBIT_EU))
}

pub fn gateio() -> Option<NativeMultiBroker<otw_connector_providers::gateio::Gateio>> {
    let (spot, futures) = crate::histdata::native::gateio_drivers()?;
    Some(NativeMultiBroker::new(
        "books",
        vec![
            BookBroker { names: &["spot"], broker: NativeBroker::new(spot, &super::caps::GATEIO) },
            BookBroker { names: &["futures", "usdt-futures"], broker: NativeBroker::new(futures, &super::caps::GATEIO) },
        ],
        &super::caps::GATEIO,
    ))
}

pub fn mexc() -> Option<NativeMultiBroker<otw_connector_providers::mexc::Mexc>> {
    let (spot, futures) = crate::histdata::native::mexc_drivers()?;
    Some(NativeMultiBroker::new(
        "books",
        vec![
            BookBroker { names: &["spot"], broker: NativeBroker::new(spot, &super::caps::MEXC) },
            BookBroker { names: &["futures"], broker: NativeBroker::new(futures, &super::caps::MEXC) },
        ],
        &super::caps::MEXC,
    ))
}

/// Bitfinex: one key sees both books, read once through the spot adapter.
pub fn bitfinex() -> Option<NativeBroker<otw_connector_providers::bitfinex::Bitfinex>> {
    Some(NativeBroker::new(crate::histdata::native::bitfinex_driver()?, &super::caps::BITFINEX))
}

pub fn deribit() -> Option<NativeBroker<otw_connector_providers::deribit::Deribit>> {
    Some(NativeBroker::new(crate::histdata::native::deribit_driver()?, &super::caps::DERIBIT))
}

pub fn kucoin() -> Option<NativeBroker<otw_connector_providers::kucoin::Kucoin>> {
    Some(NativeBroker::new(crate::histdata::native::kucoin_driver()?, &super::caps::KUCOIN))
}

/// KuCoin Futures: its own platform, read with a key of the same account.
pub fn kucoin_futures() -> Option<NativeBroker<otw_connector_providers::kucoin::Kucoin>> {
    Some(NativeBroker::new(crate::histdata::native::kucoin_futures_driver()?, &super::caps::KUCOIN_FUTURES))
}

pub fn okx() -> Option<NativeBroker<otw_connector_providers::okx::Okx>> {
    Some(NativeBroker::new(crate::histdata::native::okx_driver()?, &super::caps::OKX))
}

pub fn coinbase() -> Option<NativeBroker<otw_connector_providers::coinbase::Coinbase>> {
    Some(NativeBroker::new(crate::histdata::native::coinbase_driver()?, &super::caps::COINBASE))
}

/// Alpaca's trading account: live, or paper on its own host by the account's `env`.
pub fn alpaca() -> Option<NativeBroker<otw_connector_providers::alpaca::Alpaca>> {
    let live = crate::histdata::native::alpaca_variants()?.into_iter().next()?.driver;
    Some(NativeBroker::new(live, &super::caps::ALPACA).with_sandbox(crate::histdata::native::alpaca_paper_driver()))
}

/// FOREX.com's account: the login's trading account, by the `trading_account_id` setting.
pub fn forexcom() -> Option<NativeBroker<otw_connector_providers::forexcom::ForexCom>> {
    Some(NativeBroker::new(crate::histdata::native::forexcom_driver()?, &super::caps::FOREXCOM))
}

/// Capital.com's account: live, or the demo platform on its own host by the account's `env`.
pub fn capitalcom() -> Option<NativeBroker<otw_connector_providers::capitalcom::CapitalCom>> {
    let live = crate::histdata::native::capitalcom_variants()?.into_iter().next()?.driver;
    Some(NativeBroker::new(live, &super::caps::CAPITALCOM).with_sandbox(crate::histdata::native::capitalcom_demo_driver()))
}

/// TradeStation's account: live, or simulated trading on its own host by the account's `env`.
pub fn tradestation() -> Option<NativeBroker<otw_connector_providers::tradestation::TradeStation>> {
    let live = crate::histdata::native::tradestation_variants()?.into_iter().next()?.driver;
    Some(NativeBroker::new(live, &super::caps::TRADESTATION).with_sandbox(crate::histdata::native::tradestation_sim_driver()))
}

/// OANDA's account: fxTrade, or fxPractice on its own host by the account's `env`.
pub fn oanda() -> Option<NativeBroker<otw_connector_providers::oanda::Oanda>> {
    let live = crate::histdata::native::oanda_variants()?.into_iter().next()?.driver;
    Some(NativeBroker::new(live, &super::caps::OANDA).with_sandbox(crate::histdata::native::oanda_practice_driver()))
}

static NINJATRADER_LIVE: std::sync::OnceLock<Option<Arc<Driver<otw_connector_providers::ninjatrader::NinjaTrader>>>> = std::sync::OnceLock::new();
static NINJATRADER_DEMO: std::sync::OnceLock<Option<Arc<Driver<otw_connector_providers::ninjatrader::NinjaTrader>>>> = std::sync::OnceLock::new();

/// NinjaTrader's account: live, or the simulation platform on its own host by the
/// account's `env`. Account only: no data connector rides on it.
pub fn ninjatrader() -> Option<NativeBroker<otw_connector_providers::ninjatrader::NinjaTrader>> {
    use otw_connector_providers::ninjatrader::NinjaTrader;
    let live = crate::histdata::native::shared_driver(&NINJATRADER_LIVE, "ninjatrader", || NinjaTrader::new().ok())?;
    let demo = crate::histdata::native::sandbox_driver(&NINJATRADER_DEMO, "ninjatrader", || NinjaTrader::new().ok());
    Some(NativeBroker::new(live, &super::caps::NINJATRADER).with_sandbox(demo))
}

/// Interactive Brokers' Flex statements: the account's past, by a saved query.
pub fn ibkr_flex() -> Option<NativeBroker<otw_connector_providers::ibkr::flex::IbkrFlex>> {
    Some(NativeBroker::new(crate::histdata::native::ibkr_flex_driver()?, &super::caps::IBKR_FLEX))
}

pub fn kraken() -> Option<NativeBroker<otw_connector_providers::kraken::Kraken>> {
    Some(NativeBroker::new(crate::histdata::native::kraken_driver()?, &super::caps::KRAKEN))
}

pub(super) fn binance_futures_native() -> Option<NativeBroker<otw_connector_providers::binance::Binance>> {
    Some(
        NativeBroker::new(crate::histdata::native::binance_futures_driver()?, &super::caps::BINANCE_FUTURES)
            .with_sandbox(crate::histdata::native::binance_futures_testnet_driver()),
    )
}

#[cfg(test)]
mod parity_tests;
