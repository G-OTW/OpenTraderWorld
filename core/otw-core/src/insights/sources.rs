//! The three sources, mapped into one [`TradeSet`].
//!
//! * journal: the lab's own loader ([`journal_whatif::load`]), so the set is the book the
//!   lab replays (same filters, same FX pass, same local clock);
//! * backtest: a recorded run replayed over its own datasets and window, the way the run
//!   history and the Monte Carlo panel regenerate its trades;
//! * paper: the closed round trips of a session (`paper_trades`), each the engine's own
//!   trade stored verbatim.
//!
//! The mapping of an engine trade ([`from_engine`]) is pure. A simulation names no
//! currency: its results are in the account currency, `currency` stays `None`, and a
//! fixed-amount fee schedule cannot be priced against it (only a percent one can).

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::journal::{self, Trade, TradeFilter};
use otw_store::journal_analytics::trade_stop;

use super::{Context, SetTrade, SourceKind, TradeSet};
use crate::backtest::Settings;
use crate::journal_whatif::{self as whatif, Book, LabTrade, Scope, Unmeasured};

// ── Journal ──────────────────────────────────────────────────────────────────

/// The journal's closed trades as a set: the lab's book, plus the entry notional and the
/// planned stop distance read from the stored rows.
pub fn from_book(book: Book, currency: &str) -> TradeSet {
    let trades = book
        .trades
        .into_iter()
        .map(|lab| {
            let raw = book.raw.get(&lab.id);
            let notional = raw.and_then(|t| journal_notional(t, lab.fx));
            let stop_pct = raw.and_then(stop_distance);
            SetTrade { lab, notional, stop_pct }
        })
        .collect();
    TradeSet { kind: SourceKind::Journal, currency: Some(currency.to_string()), trades, unmeasured: book.unmeasured }
}

fn journal_notional(t: &Trade, fx: f64) -> Option<f64> {
    let avg = t.avg_entry.filter(|p| *p > 0.0)?;
    let n = avg * journal::trade_entry_qty(t).abs() * t.multiplier * fx;
    (n > 0.0).then_some(n)
}

fn stop_distance(t: &Trade) -> Option<f64> {
    let avg = t.avg_entry.filter(|p| *p > 0.0)?;
    let stop = trade_stop(t)?;
    let pct = (avg - stop).abs() / avg * 100.0;
    (pct > 0.0).then_some(pct)
}

pub async fn load_journal(pool: &sqlx::PgPool, filter: &TradeFilter, currency: &str, tz_offset: i32) -> Result<TradeSet> {
    let book = whatif::load(pool, Scope::Filter(filter), currency, tz_offset).await?;
    Ok(from_book(book, currency))
}

// ── Engine trades (backtest run, paper session) ──────────────────────────────

/// The engine's `Trade` as a run serializes it and a paper session stores it.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineTrade {
    #[serde(default)]
    pub ticker: String,
    pub entry_ts: String,
    pub exit_ts: String,
    pub entry_price: f64,
    pub qty: f64,
    pub direction: String,
    pub pnl: f64,
    #[serde(default)]
    pub fees: f64,
    /// Worst and best open result while in the trade (account currency, >= 0). Both zero
    /// = not tracked by the engine path that closed it.
    #[serde(default)]
    pub mae: f64,
    #[serde(default)]
    pub mfe: f64,
}

/// What the mapping needs to know about the run besides its trades.
#[derive(Default)]
pub struct EngineMeta {
    /// The run's settings, when they parse: the contract multiplier, the stops and the
    /// trading-window clock come from them. Without them, nothing of the three is stated.
    pub settings: Option<Settings>,
    /// Ticker to asset class, from the datasets (journal vocabulary: stock, etf, ...).
    pub classes: HashMap<String, String>,
    /// Ticker of a single-asset run, whose trades carry none.
    pub default_ticker: String,
}

/// A dataset's asset type in the journal's vocabulary (the inverse of
/// `journal_market::asset_type_for`); anything else is kept as is.
pub fn class_of(asset_type: &str) -> String {
    match asset_type {
        "equity" => "stock",
        "fx" => "forex",
        other => other,
    }
    .to_string()
}

fn parse(ts: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(ts.trim(), &Rfc3339).ok()
}

/// Map engine trades into set trades, in exit order. Hours and weekdays are read on the
/// strategy's own trading-window clock (its timezone or offset), so a window found here is
/// the window the strategy's filters speak. A trade whose exit time does not parse is
/// unmeasured (`undated`).
pub fn from_engine(rows: &[EngineTrade], meta: &EngineMeta) -> (Vec<SetTrade>, Vec<Unmeasured>) {
    let mult = meta.settings.as_ref().map(|s| s.instrument.multiplier).filter(|m| *m > 0.0);
    let mut out = Vec::with_capacity(rows.len());
    let mut gone = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let id = Uuid::from_u128(i as u128 + 1);
        let ticker = if r.ticker.is_empty() { meta.default_ticker.clone() } else { r.ticker.clone() };
        let Some(exit) = parse(&r.exit_ts) else {
            gone.push(Unmeasured { id, ticker, reason: "undated" });
            continue;
        };
        let entry = parse(&r.entry_ts);
        let local = |dt: OffsetDateTime| match &meta.settings {
            Some(s) => s.filters.local(dt),
            None => dt,
        };
        let short = r.direction == "short";
        let notional = mult.map(|m| r.entry_price.abs() * r.qty.abs() * m).filter(|n| *n > 0.0);
        let stop_pct = meta.settings.as_ref().and_then(|s| {
            let side = if short { s.short.as_ref() } else { s.long.as_ref() }?;
            let rule = side.sl_rule()?;
            (rule.kind == "pct" && rule.value > 0.0).then_some(rule.value * 100.0)
        });
        let tracked = r.mae > 0.0 || r.mfe > 0.0;
        let pct = |v: f64| notional.filter(|_| tracked).map(|n| v / n * 100.0);
        let lab = LabTrade {
            id,
            ticker: ticker.clone(),
            asset_class: meta.classes.get(&ticker).cloned().unwrap_or_default(),
            side: if short { "short" } else { "long" }.into(),
            exit_at: exit,
            entry_local: entry.map(local),
            exit_local: local(exit),
            qty: r.qty.abs(),
            entry_price: Some(r.entry_price).filter(|p| *p > 0.0),
            net: r.pnl,
            fees: r.fees,
            risk: match (stop_pct, notional) {
                (Some(s), Some(n)) => Some(s / 100.0 * n),
                _ => None,
            },
            fx: 1.0,
            regime: None,
            mae_pct: pct(r.mae),
            mfe_pct: pct(r.mfe),
        };
        out.push(SetTrade { lab, notional, stop_pct });
    }
    out.sort_by_key(|t| t.lab.exit_at);
    (out, gone)
}

/// Ticker to asset class for a run's datasets.
async fn classes(pool: &sqlx::PgPool, ids: &[Uuid]) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for id in ids {
        if let Some(d) = otw_store::histdata::get_dataset(pool, *id).await? {
            out.insert(d.ticker.clone(), class_of(&d.asset_type));
        }
    }
    Ok(out)
}

pub fn engine_set(kind: SourceKind, rows: &[EngineTrade], meta: &EngineMeta) -> TradeSet {
    let (trades, unmeasured) = from_engine(rows, meta);
    TradeSet { kind, currency: None, trades, unmeasured }
}

/// A recorded backtest run, replayed. The caller has already found the run.
pub async fn load_backtest(
    state: &crate::AppState,
    run_id: Uuid,
) -> Result<(TradeSet, otw_store::backtest::SavedRun), crate::ApiError> {
    let (run, settings, _, result) = crate::backtest_api::replay_run(state, run_id).await?;
    let rows: Vec<EngineTrade> = serde_json::to_value(&result.trades)
        .and_then(serde_json::from_value)
        .map_err(|e| crate::ApiError::from(anyhow::anyhow!("reading the run's trades: {e}")))?;
    let ids: Vec<Uuid> = match run.dataset_ids.as_deref() {
        Some(v) if !v.is_empty() => v.to_vec(),
        _ => run.dataset_id.map(|d| vec![d]).unwrap_or_default(),
    };
    let meta = EngineMeta {
        settings: Some(settings),
        classes: classes(&state.pool, &ids).await?,
        default_ticker: run.ticker.clone(),
    };
    Ok((engine_set(SourceKind::Backtest, &rows, &meta), run))
}

/// A paper session's closed round trips. A stored trade that no longer reads as an engine
/// trade is unmeasured (`unreadable`), never dropped silently.
pub async fn load_paper(pool: &sqlx::PgPool, session: &otw_store::paper::Session) -> Result<TradeSet> {
    let stored = otw_store::paper::list_trades(pool, session.id, false).await?;
    let mut rows = Vec::new();
    let mut unreadable = Vec::new();
    for (i, t) in stored.iter().filter(|t| !t.open).enumerate() {
        match serde_json::from_value::<EngineTrade>(t.trade.clone()) {
            Ok(mut r) => {
                if r.ticker.is_empty() {
                    r.ticker = t.ticker.clone();
                }
                rows.push(r);
            }
            Err(_) => unreadable.push(Unmeasured {
                id: Uuid::from_u128(u128::MAX - i as u128),
                ticker: t.ticker.clone(),
                reason: "unreadable",
            }),
        }
    }
    let meta = EngineMeta {
        settings: serde_json::from_value::<Settings>(session.settings.clone()).ok(),
        classes: classes(pool, &session.dataset_ids).await?,
        default_ticker: String::new(),
    };
    let mut set = engine_set(SourceKind::Paper, &rows, &meta);
    set.unmeasured.extend(unreadable);
    Ok(set)
}

// ── Context ──────────────────────────────────────────────────────────────────

/// Fee schedules and their rates, and the SIC codes the fundamentals module stored.
pub async fn context(pool: &sqlx::PgPool, set: &TradeSet, journal_installed: bool) -> Result<Context> {
    let mut ctx = Context { journal_installed, ..Default::default() };
    if journal_installed {
        ctx.schedules = journal::list_fee_schedules(pool).await?;
        let lab: Vec<LabTrade> = set.lab();
        for s in &ctx.schedules {
            let rates = match &set.currency {
                Some(cur) => whatif::schedule_rates(pool, &lab, s, cur).await?,
                // A simulation's money is not named: a percent schedule reads the trade's
                // own price, a fixed amount cannot be converted and is not priced.
                None => lab.iter().map(|_| (s.amount_kind == "pct").then_some(1.0)).collect(),
            };
            ctx.schedule_rates.insert(s.id, rates);
        }
    }
    for c in otw_store::fundamentals::list_companies(pool).await? {
        if !c.sic.trim().is_empty() {
            ctx.sectors.insert(c.ticker.trim().to_uppercase(), (c.sic.trim().to_string(), c.industry.trim().to_string()));
        }
    }
    Ok(ctx)
}
