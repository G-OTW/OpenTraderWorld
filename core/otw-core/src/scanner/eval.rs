//! One IR for the four builders, the AND of an alert's signals on a closed bar, and the
//! pure decisions around a hit: cooldown, the notification text (digest or one per hit),
//! and when an error is news.

use std::collections::HashMap;

use ::time::{Duration, OffsetDateTime};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use otw_store::market_sessions::Session;
use otw_store::scanner::ItemCache;

use super::events::{self, Event};
use super::patterns::Pattern;
use super::time::{self as tsig, TimeGate, TimeSpec};
use super::Ohlcv;
use crate::backtest::script::{ScriptDefs, ScriptHistory, WarmBars};
use crate::backtest::{self, Bars, Defs, IndicatorDefs, Operand, SignalGroup};

/// Bars read beyond the longest lookback, so the series is warm and the bar before the
/// newest (the one a cross is measured against) is there.
pub const EXTRA_BARS: usize = 5;
/// Ceiling on the closed bars one item read may ask for (a day of 1m bars fits).
pub const MAX_BARS: usize = 1500;
/// Closes kept for the sparkline.
pub const SPARK_BARS: usize = 60;
/// Bars the board's average volume is measured over.
pub const AVG_VOLUME_BARS: usize = 20;
/// Hits listed in one digest before "and N more".
pub const DIGEST_LINES: usize = 20;
/// Closed bars a signal reading a script is judged over: a script declares no lookback, and
/// its own `na` bars gate what it plots. The window a chart alert reads a script over.
pub const SCRIPT_BARS: usize = crate::histviz_alerts::MAX_BARS as usize;
/// Run cache one item's script history may hold.
const SCRIPT_CACHE_BYTES: usize = 4 << 20;

/// What a signal row's `config` means, by builder.
#[derive(Debug, Clone)]
pub enum Spec {
    Signal(SignalGroup),
    Event(Event),
    Pattern(Pattern),
    Time(TimeSpec),
}

/// Read a stored or submitted config. `Err` is the sentence the API answers.
pub fn parse(builder: &str, config: &Value) -> Result<Spec, String> {
    let spec = match builder {
        "signal" => Spec::Signal(
            serde_json::from_value(config.clone()).map_err(|e| format!("not a signal group: {e}"))?,
        ),
        "event" => Spec::Event(serde_json::from_value(config.clone()).map_err(|e| format!("not an event: {e}"))?),
        "pattern" => {
            Spec::Pattern(serde_json::from_value(config.clone()).map_err(|e| format!("not a pattern: {e}"))?)
        }
        "time" => Spec::Time(
            serde_json::from_value(config.clone()).map_err(|e| format!("not a time window: {e}"))?,
        ),
        _ => return Err("builder must be signal, event, pattern or time".into()),
    };
    spec.validate()?;
    Ok(spec)
}

impl Spec {
    /// Why this signal cannot be judged, without the library.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Spec::Signal(g) => {
                if g.conditions.is_empty() {
                    return Err("a signal group needs at least one condition".into());
                }
                if g.logic != "all" && g.logic != "any" {
                    return Err("logic must be all or any".into());
                }
                for c in &g.conditions {
                    crate::histviz_alerts::check_operand(&c.left)?;
                    if let Some(r) = &c.right {
                        crate::histviz_alerts::check_operand(r)?;
                    }
                }
                Ok(())
            }
            Spec::Event(e) => e.validate(),
            Spec::Pattern(p) => p.validate(),
            Spec::Time(t) => t.validate(),
        }
    }

    /// Library indicators the signal reads, by id.
    pub fn custom_ids(&self) -> Vec<String> {
        let Spec::Signal(g) = self else { return Vec::new() };
        let mut out = Vec::new();
        for c in &g.conditions {
            for op in std::iter::once(&c.left).chain(c.right.iter()) {
                if let Operand::CustomIndicator { id, .. } = op {
                    if !out.contains(id) {
                        out.push(id.clone());
                    }
                }
            }
        }
        out
    }

    /// Market sessions the signal reads.
    pub fn session_ids(&self) -> Vec<Uuid> {
        match self {
            Spec::Time(t) => t.sessions.clone(),
            Spec::Event(Event::MoveFromOpen { session_id: Some(id), .. }) => vec![*id],
            _ => Vec::new(),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Spec::Signal(g) => {
                let n = g.conditions.len();
                let join = if g.logic == "any" { "any of" } else { "all of" };
                if n == 1 { "1 condition".into() } else { format!("{join} {n} conditions") }
            }
            Spec::Event(e) => e.describe(),
            Spec::Pattern(p) => p.describe(),
            Spec::Time(t) => t.describe(),
        }
    }
}

/// What a signal may read from the library: node-graph indicators and scripts by id, and the
/// market sessions.
#[derive(Default)]
pub struct Library {
    pub dag: IndicatorDefs,
    pub scripts: ScriptDefs,
    pub sessions: Vec<Session>,
}

/// A signal resolved against the library, ready to judge any item's bars.
pub struct Compiled {
    pub id: Uuid,
    pub name: String,
    pub spec: Spec,
    gate: Option<TimeGate>,
    session: Option<Session>,
    dag: IndicatorDefs,
    scripts: ScriptDefs,
}

impl Compiled {
    pub fn new(id: Uuid, name: &str, spec: Spec, lib: &Library) -> Result<Compiled, String> {
        let mut dag = IndicatorDefs::new();
        let mut scripts = ScriptDefs::new();
        if let Spec::Signal(g) = &spec {
            for op in g.conditions.iter().flat_map(|c| std::iter::once(&c.left).chain(c.right.iter())) {
                let Operand::CustomIndicator { id, plot } = op else { continue };
                if let Some(def) = lib.dag.get(id) {
                    dag.insert(id.clone(), def.clone());
                } else if let Some(def) = lib.scripts.get(id) {
                    // The same plot check as a chart alert: a plot that repaints would report
                    // a hit that later disappears.
                    if let Some(why) = backtest::script::check_plot(def, plot.as_deref()) {
                        return Err(format!("{name}: {why}"));
                    }
                    scripts.insert(id.clone(), def.clone());
                } else {
                    return Err(format!("an indicator {name} reads no longer exists"));
                }
            }
        }
        let gate = match &spec {
            Spec::Time(t) => Some(t.gate(&lib.sessions)?),
            _ => None,
        };
        let session = match &spec {
            Spec::Event(Event::MoveFromOpen { session_id: Some(sid), .. }) => Some(
                lib.sessions
                    .iter()
                    .find(|s| s.id == *sid)
                    .cloned()
                    .ok_or_else(|| format!("the session {name} measures from was deleted"))?,
            ),
            _ => None,
        };
        Ok(Compiled { id, name: name.to_string(), spec, gate, session, dag, scripts })
    }

    fn defs(&self) -> Defs<'_> {
        Defs { dag: &self.dag, scripts: &self.scripts }
    }

    /// A script either side of a condition: judged over the whole window, on a history.
    pub fn reads_script(&self) -> bool {
        !self.scripts.is_empty()
    }

    /// Closed bars needed before the newest bar of a `tf` series can be judged.
    pub fn lookback(&self, tf: i64) -> usize {
        let day_bars = (86_400 / tf.max(1)).max(1) as usize;
        match &self.spec {
            Spec::Signal(_) if self.reads_script() => SCRIPT_BARS,
            Spec::Signal(g) => backtest::group_lookback(g, &self.defs()) + 1,
            Spec::Event(e) => e.lookback(day_bars),
            Spec::Pattern(p) => p.lookback(),
            Spec::Time(_) => 1,
        }
    }

    /// Per bar, the first bar of its session for a move from the open.
    fn anchors(&self, s: &Ohlcv, tf: i64) -> Vec<Option<usize>> {
        let Spec::Event(Event::MoveFromOpen { timezone, .. }) = &self.spec else {
            return vec![None; s.len()];
        };
        // From a day up, a bar is its own session.
        if tf >= 86_400 {
            return (0..s.len()).map(Some).collect();
        }
        let keys = match &self.session {
            Some(sess) => tsig::session_keys(&s.ts, tf, sess),
            None => {
                let name = timezone.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or("UTC");
                match crate::automator::schedule::timezone(name) {
                    Some(tz) => tsig::day_keys(&s.ts, tf, tz),
                    None => vec![None; s.len()],
                }
            }
        };
        // Bar 0 opens its session when it started within one bar of the session's open.
        let first_is_open = match (keys.first().copied().flatten(), s.ts.first()) {
            (Some(open), Some(t0)) => t0.unix_timestamp() - open < tf,
            _ => false,
        };
        events::anchors(&keys, first_is_open)
    }

    /// Whether the signal held on each bar. `h` is the item's script history, read when the
    /// signal reads a script.
    pub fn judge(&self, s: &Ohlcv, tf: i64, h: Option<&ScriptHistory>) -> Vec<bool> {
        match &self.spec {
            Spec::Signal(g) => {
                let cols = Cols::of(s);
                backtest::eval_group(g, &cols.bars(s, h), &self.defs())
            }
            Spec::Event(e) => e.judge(s, &self.anchors(s, tf)),
            Spec::Pattern(p) => p.judge(s),
            Spec::Time(_) => match &self.gate {
                Some(g) => g.judge(&s.ts, tf),
                None => vec![false; s.len()],
            },
        }
    }

    /// The reading a hit carries for this signal on the newest bar, when it has one.
    pub fn reading(&self, s: &Ohlcv, tf: i64, h: Option<&ScriptHistory>) -> Option<f64> {
        match &self.spec {
            Spec::Event(e) => e.measure(s, &self.anchors(s, tf)).last().copied().flatten(),
            Spec::Signal(g) => {
                let c = g.conditions.first()?;
                let cols = Cols::of(s);
                backtest::resolve(&c.left, &cols.bars(s, h), &self.defs()).last().copied().flatten()
            }
            _ => None,
        }
        .filter(|v| v.is_finite())
    }
}

/// Owned string stamps for a backtest `Bars` view.
struct Cols {
    ts: Vec<String>,
}

impl Cols {
    fn of(s: &Ohlcv) -> Cols {
        use ::time::format_description::well_known::Rfc3339;
        Cols { ts: s.ts.iter().map(|t| t.format(&Rfc3339).unwrap_or_default()).collect() }
    }

    fn bars<'a>(&'a self, s: &'a Ohlcv, script: Option<&'a ScriptHistory>) -> Bars<'a> {
        Bars {
            ticker: "",
            ts: &self.ts,
            open: &s.open,
            high: &s.high,
            low: &s.low,
            close: &s.close,
            volume: &s.volume,
            quotes: None,
            sub: None,
            script,
        }
    }
}

/// Bars an alert wants read for an item: its signals' longest lookback, plus the margin,
/// capped.
pub fn bars_wanted(signals: &[&Compiled], tf: i64) -> usize {
    let lb = signals.iter().map(|c| c.lookback(tf)).max().unwrap_or(1);
    (lb + EXTRA_BARS).clamp(EXTRA_BARS, MAX_BARS)
}

/// An alert judged on an item's newest closed bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub bar_ts: OffsetDateTime,
    /// Every signal held on that bar.
    pub held: bool,
    /// Each signal's own state on that bar, by id.
    pub states: Vec<(Uuid, bool)>,
    /// What a hit records: the close and each signal's reading.
    pub values: Value,
}

/// Whether any of these signals runs a script.
pub fn reads_script(signals: &[&Compiled]) -> bool {
    signals.iter().any(|c| c.reads_script())
}

/// The history an item's scripts run on: no stored warmup, the window read is the history.
/// Built per judgement, since the next one reads other bars.
pub fn script_history(syminfo: otw_script::input::SymInfo, timeframe: &str) -> ScriptHistory {
    ScriptHistory::new(WarmBars::default(), syminfo, timeframe.to_string()).with_budget(SCRIPT_CACHE_BYTES)
}

/// AND the signals on the newest closed bar. `None` = no bar to judge.
pub fn verdict(signals: &[&Compiled], s: &Ohlcv, tf: i64, h: Option<&ScriptHistory>) -> Option<Verdict> {
    if s.is_empty() {
        return None;
    }
    let last = s.len() - 1;
    let mut states = Vec::with_capacity(signals.len());
    let mut values = Map::new();
    values.insert("close".into(), json!(s.close[last]));
    for c in signals {
        let held = c.judge(s, tf, h).get(last).copied().unwrap_or(false);
        states.push((c.id, held));
        if let Some(v) = c.reading(s, tf, h) {
            values.insert(c.name.clone(), json!(round(v)));
        }
    }
    let held = !states.is_empty() && states.iter().all(|(_, h)| *h);
    Some(Verdict { bar_ts: s.ts[last], held, states, values: Value::Object(values) })
}

/// Bars on which every signal held, for the "test on item" route.
pub fn held_bars(signals: &[&Compiled], s: &Ohlcv, tf: i64, h: Option<&ScriptHistory>) -> Vec<usize> {
    let per: Vec<Vec<bool>> = signals.iter().map(|c| c.judge(s, tf, h)).collect();
    (0..s.len()).filter(|&i| !per.is_empty() && per.iter().all(|v| v[i])).collect()
}

fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// Whether the cooldown lets an alert hit this item on `bar_ts`, given the bar it last hit.
pub fn cooldown_allows(last_hit_bar: Option<OffsetDateTime>, bar_ts: OffsetDateTime, cooldown_secs: i32) -> bool {
    match last_hit_bar {
        None => true,
        Some(prev) => cooldown_secs <= 0 || (bar_ts - prev).whole_seconds() >= cooldown_secs as i64,
    }
}

/// The board's cache from an item's closed bars.
pub fn cache_of(s: &Ohlcv, tf: i64) -> ItemCache {
    let n = s.len();
    if n == 0 {
        return ItemCache::default();
    }
    let last = n - 1;
    let avg = |k: usize| {
        let from = last.saturating_sub(k);
        (from < last).then(|| s.volume[from..last].iter().sum::<f64>() / (last - from) as f64)
    };
    // The day's open: the first bar of the newest bar's UTC day, when the window reaches it.
    let day_open = if tf >= 86_400 {
        Some(s.open[last])
    } else {
        let day = s.ts[last].date();
        let first = (0..=last).rev().take_while(|&i| s.ts[i].date() == day).last();
        first.and_then(|i| {
            let at_midnight = s.ts[i].time() < ::time::Time::MIDNIGHT + Duration::seconds(tf);
            (i > 0 || at_midnight).then_some(s.open[i])
        })
    };
    ItemCache {
        last_close: Some(s.close[last]),
        last_open: Some(s.open[last]),
        last_volume: Some(s.volume[last]),
        avg_volume: avg(AVG_VOLUME_BARS),
        prev_close: last.checked_sub(1).map(|p| s.close[p]),
        day_open,
        last_bar_ts: Some(s.ts[last]),
        spark: (n.saturating_sub(SPARK_BARS)..n).map(|i| (s.ts[i].unix_timestamp(), s.close[i])).collect(),
    }
}

/// What changed about an alert's ability to run, worth telling the user.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// A new reason it cannot run.
    Failing(String),
    /// It ran cleanly after failing.
    Recovered,
    Quiet,
}

/// Once per distinct error, and once when it clears.
pub fn transition(prev: &str, now: &str) -> Notice {
    match (prev.is_empty(), now.is_empty()) {
        (_, false) if prev != now => Notice::Failing(now.to_string()),
        (false, true) => Notice::Recovered,
        _ => Notice::Quiet,
    }
}

/// One hit, as a notification lists it.
#[derive(Debug, Clone)]
pub struct HitLine {
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    pub connector_id: Option<Uuid>,
    pub bar_ts: OffsetDateTime,
    pub values: Value,
}

impl HitLine {
    /// The chart for this item.
    pub fn chart_url(&self) -> String {
        let mut q = format!(
            "/histviz?provider={}&asset_type={}&ticker={}&tf={}",
            enc(&self.provider),
            enc(&self.asset_type),
            enc(&self.ticker),
            enc(&self.timeframe)
        );
        if let Some(c) = self.connector_id {
            q.push_str(&format!("&connector={c}"));
        }
        q
    }

    fn line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(m) = self.values.as_object() {
            for (k, v) in m {
                if let Some(x) = v.as_f64() {
                    parts.push(format!("{k} {}", fmt(x)));
                }
            }
        }
        format!("{} {} bar of {}: {}", self.ticker, self.timeframe, stamp(self.bar_ts), parts.join(", "))
    }
}

fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A notification ready to write and send.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub title: String,
    pub details: String,
    pub url: String,
    pub link_label: String,
}

/// Notifications for one run of one alert: one digest listing every hit, or one per hit.
pub fn compose(alert_id: Uuid, alert_name: &str, notify: &str, hits: &[HitLine]) -> Vec<Message> {
    if hits.is_empty() {
        return Vec::new();
    }
    let name = if alert_name.trim().is_empty() { "Scanner alert" } else { alert_name.trim() };
    if notify == "per_hit" {
        return hits
            .iter()
            .map(|h| Message {
                title: format!("{name}: {}", h.ticker),
                details: h.line(),
                url: h.chart_url(),
                link_label: h.ticker.clone(),
            })
            .collect();
    }
    let mut lines: Vec<String> = hits.iter().take(DIGEST_LINES).map(HitLine::line).collect();
    if hits.len() > DIGEST_LINES {
        lines.push(format!("and {} more.", hits.len() - DIGEST_LINES));
    }
    let (url, link_label) = if hits.len() == 1 {
        (hits[0].chart_url(), hits[0].ticker.clone())
    } else {
        (format!("/scanner?tab=hits&alert={alert_id}"), "Scanner".to_string())
    };
    let count = if hits.len() == 1 { "1 hit".to_string() } else { format!("{} hits", hits.len()) };
    vec![Message { title: format!("{name}: {count}"), details: lines.join("\n"), url, link_label }]
}

/// The notification for an alert that cannot run, or runs again.
pub fn notice_message(alert_id: Uuid, alert_name: &str, n: &Notice) -> Option<Message> {
    let name = if alert_name.trim().is_empty() { "Scanner alert" } else { alert_name.trim() };
    let url = format!("/scanner?tab=alerts&alert={alert_id}");
    match n {
        Notice::Failing(why) => Some(Message {
            title: format!("{name} cannot scan"),
            details: format!("{why} It will try again on the next bar."),
            url,
            link_label: "Scanner".into(),
        }),
        Notice::Recovered => Some(Message {
            title: format!("{name} scans again"),
            details: "The last run read every item and judged its signals.".into(),
            url,
            link_label: "Scanner".into(),
        }),
        Notice::Quiet => None,
    }
}

fn stamp(ts: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        ts.year(),
        ts.month() as u8,
        ts.day(),
        ts.hour(),
        ts.minute()
    )
}

fn fmt(v: f64) -> String {
    let digits = if v.abs() >= 1000.0 { 2 } else if v.abs() >= 1.0 { 4 } else { 8 };
    let s = format!("{v:.digits$}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One entry per item an alert reads: the widest window any of its alerts needs. Items
/// shared by several alerts are read once.
pub fn read_plan(wants: impl IntoIterator<Item = (Uuid, usize)>) -> HashMap<Uuid, usize> {
    let mut out: HashMap<Uuid, usize> = HashMap::new();
    for (item, want) in wants {
        let e = out.entry(item).or_insert(0);
        *e = (*e).max(want);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::fixtures::{bars, closes};
    use ::time::macros::datetime;

    fn compile(builder: &str, config: Value) -> Compiled {
        let spec = parse(builder, &config).unwrap();
        Compiled::new(Uuid::new_v4(), builder, spec, &Library::default()).unwrap()
    }

    #[test]
    fn scanner_eval_reads_all_four_builders_and_refuses_nonsense() {
        assert!(matches!(parse("event", &json!({"kind": "gap", "pct": 1.0})), Ok(Spec::Event(_))));
        assert!(matches!(parse("pattern", &json!({"kind": "doji"})), Ok(Spec::Pattern(_))));
        assert!(matches!(parse("time", &json!({"at": "09:30"})), Ok(Spec::Time(_))));
        let g = json!({"logic": "all", "conditions": [
            {"left": {"kind": "indicator", "indicator": "sma", "period": 9}, "op": "crosses_above",
             "right": {"kind": "indicator", "indicator": "sma", "period": 21}},
            {"left": {"kind": "indicator", "indicator": "macd_hist"}, "op": "rising"}
        ]});
        assert!(matches!(parse("signal", &g), Ok(Spec::Signal(_))));
        assert!(parse("signal", &json!({"logic": "all", "conditions": []})).unwrap_err().contains("at least one"));
        assert!(parse("signal", &json!({"conditions": [{"left": {"kind": "position", "field": "pnl_pct"}, "op": "rising"}]}))
            .is_err());
        assert!(parse("vibes", &json!({})).is_err());
        assert!(parse("event", &json!({"kind": "calendar", "kind2": 1})).is_err());
    }

    const CROSS: &str = "//@version=6\nindicator(\"Cross\")\nlen = input.int(3, minval = 2)\nplot(ta.sma(close, len), \"fast\")\nplot(ta.sma(close, 20), \"slow\")";

    fn script_lib(source: &str) -> Library {
        let def = backtest::script::ScriptDef { name: "Cross".into(), source: source.into(), inputs: Default::default() };
        Library { scripts: ScriptDefs::from([("s1".to_string(), def)]), ..Library::default() }
    }

    #[test]
    fn scanner_eval_a_deleted_indicator_or_a_bad_script_plot_is_refused() {
        let g = json!({"conditions": [{"left": {"kind": "custom_indicator", "id": "abc"}, "op": "rising"}]});
        let spec = parse("signal", &g).unwrap();
        let why = Compiled::new(Uuid::nil(), "S", spec, &Library::default()).err().unwrap();
        assert!(why.contains("no longer exists"), "{why}");
        let g = json!({"conditions": [{"left": {"kind": "custom_indicator", "id": "s1", "plot": "nope"}, "op": "rising"}]});
        let why = Compiled::new(Uuid::nil(), "S", parse("signal", &g).unwrap(), &script_lib(CROSS)).err().unwrap();
        assert!(why.contains("no plot"), "{why}");
        let repaint = "//@version=6\nindicator(\"T\")\nplot(barstate.islast ? close : na, \"x\")";
        let g = json!({"conditions": [{"left": {"kind": "custom_indicator", "id": "s1", "plot": "x"}, "op": "rising"}]});
        assert!(Compiled::new(Uuid::nil(), "S", parse("signal", &g).unwrap(), &script_lib(repaint)).is_err());
    }

    #[test]
    fn scanner_eval_two_script_plots_crossing_hold_like_the_same_averages_built_in() {
        let px: Vec<f64> = (0..80).map(|i| 100.0 + (i as f64 * 0.25).sin() * 5.0).collect();
        let s = closes(&px);
        let plot = |p: &str| json!({"kind": "custom_indicator", "id": "s1", "plot": p});
        let g = json!({"conditions": [{"left": plot("fast"), "op": "crosses_above", "right": plot("slow")}]});
        let script = Compiled::new(Uuid::nil(), "S", parse("signal", &g).unwrap(), &script_lib(CROSS)).unwrap();
        assert!(script.reads_script());
        assert_eq!(script.lookback(3600), SCRIPT_BARS);
        let sma = |n: usize| json!({"kind": "indicator", "indicator": "sma", "period": n});
        let builtin = compile("signal", json!({"conditions": [{"left": sma(3), "op": "crosses_above", "right": sma(20)}]}));
        let syminfo = otw_script::input::SymInfo { ticker: "X".into(), tickerid: "T:X".into(), ..Default::default() };
        let h = script_history(syminfo, "1h");
        let want = held_bars(&[&builtin], &s, 3600, None);
        assert!(!want.is_empty());
        assert_eq!(held_bars(&[&script], &s, 3600, Some(&h)), want);
        // Without a history the script reads nothing, so nothing holds.
        assert!(held_bars(&[&script], &s, 3600, None).is_empty());
    }

    #[test]
    fn scanner_eval_signals_are_anded_on_the_same_bar() {
        // A rising close and a doji: both on the last bar only when the last bar is a doji
        // that closed higher.
        let rising = compile(
            "signal",
            json!({"conditions": [{"left": {"kind": "price", "field": "close"}, "op": "rising"}]}),
        );
        let doji = compile("pattern", json!({"kind": "doji"}));
        let s = bars(&[(10.0, 10.5, 9.5, 10.0), (10.0, 11.0, 9.0, 10.05)]);
        let v = verdict(&[&rising, &doji], &s, 3600, None).unwrap();
        assert!(v.held);
        assert_eq!(v.states.len(), 2);
        assert_eq!(v.values["close"], json!(10.05));
        // Not a doji: one of two is not enough.
        let s = bars(&[(10.0, 10.5, 9.5, 10.0), (10.0, 11.0, 9.0, 10.9)]);
        let v = verdict(&[&rising, &doji], &s, 3600, None).unwrap();
        assert!(!v.held);
        assert_eq!(v.states.iter().filter(|(_, h)| *h).count(), 1);
    }

    #[test]
    fn scanner_eval_sma_cross_and_macd_rising_reads_like_the_backtest() {
        let px: Vec<f64> = (0..80).map(|i| 100.0 + (i as f64 * 0.2).sin() * 5.0 + i as f64 * 0.05).collect();
        let s = closes(&px);
        let c = compile(
            "signal",
            json!({"logic": "all", "conditions": [
                {"left": {"kind": "indicator", "indicator": "sma", "period": 9}, "op": "crosses_above",
                 "right": {"kind": "indicator", "indicator": "sma", "period": 21}}
            ]}),
        );
        let held = c.judge(&s, 3600, None);
        assert!(held.iter().any(|h| *h), "an oscillating series crosses its averages");
        assert!(c.lookback(3600) >= 21);
        assert!(bars_wanted(&[&c], 3600) >= 21 + EXTRA_BARS);
    }

    #[test]
    fn scanner_eval_held_bars_lists_where_every_signal_held() {
        let gap = compile("event", json!({"kind": "gap", "pct": 2.0, "direction": "up"}));
        let s = bars(&[(100.0, 101.0, 99.0, 100.0), (103.0, 104.0, 102.0, 103.5), (103.4, 104.0, 103.0, 103.8)]);
        assert_eq!(held_bars(&[&gap], &s, 3600, None), vec![1]);
    }

    #[test]
    fn scanner_eval_time_only_alert_is_a_schedule() {
        let t = compile("time", json!({"at": "02:30", "timezone": "UTC"}));
        let s = closes(&[1.0, 2.0, 3.0, 4.0]);
        // Hourly from 00:00: the 02:00 bar holds 02:30.
        assert_eq!(held_bars(&[&t], &s, 3600, None), vec![2]);
    }

    #[test]
    fn scanner_eval_cooldown_silences_repeats_per_item() {
        let t = datetime!(2026-09-07 10:00 UTC);
        assert!(cooldown_allows(None, t, 3600));
        assert!(!cooldown_allows(Some(t), t + Duration::minutes(59), 3600));
        assert!(cooldown_allows(Some(t), t + Duration::minutes(60), 3600));
        assert!(cooldown_allows(Some(t), t + Duration::minutes(1), 0));
    }

    #[test]
    fn scanner_eval_error_notified_once_then_recovery() {
        assert_eq!(transition("", "refused key"), Notice::Failing("refused key".into()));
        // The same error again is not news.
        assert_eq!(transition("refused key", "refused key"), Notice::Quiet);
        // A different one is.
        assert_eq!(transition("refused key", "quota"), Notice::Failing("quota".into()));
        assert_eq!(transition("quota", ""), Notice::Recovered);
        assert_eq!(transition("", ""), Notice::Quiet);
        let m = notice_message(Uuid::nil(), "Breakouts", &Notice::Failing("No connector.".into())).unwrap();
        assert!(m.title.contains("cannot scan") && m.details.starts_with("No connector."));
        assert!(notice_message(Uuid::nil(), "", &Notice::Recovered).unwrap().title.contains("scans again"));
    }

    fn line(t: &str) -> HitLine {
        HitLine {
            provider: "binance".into(),
            asset_type: "crypto".into(),
            ticker: t.into(),
            timeframe: "1h".into(),
            connector_id: None,
            bar_ts: datetime!(2026-09-07 10:00 UTC),
            values: json!({"close": 1.5}),
        }
    }

    #[test]
    fn scanner_eval_digest_vs_per_hit() {
        let hits: Vec<HitLine> = ["A", "B", "C"].iter().map(|t| line(t)).collect();
        let d = compose(Uuid::nil(), "Movers", "digest", &hits);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].title, "Movers: 3 hits");
        assert_eq!(d[0].details.lines().count(), 3);
        assert!(d[0].url.starts_with("/scanner?tab=hits"));
        let p = compose(Uuid::nil(), "Movers", "per_hit", &hits);
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].title, "Movers: B");
        assert_eq!(p[1].url, "/histviz?provider=binance&asset_type=crypto&ticker=B&tf=1h");
        // One hit in a digest links straight to its chart.
        assert!(compose(Uuid::nil(), "", "digest", &hits[..1])[0].url.starts_with("/histviz?"));
        let many: Vec<HitLine> = (0..25).map(|i| line(&format!("T{i}"))).collect();
        let d = compose(Uuid::nil(), "M", "digest", &many);
        assert!(d[0].details.ends_with("and 5 more."));
        assert!(compose(Uuid::nil(), "M", "digest", &[]).is_empty());
    }

    #[test]
    fn scanner_eval_chart_link_escapes_the_ticker() {
        let mut h = line("BTC/USD");
        h.connector_id = Some(Uuid::nil());
        let u = h.chart_url();
        assert!(u.contains("ticker=BTC%2FUSD"), "{u}");
        assert!(u.ends_with("&connector=00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn scanner_eval_cache_of_the_last_bars() {
        let s = closes(&[10.0, 11.0, 12.0]);
        let c = cache_of(&s, 3600);
        assert_eq!(c.last_close, Some(12.0));
        assert_eq!(c.prev_close, Some(11.0));
        // Hourly from midnight: the day's open is the first bar's.
        assert_eq!(c.day_open, Some(10.0));
        assert_eq!(c.avg_volume, Some(100.0));
        let closes: Vec<f64> = c.spark.iter().map(|p| p.1).collect();
        assert_eq!(closes, vec![10.0, 11.0, 12.0]);
        assert_eq!(c.spark[1].0 - c.spark[0].0, 3600);
        assert_eq!(cache_of(&Ohlcv::default(), 3600), ItemCache::default());
    }

    #[test]
    fn scanner_eval_items_shared_by_alerts_are_read_once_at_the_widest_window() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let plan = read_plan([(a, 10), (b, 30), (a, 50)]);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[&a], 50);
        assert_eq!(plan[&b], 30);
    }
}
