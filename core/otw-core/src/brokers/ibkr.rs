//! Interactive Brokers through a local IB Gateway or TWS: the account as the socket API sees
//! it right now (positions, working orders), where the Flex broker reads yesterday's
//! statement.
//!
//! The account is an **address**, not a key: the user runs IB Gateway (or TWS) with "Enable
//! ActiveX and Socket Clients" ticked, and the account carries `host` and `port`. It rides
//! the same session as the market data connector ([`gateway::session`]), so a gateway that
//! already feeds the charts is one socket, not two.
//!
//! **Orders.** An account with the `trading` grant takes the chart's tickets here, turned into
//! canonical [`OrderRequest`]s and sent by the gateway adapter
//! ([`otw_connector_providers::ibkr::gateway::orders`]) on the account's own session: the
//! port the account names, never the market data connector's. Everything about IB (the
//! translation, the tick, the paper-only rule below E4, the never-retried submission) lives
//! there; this file only names the instrument and converts the answers.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use ibapi::contracts::{Contract, SecurityType};
use ibapi::accounts::Position as IbPosition;
use ibapi::orders::{Action, OrderData, OrderUpdate, Orders};
use otw_connector_model::{Mapped, OrderAmend, OrderRequest, Timestamp};
use otw_connector_providers::ibkr::gateway::{account, events, mapper, margin, orders, resolve, session, IbkrGateway};

use super::{AccountEvent, Broker, BrokerCapability, Lookup, OpenOrder, OrderTicket, OrderUnknown, Placed, Position};

pub struct IbkrGatewayBroker {
    gateway: IbkrGateway,
    cap: &'static BrokerCapability,
}

/// The broker, or None when the gateway's capability file does not parse (its provider test
/// makes that a build failure).
pub fn ibkr() -> Option<IbkrGatewayBroker> {
    Some(IbkrGatewayBroker { gateway: IbkrGateway::new().ok()?, cap: &super::caps::IBKR })
}

fn err(e: otw_connector_model::ConnectorError) -> anyhow::Error {
    anyhow::Error::new(e)
}

/// A placement's failure: IB's refusal as it is, an answer lost as [`OrderUnknown`], so the
/// legs it names stay unknown until [`Broker::lookup`] settles them.
fn place_err(e: orders::PlaceError) -> anyhow::Error {
    match e {
        orders::PlaceError::Refused(e) => err(e),
        orders::PlaceError::Unknown { keys, message, unmapped } => anyhow::Error::new(OrderUnknown { keys, message, alert: unmapped }),
    }
}

/// What IB lists under a client id, in the trait's words. An id is empty only for an order IB
/// lists among the day's completed ones with no fill to name it: nothing to cancel there.
fn lookup_of(seen: Option<orders::Seen>) -> Lookup {
    match seen {
        Some(s) => Lookup::Found { id: s.order_id.map(|i| i.to_string()).unwrap_or_default(), status: s.status, filled_qty: s.filled },
        None => Lookup::NotFound,
    }
}

/// The ticker OTW charts this contract under, so a book line lands on the chart showing it.
/// A dated contract is its local symbol (`MNQZ6`, the OCC option code), a currency pair is
/// both currencies, everything else its symbol. Never a root for a dated contract: the
/// continuous `MNQ` chart is not the December contract.
pub(crate) fn ticker_of(c: &Contract) -> String {
    match c.security_type {
        SecurityType::Future | SecurityType::Option | SecurityType::FuturesOption if !c.local_symbol.is_empty() => c.local_symbol.clone(),
        SecurityType::ForexPair => format!("{}{}", c.symbol.0, c.currency.0),
        _ => c.symbol.0.clone(),
    }
}

/// A contract's market in the account vocabulary.
pub(crate) fn class_of(t: &SecurityType) -> &'static str {
    match t {
        SecurityType::Stock => "stock",
        SecurityType::Future | SecurityType::ContinuousFuture => "future",
        SecurityType::Option | SecurityType::FuturesOption => "option",
        SecurityType::ForexPair => "forex",
        SecurityType::Crypto => "crypto",
        SecurityType::Index => "index",
        _ => "other",
    }
}

/// Contract size: IB sends it as text, empty for a share.
fn multiplier(c: &Contract) -> f64 {
    c.multiplier.trim().parse::<f64>().ok().filter(|m| *m > 0.0).unwrap_or(1.0)
}

/// IB reports a resting price of zero, or `f64::MAX`, for a level an order does not carry.
fn level(v: Option<f64>) -> Option<f64> {
    v.filter(|p| p.is_finite() && *p > 0.0 && *p < 1e300)
}

impl IbkrGatewayBroker {
    /// The positions, valued at IB's own market price, each with when it opened and what
    /// that cost when the last days of fills reach back to it. The fills are a garnish: a
    /// refusal there leaves the positions without an entry rather than without positions.
    ///
    /// What is held comes from the positions feed, the one the live events come from: IB's
    /// portfolio lags it by a second or two after a fill, long enough for a read right after
    /// an exit to bring back the position just closed. The portfolio only adds its valuation,
    /// and only to a row it agrees on; without the feed it is read alone.
    async fn read_positions(&self, settings: &HashMap<String, String>) -> Result<Vec<Position>> {
        let s = session::get(settings).await.map_err(err)?;
        let rows = account::portfolio(&s).await.map_err(err)?;
        let fills = match account::fills(&s).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("ibkr: no fills for position entries: {e}");
                Vec::new()
            }
        };
        let held = match events::held(&s).await {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::warn!("ibkr: positions read from the portfolio alone: {e}");
                None
            }
        };
        let entry_of = |conid: i32, account: &str, held: f64| {
            let mine: Vec<account::Fill> = fills.iter().filter(|f| f.contract_id == conid && (account.is_empty() || f.account == account)).cloned().collect();
            mapper::dec(held).ok().and_then(|h| account::entry(&mine, h))
        };
        let mut out = Vec::new();
        match held {
            Some(held) => {
                for h in held.iter().filter(|h| h.position != 0.0) {
                    let mut row = position_of(h);
                    let p = rows.iter().find(|p| p.contract.contract_id == h.contract.contract_id && p.account.as_deref().is_none_or(|a| a == h.account));
                    if let Some(p) = p {
                        if row.venue.is_empty() {
                            row.venue = p.contract.exchange.0.clone();
                        }
                        row.market_price = level(Some(p.market_price));
                        // The portfolio's result belongs to the same holding only.
                        if p.position == h.position {
                            row.unrealized_pnl = Some(p.unrealized_pnl).filter(|v| v.is_finite() && v.abs() < 1e300);
                        }
                    }
                    let entry = entry_of(h.contract.contract_id, &h.account, h.position);
                    row.opened_at = entry.as_ref().and_then(|e| e.at.to_offset_datetime());
                    row.entry_fee = entry.as_ref().and_then(|e| e.fee);
                    row.fee_currency = entry.map(|e| e.fee_currency).unwrap_or_default();
                    out.push(row);
                }
            }
            // A closed position lingers in the list at zero until the gateway restarts.
            None => {
                for p in rows.into_iter().filter(|p| p.position != 0.0) {
                    let mult = multiplier(&p.contract);
                    let account = p.account.clone().unwrap_or_default();
                    let entry = entry_of(p.contract.contract_id, &account, p.position);
                    out.push(Position {
                        symbol: ticker_of(&p.contract),
                        side: if p.position < 0.0 { "short".into() } else { "long".into() },
                        qty: p.position.abs(),
                        // IB's average cost is per contract, multiplier included: the chart wants
                        // the price.
                        avg_price: (p.average_cost > 0.0).then(|| p.average_cost / mult),
                        currency: p.contract.currency.0.clone(),
                        asset_class: class_of(&p.contract.security_type).into(),
                        multiplier: mult,
                        venue: p.contract.exchange.0.clone(),
                        native_id: Some(p.contract.contract_id.to_string()),
                        market_price: level(Some(p.market_price)),
                        unrealized_pnl: Some(p.unrealized_pnl).filter(|v| v.is_finite() && v.abs() < 1e300),
                        opened_at: entry.as_ref().and_then(|e| e.at.to_offset_datetime()),
                        entry_fee: entry.as_ref().and_then(|e| e.fee),
                        fee_currency: entry.map(|e| e.fee_currency).unwrap_or_default(),
                        leverage: None,
                        margin_mode: None,
                        account,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(out)
    }

    async fn read_orders(&self, settings: &HashMap<String, String>) -> Result<Vec<OpenOrder>> {
        let s = session::get(settings).await.map_err(err)?;
        let (data, filled) = s
            .ask("open orders", |client| async move {
                let mut sub = client.all_open_orders().await?;
                let mut data = Vec::new();
                let mut filled: HashMap<i32, f64> = HashMap::new();
                while let Some(item) = sub.next().await {
                    match item? {
                        Orders::OrderData(d) => data.push(d),
                        Orders::OrderStatus(st) => {
                            filled.insert(st.order_id, st.filled);
                        }
                        Orders::Notice(_) => {}
                    }
                }
                Ok((data, filled))
            })
            .await
            .map_err(err)?;
        Ok(working_orders(data, &filled))
    }
}

/// The working orders of one open-orders read. IB can send an order more than once in a read
/// (each change, its last state latest), and an order that just ended is still listed with
/// its final status: one row per order, as it last stood, and only those still working.
fn working_orders(data: Vec<OrderData>, filled: &HashMap<i32, f64>) -> Vec<OpenOrder> {
    let mut last: Vec<OrderData> = Vec::new();
    for d in data {
        match last.iter_mut().find(|x| x.order_id == d.order_id) {
            Some(x) => *x = d,
            None => last.push(d),
        }
    }
    last.iter()
        .filter(|d| done_status(&d.order_state.status).is_none())
        .map(|d| open_order_of(d, filled.get(&d.order_id).copied().unwrap_or(0.0)))
        .collect()
}

/// A working order as the book lists it.
fn open_order_of(d: &OrderData, filled_qty: f64) -> OpenOrder {
    let side = match d.order.action {
        Action::Buy => "buy",
        _ => "sell",
    };
    // A trailing stop's `aux_price` is its distance, not a price: where it stands now is
    // `trail_stop_price`.
    let trailing = d.order.order_type.starts_with("TRAIL");
    OpenOrder {
        id: d.order_id.to_string(),
        symbol: ticker_of(&d.contract),
        side: side.into(),
        order_type: d.order.order_type.clone(),
        qty: d.order.total_quantity,
        filled_qty,
        limit_price: level(d.order.limit_price),
        stop_price: level(if trailing { d.order.trail_stop_price } else { d.order.aux_price }),
        trail_start: None,
        trail_rate: if trailing { d.order.trailing_percent.filter(|p| p.is_finite() && *p > 0.0) } else { None },
        currency: d.contract.currency.0.clone(),
        asset_class: class_of(&d.contract.security_type).into(),
        placed_at: None,
        venue: d.contract.exchange.0.clone(),
        account: d.order.account.clone(),
        parent_id: (d.order.parent_id != 0).then(|| d.order.parent_id.to_string()),
        reduce_only: false,
    }
}

/// A position as IB's positions feed reports it: quantity and cost, none of the figures the
/// portfolio read adds (mark, result, entry).
fn position_of(p: &IbPosition) -> Position {
    let mult = multiplier(&p.contract);
    Position {
        symbol: ticker_of(&p.contract),
        side: if p.position < 0.0 { "short".into() } else { "long".into() },
        qty: p.position.abs(),
        avg_price: (p.average_cost > 0.0).then(|| p.average_cost / mult),
        currency: p.contract.currency.0.clone(),
        asset_class: class_of(&p.contract.security_type).into(),
        multiplier: mult,
        venue: p.contract.exchange.0.clone(),
        account: p.account.clone(),
        native_id: Some(p.contract.contract_id.to_string()),
        market_price: None,
        unrealized_pnl: None,
        opened_at: None,
        entry_fee: None,
        fee_currency: String::new(),
        leverage: None,
        margin_mode: None,
    }
}

/// IB's status word as the book's: `None` for an order still working.
fn done_status(ib: &str) -> Option<String> {
    match orders::status_of(ib) {
        Mapped::Known(st) if st.is_final() => serde_json::to_value(st).ok().and_then(|v| v.as_str().map(String::from)),
        _ => None,
    }
}

/// What a follower remembers between feed events: what has filled of each order, so an open
/// order sent again (a move) keeps it, and the orders that ended.
#[derive(Default)]
struct Followed {
    filled: HashMap<i32, f64>,
    done: HashSet<i32>,
}

/// One feed event as the book's. IB's messages can arrive out of order (a status before its
/// open order, an older status after a newer one): a filled quantity never shrinks, and an
/// order that ended never works again (spec 2.2), whatever a late message says.
fn account_event(e: &events::Event, seen: &mut Followed) -> Option<AccountEvent> {
    match e {
        events::Event::Resync => Some(AccountEvent::Resync),
        events::Event::Position(p) => Some(AccountEvent::Position { position: position_of(p) }),
        events::Event::Order(u) => match u.as_ref() {
            // A margin what-if echoes as an open order; it was never placed.
            OrderUpdate::OpenOrder(d) if d.order.what_if => None,
            OrderUpdate::OpenOrder(d) => match done_status(&d.order_state.status) {
                Some(status) => {
                    seen.filled.remove(&d.order_id);
                    seen.done.insert(d.order_id);
                    Some(AccountEvent::Done { id: d.order_id.to_string(), status, reason: None })
                }
                None if seen.done.contains(&d.order_id) => None,
                None => Some(AccountEvent::Order {
                    order: open_order_of(d, seen.filled.get(&d.order_id).copied().unwrap_or(0.0)),
                    key: Some(d.order.order_ref.clone()).filter(|k| !k.is_empty()),
                }),
            },
            OrderUpdate::OrderStatus(st) => match done_status(&st.status) {
                Some(status) => {
                    seen.filled.remove(&st.order_id);
                    seen.done.insert(st.order_id);
                    Some(AccountEvent::Done { id: st.order_id.to_string(), status, reason: None })
                }
                None if seen.done.contains(&st.order_id) => None,
                None => {
                    let known = seen.filled.entry(st.order_id).or_insert(0.0);
                    *known = known.max(st.filled);
                    Some(AccountEvent::Filled { id: st.order_id.to_string(), filled_qty: *known })
                }
            },
            // A fill moves the position, which the positions feed reports.
            _ => None,
        },
    }
}

/// IB's account summary, one line per account. Its figures are in the account's base currency
/// (the one `NetLiquidation` is stated in); the ledger's results are read as IB's `BASE` line,
/// else in that currency, never in another one.
fn account_summaries(figures: &[account::Figure]) -> Vec<super::AccountSummary> {
    let mut out: Vec<super::AccountSummary> = Vec::new();
    // The ledger's results wait for the base currency, which may come after them.
    let mut ledger: Vec<(usize, &account::Figure, f64)> = Vec::new();
    for f in figures {
        let i = match out.iter().position(|a| a.account == f.account) {
            Some(i) => i,
            None => {
                out.push(super::AccountSummary { account: f.account.clone(), ..Default::default() });
                out.len() - 1
            }
        };
        let a = &mut out[i];
        let Ok(v) = f.value.trim().parse::<f64>() else { continue };
        if f.tag == "NetLiquidation" && !f.currency.is_empty() && f.currency != "BASE" {
            a.currency = f.currency.clone();
        }
        // `ibapi` names a ledger figure `$LEDGER-<tag>`.
        let tag = f.tag.strip_prefix("$LEDGER-").unwrap_or(&f.tag);
        let slot = match tag {
            "NetLiquidation" => &mut a.equity,
            "TotalCashValue" => &mut a.cash,
            "InitMarginReq" => &mut a.margin_used,
            "MaintMarginReq" => &mut a.maintenance_margin,
            "AvailableFunds" => &mut a.available,
            "ExcessLiquidity" => &mut a.excess_liquidity,
            "BuyingPower" => &mut a.buying_power,
            "GrossPositionValue" => &mut a.gross_position_value,
            "UnrealizedPnL" | "RealizedPnL" => {
                ledger.push((i, f, v));
                continue;
            }
            _ => continue,
        };
        *slot = Some(v);
    }
    // The ledger may state a result per currency as well as in `BASE`: the base one wins, a
    // line in the base currency stands in, any other currency is not the account's result.
    for (i, f, v) in ledger {
        let a = &mut out[i];
        let base = f.currency == "BASE";
        if !base && (a.currency.is_empty() || f.currency != a.currency) {
            continue;
        }
        let slot = if f.tag.ends_with("UnrealizedPnL") { &mut a.unrealized } else { &mut a.realized };
        if slot.is_none() || base {
            *slot = Some(v);
        }
    }
    out.sort_by(|a, b| a.account.cmp(&b.account));
    out
}

#[async_trait::async_trait]
impl Broker for IbkrGatewayBroker {
    fn capability(&self) -> &'static BrokerCapability {
        self.cap
    }

    fn order_caps(&self) -> Option<otw_connector_model::OrderCaps> {
        Some(self.gateway.capabilities().orders.clone())
    }

    async fn is_live(&self, settings: &HashMap<String, String>) -> Result<bool> {
        self.gateway.is_live(settings).await.map_err(err)
    }

    async fn test(&self, _client: &reqwest::Client, settings: &HashMap<String, String>) -> Result<String> {
        self.gateway.test(settings).await.map_err(err)
    }

    async fn symbols(&self, _client: &reqwest::Client, settings: &HashMap<String, String>) -> Result<Vec<String>> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for s in self.read_positions(settings).await?.into_iter().map(|p| p.symbol).chain(self.read_orders(settings).await?.into_iter().map(|o| o.symbol)) {
            if seen.insert(s.clone()) {
                out.push(s);
            }
        }
        out.sort();
        Ok(out)
    }

    /// The gateway serves the last seven days only: enough for a chart's markers, not for
    /// an import, which reads the Flex statement.
    fn chart_fills(&self) -> bool {
        true
    }

    /// The fills IB still serves (seven days), cut to the window, oldest first.
    async fn executions(&self, _client: &reqwest::Client, settings: &HashMap<String, String>, q: &super::ExecQuery) -> Result<Vec<super::Execution>> {
        let s = session::get(settings).await.map_err(err)?;
        let mut out: Vec<super::Execution> = account::fills(&s)
            .await
            .map_err(err)?
            .into_iter()
            .filter_map(|f| {
                let at = f.at.to_offset_datetime()?;
                if at < q.from || at > q.to {
                    return None;
                }
                Some(super::Execution {
                    id: f.id,
                    order_id: if f.order_id > 0 { f.order_id.to_string() } else { String::new() },
                    at,
                    symbol: ticker_of(&f.contract),
                    side: if f.qty.is_sign_negative() { "sell".into() } else { "buy".into() },
                    qty: f64::try_from(f.qty.abs()).ok()?,
                    price: f.price,
                    fee: f.fee.and_then(|v| f64::try_from(v).ok()).unwrap_or(0.0),
                    fee_currency: f.fee_currency,
                    currency: f.contract.currency.0.clone(),
                    asset_class: class_of(&f.contract.security_type).into(),
                    multiplier: multiplier(&f.contract),
                    venue: f.contract.exchange.0.clone(),
                    account: f.account,
                })
            })
            .collect();
        out.sort_by_key(|e| e.at);
        Ok(out)
    }

    async fn positions(&self, _client: &reqwest::Client, settings: &HashMap<String, String>) -> Result<Vec<Position>> {
        self.read_positions(settings).await
    }

    async fn orders(&self, _client: &reqwest::Client, settings: &HashMap<String, String>) -> Result<Vec<OpenOrder>> {
        self.read_orders(settings).await
    }

    async fn place(&self, settings: &HashMap<String, String>, ticket: &OrderTicket) -> Result<Vec<Placed>> {
        let id = self.gateway.instrument_for(&ticket.ticker, crate::histdata::native::market(&ticket.asset_type)).map_err(err)?;
        let requests: Vec<OrderRequest> = ticket.legs.iter().map(|l| l.request(&id)).collect::<Result<_>>()?;
        let now = Timestamp::from_offset_datetime(time::OffsetDateTime::now_utc()).ok_or_else(|| anyhow!("the clock is out of range"))?;
        let (acks, tick) = self.gateway.place_orders(settings, &requests, now).await.map_err(place_err)?;
        Ok(acks.into_iter().map(|a| Placed { key: a.client_order_id, id: a.order_id, tick }).collect())
    }

    async fn dry_run(&self, settings: &HashMap<String, String>, ticket: &OrderTicket) -> Result<String> {
        let id = self.gateway.instrument_for(&ticket.ticker, crate::histdata::native::market(&ticket.asset_type)).map_err(err)?;
        let requests: Vec<OrderRequest> = ticket.legs.iter().map(|l| l.request(&id)).collect::<Result<_>>()?;
        let now = Timestamp::from_offset_datetime(time::OffsetDateTime::now_utc()).ok_or_else(|| anyhow!("the clock is out of range"))?;
        self.gateway.dry_run_orders(settings, &requests, now).await.map_err(err)?;
        let entries = requests.iter().filter(|o| o.parent.is_none()).count();
        Ok(format!("Interactive Brokers answered the what-if for {entries} order(s) without objection; nothing was placed"))
    }

    async fn modify(&self, settings: &HashMap<String, String>, id: &str, change: &OrderAmend) -> Result<Option<Placed>> {
        let now = Timestamp::from_offset_datetime(time::OffsetDateTime::now_utc()).ok_or_else(|| anyhow!("the clock is out of range"))?;
        self.gateway.amend_order(settings, id, *change, now).await.map_err(err)?;
        Ok(None)
    }

    async fn cancel(&self, settings: &HashMap<String, String>, id: &str) -> Result<()> {
        self.gateway.cancel_order(settings, id).await.map_err(err)
    }

    /// By `orderRef` among every order IB lists today (working, manual ones included, and
    /// completed) and the day's executions: the client id is unique per account, so the
    /// instrument needs no asking.
    async fn lookup(&self, settings: &HashMap<String, String>, _ticker: &str, _asset_type: &str, key: &str) -> Result<Lookup> {
        Ok(lookup_of(self.gateway.lookup_order(settings, key).await.map_err(err)?))
    }

    async fn symbol_for(&self, settings: &HashMap<String, String>, ticker: &str, asset_type: &str) -> Result<Option<String>> {
        let id = self.gateway.instrument_for(ticker, crate::histdata::native::market(asset_type)).map_err(err)?;
        let s = session::get(settings).await.map_err(err)?;
        let c = resolve::contract_for(&s, &id.native, mapper::asset_type(&id)).await.map_err(err)?;
        // The chart just opened this instrument on the account: have its margin at hand
        // before the first ticket.
        margin::warm(s.clone(), c.clone());
        Ok(Some(ticker_of(&c)))
    }

    async fn close_position(&self, settings: &HashMap<String, String>, native_id: &str, account: &str, key: &str) -> Result<Placed> {
        let conid: i32 = native_id.trim().parse().map_err(|_| anyhow!("{native_id:?} is not an Interactive Brokers contract id"))?;
        let now = Timestamp::from_offset_datetime(time::OffsetDateTime::now_utc()).ok_or_else(|| anyhow!("the clock is out of range"))?;
        let ack = self.gateway.close_position(settings, conid, account, key, now).await.map_err(place_err)?;
        Ok(Placed { key: ack.client_order_id, id: ack.order_id, tick: None })
    }

    async fn marks(&self, settings: &HashMap<String, String>, native_ids: &[String]) -> Result<HashMap<String, f64>> {
        let ids: Vec<i32> = native_ids.iter().filter_map(|n| n.trim().parse().ok()).collect();
        let marks = self.gateway.marks(settings, &ids).await.map_err(err)?;
        Ok(marks.into_iter().map(|(id, p)| (id.to_string(), p)).collect())
    }

    async fn account(&self, settings: &HashMap<String, String>, _symbol: Option<&str>) -> Result<Vec<super::AccountSummary>> {
        let figures = self.gateway.account_summary(settings).await.map_err(err)?;
        Ok(account_summaries(&figures))
    }

    async fn events(&self, settings: &HashMap<String, String>) -> Result<Option<futures::stream::BoxStream<'static, AccountEvent>>> {
        let rx = self.gateway.follow(settings).await.map_err(err)?;
        let stream = futures::stream::unfold((rx, Followed::default()), |(mut rx, mut seen)| async move {
            loop {
                let e = match rx.recv().await {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => events::Event::Resync,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                };
                if let Some(ev) = account_event(&e, &mut seen) {
                    return Some((ev, (rx, seen)));
                }
            }
        });
        Ok(Some(Box::pin(stream)))
    }

    async fn cancel_all(&self, settings: &HashMap<String, String>) -> Result<usize> {
        self.gateway.cancel_all_orders(settings).await.map_err(err)
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        self.gateway.check_settings(config).map_err(|e| anyhow!("{e}"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_account_summary_reads_the_base_currency_and_prefers_the_base_ledger_line() {
        let f = |account: &str, tag: &str, value: &str, currency: &str| account::Figure {
            account: account.into(),
            tag: tag.into(),
            value: value.into(),
            currency: currency.into(),
        };
        let rows = account_summaries(&[
            f("DU1", "NetLiquidation", "101234.5", "USD"),
            f("DU1", "AvailableFunds", "96934.5", "USD"),
            f("DU1", "InitMarginReq", "4300", "USD"),
            f("DU1", "$LEDGER-UnrealizedPnL", "12", "EUR"),
            f("DU1", "$LEDGER-UnrealizedPnL", "-40.25", "BASE"),
            f("DU1", "AccountType", "INDIVIDUAL", ""),
            f("DU2", "$LEDGER-RealizedPnL", "7", "USD"),
            f("DU2", "$LEDGER-UnrealizedPnL", "3", "USD"),
            f("DU2", "$LEDGER-UnrealizedPnL", "-8", "EUR"),
            f("DU2", "NetLiquidation", "5000", "EUR"),
        ]);
        assert_eq!(rows.len(), 2);
        let a = &rows[0];
        assert_eq!((a.account.as_str(), a.currency.as_str()), ("DU1", "USD"));
        assert_eq!((a.equity, a.available, a.margin_used, a.unrealized), (Some(101234.5), Some(96934.5), Some(4300.0), Some(-40.25)));
        assert_eq!(a.cash, None, "a figure IB did not state stays unknown");
        assert_eq!((rows[1].currency.as_str(), rows[1].equity), ("EUR", Some(5000.0)));
        assert_eq!(rows[1].unrealized, Some(-8.0), "without a BASE line, the base currency's line stands in");
        assert_eq!(rows[1].realized, None, "a result in another currency is not the account's");
    }

    fn contract(t: SecurityType, symbol: &str, local: &str, currency: &str) -> Contract {
        Contract { security_type: t, symbol: symbol.into(), local_symbol: local.into(), currency: currency.into(), ..Default::default() }
    }

    #[test]
    fn a_contract_is_named_the_way_its_chart_is() {
        assert_eq!(ticker_of(&contract(SecurityType::Stock, "AAPL", "AAPL", "USD")), "AAPL");
        assert_eq!(ticker_of(&contract(SecurityType::Future, "MNQ", "MNQZ6", "USD")), "MNQZ6");
        assert_eq!(ticker_of(&contract(SecurityType::ForexPair, "EUR", "EUR.USD", "USD")), "EURUSD");
        assert_eq!(class_of(&SecurityType::Future), "future");
        assert_eq!(class_of(&SecurityType::ForexPair), "forex");
    }

    #[test]
    fn an_unset_level_is_no_level() {
        assert_eq!(level(Some(0.0)), None);
        assert_eq!(level(Some(f64::MAX)), None);
        assert_eq!(level(None), None);
        assert_eq!(level(Some(19876.25)), Some(19876.25));
    }

    fn open(id: i32, status: &str, key: &str) -> events::Event {
        let mut d = OrderData { order_id: id, contract: contract(SecurityType::Future, "MNQ", "MNQZ6", "USD"), ..Default::default() };
        d.order.order_ref = key.into();
        d.order.total_quantity = 2.0;
        d.order.limit_price = Some(19876.25);
        d.order_state.status = status.into();
        events::Event::Order(std::sync::Arc::new(OrderUpdate::OpenOrder(d)))
    }

    #[test]
    fn a_read_lists_each_working_order_once_as_it_last_stood() {
        let row = |id: i32, status: &str, limit: f64| {
            let mut d = OrderData { order_id: id, contract: contract(SecurityType::Future, "MNQ", "MNQZ6", "USD"), ..Default::default() };
            d.order.limit_price = Some(limit);
            d.order_state.status = status.into();
            d
        };
        let data = vec![row(14, "Submitted", 1.0), row(15, "Submitted", 2.0), row(14, "Submitted", 3.0), row(18, "Filled", 4.0)];
        let out = working_orders(data, &HashMap::new());
        assert_eq!(out.iter().map(|o| (o.id.as_str(), o.limit_price)).collect::<Vec<_>>(), vec![("14", Some(3.0)), ("15", Some(2.0))]);
    }

    #[test]
    fn a_trailing_stop_stands_at_its_trail_stop_price_not_its_distance() {
        let mut d = OrderData { order_id: 7, contract: contract(SecurityType::Future, "MNQ", "MNQZ6", "USD"), ..Default::default() };
        d.order.order_type = "TRAIL".into();
        d.order.aux_price = Some(50.0);
        d.order.trail_stop_price = Some(26_950.25);
        let o = open_order_of(&d, 0.0);
        assert_eq!((o.stop_price, o.trail_rate), (Some(26_950.25), None));
        d.order.aux_price = None;
        d.order.trailing_percent = Some(0.5);
        assert_eq!(open_order_of(&d, 0.0).trail_rate, Some(0.5));
        d.order.order_type = "STP".into();
        d.order.aux_price = Some(26_900.0);
        assert_eq!(open_order_of(&d, 0.0).stop_price, Some(26_900.0));
    }

    fn status(id: i32, status: &str, filled: f64) -> events::Event {
        let st = ibapi::orders::OrderStatus { order_id: id, status: status.into(), filled, ..Default::default() };
        events::Event::Order(std::sync::Arc::new(OrderUpdate::OrderStatus(st)))
    }

    #[test]
    fn a_working_order_is_sent_whole_with_the_key_it_was_placed_under() {
        let mut filled = Followed::default();
        let Some(AccountEvent::Order { order, key }) = account_event(&open(56, "Submitted", "omvj4qeub1"), &mut filled) else { panic!("not an order") };
        assert_eq!(key.as_deref(), Some("omvj4qeub1"));
        assert_eq!((order.id.as_str(), order.symbol.as_str(), order.qty, order.limit_price), ("56", "MNQZ6", 2.0, Some(19876.25)));
    }

    #[test]
    fn a_partial_fill_is_kept_when_the_order_is_sent_again() {
        let mut filled = Followed::default();
        let Some(AccountEvent::Filled { id, filled_qty }) = account_event(&status(56, "Submitted", 1.0), &mut filled) else { panic!("not a fill") };
        assert_eq!((id.as_str(), filled_qty), ("56", 1.0));
        let Some(AccountEvent::Order { order, .. }) = account_event(&open(56, "Submitted", ""), &mut filled) else { panic!("not an order") };
        assert_eq!(order.filled_qty, 1.0);
    }

    #[test]
    fn an_order_that_stopped_working_is_done_in_the_book_s_words() {
        let mut filled = Followed::default();
        let Some(AccountEvent::Done { id, status: st, .. }) = account_event(&status(56, "Filled", 2.0), &mut filled) else { panic!("not done") };
        assert_eq!((id.as_str(), st.as_str()), ("56", "filled"));
        assert!(matches!(account_event(&open(57, "Cancelled", ""), &mut filled), Some(AccountEvent::Done { status, .. }) if status == "cancelled"));
        assert!(matches!(account_event(&status(58, "Inactive", 0.0), &mut filled), Some(AccountEvent::Done { status, .. }) if status == "rejected"));
    }

    #[test]
    fn a_position_from_the_feed_is_priced_per_unit_and_carries_no_mark() {
        let mut c = contract(SecurityType::Future, "MNQ", "MNQZ6", "USD");
        c.contract_id = 730283085;
        c.multiplier = "2".into();
        let p = IbPosition { account: "DU123".into(), contract: c, position: -3.0, average_cost: 39752.5 };
        let Some(AccountEvent::Position { position }) = account_event(&events::Event::Position(std::sync::Arc::new(p)), &mut Followed::default()) else { panic!("not a position") };
        assert_eq!((position.side.as_str(), position.qty, position.avg_price), ("short", 3.0, Some(19876.25)));
        assert_eq!((position.native_id.as_deref(), position.account.as_str(), position.market_price), (Some("730283085"), "DU123", None));
    }

    #[test]
    fn late_and_out_of_order_messages_never_reopen_an_order_nor_shrink_its_fill() {
        // T24: an older status after a newer one, a stale open order after the order ended.
        let mut seen = Followed::default();
        assert!(matches!(account_event(&status(60, "Submitted", 2.0), &mut seen), Some(AccountEvent::Filled { filled_qty, .. }) if filled_qty == 2.0));
        assert!(matches!(account_event(&status(60, "Submitted", 1.0), &mut seen), Some(AccountEvent::Filled { filled_qty, .. }) if filled_qty == 2.0));
        assert!(matches!(account_event(&status(60, "Filled", 3.0), &mut seen), Some(AccountEvent::Done { status, .. }) if status == "filled"));
        assert!(account_event(&open(60, "Submitted", "k"), &mut seen).is_none(), "a filled order does not work again");
        assert!(account_event(&status(60, "PreSubmitted", 1.0), &mut seen).is_none());
        // A status before its open order: the order comes with what filled.
        assert!(account_event(&status(61, "Submitted", 1.0), &mut seen).is_some());
        let Some(AccountEvent::Order { order, .. }) = account_event(&open(61, "Submitted", "k2"), &mut seen) else { panic!("not an order") };
        assert_eq!(order.filled_qty, 1.0);
    }

    #[test]
    fn a_lookup_answers_in_the_trait_s_words() {
        use otw_connector_model::{Decimal, OrderStatus};
        let found = lookup_of(Some(orders::Seen { order_id: Some(42), status: OrderStatus::PartiallyFilled, filled: Decimal::ONE }));
        assert_eq!(found, Lookup::Found { id: "42".into(), status: OrderStatus::PartiallyFilled, filled_qty: Decimal::ONE });
        assert_eq!(lookup_of(None), Lookup::NotFound);
        // T20/T21: an answer lost reaches the API as an OrderUnknown naming the legs.
        let e = place_err(orders::PlaceError::Unknown { keys: vec!["k1".into()], message: "no answer".into(), unmapped: true });
        assert_eq!(e.downcast_ref::<OrderUnknown>().map(|u| (u.keys.clone(), u.alert)), Some((vec!["k1".to_string()], true)));
        let e = place_err(orders::PlaceError::Refused(otw_connector_model::ConnectorError::invalid("off tick")));
        assert!(e.downcast_ref::<OrderUnknown>().is_none());
    }

    #[test]
    fn settings_must_name_a_gateway() {
        let b = ibkr().unwrap();
        assert!(b.validate_config(&HashMap::new()).is_err());
        let ok: HashMap<String, String> = [("host".to_string(), "127.0.0.1".to_string()), ("port".to_string(), "4002".to_string())].into();
        assert!(b.validate_config(&ok).is_ok());
    }
}
