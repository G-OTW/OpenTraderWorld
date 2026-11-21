//! What a robot's page and the fleet read, computed here from the robot's own trades.
//!
//! Every figure counts the robot's size (`robot_qty`, see [`super::sizing`]): a trade it did
//! not take counts nothing, a scaled one its share. The marks are the last stored closes; the
//! page moves the unrealized figures on the live hub between two reads.

use std::collections::BTreeMap;

use otw_connector_model::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use time_tz::{OffsetDateTimeExt, Tz};
use uuid::Uuid;

use otw_store::histdata as hd;
use otw_store::robots as store;

use super::guards;
use crate::AppState;

fn rfc(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse(ts: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(ts, &Rfc3339).ok()
}

/// A decimal figure as the page reads it: a JSON number, `dp` decimals. The sums behind it are
/// exact; this is the display edge.
fn num(v: Decimal, dp: u32) -> Value {
    json!(v.round_dp(dp).normalize().to_f64().unwrap_or_default())
}

fn dec(v: f64) -> Decimal {
    otw_store::paper_broker::decimal_of(v).unwrap_or_default()
}

const HUNDRED: Decimal = Decimal::ONE_HUNDRED;

/// One closed robot trade, as the aggregates read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub ticker: String,
    pub direction: String,
    pub entry: Option<OffsetDateTime>,
    pub exit: Option<OffsetDateTime>,
    pub pnl: Decimal,
    pub fees: Decimal,
}

/// The stats grid over the robot's closed trades, oldest first. Every sum and ratio in
/// Decimal. Pure.
pub fn stats(rows: &[Row], start: Decimal) -> Value {
    let n = rows.len();
    let zero = Decimal::ZERO;
    let wins: Vec<Decimal> = rows.iter().map(|r| r.pnl).filter(|p| *p > zero).collect();
    let losses: Vec<Decimal> = rows.iter().map(|r| r.pnl).filter(|p| *p < zero).collect();
    let gross_win: Decimal = wins.iter().sum();
    let gross_loss: Decimal = -losses.iter().sum::<Decimal>();
    let net: Decimal = rows.iter().map(|r| r.pnl).sum();
    let fees: Decimal = rows.iter().map(|r| r.fees).sum();
    let (mut equity, mut peak, mut max_dd, mut max_dd_pct) = (start, start, zero, zero);
    for r in rows {
        equity += r.pnl;
        peak = peak.max(equity);
        max_dd = max_dd.max(peak - equity);
        if peak > zero {
            max_dd_pct = max_dd_pct.max((peak - equity) / peak * HUNDRED);
        }
    }
    // Streaks: the run the last trade belongs to, signed (wins up, losses down), and the
    // longest of each.
    let (mut cur, mut best_win, mut best_loss) = (0i64, 0i64, 0i64);
    for r in rows {
        if r.pnl > zero {
            cur = if cur > 0 { cur + 1 } else { 1 };
        } else if r.pnl < zero {
            cur = if cur < 0 { cur - 1 } else { -1 };
        } else {
            cur = 0;
        }
        best_win = best_win.max(cur);
        best_loss = best_loss.max(-cur);
    }
    let holds: Vec<i64> = rows.iter().filter_map(|r| Some((r.exit? - r.entry?).whole_seconds())).collect();
    let avg_hold = (!holds.is_empty()).then(|| holds.iter().sum::<i64>() / holds.len() as i64);
    let count = |k: usize| Decimal::from(k as u64);
    json!({
        "trades": n,
        "wins": wins.len(),
        "losses": losses.len(),
        "net_pnl": num(net, 8),
        "net_pct": (start > zero).then(|| num(net / start * HUNDRED, 4)),
        "win_rate": (n > 0).then(|| num(count(wins.len()) * HUNDRED / count(n), 4)),
        "profit_factor": (gross_loss > zero).then(|| num(gross_win / gross_loss, 4)),
        "expectancy": (n > 0).then(|| num(net / count(n), 8)),
        "avg_win": (!wins.is_empty()).then(|| num(gross_win / count(wins.len()), 8)),
        "avg_loss": (!losses.is_empty()).then(|| num(-gross_loss / count(losses.len()), 8)),
        "max_drawdown": num(max_dd, 8),
        "max_drawdown_pct": num(max_dd_pct, 4),
        "avg_hold_secs": avg_hold,
        "best": rows.iter().map(|r| r.pnl).max().map(|v| num(v, 8)),
        "worst": rows.iter().map(|r| r.pnl).min().map(|v| num(v, 8)),
        "current_streak": cur,
        "longest_win_streak": best_win,
        "longest_loss_streak": best_loss,
        "fees": num(fees, 8),
    })
}

/// The equity curve: the start, then one point per closed trade. Pure.
pub fn curve(rows: &[Row], start: Decimal, from: OffsetDateTime) -> Vec<Value> {
    let mut equity = start;
    let mut out = vec![json!({ "at": rfc(from), "equity": num(start, 8) })];
    for r in rows {
        equity += r.pnl;
        out.push(json!({ "at": rfc(r.exit.unwrap_or(from)), "equity": num(equity, 8) }));
    }
    out
}

/// Realized P&L per day of the robot's timezone, for the calendar, in the shape the journal's
/// calendar reads (`date`, `net_pnl`, `trades`, `wins`, `losses`). Pure.
pub fn calendar(rows: &[Row], tz: Option<&Tz>) -> Vec<Value> {
    let mut days: BTreeMap<String, (Decimal, i64, i64, i64)> = BTreeMap::new();
    for r in rows {
        let Some(at) = r.exit else { continue };
        let at = match tz {
            Some(tz) => at.to_timezone(tz),
            None => at,
        };
        let e = days.entry(at.date().to_string()).or_default();
        e.0 += r.pnl;
        e.1 += 1;
        if r.pnl > Decimal::ZERO {
            e.2 += 1;
        } else if r.pnl < Decimal::ZERO {
            e.3 += 1;
        }
    }
    days.into_iter()
        .map(|(date, (pnl, n, w, l))| json!({ "date": date, "net_pnl": num(pnl, 8), "trades": n, "wins": w, "losses": l }))
        .collect()
}

/// P&L by instrument and by direction, in the shape the journal's distribution strip reads
/// (`key`, `trades`, `net`, `avg`). Pure.
pub fn breakdown(rows: &[Row]) -> Value {
    let group = |key: &dyn Fn(&Row) -> String| -> Vec<Value> {
        let mut m: BTreeMap<String, (i64, Decimal, i64)> = BTreeMap::new();
        for r in rows {
            let e = m.entry(key(r)).or_default();
            e.0 += 1;
            e.1 += r.pnl;
            if r.pnl > Decimal::ZERO {
                e.2 += 1;
            }
        }
        m.into_iter()
            .map(|(k, (n, net, w))| json!({ "key": k, "trades": n, "net": num(net, 8), "avg": num(net / Decimal::from(n), 8), "wins": w }))
            .collect()
    };
    json!({
        "instrument": group(&|r: &Row| r.ticker.clone()),
        "direction": group(&|r: &Row| r.direction.clone()),
    })
}

/// The robot's closed trades as the aggregates read them: taken, in the order they closed.
pub fn closed_rows(trades: &[store::Trade]) -> Vec<Row> {
    let mut rows: Vec<Row> = trades
        .iter()
        .filter(|t| !t.open && t.robot_qty.is_none_or(|q| !q.is_zero()))
        .map(|t| Row {
            ticker: t.ticker.clone(),
            direction: t.direction.clone(),
            entry: parse(&t.entry_ts),
            exit: parse(&t.exit_ts).or(t.closed_at),
            pnl: t.robot_pnl(),
            fees: t.robot_fees(),
        })
        .collect();
    rows.sort_by_key(|r| r.exit);
    rows
}

/// The open trades of a broker robot's book that hold nothing at the broker: their opening
/// order never left, was refused, or did not fill, or the position was closed there outside
/// the engine (Stop and flatten, the user's own close). Shown nowhere as a position: the log
/// says why. Empty for a simulated robot. Pure.
pub fn not_at_broker(session: &store::Session, trades: &[store::Trade], orders: &[otw_store::robots_exec::Order]) -> std::collections::HashSet<String> {
    if !session.on_broker() {
        return Default::default();
    }
    trades
        .iter()
        .filter(|t| t.open && !t.trade_key.contains("|pending:"))
        .filter(|t| {
            let Some(o) = super::exec::opening_order(orders, trades, t) else { return true };
            let closed_outside = orders.iter().any(|x| x.position == o.client_id && super::exec::outside_close(x) && x.filled_qty > Decimal::ZERO);
            o.outcome != "sent" || o.filled_qty.is_zero() || closed_outside
        })
        .map(|t| t.trade_key.clone())
        .collect()
}

/// A broker robot's open trades that hold nothing at the broker ([`not_at_broker`]).
async fn hidden(state: &AppState, session: &store::Session, trades: &[store::Trade]) -> std::collections::HashSet<String> {
    if !session.on_broker() {
        return Default::default();
    }
    let orders = otw_store::robots_exec::orders(&state.pool, session.id).await.unwrap_or_default();
    not_at_broker(session, trades, &orders)
}

/// The positions the robot holds, at its own size, marked to the last stored close. A broker
/// robot's are the ones its orders filled at the broker (`hide` the others).
fn positions(session: &store::Session, trades: &[store::Trade], marks: &BTreeMap<String, (f64, String)>, hide: &std::collections::HashSet<String>) -> Vec<Value> {
    let levels = |t: &store::Trade| -> (Option<f64>, Option<f64>) {
        let Some(list) = session.stats.as_ref().and_then(|s| s.get("open_positions")).and_then(Value::as_array) else { return (None, None) };
        let p = list.iter().find(|p| {
            p.get("direction").and_then(Value::as_str) == Some(t.direction.as_str())
                && p.get("entry_ts").and_then(Value::as_str) == Some(t.entry_ts.as_str())
                && p.get("ticker").and_then(Value::as_str).is_none_or(|x| x == t.ticker)
        });
        let n = |k: &str| p.and_then(|p| p.get(k)).and_then(Value::as_f64);
        (n("stop"), n("take_profit").or_else(|| n("exit_limit")))
    };
    let mult = session.settings.pointer("/instrument/multiplier").and_then(Value::as_f64).map(dec).filter(|m| *m > Decimal::ZERO).unwrap_or(Decimal::ONE);
    let now = OffsetDateTime::now_utc();
    trades
        .iter()
        .filter(|t| t.open && !t.trade_key.contains("|pending:") && t.robot_qty.is_none_or(|q| !q.is_zero()) && !hide.contains(&t.trade_key))
        .map(|t| {
            let qty = dec(t.engine_qty()) * t.robot_ratio();
            let avg = t.trade.get("entry_price").and_then(Value::as_f64);
            let mark = marks.get(&t.ticker);
            let sign = if t.direction == "short" { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
            let unrealized = match (avg, mark) {
                (Some(a), Some((m, _))) => Some(num((dec(*m) - dec(a)) * qty * mult * sign, 8)),
                _ => None,
            };
            let (stop, target) = levels(t);
            // What the position puts to work: its size at the mark (the entry until one is read).
            let notional = mark.map(|m| m.0).or(avg).map(|px| num((qty * dec(px) * mult).abs(), 8));
            json!({
                "key": t.trade_key, "ticker": t.ticker, "direction": t.direction, "notional": notional,
                "qty": num(qty, 10), "engine_qty": t.engine_qty(), "scale": t.scale.normalize().to_string(),
                "avg_price": avg, "price": mark.map(|m| m.0), "price_at": mark.map(|m| m.1.clone()),
                "unrealized": unrealized, "stop": stop, "target": target, "entry_ts": t.entry_ts,
                "age_secs": parse(&t.entry_ts).map(|e| (now - e).whole_seconds()),
            })
        })
        .collect()
}

/// What is working: a simulated robot's engine intents (a market entry at the next open, a
/// resting limit); a broker robot's orders its last sync saw working at the venue, and only
/// those (an intent that never left is in the log, not here).
fn pending(session: &store::Session) -> Vec<Value> {
    let mut out = Vec::new();
    let engine = if session.on_broker() { None } else { session.stats.as_ref().and_then(|s| s.get("pending_orders")).and_then(Value::as_array) };
    for o in engine.into_iter().flatten() {
        out.push(json!({
            "source": "engine", "ticker": o.get("ticker"), "kind": o.get("kind"), "direction": o.get("direction"),
            "order": o.get("order"), "qty": o.get("qty"), "price": o.get("price"), "bars_left": o.get("bars_left"),
            "signal_ts": o.get("signal_ts"), "provisional": o.get("provisional"),
        }));
    }
    for i in session.exec_book.as_ref().and_then(|b| b.get("instruments")).and_then(Value::as_array).into_iter().flatten() {
        for w in i.pointer("/broker/working").and_then(Value::as_array).into_iter().flatten() {
            out.push(json!({
                "source": "broker", "ticker": i.get("ticker"), "symbol": i.get("symbol"), "role": w.get("role"),
                "side": w.get("side"), "qty": w.get("qty"), "limit": w.get("limit"), "stop": w.get("stop"),
                "client_id": w.get("client_id"), "status": "working",
            }));
        }
    }
    out
}

/// The last stored close of each instrument: `ticker -> (price, at)`.
async fn marks(state: &AppState, session: &store::Session) -> BTreeMap<String, (f64, String)> {
    let mut out = BTreeMap::new();
    for &id in &session.dataset_ids {
        let Ok(Some(ds)) = hd::get_dataset(&state.pool, id).await else { continue };
        let bars = hd::read_bars(&state.pool, id, None, None, 1).await.unwrap_or_default();
        if let Some(b) = bars.last() {
            out.insert(ds.ticker.clone(), (b.close, rfc(b.ts)));
        }
    }
    out
}

/// What a robot's orders move: `paper` (the paper broker), `testnet` (a venue's test account,
/// or a live one not opened for trading) or `live`; `simulated` with no broker at all.
pub async fn environment(state: &AppState, session: &store::Session) -> &'static str {
    let Some(a) = session.exec_account_id.filter(|_| session.on_broker()) else { return "simulated" };
    match otw_store::brokers::get(&state.pool, a).await {
        Ok(Some(row)) => match super::exec::level(&row) {
            "paper" => "paper",
            "live" => "live",
            _ => "testnet",
        },
        _ => "missing",
    }
}

/// The robot block of the dashboard: equity, stats, calendar, breakdown, positions, pending,
/// guards and today's figures.
pub async fn robot(state: &AppState, session: &store::Session, trades: &[store::Trade]) -> anyhow::Result<Value> {
    let now = OffsetDateTime::now_utc();
    let start = super::start_equity(session);
    let rows = closed_rows(trades);
    let marks = marks(state, session).await;
    let open = positions(session, trades, &marks, &hidden(state, session, trades).await);
    let unrealized: Option<Decimal> = open.iter().map(|p| p["unrealized"].as_f64().map(dec)).sum();
    let realized: Decimal = rows.iter().map(|r| r.pnl).sum();
    let mut eq = curve(&rows, start, session.seeded_at.unwrap_or(session.created_at));
    if let Some(u) = unrealized.filter(|_| !open.is_empty()) {
        eq.push(json!({ "at": rfc(now), "equity": num(start + realized + u, 8), "marked": true }));
    }
    let g = guards::State::of(&session.guard_state);
    let timeline = otw_store::robots_guards::events(&state.pool, session.id, 100).await?;
    let today = otw_store::robots_guards::day(&state.pool, session.id, now.date()).await?;
    Ok(json!({
        "environment": environment(state, session).await,
        "start_equity": num(start, 8),
        "capital": session.capital,
        "equity": num(start + realized + unrealized.unwrap_or_default(), 8),
        "realized": num(realized, 8),
        "unrealized": unrealized.map(|u| num(u, 8)),
        "curve": eq,
        "stats": stats(&rows, start),
        "calendar": calendar(&rows, crate::automator::schedule::timezone(&session.timezone)),
        "breakdown": breakdown(&rows),
        "positions": open,
        "pending": pending(session),
        "guards": {
            "rules": session.guards,
            "state": session.guard_state,
            "badge": g.badge(now),
            "holds": g.holds(now),
            "scale": guards::scale_f64(&g),
            "described": guards::validate(&session.guards).unwrap_or_default().iter().map(guards::describe).collect::<Vec<_>>(),
            "timeline": timeline,
        },
        "today": today,
        "strategy": { "id": session.strategy_id, "name": session.strategy_name, "version": session.strategy_version },
    }))
}

/// The fleet: one line per robot, what the list page shows. One query per table, then per
/// robot its open book.
pub async fn fleet(state: &AppState) -> anyhow::Result<Value> {
    let now = OffsetDateTime::now_utc();
    let sessions = store::list_sessions(&state.pool).await?;
    let today: BTreeMap<Uuid, otw_store::robots_guards::Daily> =
        otw_store::robots_guards::today_all(&state.pool, now.date()).await?.into_iter().collect();
    let unseen: BTreeMap<Uuid, i64> = store::unseen_by_session(&state.pool).await?.into_iter().collect();
    let mut accounts: BTreeMap<Uuid, (String, &'static str)> = BTreeMap::new();
    let mut out = Vec::with_capacity(sessions.len());
    for s in &sessions {
        let account = match s.exec_account_id.filter(|_| s.on_broker()) {
            Some(a) => {
                if let std::collections::btree_map::Entry::Vacant(e) = accounts.entry(a) {
                    let row = otw_store::brokers::get(&state.pool, a).await?;
                    let level = match row.as_ref().map(super::exec::level) {
                        Some("paper") => "paper",
                        Some("live") => "live",
                        Some(_) => "testnet",
                        None => "missing",
                    };
                    e.insert((row.map(|r| r.name).unwrap_or_default(), level));
                }
                accounts.get(&a).cloned()
            }
            None => None,
        };
        let trades = store::list_trades(&state.pool, s.id, false).await?;
        let rows = closed_rows(&trades);
        let start = super::start_equity(s);
        let realized: Decimal = rows.iter().map(|r| r.pnl).sum();
        // The open book marked to the last stored close, as the robot's own page marks it.
        let hide = hidden(state, s, &trades).await;
        let book = positions(s, &trades, &marks(state, s).await, &hide);
        let unrealized: Option<Decimal> = book.iter().map(|p| p["unrealized"].as_f64().map(dec)).sum();
        // Each closed trade as `[exit, pnl]`, oldest first: the fleet page sums them into one
        // curve over the robots and the dates it filters on.
        let closed: Vec<Value> = rows.iter().filter_map(|r| r.exit.map(|at| json!([rfc(at), num(r.pnl, 8)]))).collect();
        let open = trades.iter().filter(|t| t.open && !t.trade_key.contains("|pending:") && t.robot_qty.is_none_or(|q| !q.is_zero()) && !hide.contains(&t.trade_key)).count();
        let g = guards::State::of(&s.guard_state);
        let d = today.get(&s.id).cloned().unwrap_or_default();
        let mut kinds: BTreeMap<String, String> = BTreeMap::new();
        let tickers: Vec<String> = {
            let mut v = Vec::new();
            for &id in &s.dataset_ids {
                if let Ok(Some(ds)) = hd::get_dataset(&state.pool, id).await {
                    kinds.insert(ds.ticker.clone(), crate::histdata::native::canonical_asset_type(&ds.asset_type));
                    v.push(ds.ticker);
                }
            }
            v
        };
        // The open book's size by asset type: what the robot has at work in each market.
        let mut exposure: BTreeMap<String, Decimal> = BTreeMap::new();
        for p in &book {
            let (Some(ticker), Some(n)) = (p["ticker"].as_str(), p["notional"].as_f64()) else { continue };
            let kind = kinds.get(ticker).cloned().unwrap_or_else(|| "other".into());
            *exposure.entry(kind).or_default() += dec(n);
        }
        let exposure: serde_json::Map<String, Value> = exposure.into_iter().map(|(k, v)| (k, json!(num(v, 8)))).collect();
        out.push(json!({
            "id": s.id, "name": s.name, "status": s.status, "running": s.running, "priority": s.priority,
            "environment": account.as_ref().map_or("simulated", |a| a.1),
            "account": account.as_ref().map(|a| a.0.clone()),
            "account_id": s.exec_account_id.filter(|_| s.on_broker()),
            "armed": s.armed_at.is_some(), "exec_state": s.exec_state, "exec_note": s.exec_note,
            "exec_synced_at": s.exec_synced_at.map(rfc),
            "tickers": tickers, "exposure": exposure,
            "strategy": s.strategy_name,
            "equity": num(start + realized, 8), "start_equity": num(start, 8), "realized": num(realized, 8),
            "unrealized": unrealized.filter(|_| !book.is_empty()).map(|u| num(u, 8)), "closed": closed,
            "started_at": rfc(s.seeded_at.unwrap_or(s.created_at)),
            "currency": s.capital.get("currency").and_then(Value::as_str).unwrap_or_default(),
            "today_pnl": num(d.realized, 8), "orders_today": d.orders_sent,
            "open": open, "trades": rows.len(), "unseen": unseen.get(&s.id).copied().unwrap_or(0),
            "guard": g.badge(now), "guard_holds": g.holds(now),
            "next_run_at": s.next_run_at.map(rfc), "last_run_at": s.last_run_at.map(rfc),
            "last_error": s.last_error,
        }));
    }
    let overview = store::overview(&state.pool).await?;
    Ok(json!({ "robots": out, "overview": overview, "at": rfc(now) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(v: f64) -> Decimal {
        Decimal::try_from(v).unwrap()
    }

    fn row(pnl: f64, min: i64, ticker: &str, dir: &str) -> Row {
        let t = OffsetDateTime::from_unix_timestamp(1_790_000_000 + min * 60).unwrap();
        Row { ticker: ticker.into(), direction: dir.into(), entry: Some(t - time::Duration::minutes(30)), exit: Some(t), pnl: d(pnl), fees: d(0.5) }
    }

    #[test]
    fn the_stats_grid_reads_the_robot_trades() {
        let rows = vec![row(100.0, 0, "A", "long"), row(-50.0, 60, "A", "short"), row(-25.0, 120, "B", "long"), row(40.0, 180, "B", "long")];
        let s = stats(&rows, d(1000.0));
        assert_eq!(s["trades"], 4);
        assert_eq!(s["net_pnl"], 65.0);
        assert_eq!(s["win_rate"], 50.0);
        assert_eq!(s["profit_factor"], 1.8667);
        assert_eq!(s["expectancy"], 16.25);
        assert_eq!(s["max_drawdown"], 75.0);
        assert_eq!(s["current_streak"], 1);
        assert_eq!(s["longest_loss_streak"], 2);
        assert_eq!(s["avg_hold_secs"], 1800.0);
        assert_eq!(s["fees"], 2.0);
        assert_eq!(stats(&[], d(1000.0))["win_rate"], Value::Null);
    }

    #[test]
    fn curve_calendar_and_breakdown_agree_on_the_total() {
        let rows = vec![row(10.0, 0, "A", "long"), row(-4.0, 60 * 24, "B", "short"), row(6.0, 60 * 25, "A", "long")];
        let c = curve(&rows, d(100.0), OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap());
        assert_eq!(c.len(), 4);
        assert_eq!(c.last().unwrap()["equity"], 112.0);
        let cal = calendar(&rows, None);
        assert_eq!(cal.len(), 2);
        assert_eq!(cal.iter().map(|d| d["net_pnl"].as_f64().unwrap()).sum::<f64>(), 12.0);
        assert_eq!(cal[1]["losses"], 1);
        let b = breakdown(&rows);
        assert_eq!(b["instrument"][0]["key"], "A");
        assert_eq!(b["instrument"][0]["net"], 16.0);
        assert_eq!(b["direction"][1]["key"], "short");
    }
}
