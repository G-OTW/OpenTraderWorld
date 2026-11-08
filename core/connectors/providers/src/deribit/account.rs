//! Deribit accounts, read only.
//!
//!   - Summary and balances: `private/get_account_summaries`, one row per currency.
//!   - Positions: `private/get_positions?currency=any`, every kind at once.
//!   - Open orders: `private/get_open_orders`, every kind at once.
//!   - Fills: `private/get_user_trades_by_currency_and_time`, one (currency, kind) group at a
//!     time, oldest first, 1000 a page, the next page starting at the last fill's
//!     millisecond. Each group is read twice: `historical=true` (kept for good, indexed after
//!     a delay) over the whole period, then `historical=false` (the last 24 hours, at once)
//!     where the period reaches them. A fill seen twice (the two reads overlap, a spot pair
//!     sits under its base and its quote) is handed out once.
//!
//! Quantities are the venue's amounts: the base coin on spot, options and linear contracts,
//! USD on an inverse one. Inverse positions and orders are converted to coins (USD over the
//! entry, order or trigger price) and quoted in USD; their profit is counted in the coin and
//! is not carried. **Inverse fills are not read**: their profit is quantity × (1/entry −
//! 1/exit) in the coin, which the journal does not compute. Coin-settled options are fine:
//! their amount and premium are both in the coin.

use std::collections::{HashMap, HashSet};

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event, Fill, InstrumentId,
    Mapped, Money, Order, Position, Price, Qty, Side, TimeRange, Timestamp,
};
use otw_connector_sdk::parse::{dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{at_ms, money, Kind, Name, Rpc, REST, VENUE};

/// Fills per call, the venue's most.
const FILL_PAGE: usize = 1000;
const MAX_PAGES: u32 = 2_000;
/// How far back the recent store reaches.
const RECENT_MS: i64 = 86_400_000;
/// The (currency, kind) groups a fill walk reads when no instrument is named: the options
/// settled in BTC, ETH and USDC, the USDC-settled futures, and the spot pairs under every
/// currency that quotes one. Inverse futures (BTC and ETH `future`) are left out.
const GROUPS: &[(&str, &str)] =
    &[("BTC", "option"), ("ETH", "option"), ("USDC", "option"), ("USDC", "future"), ("USDC", "spot"), ("USDT", "spot"), ("BTC", "spot"), ("ETH", "spot")];

pub type Group = (String, &'static str);

pub enum Pending {
    Summary,
    Balances,
    Positions,
    Orders,
    Fills { groups: Vec<Group>, cur: Cursor, range: TimeRange },
}

/// Where a fill walk stands: the group, which store (`recent`), the first millisecond the
/// next page reads, pages used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub g: usize,
    pub recent: bool,
    pub start: i64,
    pub pages: u32,
}

fn bad(c: &str) -> ConnectorError {
    ConnectorError::invalid(format!("bad cursor `{c}`"))
}

impl Cursor {
    fn encode(&self) -> String {
        format!("{}:{}:{}:{}", self.g, u8::from(self.recent), self.start, self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let parts: Vec<&str> = c.split(':').collect();
        let [g, recent, start, pages] = parts.as_slice() else { return Err(bad(c)) };
        Ok(Cursor { g: g.parse().map_err(|_| bad(c))?, recent: *recent == "1", start: start.parse().map_err(|_| bad(c))?, pages: pages.parse().map_err(|_| bad(c))? })
    }
}

/// Where the recent store's read starts, when the period reaches it.
fn recent_from(range: &TimeRange, now: Timestamp) -> Option<i64> {
    let from = range.from.millis().max(now.millis() - RECENT_MS);
    (from < range.to.millis()).then_some(from)
}

/// After a group's store is read: its recent store, the next group, or the end.
fn after_pass(cur: Cursor, pages: u32, groups: usize, range: &TimeRange, now: Timestamp) -> Option<Cursor> {
    if !cur.recent {
        if let Some(start) = recent_from(range, now) {
            return Some(Cursor { recent: true, start, pages, ..cur });
        }
    }
    (cur.g + 1 < groups).then_some(Cursor { g: cur.g + 1, recent: false, start: range.from.millis(), pages })
}

fn signed(endpoint: &'static str, path: &str, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, req_id).signed()
}

/// The groups that hold the named instruments, or every group.
fn groups_for(symbols: &[InstrumentId]) -> Result<Vec<Group>, ConnectorError> {
    if symbols.is_empty() {
        return Ok(GROUPS.iter().map(|(c, k)| (c.to_string(), *k)).collect());
    }
    let mut out: Vec<Group> = Vec::new();
    for s in symbols {
        let name = Name::of(&s.native);
        let quote = name.quote().unwrap_or_default();
        let g = match name.kind {
            Kind::Spot => (quote, "spot"),
            Kind::Option => (quote, "option"),
            Kind::Perp | Kind::Future if name.linear() => (quote, "future"),
            Kind::Perp | Kind::Future => {
                return Err(ConnectorError::unsupported(format!(
                    "Deribit inverse fills are not imported ({}): an inverse contract's profit is counted in the coin, quantity × (1/entry − 1/exit), \
                     and the journal counts quantity × price. Its positions and orders are read.",
                    s.native
                )))
            }
            Kind::Combo => {
                return Err(ConnectorError::unsupported(format!("Deribit combos are read through their legs: name the legs of {} instead.", s.native)));
            }
        };
        if !out.contains(&g) {
            out.push(g);
        }
    }
    Ok(out)
}

fn fills_request(req_id: CorrId, groups: &[Group], cur: &Cursor, range: &TimeRange) -> Result<HttpRequest, ConnectorError> {
    if cur.pages >= MAX_PAGES {
        return Err(ConnectorError::invalid("this period holds more Deribit fills than one pull reads. Ask for a shorter period."));
    }
    let (currency, kind) = groups.get(cur.g).ok_or_else(|| ConnectorError::invalid("cursor past the group list"))?;
    Ok(signed("userTrades", "/api/v2/private/get_user_trades_by_currency_and_time", req_id)
        .query("currency", currency)
        .query("kind", kind)
        .query("start_timestamp", cur.start)
        .query("end_timestamp", range.to.millis() - 1)
        .query("count", FILL_PAGE)
        .query("sorting", "asc")
        .query("historical", if cur.recent { "false" } else { "true" }))
}

pub fn encode(cmd: &Command, pending: &mut HashMap<CorrId, super::Pending>, seen: &mut HashMap<CorrId, HashSet<String>>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, symbols, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let req_id = *req_id;
    let (req, p) = match kind {
        AccountKind::Summary => (signed("accountSummaries", "/api/v2/private/get_account_summaries", req_id).query("extended", "true"), Pending::Summary),
        AccountKind::Balances => (signed("accountSummaries", "/api/v2/private/get_account_summaries", req_id), Pending::Balances),
        AccountKind::Positions => (signed("positions", "/api/v2/private/get_positions", req_id).query("currency", "any"), Pending::Positions),
        AccountKind::OpenOrders => (signed("openOrders", "/api/v2/private/get_open_orders", req_id), Pending::Orders),
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to.millis() <= range.from.millis() {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let groups = groups_for(symbols)?;
            let cur = match cursor {
                Some(c) => Cursor::parse(c)?,
                None => {
                    seen.insert(req_id, HashSet::new());
                    Cursor { g: 0, recent: false, start: range.from.millis(), pages: 0 }
                }
            };
            (fills_request(req_id, &groups, &cur, &range)?, Pending::Fills { groups, cur, range })
        }
    };
    pending.insert(req_id, super::Pending::Account(p));
    Ok(vec![NativeRequest::Http(req)])
}

/// What to check when Deribit refuses the key.
fn hint(e: ConnectorError) -> ConnectorError {
    let fix = match e.kind {
        ErrorKind::Auth if e.message.contains("timestamp") => "This server's clock is off from Deribit's by more than the 60 s a signature lives: set it right (NTP).",
        ErrorKind::Auth => "Check the Client Id and Client Secret from Deribit's API page. A key of test.deribit.com does not work on www.deribit.com.",
        ErrorKind::Permission => "The key needs the read scopes for account (account:read) and trade (trade:read).",
        _ => return e,
    };
    ConnectorError { message: format!("{}. {fix}", e.message), ..e }
}

/// A refused account call. A spot group Deribit refuses because its pairs are matched on
/// Coinbase (11060) is passed over, said in the log; anything else stops the read.
pub fn refused(p: Pending, corr: CorrId, e: ConnectorError, seen: &mut HashMap<CorrId, HashSet<String>>, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    if let Pending::Fills { groups, cur, range } = &p {
        let spot = groups.get(cur.g).is_some_and(|(_, k)| *k == "spot");
        if spot && e.provider_code.as_deref() == Some("11060") {
            tracing::warn!("deribit fills: {} spot fills refused ({}), passed over", groups.get(cur.g).map(|g| g.0.as_str()).unwrap_or_default(), e.message);
            let next = after_pass(*cur, cur.pages + 1, groups.len(), range, ctx.received_ts);
            if next.is_none() {
                seen.remove(&corr);
            }
            return Ok(vec![chunk(ctx, corr, AccountItems::Fills(vec![]), next.map(|c| c.encode()))]);
        }
    }
    seen.remove(&corr);
    Err(hint(e))
}

#[derive(Deserialize)]
struct Summaries {
    #[serde(default)]
    summaries: Vec<CurrencySummary>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct CurrencySummary {
    currency: String,
    #[serde(default)]
    balance: OptDec,
    #[serde(default)]
    equity: OptDec,
    #[serde(default)]
    available_funds: OptDec,
    #[serde(default)]
    total_equity_usd: OptDec,
}

#[derive(Deserialize)]
struct PositionRow {
    instrument_name: String,
    /// `buy`, `sell` or `zero`.
    #[serde(default)]
    direction: String,
    /// USD on a future (inverse or not: the quote), the base coin on an option.
    #[serde(default)]
    size: OptDec,
    /// Futures only: the size in the base coin.
    #[serde(default)]
    size_currency: OptDec,
    #[serde(default)]
    average_price: OptDec,
    #[serde(default)]
    mark_price: OptDec,
    /// In the settlement currency.
    #[serde(default)]
    floating_profit_loss: OptDec,
}

/// A price that may be a number or a word (`"market_price"` on an untriggered market
/// order): the word is no price.
#[derive(Default)]
struct PriceOrWord(Option<Decimal>);

impl<'de> Deserialize<'de> for PriceOrWord {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Option::<Box<RawValue>>::deserialize(d)?;
        let Some(raw) = raw else { return Ok(PriceOrWord(None)) };
        let t = raw.get().trim();
        let t = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
        Ok(PriceOrWord(dec(t).ok()))
    }
}

#[derive(Deserialize)]
struct OrderRow {
    order_id: String,
    instrument_name: String,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    order_type: String,
    /// USD on an inverse contract, the base coin elsewhere.
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    filled_amount: OptDec,
    #[serde(default)]
    price: PriceOrWord,
    #[serde(default)]
    trigger_price: OptDec,
    #[serde(default)]
    creation_timestamp: Option<Int>,
}

#[derive(Deserialize)]
struct TradesPage {
    #[serde(default)]
    trades: Vec<TradeRow>,
    #[serde(default)]
    has_more: bool,
}

#[derive(Deserialize)]
struct TradeRow {
    trade_id: String,
    instrument_name: String,
    timestamp: Int,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    order_id: Option<String>,
    /// The base coin on spot, options and linear contracts.
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    price: OptDec,
    /// Positive is a cost, negative a rebate.
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_currency: Option<String>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("deribit {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("deribit", "direction", raw),
    }
}

fn positive(d: Option<Decimal>) -> Option<Price> {
    d.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

/// USD contracts to coins at a price.
fn per(usd: Decimal, price: Option<Decimal>) -> Option<Decimal> {
    price.filter(|p| !p.is_zero()).and_then(|p| usd.checked_div(p))
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], seen: &mut HashMap<CorrId, HashSet<String>>, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let evs = match p {
        Pending::Summary => {
            let r: Rpc<Summaries> = serde_json::from_slice(body).map_err(|e| decode_err("account summaries", e))?;
            let held: Vec<&CurrencySummary> = r.result.summaries.iter().filter(|s| s.balance.0.is_some_and(|b| !b.is_zero())).collect();
            let total = r.result.summaries.iter().find_map(|s| s.total_equity_usd.0);
            let kind = r.result.kind.clone().unwrap_or_else(|| "main".into());
            // Rounded, not cut: `Decimal` precision formatting truncates.
            let detail = match total {
                Some(t) => format!("Deribit {kind} account: {} currency(ies) with a balance, total equity {:.2} USD", held.len(), t.round_dp(2)),
                None => format!(
                    "Deribit {kind} account: {}",
                    held.iter().map(|s| format!("{} {} equity", s.equity.0.unwrap_or_default().normalize(), s.currency)).collect::<Vec<_>>().join(", ")
                ),
            };
            vec![env(
                ctx,
                corr,
                Event::AccountSummary(AccountSummary {
                    account_type: r.result.kind,
                    permissions: vec![],
                    assets_held: u32::try_from(held.len()).unwrap_or(u32::MAX),
                    can_trade: None,
                    open_positions: None,
                    wallet_balance: None,
                    detail: Some(detail),
                }),
            )]
        }
        Pending::Balances => {
            let r: Rpc<Summaries> = serde_json::from_slice(body).map_err(|e| decode_err("account summaries", e))?;
            let out = r
                .result
                .summaries
                .into_iter()
                .filter_map(|s| {
                    let total = s.balance.0.filter(|b| b.is_sign_positive() && !b.is_zero())?;
                    let available = s.available_funds.0.unwrap_or(total).clamp(Decimal::ZERO, total);
                    Some(Balance { asset: s.currency.to_ascii_uppercase(), total: Money(total), available: Money(available), name: None, fiat: false, account: None })
                })
                .collect();
            vec![chunk(ctx, corr, AccountItems::Balances(out), None)]
        }
        Pending::Positions => {
            let r: Rpc<Vec<PositionRow>> = serde_json::from_slice(body).map_err(|e| decode_err("positions", e))?;
            let mut out = Vec::new();
            for p in r.result {
                let name = Name::of(&p.instrument_name);
                let sign = match p.direction.as_str() {
                    "buy" => Decimal::ONE,
                    "sell" => -Decimal::ONE,
                    "zero" => continue,
                    other => return Err(ConnectorError::decode(format!("deribit positions: direction `{other}` on {}", p.instrument_name))),
                };
                let size = p.size.0.unwrap_or_default().abs();
                if size.is_zero() {
                    continue;
                }
                let (qty, pnl, label) = match name.kind {
                    // USD contracts to coins at the entry price; the profit is in the coin.
                    Kind::Perp | Kind::Future if name.inverse() => {
                        let coins = per(size, p.average_price.0)
                            .or(p.size_currency.0.map(|c| c.abs()))
                            .ok_or_else(|| ConnectorError::decode(format!("deribit positions: {} has no entry price to count its coins", p.instrument_name)))?;
                        (coins, None, Some(format!("inverse, {} USD contracts, profit counted in the coin", size.normalize())))
                    }
                    Kind::Perp | Kind::Future => (p.size_currency.0.map(|c| c.abs()).unwrap_or(size), p.floating_profit_loss.0.map(Money), None),
                    Kind::Option | Kind::Spot => (size, p.floating_profit_loss.0.map(Money), None),
                    Kind::Combo => {
                        tracing::warn!("deribit positions: {} is not a single instrument, left out", p.instrument_name);
                        continue;
                    }
                };
                out.push(Position {
                    instrument: InstrumentId::new(VENUE, &p.instrument_name),
                    qty: Qty(sign * qty),
                    avg_price: positive(p.average_price.0),
                    unrealized_pnl: pnl,
                    currency: name.quote(),
                    mark_price: positive(p.mark_price.0),
                    venue: None,
                    account: None,
                    class: name.class(),
                    multiplier: None,
                    name: label,
                });
            }
            vec![chunk(ctx, corr, AccountItems::Positions(out), None)]
        }
        Pending::Orders => {
            let r: Rpc<Vec<OrderRow>> = serde_json::from_slice(body).map_err(|e| decode_err("open orders", e))?;
            let mut out = Vec::new();
            for o in r.result {
                let name = Name::of(&o.instrument_name);
                let amount = o.amount.0.unwrap_or_default();
                let filled = o.filled_amount.0.unwrap_or_default();
                let (qty, filled) = if name.inverse() {
                    // USD contracts to coins at the order's own price, else its trigger.
                    let at = o.price.0.filter(|p| !p.is_zero()).or(o.trigger_price.0);
                    match (per(amount, at), per(filled, at)) {
                        (Some(q), Some(f)) => (q, f),
                        _ => return Err(ConnectorError::decode(format!("deribit open orders: inverse order {} names no price to count its coins", o.order_id))),
                    }
                } else {
                    (amount, filled)
                };
                out.push(Order {
                    id: o.order_id,
                    instrument: InstrumentId::new(VENUE, &o.instrument_name),
                    side: side(&o.direction),
                    order_type: o.order_type,
                    qty: Qty(qty),
                    filled_qty: Qty(filled),
                    limit_price: positive(o.price.0),
                    stop_price: positive(o.trigger_price.0),
                    placed_at: at_ms(o.creation_timestamp)?,
                    quote_ccy: name.quote(),
                    account: None,
                    venue: None,
                    class: name.class(),
                });
            }
            vec![chunk(ctx, corr, AccountItems::Orders(out), None)]
        }
        Pending::Fills { groups, cur, range } => {
            let r: Rpc<TradesPage> = serde_json::from_slice(body).map_err(|e| decode_err("user trades", e))?;
            let page = r.result;
            let n = page.trades.len();
            let last = page.trades.iter().map(|t| t.timestamp.0).max();
            let ids = seen.entry(corr).or_default();
            let mut fills = Vec::with_capacity(n);
            for t in page.trades {
                if !ids.insert(t.trade_id.clone()) {
                    continue;
                }
                let name = Name::of(&t.instrument_name);
                if name.inverse() {
                    tracing::warn!("deribit fills: {} is an inverse contract, its fill {} is not imported", t.instrument_name, t.trade_id);
                    continue;
                }
                fills.push(Fill {
                    id: t.trade_id,
                    order_id: t.order_id.filter(|o| !o.is_empty()),
                    instrument: InstrumentId::new(VENUE, &t.instrument_name),
                    ts: otw_connector_sdk::parse::ts_millis(t.timestamp.0)?,
                    side: side(&t.direction),
                    qty: Qty(t.amount.0.unwrap_or_default()),
                    price: Price(t.price.0.unwrap_or_default()),
                    fee: money(t.fee.0),
                    fee_asset: t.fee_currency.map(|a| a.to_ascii_uppercase()).filter(|a| !a.is_empty()),
                    quote_ccy: name.quote(),
                    account: None,
                    venue: None,
                    class: name.class(),
                    multiplier: None,
                });
            }
            let pages = cur.pages + 1;
            let next = match last {
                // A full page: the next one starts at its last millisecond (the fills already
                // handed out there are skipped).
                Some(last) if page.has_more || n >= FILL_PAGE => {
                    if last <= cur.start {
                        return Err(ConnectorError::invalid(format!(
                            "Deribit answered more than {FILL_PAGE} fills in one millisecond ({}); this period cannot be read over the API.",
                            otw_connector_sdk::parse::ts_millis(last).map(|t| t.to_string()).unwrap_or_default()
                        )));
                    }
                    Some(Cursor { start: last, pages, ..cur })
                }
                _ => after_pass(cur, pages, groups.len(), &range, ctx.received_ts),
            };
            if next.is_none() {
                seen.remove(&corr);
            }
            vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))]
        }
    };
    Ok(evs)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::ts;

    #[test]
    fn a_walk_reads_each_group_twice_where_the_period_reaches_the_last_day() {
        let now = ts(1_792_400_000);
        let range = TimeRange::new(ts(1_792_400_000 - 3 * 86_400), now);
        let first = Cursor { g: 0, recent: false, start: range.from.millis(), pages: 0 };
        let recent = after_pass(first, 1, 2, &range, now).unwrap();
        assert_eq!((recent.g, recent.recent, recent.start), (0, true, now.millis() - RECENT_MS));
        let second = after_pass(recent, 2, 2, &range, now).unwrap();
        assert_eq!((second.g, second.recent, second.start), (1, false, range.from.millis()));
        assert!(after_pass(Cursor { recent: true, ..second }, 3, 2, &range, now).is_none());
        // A period that ends before the last day reads the historical store only.
        let old = TimeRange::new(ts(1_700_000_000), ts(1_700_086_400));
        assert_eq!(after_pass(Cursor { start: old.from.millis(), ..first }, 1, 2, &old, now).map(|c| c.g), Some(1));
        assert_eq!(Cursor::parse(&recent.encode()).unwrap(), recent);
    }

    #[test]
    fn named_instruments_pick_their_groups_and_inverse_ones_are_refused_by_name() {
        let ids = |names: &[&str]| names.iter().map(|n| InstrumentId::new(VENUE, n)).collect::<Vec<_>>();
        let g = groups_for(&ids(&["BTC-27DEC26-100000-C", "SOL_USDC-27DEC26-150-P", "BTC_USDC-PERPETUAL", "ETH_BTC", "BTC-25DEC26-90000-P"])).unwrap();
        assert_eq!(g, vec![("BTC".to_string(), "option"), ("USDC".to_string(), "option"), ("USDC".to_string(), "future"), ("BTC".to_string(), "spot")]);
        let e = groups_for(&ids(&["BTC-PERPETUAL"])).unwrap_err();
        assert!(e.kind == ErrorKind::Unsupported && e.message.contains("BTC-PERPETUAL"), "{e:?}");
        assert_eq!(groups_for(&[]).unwrap().len(), GROUPS.len());
    }
}
