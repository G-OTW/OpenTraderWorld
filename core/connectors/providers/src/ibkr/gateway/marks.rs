//! Live marks for the contracts an account holds: one `reqMktData` line per contract, opened
//! the first time a position is asked about and closed once nobody has asked for [`IDLE`].
//!
//! What a position is valued at between two account reads: IB's portfolio stream refreshes
//! its market price every few minutes only. A mark is the contract's last trade, or the
//! bid/ask midpoint where the market has no tape (forex), in the contract's own currency, so
//! the result is computed the way the chart computes it. Not `reqPnLSingle`: IB does not say
//! which currency that one answers in. Delayed ticks are not a mark and are ignored; a line IB
//! refuses (354, no subscription) leaves the position on the broker's own figures and is not
//! asked again before [`RETRY`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ibapi::contracts::tick_types::TickType;
use ibapi::contracts::{Contract, SecurityType};
use ibapi::market_data::realtime::TickTypes;
use tokio::sync::{watch, Mutex, OnceCell};

use super::session::Session;

/// A line nobody asked about for this long is closed: the page that wanted it is gone.
const IDLE: Duration = Duration::from_secs(30);
/// How long a line that ended (refused, or its connection dropped) waits before it is opened
/// again.
const RETRY: Duration = Duration::from_secs(60);

struct Line {
    mark: watch::Receiver<Option<f64>>,
    asked: Instant,
    opened: Instant,
    task: tokio::task::JoinHandle<()>,
}

/// Open lines by `(gateway address, contract id)`.
static LINES: OnceCell<Mutex<HashMap<(String, i32), Line>>> = OnceCell::const_new();
/// Contracts by `(gateway address, contract id)`: a definition does not change.
static CONTRACTS: OnceCell<Mutex<HashMap<(String, i32), Contract>>> = OnceCell::const_new();

async fn lines() -> &'static Mutex<HashMap<(String, i32), Line>> {
    LINES.get_or_init(|| async { Mutex::new(HashMap::new()) }).await
}

async fn contracts() -> &'static Mutex<HashMap<(String, i32), Contract>> {
    CONTRACTS.get_or_init(|| async { Mutex::new(HashMap::new()) }).await
}

/// The latest mark of each contract that has one, opening a line for those that have none.
/// A contract missing from the answer has no live mark yet, or none to be had.
pub async fn marks(s: &Arc<Session>, contract_ids: &[i32]) -> HashMap<i32, f64> {
    let now = Instant::now();
    let mut lines = lines().await.lock().await;
    lines.retain(|_, l| {
        let keep = now.duration_since(l.asked) < IDLE;
        if !keep {
            l.task.abort();
        }
        keep
    });
    let mut out = HashMap::new();
    for &id in contract_ids {
        let key = (s.addr().to_string(), id);
        if lines.get(&key).is_some_and(|l| l.task.is_finished() && now.duration_since(l.opened) >= RETRY) {
            lines.remove(&key);
        }
        let line = lines.entry(key).or_insert_with(|| open(s.clone(), id, now));
        line.asked = now;
        // A line that ended holds the last price it saw, which is no longer live.
        if line.task.is_finished() {
            continue;
        }
        if let Some(p) = *line.mark.borrow() {
            out.insert(id, p);
        }
    }
    out
}

fn open(s: Arc<Session>, contract_id: i32, now: Instant) -> Line {
    let (tx, mark) = watch::channel(None);
    let task = tokio::spawn(async move {
        if let Err(e) = follow(&s, contract_id, &tx).await {
            tracing::warn!("ibkr: no live mark for contract {contract_id}: {e}");
        }
    });
    Line { mark, asked: now, opened: now, task }
}

async fn follow(s: &Session, contract_id: i32, tx: &watch::Sender<Option<f64>>) -> Result<(), String> {
    let c = contract_of(s, contract_id).await?;
    let (client, _) = s.client().await.map_err(|e| e.to_string())?;
    let mut sub = client.market_data(&c).subscribe().await.map_err(|e| e.to_string())?;
    let (mut last, mut bid, mut ask) = (None::<f64>, None::<f64>, None::<f64>);
    while let Some(item) = sub.next().await {
        let (kind, price) = match item.map_err(|e| e.to_string())? {
            TickTypes::Price(p) => (p.tick_type, p.price),
            TickTypes::PriceSize(p) => (p.price_tick_type, p.price),
            _ => continue,
        };
        // IB sends -1 (or 0) for a side with nothing on it.
        let price = (price > 0.0 && price.is_finite()).then_some(price);
        match kind {
            TickType::Last => last = price.or(last),
            TickType::Bid => bid = price,
            TickType::Ask => ask = price,
            _ => continue,
        }
        tx.send_replace(mark(last, bid, ask));
    }
    Ok(())
}

/// The last trade, else the midpoint of a two-sided market, taken in decimals: the prices IB
/// wrote, not their binary approximations.
fn mark(last: Option<f64>, bid: Option<f64>, ask: Option<f64>) -> Option<f64> {
    last.or_else(|| {
        let mid = (super::mapper::dec(bid?).ok()? + super::mapper::dec(ask?).ok()?) / otw_connector_model::Decimal::TWO;
        mid.to_string().parse::<f64>().ok()
    })
}

/// The contract a position names by id. The id alone is the question: no security type,
/// exchange or currency, which `Contract::default()` would otherwise fill as a stock on SMART
/// in USD.
async fn contract_of(s: &Session, contract_id: i32) -> Result<Contract, String> {
    let key = (s.addr().to_string(), contract_id);
    if let Some(hit) = contracts().await.lock().await.get(&key) {
        return Ok(hit.clone());
    }
    let partial = Contract {
        contract_id,
        security_type: SecurityType::Other(String::new()),
        exchange: "".into(),
        currency: "".into(),
        ..Default::default()
    };
    let details = s
        .ask("contract by id", move |client| {
            let partial = partial.clone();
            async move { client.contract_details(&partial).await }
        })
        .await
        .map_err(|e| e.to_string())?;
    let c = details
        .into_iter()
        .map(|d| d.contract)
        .find(|c| c.contract_id == contract_id)
        .ok_or_else(|| format!("Interactive Brokers has no contract {contract_id}"))?;
    contracts().await.lock().await.insert(key, c.clone());
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_is_the_last_trade_else_the_midpoint() {
        assert_eq!(mark(Some(101.0), Some(100.0), Some(100.5)), Some(101.0));
        assert_eq!(mark(None, Some(1.1), Some(1.2)), Some(1.15));
        assert_eq!(mark(None, Some(1.1), None), None);
        assert_eq!(mark(None, None, None), None);
    }
}
