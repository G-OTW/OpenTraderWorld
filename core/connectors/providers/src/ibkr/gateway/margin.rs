//! Margin known before an order goes out, without making the order wait for it.
//!
//! IB's what-if order (`whatIf`) answers the margin an order would take and the account's
//! equity, and places nothing. Asked for one unit when a chart opens the instrument
//! ([`warm`]), then again in the background every few minutes while the instrument is in use,
//! it gives a margin per unit and the funds available. An order whose rough need fits passes
//! with no extra call. One that does not is asked of IB exactly (what-if on the order itself,
//! which counts the positions it offsets) and refused only when IB's own figures say the
//! account cannot carry it. Whatever slips through is still IB's to refuse, and its reason
//! reaches the account feed ([`super::orders::rejection`]).
//!
//! The same what-if is the dry run before a live account's first order ([`dry_run`]).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use ibapi::contracts::{Contract, SecurityType};
use ibapi::orders::{Action, Order, OrderState, PlaceOrder};
use otw_connector_model::{ConnectorError, Decimal, ErrorKind, OrderRequest};

use super::orders::{ib_order, ORDER_WAIT};
use super::session::Session;

/// How often a margin in use is asked again.
const REFRESH: Duration = Duration::from_secs(300);
/// A margin nobody asked for this long stops being refreshed and is forgotten.
const IDLE: Duration = Duration::from_secs(1800);

/// What one unit of a contract takes, as IB said last.
#[derive(Debug, Clone, Copy)]
pub struct PerUnit {
    /// Initial margin one unit adds.
    pub margin: Decimal,
    /// Equity with loan minus the initial margin already used.
    pub available: Decimal,
}

struct Entry {
    last: Option<PerUnit>,
    used: Instant,
}

/// By gateway address and contract id.
static CACHE: LazyLock<Mutex<HashMap<(String, i32), Entry>>> = LazyLock::new(Default::default);

/// IB writes an unset figure as `f64::MAX`.
fn figure(v: Option<f64>) -> Option<Decimal> {
    v.filter(|x| x.is_finite() && x.abs() < 1e300).and_then(|x| super::mapper::dec(x).ok())
}

/// IB's what-if answer for `order` on `c`: nothing is placed.
async fn what_if(s: &Session, c: &Contract, mut order: Order) -> Result<OrderState, ConnectorError> {
    order.what_if = true;
    order.transmit = true;
    order.order_ref = String::new();
    let (client, _) = s.client().await?;
    let id = client.next_order_id();
    let mut sub = client
        .place_order(id, c, &order)
        .await
        .map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("margin what-if: {e}")))?;
    let wait = async {
        while let Some(item) = sub.next().await {
            match item {
                Ok(PlaceOrder::OpenOrder(d)) if d.order.what_if || figure(d.order_state.initial_margin_after).is_some() => return Ok(d.order_state),
                Ok(PlaceOrder::Message(n)) if !ibapi::messages::WARNING_CODE_RANGE.contains(&n.code) => {
                    return Err(ConnectorError::new(ErrorKind::InvalidRequest, format!("margin what-if: {} ({})", n.message.replace("<br>", ""), n.code)).with_code(n.code.to_string()))
                }
                Err(ibapi::Error::Message(code, msg)) if !ibapi::messages::WARNING_CODE_RANGE.contains(&code) => {
                    return Err(ConnectorError::new(ErrorKind::InvalidRequest, format!("margin what-if: {} ({code})", msg.replace("<br>", ""))).with_code(code.to_string()))
                }
                _ => {}
            }
        }
        Err(ConnectorError::new(ErrorKind::Transient, "margin what-if: the connection closed before IB answered"))
    };
    tokio::time::timeout(ORDER_WAIT, wait)
        .await
        .map_err(|_| ConnectorError::new(ErrorKind::Transient, "margin what-if: no answer in time"))?
}

/// One unit bought at the market, as a what-if.
async fn per_unit(s: &Session, c: &Contract) -> Result<PerUnit, ConnectorError> {
    let order = Order { action: Action::Buy, order_type: "MKT".into(), total_quantity: 1.0, ..Order::default() };
    let st = what_if(s, c, order).await?;
    match (figure(st.initial_margin_change), figure(st.equity_with_loan_before), figure(st.initial_margin_before)) {
        (Some(m), Some(ewl), Some(im)) => Ok(PerUnit { margin: m.max(Decimal::ZERO), available: ewl - im }),
        _ => Err(ConnectorError::new(ErrorKind::Decode, "margin what-if: IB answered no margin figures")),
    }
}

/// Start following this contract's margin on this gateway, if nobody does. Returns at once;
/// the first figure lands when IB answers.
pub fn warm(s: Arc<Session>, c: Contract) {
    if c.contract_id == 0 || c.security_type == SecurityType::ContinuousFuture {
        return;
    }
    let key = (s.addr().to_string(), c.contract_id);
    {
        let mut map = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = map.get_mut(&key) {
            e.used = Instant::now();
            return;
        }
        map.insert(key.clone(), Entry { last: None, used: Instant::now() });
    }
    tokio::spawn(async move {
        loop {
            match per_unit(&s, &c).await {
                Ok(p) => {
                    if let Some(e) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&key) {
                        e.last = Some(p);
                    }
                }
                Err(e) => tracing::warn!("ibkr: margin of contract {} at {}: {e}", c.contract_id, s.addr()),
            }
            tokio::time::sleep(REFRESH).await;
            let mut map = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if map.get(&key).is_none_or(|e| e.used.elapsed() >= IDLE) {
                map.remove(&key);
                return;
            }
        }
    });
}

/// The last figures for this contract, if any.
pub fn cached(addr: &str, contract_id: i32) -> Option<PerUnit> {
    let mut map = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let e = map.get_mut(&(addr.to_string(), contract_id))?;
    e.used = Instant::now();
    e.last
}

/// Refuse a ticket the account cannot carry, as IB's own figures say. No figure yet, or a
/// rough need that fits: passes at once. Otherwise each entry order (exits aside) is asked
/// of IB as a what-if, and refused when its initial margin after would pass the equity.
/// A what-if that fails lets the order through: IB decides.
pub async fn check(s: &Arc<Session>, c: &Contract, orders: &[OrderRequest]) -> Result<(), ConnectorError> {
    warm(s.clone(), c.clone());
    let Some(p) = cached(s.addr(), c.contract_id) else { return Ok(()) };
    let entries: Vec<&OrderRequest> = orders.iter().filter(|o| o.parent.is_none()).collect();
    let qty: Decimal = entries.iter().map(|o| o.qty.value()).sum();
    if qty * p.margin <= p.available {
        return Ok(());
    }
    for o in entries {
        let st = match ib_order(o) {
            Ok(order) => what_if(s, c, order).await,
            Err(e) => Err(e),
        };
        let st = match st {
            Ok(st) => st,
            Err(e) => {
                tracing::warn!("ibkr: margin check skipped: {e}");
                continue;
            }
        };
        if let Some(why) = short_of_margin(&st) {
            return Err(ConnectorError::new(ErrorKind::InvalidRequest, why));
        }
    }
    Ok(())
}

/// A what-if answer whose initial margin after the order would pass the equity, said with IB's
/// figures.
fn short_of_margin(st: &OrderState) -> Option<String> {
    let (after, ewl) = (figure(st.initial_margin_after)?, figure(st.equity_with_loan_after)?);
    (after > ewl).then(|| {
        let cur = if st.margin_currency.is_empty() { String::new() } else { format!(" {}", st.margin_currency) };
        format!("not enough margin: Interactive Brokers says this order takes the initial margin to {}{cur} against {}{cur} of equity with loan", after.round_dp(2), ewl.round_dp(2))
    })
}

/// The dry run before the first order of a connection signed in to a live account (spec 9,
/// E4; paper accounts skip it, [`super::orders::dry_run_due`]): each entry order (exits aside)
/// is sent to IB as a what-if first. IB refusing it, or saying the account cannot carry it,
/// refuses the ticket with IB's reason; a what-if that cannot run refuses it too, since the
/// check did not happen. Once one passed, the connection's later orders go straight out.
pub async fn dry_run(s: &Session, c: &Contract, orders: &[OrderRequest]) -> Result<(), ConnectorError> {
    if s.dry_run_passed() {
        return Ok(());
    }
    if !super::orders::dry_run_due(&super::orders::accounts(s).await?, false) {
        return Ok(());
    }
    what_if_all(s, c, orders).await?;
    s.dry_run_done();
    tracing::info!("ibkr: dry run passed at {}: live orders go out on this connection", s.addr());
    Ok(())
}

/// Each entry order (exits aside) sent to IB as a what-if: refused with IB's reason, or when
/// the account cannot carry it, or when the what-if cannot run. Nothing is placed.
pub async fn what_if_all(s: &Session, c: &Contract, orders: &[OrderRequest]) -> Result<(), ConnectorError> {
    for o in orders.iter().filter(|o| o.parent.is_none()) {
        let st = match what_if(s, c, ib_order(o)?).await {
            Ok(st) => st,
            Err(e) if e.kind == ErrorKind::InvalidRequest => {
                let why = e.message.trim_start_matches("margin what-if: ").to_string();
                let refusal = ConnectorError::new(ErrorKind::InvalidRequest, format!("dry run before the first live order: Interactive Brokers refuses it: {why}. Nothing was sent"));
                return Err(match e.provider_code {
                    Some(code) => refusal.with_code(code),
                    None => refusal,
                });
            }
            Err(e) => {
                let why = e.message.trim_start_matches("margin what-if: ").to_string();
                return Err(ConnectorError::new(e.kind, format!("dry run before the first live order could not run ({why}). Nothing was sent: try again")));
            }
        };
        if let Some(why) = short_of_margin(&st) {
            return Err(ConnectorError::new(ErrorKind::InvalidRequest, format!("dry run before the first live order: {why}. Nothing was sent")));
        }
    }
    Ok(())
}
