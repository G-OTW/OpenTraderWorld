//! Upcoming events per watchlist symbol: next earnings, ex-dividend and split dates.
//!
//! The default quote sources serve no event calendar (the keyless Yahoo chart endpoint has
//! none, CoinGecko has none), so events come only from what Fundamentals has already
//! stored: the per-ticker `earnings` and `dividends` snapshots and the market-wide
//! `calendar`. Read only, nothing is fetched here. A symbol with nothing stored says "no
//! event data for this source" instead of an empty strip that would read as "nothing due".

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Event {
    pub item_id: Uuid,
    pub symbol: String,
    /// "earnings" | "ex_dividend" | "split".
    pub kind: &'static str,
    /// ISO date (YYYY-MM-DD).
    pub date: String,
    /// "BMO" / "AMC" for earnings, the ratio for a split, the amount for a dividend.
    pub detail: String,
    /// The Fundamentals provider the date came from.
    pub provider: String,
}

/// Per-item status: "events" (at least one upcoming), "none_upcoming" (data stored, nothing
/// ahead) or "no_data" (nothing stored for this symbol, or a crypto item).
pub type Status = HashMap<Uuid, &'static str>;

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Build the upcoming events of a list. `rows` = (item id, symbol, Fundamentals subject;
/// empty subject = no equity calendar for it), `snaps` = (subject, dataset, provider, data)
/// as stored, `today` = ISO date. Events are sorted by date then symbol.
pub fn build(
    rows: &[(Uuid, String, String)],
    snaps: &[(String, String, String, Value)],
    today: &str,
) -> (Vec<Event>, Status) {
    let mut events: Vec<Event> = Vec::new();
    let mut status: Status = HashMap::new();
    let mut seen: HashSet<(Uuid, &'static str, String)> = HashSet::new();
    let calendar = snaps.iter().find(|s| s.0 == "_" && s.1 == "calendar");

    for (id, symbol, subject) in rows {
        let mut has_data = false;
        let mut push = |kind: &'static str, date: String, detail: String, provider: &str, events: &mut Vec<Event>| {
            if date.len() < 10 || date.as_str() < today {
                return;
            }
            let date = date[..10].to_string();
            if seen.insert((*id, kind, date.clone())) {
                events.push(Event {
                    item_id: *id,
                    symbol: symbol.clone(),
                    kind,
                    date,
                    detail,
                    provider: provider.to_string(),
                });
            }
        };
        if subject.is_empty() {
            status.insert(*id, "no_data");
            continue;
        }
        for (subj, dataset, provider, data) in snaps {
            if subj != subject {
                continue;
            }
            has_data = true;
            match dataset.as_str() {
                "earnings" => {
                    if let Some(next) = data.get("next").filter(|n| !n.is_null()) {
                        push("earnings", text(next.get("date")), text(next.get("time")), provider, &mut events);
                    }
                }
                "dividends" => {
                    for d in data.get("dividends").and_then(Value::as_array).into_iter().flatten() {
                        push("ex_dividend", text(d.get("ex_date")), text(d.get("amount")), provider, &mut events);
                    }
                    for s in data.get("splits").and_then(Value::as_array).into_iter().flatten() {
                        push("split", text(s.get("date")), text(s.get("ratio")), provider, &mut events);
                    }
                }
                _ => {}
            }
        }
        if let Some((_, _, provider, data)) = calendar {
            for e in data.get("earnings").and_then(Value::as_array).into_iter().flatten() {
                if text(e.get("ticker")).eq_ignore_ascii_case(subject) {
                    has_data = true;
                    push("earnings", text(e.get("date")), text(e.get("time")), provider, &mut events);
                }
            }
            for c in data.get("corporate").and_then(Value::as_array).into_iter().flatten() {
                let is_split = text(c.get("type")).to_lowercase().contains("split");
                if is_split && text(c.get("ticker")).eq_ignore_ascii_case(subject) {
                    has_data = true;
                    push("split", text(c.get("date")), text(c.get("detail")), provider, &mut events);
                }
            }
        }
        let any = events.iter().any(|e| e.item_id == *id);
        status.insert(*id, if any { "events" } else if has_data { "none_upcoming" } else { "no_data" });
    }
    events.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.symbol.cmp(&b.symbol)));
    (events, status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn upcoming_events_are_sorted_by_date_and_past_ones_dropped() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let rows = vec![(a, "AAPL".to_string(), "AAPL".to_string()), (b, "MSFT".to_string(), "MSFT".to_string())];
        let snaps = vec![
            ("AAPL".into(), "earnings".into(), "fmp".into(), json!({ "next": { "date": "2026-11-20", "time": "AMC" } })),
            (
                "AAPL".into(),
                "dividends".into(),
                "eodhd".into(),
                json!({ "dividends": [{ "ex_date": "2026-08-10", "amount": 0.26 }, { "ex_date": "2026-11-09", "amount": 0.26 }], "splits": [] }),
            ),
            ("_".into(), "calendar".into(), "finnhub".into(), json!({ "earnings": [{ "ticker": "MSFT", "date": "2026-11-12", "time": "BMO" }] })),
        ];
        let (ev, st) = build(&rows, &snaps, "2026-11-06");
        let order: Vec<(&str, &str)> = ev.iter().map(|e| (e.symbol.as_str(), e.date.as_str())).collect();
        assert_eq!(order, vec![("AAPL", "2026-11-09"), ("MSFT", "2026-11-12"), ("AAPL", "2026-11-20")]);
        assert_eq!(ev[0].kind, "ex_dividend");
        assert_eq!(st[&a], "events");
        assert_eq!(st[&b], "events");
    }

    #[test]
    fn nothing_stored_says_no_data_not_none_upcoming() {
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let rows = vec![
            (a, "NVDA".to_string(), "NVDA".to_string()),
            (b, "BTC".to_string(), String::new()),
            (c, "AAPL".to_string(), "AAPL".to_string()),
        ];
        let snaps = vec![(
            "AAPL".into(),
            "earnings".into(),
            "fmp".into(),
            json!({ "next": { "date": "2026-01-01" } }),
        )];
        let (ev, st) = build(&rows, &snaps, "2026-11-06");
        assert!(ev.is_empty());
        assert_eq!(st[&a], "no_data");
        assert_eq!(st[&b], "no_data");
        assert_eq!(st[&c], "none_upcoming");
    }

    #[test]
    fn the_same_date_from_two_snapshots_is_listed_once() {
        let a = Uuid::new_v4();
        let rows = vec![(a, "AAPL".to_string(), "AAPL".to_string())];
        let snaps = vec![
            ("AAPL".into(), "earnings".into(), "fmp".into(), json!({ "next": { "date": "2026-11-20" } })),
            ("_".into(), "calendar".into(), "finnhub".into(), json!({ "earnings": [{ "ticker": "AAPL", "date": "2026-11-20" }] })),
        ];
        assert_eq!(build(&rows, &snaps, "2026-11-06").0.len(), 1);
    }
}
