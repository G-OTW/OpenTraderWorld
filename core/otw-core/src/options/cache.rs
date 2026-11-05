//! Chain snapshots kept 30 seconds in memory, so a column, filter or rate change costs no
//! provider request. Keyed by what was fetched (connector, underlying and its asset type,
//! expiries, strike window), never by how it is read.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use time::Date;
use uuid::Uuid;

use super::chain::Snapshot;

const TTL: Duration = Duration::from_secs(30);

fn map() -> &'static Mutex<HashMap<String, (Instant, Arc<Snapshot>)>> {
    static C: OnceLock<Mutex<HashMap<String, (Instant, Arc<Snapshot>)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The same chain asked twice gives the same key, whatever order the expiries came in. A
/// window left to the spot (`None`) is its own key.
pub fn key(
    connector: Uuid,
    underlying: &str,
    asset_type: &str,
    expiries: &[Date],
    lo: Option<f64>,
    hi: Option<f64>,
) -> String {
    let mut e = expiries.to_vec();
    e.sort();
    e.dedup();
    let days: Vec<String> = e.iter().map(Date::to_string).collect();
    format!("{connector}|{}|{asset_type}|{}|{lo:?}|{hi:?}", underlying.to_ascii_uppercase(), days.join(","))
}

pub fn get(key: &str) -> Option<Arc<Snapshot>> {
    let m = map().lock().ok()?;
    let (at, v) = m.get(key)?;
    (at.elapsed() < TTL).then(|| v.clone())
}

pub fn put(key: String, v: Arc<Snapshot>) {
    if let Ok(mut m) = map().lock() {
        m.retain(|_, (at, _)| at.elapsed() < TTL);
        m.insert(key, (Instant::now(), v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn the_key_ignores_expiry_order_and_ticker_case() {
        let id = Uuid::nil();
        let a = key(id, "spy", "etf", &[date!(2026 - 12 - 18), date!(2026 - 11 - 20)], Some(500.0), Some(600.0));
        let b = key(id, "SPY", "etf", &[date!(2026 - 11 - 20), date!(2026 - 12 - 18), date!(2026 - 11 - 20)], Some(500.0), Some(600.0));
        assert_eq!(a, b);
        assert_ne!(a, key(id, "SPY", "etf", &[date!(2026 - 11 - 20)], Some(500.0), Some(600.0)));
        assert_ne!(a, key(id, "SPY", "etf", &[date!(2026 - 11 - 20), date!(2026 - 12 - 18)], Some(500.0), Some(610.0)));
        assert_ne!(a, key(id, "SPY", "etf", &[date!(2026 - 11 - 20), date!(2026 - 12 - 18)], None, None));
        assert_ne!(a, key(id, "SPY", "index", &[date!(2026 - 11 - 20), date!(2026 - 12 - 18)], Some(500.0), Some(600.0)));
    }

    #[test]
    fn a_stored_snapshot_is_served_back() {
        let k = key(Uuid::new_v4(), "QQQ", "etf", &[date!(2026 - 11 - 20)], None, None);
        assert!(get(&k).is_none());
        put(k.clone(), Arc::new(Snapshot::default()));
        assert!(get(&k).is_some());
    }
}
