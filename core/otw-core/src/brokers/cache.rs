//! The last pulls from each account, kept a few minutes.
//!
//! A preview and the import after it ask for the same window. Answering the second from
//! the first means the import writes exactly what the preview showed, and a pull the user
//! left running in the background is not paid a second time when they come back to it.
//! A scheduled sync asks for a window ending at the second it runs, which no earlier pull
//! matches, so it always reads the venue.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use uuid::Uuid;

use super::estimate::{self, Kind};
use super::{Broker, ExecQuery, Execution, Holding};

/// Long enough to read a preview and decide, short enough that a new fill shows up.
const EXEC_TTL: Duration = Duration::from_secs(10 * 60);
/// A balance sheet goes stale faster: the user may be trading in another tab.
const HOLDINGS_TTL: Duration = Duration::from_secs(5 * 60);

type Slot<T> = Mutex<HashMap<String, (Instant, Vec<T>)>>;

fn execs() -> &'static Slot<Execution> {
    static S: OnceLock<Slot<Execution>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn holdings_slot() -> &'static Slot<Holding> {
    static S: OnceLock<Slot<Holding>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn exec_key(account: Uuid, q: &ExecQuery) -> String {
    let mut symbols: Vec<String> = q.symbols.iter().map(|s| s.trim().to_uppercase()).collect();
    symbols.sort();
    symbols.dedup();
    format!(
        "{account}|{}|{}|{}",
        q.from.unix_timestamp(),
        q.to.unix_timestamp(),
        symbols.join(",")
    )
}

fn get<T: Clone>(slot: &Slot<T>, key: &str, ttl: Duration) -> Option<Vec<T>> {
    let mut s = slot.lock().ok()?;
    s.retain(|_, (at, _)| at.elapsed() < ttl);
    s.get(key).map(|(_, v)| v.clone())
}

fn put<T>(slot: &Slot<T>, key: String, value: Vec<T>) {
    if let Ok(mut s) = slot.lock() {
        s.insert(key, (Instant::now(), value));
    }
}

/// Whether this exact window is already in hand, so the pull costs nothing.
pub fn has_executions(account: Uuid, q: &ExecQuery) -> bool {
    get(execs(), &exec_key(account, q), EXEC_TTL).is_some()
}

pub fn has_holdings(account: Uuid) -> bool {
    get(holdings_slot(), &account.to_string(), HOLDINGS_TTL).is_some()
}

/// Fills over the window, from the last identical pull when it is recent.
pub async fn executions(
    account: Uuid,
    connector: &dyn Broker,
    settings: &HashMap<String, String>,
    q: &ExecQuery,
) -> Result<Vec<Execution>> {
    let key = exec_key(account, q);
    if let Some(v) = get(execs(), &key, EXEC_TTL) {
        return Ok(v);
    }
    let broker = connector.capability().broker;
    let model = estimate::model_executions(broker, q.from, q.to, q.symbols.len());
    let started = Instant::now();
    let http = super::client()?;
    let v = connector.executions(&http, settings, q).await?;
    estimate::learn(broker, Kind::Executions, model, started.elapsed());
    put(execs(), key, v.clone());
    Ok(v)
}

/// What the account owns, from the last reading when it is recent.
pub async fn holdings(
    account: Uuid,
    connector: &dyn Broker,
    settings: &HashMap<String, String>,
) -> Result<Vec<Holding>> {
    let key = account.to_string();
    if let Some(v) = get(holdings_slot(), &key, HOLDINGS_TTL) {
        return Ok(v);
    }
    let broker = connector.capability().broker;
    let started = Instant::now();
    let http = super::client()?;
    let v = connector.holdings(&http, settings).await?;
    estimate::learn(broker, Kind::Holdings, estimate::model_holdings(broker), started.elapsed());
    put(holdings_slot(), key, v.clone());
    Ok(v)
}
