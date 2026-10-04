//! Scenario tests (audit 2026-10-03): the execution rules checked against an independent
//! reading of the same prices, over long paths and a sweep of settings, rather than hand-built
//! candles. Each property is one way a backtest flatters itself:
//!
//! * a prefix of the data reproduces every trade the full run closed before it (no lookahead
//!   across bars), and an entry is the same when the entry bar's unknown high, low and close
//!   are flattened (no lookahead inside the entry bar: sizing, stops, ATR);
//! * every fill is a price the bar traded, on the side that deals;
//! * stops and targets resolved on the lower timeframe match a minute-by-minute walk;
//! * a limit entry fills at its own price or better, only when the market reached it;
//! * the levels a paper session watches after a close are the levels the next bar is tested
//!   against, and replaying the run's own exits as live fills reproduces the run;
//! * equity is the starting capital plus the trades.
//! * what is decided at an open (an entry, a pyramid add, a reverse, a portfolio slot) is the
//!   same when the rest of that bar is flattened, including on the bars a position then leaves
//!   inside: pyramiding, `exit_on_reverse`, stop-and-reverse, a trading window, a portfolio cap.
//!
//! `real_btcusdt` reads exported candles (ignored by default):
//! `OTW_SCENARIO_HOURS=h.csv OTW_SCENARIO_MINUTES=m.csv cargo test -p otw-core scenarios -- --ignored --nocapture`
//! with `epoch,open,high,low,close` rows.

use super::tests::settings_always_long;
use super::*;
use std::collections::HashMap;
use std::sync::Arc;

const EPS: f64 = 1e-9;

fn close_enough(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * a.abs().max(b.abs()).max(1.0)
}

// ── Market data ──

struct Subs(Vec<Option<Arc<SubSlice>>>, &'static str);

impl SubSource for Subs {
    fn candles(&self, bar: usize) -> Option<Arc<SubSlice>> {
        self.0.get(bar).cloned().flatten()
    }
    fn timeframe(&self) -> Option<String> {
        Some(self.1.into())
    }
}

struct Market {
    ts: Vec<String>,
    o: Vec<f64>,
    h: Vec<f64>,
    l: Vec<f64>,
    c: Vec<f64>,
    subs: Subs,
    quotes: Vec<Option<Quote>>,
}

fn iso(epoch: i64) -> String {
    let t = time::OffsetDateTime::from_unix_timestamp(epoch).unwrap();
    t.format(&time::format_description::well_known::Rfc3339).unwrap()
}

impl Market {
    fn len(&self) -> usize {
        self.ts.len()
    }
    /// The first `k` bars, with the lower timeframe and the bid/ask when asked for.
    fn bars(&self, k: usize, sub: bool, quotes: bool) -> Bars<'_> {
        Bars {
            ticker: "X",
            ts: &self.ts[..k],
            open: &self.o[..k],
            high: &self.h[..k],
            low: &self.l[..k],
            close: &self.c[..k],
            volume: &self.c[..k],
            quotes: quotes.then_some(&self.quotes as &(dyn QuoteSource + Sync)),
            sub: sub.then_some(&self.subs as &(dyn SubSource + Sync)),
        }
    }
    /// Bars built from lower-timeframe candles, `per` to a bar, with a bid/ask `half` (a
    /// fraction) around every price.
    fn from_minutes(epoch0: i64, step: i64, per: usize, m: &[[f64; 4]], half: f64, tf: &'static str) -> Market {
        let n = m.len() / per;
        let mut mk = Market {
            ts: vec![],
            o: vec![],
            h: vec![],
            l: vec![],
            c: vec![],
            subs: Subs(vec![], tf),
            quotes: vec![],
        };
        for i in 0..n {
            let s = &m[i * per..(i + 1) * per];
            let (o, c) = (s[0][0], s[per - 1][3]);
            let h = s.iter().map(|x| x[1]).fold(f64::NEG_INFINITY, f64::max);
            let l = s.iter().map(|x| x[2]).fold(f64::INFINITY, f64::min);
            mk.push(iso(epoch0 + i as i64 * step), o, h, l, c, s, half);
        }
        mk
    }
    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, ts: String, o: f64, h: f64, l: f64, c: f64, s: &[[f64; 4]], half: f64) {
        let q = |x: f64| (x * (1.0 - half), x * (1.0 + half));
        let quote = |o: f64, h: f64, l: f64, c: f64| Quote {
            bid_open: q(o).0,
            bid_high: q(h).0,
            bid_low: q(l).0,
            bid_close: q(c).0,
            ask_open: q(o).1,
            ask_high: q(h).1,
            ask_low: q(l).1,
            ask_close: q(c).1,
        };
        self.ts.push(ts);
        self.o.push(o);
        self.h.push(h);
        self.l.push(l);
        self.c.push(c);
        self.quotes.push(Some(quote(o, h, l, c)));
        self.subs.0.push((!s.is_empty()).then(|| {
            Arc::new(SubSlice {
                open: s.iter().map(|x| x[0]).collect(),
                high: s.iter().map(|x| x[1]).collect(),
                low: s.iter().map(|x| x[2]).collect(),
                quotes: Some(s.iter().map(|x| Some(quote(x[0], x[1], x[2], x[3]))).collect()),
            })
        }));
    }
    /// A copy cut after bar `e`, flat at its open from there: what the market looked like when
    /// `e` opened. One more flat bar follows, as the engine fills nothing on a run's last bar.
    fn flattened_at(&self, e: usize) -> Market {
        let k = e + 2;
        let mut m = Market {
            ts: self.ts[..k].to_vec(),
            o: self.o[..k].to_vec(),
            h: self.h[..k].to_vec(),
            l: self.l[..k].to_vec(),
            c: self.c[..k].to_vec(),
            subs: Subs(self.subs.0[..k].to_vec(), self.subs.1),
            quotes: self.quotes[..k].to_vec(),
        };
        let o = m.o[e];
        for i in e..k {
            (m.o[i], m.h[i], m.l[i], m.c[i]) = (o, o, o, o);
            m.subs.0[i] = None;
            m.quotes[i] = self.quotes[e].map(|q| Quote {
                bid_high: q.bid_open,
                bid_low: q.bid_open,
                bid_close: q.bid_open,
                ask_high: q.ask_open,
                ask_low: q.ask_open,
                ask_close: q.ask_open,
                ..q
            });
        }
        m
    }
}

/// Deterministic random walk of lower-timeframe candles (LCG): momentum, intrabar wicks, and
/// a gap at some bar opens. Inside a bar every candle opens where the last one closed.
fn synthetic(n: usize, per: usize, seed: u64) -> Vec<[f64; 4]> {
    let mut st = seed;
    let mut next = || {
        st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((st >> 32) as f64) / ((1u64 << 31) as f64) - 1.0
    };
    let (mut px, mut mom) = (100.0f64, 0.0f64);
    let mut out = Vec::with_capacity(n * per);
    for i in 0..n * per {
        if i % per == 0 && i > 0 && next().abs() > 0.85 {
            px *= 1.0 + next() * 0.012;
        }
        let open = px;
        mom = mom * 0.9 + next() * 0.03;
        let close = (open * (1.0 + mom / 100.0 + next() * 0.004)).max(1.0);
        let high = open.max(close) * (1.0 + next().abs() * 0.0025);
        let low = open.min(close) * (1.0 - next().abs() * 0.0025);
        out.push([open, high, low, close]);
        px = close;
    }
    out
}

// ── Strategies ──

fn sma(period: usize) -> Operand {
    Operand::Indicator { indicator: "sma".into(), period, fast: 0, slow: 0, mult: 0.0, signal_period: 0 }
}

fn when(op: Op, right: Operand) -> SignalGroup {
    SignalGroup {
        logic: "all".into(),
        conditions: vec![Signal { left: Operand::Price { field: "close".into() }, op, right: Some(right) }],
    }
}

/// Close crossing its 20-bar average, one direction.
fn strategy(short: bool) -> Settings {
    let mut s = settings_always_long();
    s.sizing = Sizing::FixedQty { qty: 1.0 };
    let mut side = s.long.take().unwrap();
    side.entry = Some(when(if short { Op::CrossesBelow } else { Op::CrossesAbove }, sma(20)));
    if short {
        s.mode = Mode::Short;
        s.short = Some(side);
    } else {
        s.long = Some(side);
    }
    s
}

fn side(s: &mut Settings) -> &mut Side {
    if s.mode == Mode::Short { s.short.as_mut().unwrap() } else { s.long.as_mut().unwrap() }
}

fn limit(reference: &str, fill: &str, valid: usize, offset: f64) -> OrderSpec {
    OrderSpec {
        kind: "limit".into(),
        offset_kind: "pct".into(),
        offset,
        reference: reference.into(),
        valid_bars: valid,
        fill: fill.into(),
        ..OrderSpec::default()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stops {
    Pct,
    Atr,
    Trail,
}

#[derive(Clone, Copy)]
struct Case {
    short: bool,
    stops: Stops,
    entry_limit: Option<(&'static str, &'static str)>,
    exit_signal: bool,
    exit_limit: bool,
    intrabar: bool,
    quotes: bool,
    spread: f64,
    risk: bool,
}

const SL: f64 = 0.005;
const TP: f64 = 0.0075;
const LIMIT_OFF: f64 = 0.002;
const LIMIT_BARS: usize = 3;

impl Case {
    fn settings(&self) -> Settings {
        let mut s = strategy(self.short);
        s.spread_pct = self.spread;
        let sd = side(&mut s);
        match self.stops {
            Stops::Pct => {
                sd.stop_loss_pct = SL;
                sd.take_profit_pct = TP;
            }
            Stops::Atr => {
                sd.stop_loss = Some(Stop { kind: "atr".into(), value: 1.5, period: 14 });
                sd.take_profit = Some(Stop { kind: "atr".into(), value: 2.5, period: 14 });
            }
            Stops::Trail => {
                sd.stop_loss_pct = SL;
                sd.trailing_stop = Some(Trail { kind: "pct".into(), value: 0.006, ..Trail::default() });
            }
        }
        if self.exit_signal {
            sd.exit = Some(when(if self.short { Op::CrossesAbove } else { Op::CrossesBelow }, sma(8)));
        }
        if self.risk {
            s.sizing = Sizing::Risk { risk_pct: 1.0 };
            s.leverage = 5.0;
        }
        if let Some((reference, fill)) = self.entry_limit {
            s.execution.entry_order = limit(reference, fill, LIMIT_BARS, LIMIT_OFF);
        }
        if self.exit_limit {
            s.execution.exit_order = limit("close", "touch", 2, 0.001);
        }
        s.execution.intrabar = self.intrabar;
        s.execution.use_quotes = self.quotes;
        s
    }
    /// Prices exact enough to compare to the bar: no spread, no bid/ask.
    fn strict(&self) -> bool {
        self.spread == 0.0 && !self.quotes
    }
    fn name(&self) -> String {
        format!(
            "{} {:?} entry={:?} exit_sig={} exit_lim={} intrabar={} quotes={} spread={} risk={}",
            if self.short { "short" } else { "long" },
            match self.stops {
                Stops::Pct => "pct",
                Stops::Atr => "atr",
                Stops::Trail => "trail",
            },
            self.entry_limit,
            self.exit_signal,
            self.exit_limit,
            self.intrabar,
            self.quotes,
            self.spread,
            self.risk
        )
    }
}

fn cases() -> Vec<Case> {
    let base = Case {
        short: false,
        stops: Stops::Pct,
        entry_limit: None,
        exit_signal: false,
        exit_limit: false,
        intrabar: false,
        quotes: false,
        spread: 0.0,
        risk: false,
    };
    let mut out = vec![];
    for short in [false, true] {
        for stops in [Stops::Pct, Stops::Atr, Stops::Trail] {
            for entry_limit in [None, Some(("close", "touch")), Some(("open", "through"))] {
                for intrabar in [false, true] {
                    out.push(Case { short, stops, entry_limit, intrabar, ..base });
                }
            }
            out.push(Case { short, stops, exit_signal: true, ..base });
            out.push(Case { short, stops, exit_signal: true, exit_limit: true, intrabar: true, ..base });
            out.push(Case { short, stops, quotes: true, intrabar: true, ..base });
            out.push(Case { short, stops, spread: 0.0004, risk: true, ..base });
            out.push(Case { short, stops, risk: true, entry_limit: Some(("close", "touch")), ..base });
        }
    }
    out
}

// ── Properties ──

type Key = (String, String, String, String, String, String, String);

fn key(t: &Trade) -> Key {
    (
        t.entry_ts.clone(),
        t.exit_ts.clone(),
        format!("{:.9}", t.entry_price),
        format!("{:.9}", t.exit_price),
        format!("{:.9}", t.qty),
        format!("{:.9}", t.pnl),
        t.exit_reason.clone(),
    )
}

fn index(m: &Market) -> HashMap<&str, usize> {
    m.ts.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect()
}

/// What the run decided by bar k does not depend on the bars after k.
fn prefix_matches(name: &str, s: &Settings, m: &Market, c: &Case, full: &RunResult) -> usize {
    let n = m.len();
    let mut compared = 0;
    for k in [n / 3, n / 2, 2 * n / 3 + 7] {
        let part = run(s, &m.bars(k, c.intrabar, c.quotes));
        let cutoff = &m.ts[k - 1];
        let want: Vec<_> = full.trades.iter().filter(|t| &t.exit_ts < cutoff).map(key).collect();
        let got: Vec<_> = part.trades.iter().filter(|t| &t.exit_ts < cutoff).map(key).collect();
        assert_eq!(got, want, "{name}: prefix {k} disagrees with the full run");
        compared += want.len();
    }
    compared
}

/// An entry taken at a bar's open is the same entry when that bar's high, low and close are
/// unknown: price, size and fees come from what was known at the open.
fn entries_ignore_their_bar(name: &str, s: &Settings, m: &Market, c: &Case, full: &RunResult) -> usize {
    let idx = index(m);
    let mut checked = 0;
    for t in full.trades.iter().take(40) {
        let e = idx[t.entry_ts.as_str()];
        if e + 2 > m.len() || !close_enough(t.entry_price, m.o[e]) && c.strict() {
            // A limit filled inside the bar depends on that bar's low: legitimately.
            continue;
        }
        if c.entry_limit.is_some() {
            continue;
        }
        let flat = m.flattened_at(e);
        let r = run(s, &flat.bars(e + 2, c.intrabar, c.quotes));
        let Some(p) = r.trades.iter().find(|x| x.entry_ts == t.entry_ts) else {
            panic!("{name}: the entry at {} needs its own bar's range", t.entry_ts);
        };
        assert!(close_enough(p.entry_price, t.entry_price), "{name}: entry price at {} moves with its bar", t.entry_ts);
        assert!(close_enough(p.qty, t.qty), "{name}: size at {} moves with its bar ({} vs {})", t.entry_ts, p.qty, t.qty);
        checked += 1;
    }
    checked
}

/// Fills inside the traded range, trades in sequence, equity reconciled.
fn fills_are_sane(name: &str, s: &Settings, m: &Market, c: &Case, r: &RunResult) {
    let idx = index(m);
    let slack = |x: f64| x * (s.spread_pct + 1e-9);
    let lo = |i: usize| -> (f64, f64) {
        if c.quotes {
            let q = m.quotes[i].unwrap();
            (q.bid_low, q.ask_high)
        } else {
            (m.l[i] - slack(m.l[i]), m.h[i] + slack(m.h[i]))
        }
    };
    let mut prev: Option<&Trade> = None;
    for t in &r.trades {
        let (e, x) = (idx[t.entry_ts.as_str()], idx[t.exit_ts.as_str()]);
        assert!(x >= e, "{name}: exit before entry");
        let (el, eh) = lo(e);
        assert!(t.entry_price >= el - EPS && t.entry_price <= eh + EPS, "{name}: entry {} outside bar {e}", t.entry_price);
        let (xl, xh) = lo(x);
        assert!(t.exit_price >= xl - EPS && t.exit_price <= xh + EPS, "{name}: exit {} outside bar {x}", t.exit_price);
        if let Some(p) = prev {
            let px = idx[p.exit_ts.as_str()];
            assert!(e >= px, "{name}: {} opens before {} closed", t.entry_ts, p.exit_ts);
            if e == px {
                // Same bar: only behind an exit at that bar's very open.
                let at_open = if c.strict() { close_enough(p.exit_price, m.o[px]) } else { true };
                assert!(
                    matches!(p.exit_reason.as_str(), "stop_loss" | "take_profit" | "trailing_stop") && at_open,
                    "{name}: reopened at {} after a {} inside the bar (exit {} entry {} open {} high {} low {}) prev {:?} next {:?}",
                    t.entry_ts,
                    p.exit_reason,
                    p.exit_price,
                    t.entry_price,
                    m.o[px],
                    m.h[px],
                    m.l[px],
                    p,
                    t
                );
            }
        }
        prev = Some(t);
        if c.strict() {
            let long = t.direction == "long";
            // A market entry is the open; a limit is the open or better.
            if c.entry_limit.is_none() {
                assert!(close_enough(t.entry_price, m.o[e]), "{name}: market entry {} is not the open {}", t.entry_price, m.o[e]);
            } else if long {
                assert!(t.entry_price <= m.o[e] + EPS, "{name}: buy limit {} above the open", t.entry_price);
            } else {
                assert!(t.entry_price >= m.o[e] - EPS, "{name}: sell limit {} below the open", t.entry_price);
            }
            match t.exit_reason.as_str() {
                // A stop is never better than the open it gapped through.
                "stop_loss" | "trailing_stop" if x > e => {
                    if long {
                        assert!(t.exit_price <= m.o[x] + EPS, "{name}: long stop {} above the open", t.exit_price);
                    } else {
                        assert!(t.exit_price >= m.o[x] - EPS, "{name}: short stop {} below the open", t.exit_price);
                    }
                }
                "take_profit" => {
                    let tp = match c.stops {
                        Stops::Pct => Some(if long { t.entry_price * (1.0 + TP) } else { t.entry_price * (1.0 - TP) }),
                        _ => None,
                    };
                    if let Some(tp) = tp {
                        let gap = if long { m.o[x] >= tp } else { m.o[x] <= tp };
                        let want = if gap && x > e { m.o[x] } else { tp };
                        assert!(close_enough(t.exit_price, want), "{name}: target {} filled at {}", want, t.exit_price);
                    }
                }
                _ => {}
            }
        }
    }
    let sum: f64 = r.trades.iter().map(|t| t.pnl).sum();
    assert!((r.stats.final_equity - s.starting_capital - sum).abs() < 1e-6, "{name}: equity is not capital + trades");
}

/// A limit entry is the limit price (or the open, when the bar opened through it), for one of
/// the signals still in force, and only on a bar that reached it.
fn limit_entries_are_honest(name: &str, m: &Market, c: &Case, r: &RunResult) -> usize {
    let Some((reference, fill)) = c.entry_limit else { return 0 };
    if !c.strict() {
        return 0;
    }
    let idx = index(m);
    let mut checked = 0;
    for t in &r.trades {
        let e = idx[t.entry_ts.as_str()];
        let long = t.direction == "long";
        let at_open = close_enough(t.entry_price, m.o[e]);
        let placed = (e.saturating_sub(LIMIT_BARS)..e).any(|sig| {
            let refp = if reference == "open" { m.o[sig + 1] } else { m.c[sig] };
            let lvl = if long { refp * (1.0 - LIMIT_OFF) } else { refp * (1.0 + LIMIT_OFF) };
            if at_open {
                if long { m.o[e] <= lvl + EPS } else { m.o[e] >= lvl - EPS }
            } else {
                close_enough(t.entry_price, lvl)
            }
        });
        assert!(placed, "{name}: entry {} at {} is no limit any signal placed", t.entry_price, t.entry_ts);
        if !at_open {
            let reached = match (long, fill) {
                (true, "through") => m.l[e] < t.entry_price,
                (true, _) => m.l[e] <= t.entry_price + EPS,
                (false, "through") => m.h[e] > t.entry_price,
                (false, _) => m.h[e] >= t.entry_price - EPS,
            };
            assert!(reached, "{name}: limit {} filled on a bar that never reached it", t.entry_price);
        }
        checked += 1;
    }
    checked
}

/// The exit a candle-by-candle walk of the lower timeframe gives a position with a fixed stop
/// and target: the first candle to reach either (a gap at a candle's open fills there; both in
/// one candle, the stop).
fn walk(m: &Market, e: usize, long: bool, stop: f64, tp: f64) -> Option<(usize, &'static str, f64)> {
    for bar in e..m.len() {
        let sb = m.subs.0[bar].as_ref()?;
        for k in 0..sb.open.len() {
            let (o, h, l) = (sb.open[k], sb.high[k], sb.low[k]);
            let (gap_stop, gap_tp, hit_stop, hit_tp) = if long {
                (o <= stop, o >= tp, l <= stop, h >= tp)
            } else {
                (o >= stop, o <= tp, h >= stop, l <= tp)
            };
            if gap_stop {
                return Some((bar, "stop_loss", o));
            }
            if gap_tp {
                return Some((bar, "take_profit", o));
            }
            if hit_stop {
                return Some((bar, "stop_loss", stop));
            }
            if hit_tp {
                return Some((bar, "take_profit", tp));
            }
        }
    }
    None
}

/// Stops and targets resolved on the lower timeframe agree with the walk. `tol`: price
/// tolerance (a real series gaps between minutes inside a bar the engine reads whole).
fn matches_the_walk(name: &str, m: &Market, r: &RunResult, tol: f64) -> (usize, usize) {
    let idx = index(m);
    let (mut checked, mut skipped) = (0, 0);
    for t in &r.trades {
        let e = idx[t.entry_ts.as_str()];
        let long = t.direction == "long";
        let (stop, tp) = if long {
            (t.entry_price * (1.0 - SL), t.entry_price * (1.0 + TP))
        } else {
            (t.entry_price * (1.0 + SL), t.entry_price * (1.0 - TP))
        };
        let x = idx[t.exit_ts.as_str()];
        if (e..=x).any(|b| m.subs.0[b].is_none() || sub_slice(&m.bars(m.len(), true, false), b).is_none()) {
            skipped += 1;
            continue;
        }
        match walk(m, e, long, stop, tp) {
            Some((bar, reason, px)) => {
                assert_eq!(
                    (t.exit_ts.as_str(), t.exit_reason.as_str()),
                    (m.ts[bar].as_str(), reason),
                    "{name}: trade from {} exits on the wrong level",
                    t.entry_ts
                );
                assert!(
                    (t.exit_price - px).abs() <= tol * px + EPS,
                    "{name}: {} at {} filled {} instead of {}",
                    reason,
                    t.exit_ts,
                    t.exit_price,
                    px
                );
            }
            None => assert_eq!(t.exit_reason, "end", "{name}: {} exit the walk never reaches", t.exit_reason),
        }
        checked += 1;
    }
    (checked, skipped)
}

/// What a paper session watches after a close is what the next bar is tested against: a
/// position the full run stops or targets at bar k held exactly that level once k-1 closed,
/// including a position opened at k-1's open.
fn open_levels_hold(name: &str, s: &Settings, m: &Market, c: &Case, full: &RunResult) -> usize {
    if !c.strict() {
        return 0;
    }
    let idx = index(m);
    let mut checked = 0;
    for t in full.trades.iter().take(60) {
        let x = idx[t.exit_ts.as_str()];
        let e = idx[t.entry_ts.as_str()];
        if x <= e || !matches!(t.exit_reason.as_str(), "stop_loss" | "trailing_stop" | "take_profit") {
            continue;
        }
        if close_enough(t.exit_price, m.o[x]) {
            continue; // gap fill: the level only bounds it
        }
        let part = run_portfolio_forced(s, &[&m.bars(x, c.intrabar, c.quotes)], &[]);
        let Some(p) = part.open_positions.iter().find(|p| p.entry_ts == t.entry_ts) else {
            panic!("{name}: position from {} missing after the close before its exit ({:?}) trades {:?} pending {:?}", t.entry_ts, t, part.trades.iter().rev().take(2).collect::<Vec<_>>(), part.pending_orders);
        };
        let level = if t.exit_reason == "take_profit" { p.take_profit } else { p.stop };
        let level = level.unwrap_or_else(|| panic!("{name}: position from {} holds no {}", t.entry_ts, t.exit_reason));
        assert!(close_enough(level, t.exit_price), "{name}: watched {level}, next bar filled {}", t.exit_price);
        checked += 1;
    }
    checked
}

/// Replaying the run's own stop and target exits as live fills reproduces the run.
fn live_replay_reproduces(name: &str, s: &Settings, m: &Market, c: &Case, full: &RunResult) {
    let idx = index(m);
    let forced: Vec<ForcedFill> = full
        .trades
        .iter()
        .filter(|t| matches!(t.exit_reason.as_str(), "stop_loss" | "trailing_stop" | "take_profit"))
        .map(|t| ForcedFill {
            bar: idx[t.exit_ts.as_str()],
            kind: ForcedKind::Exit { px: t.exit_price, reason: t.exit_reason.clone(), maker: t.exit_reason == "take_profit" },
        })
        .collect();
    let b = m.bars(m.len(), c.intrabar, c.quotes);
    let r = run_portfolio_forced(s, &[&b], &[forced]);
    assert_eq!(r.execution.forced_unmatched, 0, "{name}: live exits found no position");
    let last = m.ts.last().unwrap();
    let want: Vec<_> = full.trades.iter().filter(|t| &t.exit_ts < last).map(key).collect();
    let got: Vec<_> = r.trades.iter().filter(|t| &t.exit_ts < last).map(key).collect();
    assert_eq!(got, want, "{name}: the live replay changes the run");
}

/// The close that decides an entry announces it: a forward run ending on the signal bar holds
/// the order the next bar sends, a market order at that close (its size the one it fills
/// with when nothing moves the sizing), a limit at the price it then rests at.
fn entries_are_announced_at_their_signal(name: &str, s: &Settings, m: &Market, c: &Case, full: &RunResult) -> usize {
    let idx = index(m);
    let mut checked = 0;
    for t in full.trades.iter().take(40) {
        let e = idx[t.entry_ts.as_str()];
        let candidates: Vec<usize> = match c.entry_limit {
            None => vec![e - 1],
            Some(_) => (e.saturating_sub(LIMIT_BARS)..e).collect(),
        };
        let found = candidates.iter().any(|&sig| {
            let r = run_portfolio_forced(s, &[&m.bars(sig + 1, c.intrabar, c.quotes)], &[]);
            r.pending_orders.iter().any(|o| {
                o.signal_ts.as_deref() == Some(m.ts[sig].as_str())
                    && o.direction == t.direction
                    && match c.entry_limit {
                        None => o.order == "market" && close_enough(o.price, m.c[sig]),
                        // Priced off the open after the alert: that open, the offset away.
                        Some(("open", _)) => {
                            let off = o.anchor_offset.unwrap_or(f64::NAN);
                            let lvl = if t.direction == "long" { m.o[sig + 1] * (1.0 - off) } else { m.o[sig + 1] * (1.0 + off) };
                            o.order == "limit"
                                && o.provisional
                                && o.anchor_pct
                                && (close_enough(lvl, t.entry_price) || close_enough(t.entry_price, m.o[e]))
                        }
                        Some(_) => {
                            o.order == "limit" && (close_enough(o.price, t.entry_price) || close_enough(t.entry_price, m.o[e]))
                        }
                    }
            })
        });
        assert!(found, "{name}: the entry at {} was not announced at its signal's close", t.entry_ts);
        checked += 1;
    }
    checked
}

/// A market entry filled on the live price at the next open (paper) replays at that price, and
/// the run is otherwise the backtest's.
fn live_market_entry_replays(name: &str, s: &Settings, m: &Market, c: &Case, full: &RunResult) {
    if c.entry_limit.is_some() {
        return;
    }
    let idx = index(m);
    let Some(t) = full.trades.iter().find(|t| idx[t.entry_ts.as_str()] > 30) else { return };
    let e = idx[t.entry_ts.as_str()];
    let px = t.entry_price * 1.0001;
    let forced = vec![ForcedFill { bar: e, kind: ForcedKind::Entry { long: t.direction == "long", px } }];
    let r = run_portfolio_forced(s, &[&m.bars(e + 1, c.intrabar, c.quotes)], &[forced]);
    let got = r.trades.iter().find(|x| x.entry_ts == t.entry_ts).unwrap_or_else(|| panic!("{name}: replayed entry missing"));
    assert!(close_enough(got.entry_price, px), "{name}: live market price not replayed ({} vs {px})", got.entry_price);
    assert_eq!(r.execution.forced_unmatched, 0, "{name}: live market entry unmatched");
}

/// A paper session runs forward at each close: the run that ends on a trade's entry bar holds
/// that trade (open, or already closed on that bar), at the backtest's price and size.
fn paper_sees_entries_at_their_close(name: &str, s: &Settings, m: &Market, c: &Case, full: &RunResult) -> usize {
    let idx = index(m);
    let mut checked = 0;
    for t in full.trades.iter().take(40) {
        let e = idx[t.entry_ts.as_str()];
        let r = run_portfolio_forced(s, &[&m.bars(e + 1, c.intrabar, c.quotes)], &[]);
        let Some(p) = r.trades.iter().find(|x| x.entry_ts == t.entry_ts && x.direction == t.direction) else {
            panic!("{name}: the paper run closing {} does not hold the entry made at its open", t.entry_ts);
        };
        assert!(close_enough(p.entry_price, t.entry_price) && close_enough(p.qty, t.qty), "{name}: paper entry at {} differs", t.entry_ts);
        if p.exit_reason != "end" {
            assert_eq!(key(p), key(t), "{name}: paper closed {} differently on its entry bar", t.entry_ts);
        }
        checked += 1;
    }
    checked
}

fn check_all(m: &Market, label: &str, capital: f64) -> HashMap<&'static str, usize> {
    let mut tally: HashMap<&'static str, usize> = HashMap::new();
    for c in cases() {
        let name = format!("{label}: {}", c.name());
        let mut s = c.settings();
        s.starting_capital = capital;
        assert!(s.validate().is_none(), "{name}: {:?}", s.validate());
        let full = run(&s, &m.bars(m.len(), c.intrabar, c.quotes));
        assert!(full.trades.len() > 10, "{name}: too few trades ({})", full.trades.len());
        let again = run(&s, &m.bars(m.len(), c.intrabar, c.quotes));
        assert_eq!(full.trades.iter().map(key).collect::<Vec<_>>(), again.trades.iter().map(key).collect::<Vec<_>>());
        *tally.entry("trades").or_default() += full.trades.len();
        *tally.entry("prefix").or_default() += prefix_matches(&name, &s, m, &c, &full);
        *tally.entry("entry_bar").or_default() += entries_ignore_their_bar(&name, &s, m, &c, &full);
        fills_are_sane(&name, &s, m, &c, &full);
        *tally.entry("limit").or_default() += limit_entries_are_honest(&name, m, &c, &full);
        *tally.entry("levels").or_default() += open_levels_hold(&name, &s, m, &c, &full);
        live_replay_reproduces(&name, &s, m, &c, &full);
        *tally.entry("paper_entry").or_default() += paper_sees_entries_at_their_close(&name, &s, m, &c, &full);
        *tally.entry("announced").or_default() += entries_are_announced_at_their_signal(&name, &s, m, &c, &full);
        live_market_entry_replays(&name, &s, m, &c, &full);
        if c.stops == Stops::Pct && c.intrabar && c.entry_limit.is_none() && !c.exit_signal && c.strict() {
            let (n, _) = matches_the_walk(&name, m, &full, 0.0);
            *tally.entry("walk").or_default() += n;
            *tally.entry("resolved").or_default() += full.execution.resolved_bars;
            eprintln!("{name}: ambiguous {} resolved {} unresolved {}", full.execution.ambiguous_bars, full.execution.resolved_bars, full.execution.unresolved_bars);
        }
        *tally.entry("ambiguous").or_default() += full.execution.ambiguous_bars;
        *tally.entry("quoted").or_default() += full.execution.quoted_fills;
    }
    tally
}

#[test]
fn synthetic_paths_hold_every_property() {
    for seed in [11u64, 29, 47] {
        let m = Market::from_minutes(1_704_067_200, 3600, 12, &synthetic(900, 12, seed), 0.0003, "5m");
        let t = check_all(&m, &format!("seed {seed}"), 10_000.0);
        assert!(t["prefix"] > 200, "{t:?}");
        assert!(t["entry_bar"] > 200, "{t:?}");
        assert!(t["limit"] > 100, "{t:?}");
        assert!(t["levels"] > 100, "{t:?}");
        assert!(t["paper_entry"] > 200, "{t:?}");
        assert!(t["announced"] > 200, "{t:?}");
        assert!(t["walk"] > 50 && t["resolved"] > 0, "{t:?}");
        assert!(t["quoted"] > 0, "{t:?}");
    }
}

/// Reads `epoch,open,high,low,close` rows.
fn read_csv(path: &str) -> Vec<(i64, [f64; 4])> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            let e = f.first()?.trim().parse::<f64>().ok()? as i64;
            let p = |i: usize| f[i].trim().parse::<f64>().unwrap();
            Some((e, [p(1), p(2), p(3), p(4)]))
        })
        .collect()
}

#[test]
#[ignore]
fn real_btcusdt() {
    let (Ok(hp), Ok(mp)) = (std::env::var("OTW_SCENARIO_HOURS"), std::env::var("OTW_SCENARIO_MINUTES")) else {
        panic!("set OTW_SCENARIO_HOURS and OTW_SCENARIO_MINUTES");
    };
    let hours = read_csv(&hp);
    let minutes = read_csv(&mp);
    let mut by_hour: HashMap<i64, Vec<[f64; 4]>> = HashMap::new();
    for (e, p) in &minutes {
        by_hour.entry(e - e.rem_euclid(3600)).or_default().push(*p);
    }
    let mut m = Market { ts: vec![], o: vec![], h: vec![], l: vec![], c: vec![], subs: Subs(vec![], "1m"), quotes: vec![] };
    for (e, [o, h, l, c]) in &hours {
        let subs = by_hour.get(e).cloned().unwrap_or_default();
        m.push(iso(*e), *o, *h, *l, *c, &subs, 0.00002);
    }
    let all = m.bars(m.len(), true, false);
    let usable = (0..m.len()).filter(|&b| sub_slice(&all, b).is_some()).count();
    eprintln!("real: {} hours, {} minutes, {} hours with a matching minute slice", m.len(), minutes.len(), usable);
    let t = check_all(&m, "BTCUSDT 1h", 1_000_000.0);
    eprintln!("real: {t:?}");
    // Real minutes gap inside an hour the engine reads whole: the reason and bar must match,
    // the price within 0.2 %.
    for short in [false, true] {
        let mut s = strategy(short);
        s.starting_capital = 1_000_000.0;
        side(&mut s).stop_loss_pct = SL;
        side(&mut s).take_profit_pct = TP;
        s.execution.intrabar = true;
        let r = run(&s, &all);
        let (n, skipped) = matches_the_walk("real walk", &m, &r, 0.002);
        eprintln!(
            "real walk short={short}: {n} trades checked, {skipped} skipped, ambiguous {} resolved {} unresolved {}",
            r.execution.ambiguous_bars, r.execution.resolved_bars, r.execution.unresolved_bars
        );
    }
}

/// A portfolio decides its entries and pyramid adds at the open of a row from what is known
/// then: the open of every asset, not the close. Flattening every asset's bar at its open must
/// leave what was opened or added there unchanged (positions both runs still hold).
#[test]
fn portfolio_entries_and_adds_ignore_their_bar() {
    let a = Market::from_minutes(1_704_067_200, 3600, 12, &synthetic(500, 12, 5), 0.0003, "5m");
    let b = Market::from_minutes(1_704_067_200, 3600, 12, &synthetic(500, 12, 77), 0.0003, "5m");
    let mut s = strategy(false);
    s.pyramiding = 3;
    s.sizing = Sizing::PercentEquity { percent: 30.0 };
    s.leverage = 2.0;
    {
        let sd = side(&mut s);
        sd.entry = Some(when(Op::Above, sma(20)));
        sd.stop_loss_pct = SL;
        sd.take_profit_pct = 0.03;
    }
    let pkey = |p: &OpenPosition| (p.ticker.clone(), p.entry_ts.clone(), p.direction.clone());
    let mut compared = 0;
    for r in 40..a.len() - 1 {
        let (fa, fb) = (a.flattened_at(r), b.flattened_at(r));
        // `flattened_at` adds one flat bar after `r`: cut it off, `r` is the last bar here.
        let real = run_portfolio_forced(&s, &[&tk(&a, "A", r + 1), &tk(&b, "B", r + 1)], &[]);
        let flat = run_portfolio_forced(&s, &[&tk(&fa, "A", r + 1), &tk(&fb, "B", r + 1)], &[]);
        for p in &real.open_positions {
            if let Some(q) = flat.open_positions.iter().find(|q| pkey(q) == pkey(p)) {
                assert!(
                    close_enough(p.qty, q.qty) && close_enough(p.avg_price, q.avg_price),
                    "row {r}: {} {} holds {} at {} with its bar, {} at {} without",
                    p.ticker,
                    p.entry_ts,
                    p.qty,
                    p.avg_price,
                    q.qty,
                    q.avg_price
                );
                compared += 1;
            }
        }
    }
    assert!(compared > 100, "too few positions compared ({compared})");
}

/// The first `k` bars of `m` under another ticker, no lower timeframe, no bid/ask.
fn tk<'a>(m: &'a Market, ticker: &'a str, k: usize) -> Bars<'a> {
    Bars { ticker, ..m.bars(k, false, false) }
}


// ── Decisions at the open, over the settings the fill matrix leaves out ──

/// Strategies whose decisions at an open reach beyond a single entry.
fn decision_cases() -> Vec<(&'static str, Settings)> {
    let mut out = Vec::new();
    let state = |s: &mut Settings| side(s).entry = Some(when(Op::Above, sma(20)));
    let mut pyramid = strategy(false);
    state(&mut pyramid);
    pyramid.pyramiding = 3;
    pyramid.sizing = Sizing::PercentEquity { percent: 20.0 };
    side(&mut pyramid).stop_loss_pct = SL;
    side(&mut pyramid).take_profit_pct = 0.03;
    pyramid.pyramid_steps.after_add_sl = "breakeven".into();
    out.push(("pyramid", pyramid.clone()));
    // Run for its sanity (trades, adds) only: a limit fills on its bar's range.
    let mut limit_pyramid = pyramid.clone();
    limit_pyramid.execution.entry_order = limit("close", "touch", LIMIT_BARS, LIMIT_OFF);
    out.push(("pyramid limit", limit_pyramid));
    let mut eor = strategy(false);
    state(&mut eor);
    side(&mut eor).exit_on_reverse = true;
    side(&mut eor).stop_loss_pct = SL;
    out.push(("exit_on_reverse", eor));
    let mut sar = strategy(false);
    side(&mut sar).stop_loss_pct = SL;
    let mut short = sar.long.clone().unwrap();
    short.entry = Some(when(Op::CrossesBelow, sma(20)));
    sar.short = Some(short);
    sar.mode = Mode::Both;
    sar.stop_and_reverse = true;
    out.push(("stop_and_reverse", sar));
    let mut window = strategy(false);
    state(&mut window);
    side(&mut window).stop_loss_pct = SL;
    window.filters.sessions = vec![Session { from: "08:00".into(), to: "20:00".into() }];
    window.filters.on_window_end = "flat".into();
    out.push(("window", window));
    out
}

/// Every position a forward run ending on bar `r` held across that bar's open, or opened at it:
/// (ticker, direction, entry) → (stacked entries, qty, average price).
fn held_through(r: &RunResult, ts: &str) -> HashMap<(String, String, String), (usize, String, String)> {
    r.trades
        .iter()
        .filter(|t| t.exit_ts == ts)
        .map(|t| ((t.ticker.clone(), t.direction.clone(), t.entry_ts.clone()), (t.entries, format!("{:.9}", t.qty), format!("{:.9}", t.entry_price))))
        .collect()
}

/// Real bar `r` against the same bar flattened at its open: whatever was opened, added or kept
/// at that open is identical. A position the real bar then stops still carries the add its open
/// sent; a slot another asset frees inside the bar is not free at its open.
fn decisions_at_the_open(name: &str, s: &Settings, markets: &[(&str, &Market)]) -> usize {
    let n = markets[0].1.len();
    let mut compared = 0;
    for r in (40..n - 1).step_by(2) {
        let flat: Vec<Market> = markets.iter().map(|(_, m)| m.flattened_at(r)).collect();
        let real_bars: Vec<Bars> = markets.iter().map(|(t, m)| Bars { ticker: t, ..m.bars(r + 1, false, false) }).collect();
        let flat_bars: Vec<Bars> = markets.iter().zip(&flat).map(|((t, _), f)| Bars { ticker: t, ..f.bars(r + 1, false, false) }).collect();
        let real = run_portfolio_forced(s, &real_bars.iter().collect::<Vec<_>>(), &[]);
        let flat = run_portfolio_forced(s, &flat_bars.iter().collect::<Vec<_>>(), &[]);
        let ts = markets[0].1.ts[r].as_str();
        let (a, b) = (held_through(&real, ts), held_through(&flat, ts));
        assert_eq!(a, b, "{name}: bar {r} decides its open differently once its range is known");
        compared += a.len();
    }
    compared
}

#[test]
fn decisions_at_the_open_ignore_the_rest_of_the_bar() {
    for seed in [5u64, 77] {
        let m = Market::from_minutes(1_704_067_200, 3600, 12, &synthetic(500, 12, seed), 0.0, "5m");
        for (label, s) in decision_cases() {
            let name = format!("seed {seed} {label}");
            assert!(s.validate().is_none(), "{name}: {:?}", s.validate());
            let full = run(&s, &m.bars(m.len(), false, false));
            assert!(full.trades.len() > 10, "{name}: too few trades ({})", full.trades.len());
            if label.starts_with("pyramid") {
                assert!(full.trades.iter().any(|t| t.entries > 1), "{name}: no add to test");
            }
            // A limit fills inside its bar, on that bar's range: legitimately not an open decision.
            if s.execution.entry_order.is_limit() {
                continue;
            }
            let n = decisions_at_the_open(&name, &s, &[("X", &m)]);
            assert!(n > 30, "{name}: too few positions compared ({n})");
        }
    }
}

/// Two assets under `max_open_positions: 1`: an entry on one never takes the slot the other
/// frees later inside the same bar, and no open ever holds more than one position (one that
/// leaves at that open excepted).
#[test]
fn a_portfolio_cap_is_decided_at_the_open() {
    let a = Market::from_minutes(1_704_067_200, 3600, 12, &synthetic(500, 12, 5), 0.0, "5m");
    let b = Market::from_minutes(1_704_067_200, 3600, 12, &synthetic(500, 12, 77), 0.0, "5m");
    let mut s = strategy(false);
    side(&mut s).entry = Some(when(Op::Above, sma(20)));
    side(&mut s).stop_loss_pct = SL;
    s.risk.max_open_positions = Some(1);
    let n = decisions_at_the_open("portfolio cap", &s, &[("A", &a), ("B", &b)]);
    assert!(n > 50, "too few positions compared ({n})");
    let (ba, bb) = (Bars { ticker: "A", ..a.bars(a.len(), false, false) }, Bars { ticker: "B", ..b.bars(b.len(), false, false) });
    let full = run_portfolio(&s, &[&ba, &bb]);
    for (i, ts) in a.ts.iter().enumerate() {
        let held: Vec<&Trade> = full
            .trades
            .iter()
            .filter(|t| {
                let open = if t.ticker == "A" { a.o[i] } else { b.o[i] };
                t.entry_ts <= *ts && (t.exit_ts > *ts || (t.exit_ts == *ts && (t.exit_price - open).abs() > 1e-9))
            })
            .collect();
        assert!(held.len() <= 1, "bar {i}: {} positions held at the open: {held:?}", held.len());
    }
}

/// Grid and DCA engines: the equity a prefix of the data produces is the full run's, bar for
/// bar, up to the bar before the cut (the cut bar itself settles "end"). No ladder, tranche or
/// sell reads a bar after the one it acts on.
#[test]
fn grid_and_dca_never_read_ahead() {
    let m = Market::from_minutes(1_704_067_200, 3600, 12, &synthetic(400, 12, 29), 0.0, "5m");
    let lo = m.l.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = m.h.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let base = |extra: serde_json::Value| -> Settings {
        let mut v = serde_json::json!({ "mode": "long", "sizing": { "mode": "fixed_qty", "qty": 1.0 } });
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        serde_json::from_value(v).unwrap()
    };
    let cases = [
        ("grid fixed", base(serde_json::json!({ "kind": "grid", "grid": { "lower": lo, "upper": hi, "levels": 12, "qty_per_level": 1.0, "stop_below": lo * 1.01 } }))),
        ("grid known range", base(serde_json::json!({ "kind": "grid" }))),
        ("grid anchored", base(serde_json::json!({ "kind": "grid", "grid": { "lower": 0, "upper": 0, "levels": 8, "qty_per_level": 1.0, "anchor": "ema", "anchor_period": 20, "width_kind": "atr", "width_value": 2.0, "reset_on_close": true } }))),
        ("dca", base(serde_json::json!({ "kind": "dca", "dca": {
            "contribution": { "amount": 100.0, "period": "day" },
            "buys": [{ "amount": 200.0, "condition": { "logic": "all", "conditions": [{ "left": { "kind": "price", "field": "close" }, "op": "below", "right": { "kind": "indicator", "indicator": "sma", "period": 20 } }] } }],
            "sells": [{ "amount_kind": "pct_position", "amount": 20.0, "target_gain_pct": 0.5, "cooldown_bars": 10 }]
        } }))),
    ];
    for (name, s) in cases {
        assert!(s.validate().is_none(), "{name}: {:?}", s.validate());
        let full = run(&s, &m.bars(m.len(), false, false));
        let acted = !full.trades.is_empty() || full.dca.as_ref().is_some_and(|d| d.events.iter().filter(|e| e.action == "sell").count() > 0);
        assert!(acted, "{name}: nothing traded");
        for k in [120, 200, 333] {
            let part = run(&s, &m.bars(k, false, false));
            for i in 0..k - 1 {
                assert!(
                    (part.equity[i].equity - full.equity[i].equity).abs() < 1e-6,
                    "{name}: cut at {k} changes the equity at bar {i} ({} vs {})",
                    part.equity[i].equity,
                    full.equity[i].equity
                );
            }
        }
    }
}
