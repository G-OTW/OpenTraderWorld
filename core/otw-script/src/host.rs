//! Host glue shared by the WASM build and native hosts: JSON in, JSON out, bar columns.
//! Both builds go through these functions, so a parity check compares the glue as well.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime};

use crate::diag::{RtCode, RuntimeError, Span};
use crate::input::{Bars, InputValue, Limits, RunInput, SymInfo, TfUnit, Timeframe};
use crate::output::{InputKind, InputMeta, Rgba};
use crate::session::Session;
use crate::Program;
use std::rc::Rc;

/// Bars a script runs before the first bar a host shows or trades (spec §4.2): it starts at
/// the instrument's first stored bar, or this many bars before that first bar.
pub const FIRST_BAR_WINDOW: usize = 5_000;

/// Price series an `input.source` may name.
const SOURCES: [&str; 9] = ["open", "high", "low", "close", "volume", "hl2", "hlc3", "ohlc4", "hlcc4"];

/// An input value refused: `reason` is `"type"`, `"range"` or `"option"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    pub key: String,
    pub reason: &'static str,
}

/// Raw values (form values: numbers, bools, strings, `#rrggbb[aa]` colors) keyed by
/// `InputMeta::key` to `InputValue`s. The rules of `frontend/src/lib/script/host.js`
/// `inputValues`, pinned by the shared `tests/fixtures/input_values.json`. Keys absent from
/// `raw` are left out (the script's default applies); a refused value is reported, never
/// clamped.
pub fn input_values(meta: &[InputMeta], raw: &serde_json::Map<String, serde_json::Value>) -> (BTreeMap<String, InputValue>, Vec<InputError>) {
    let mut values = BTreeMap::new();
    let mut errors = Vec::new();
    for m in meta {
        let Some(r) = raw.get(&m.key) else { continue };
        match convert(m, r) {
            Ok(v) => {
                values.insert(m.key.clone(), v);
            }
            Err(reason) => errors.push(InputError { key: m.key.clone(), reason }),
        }
    }
    (values, errors)
}

fn convert(m: &InputMeta, raw: &serde_json::Value) -> Result<InputValue, &'static str> {
    use serde_json::Value as J;
    let number = || -> Option<f64> {
        match raw {
            J::Number(n) => n.as_f64(),
            J::String(s) if !s.trim().is_empty() => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    };
    let in_range = |x: f64| m.minval.is_none_or(|lo| x >= lo) && m.maxval.is_none_or(|hi| x <= hi);
    match m.kind {
        InputKind::Int => {
            let x = number().filter(|x| x.is_finite() && x.fract() == 0.0).ok_or("type")?;
            if in_range(x) { Ok(InputValue::Int(x as i64)) } else { Err("range") }
        }
        InputKind::Float => {
            let x = number().filter(|x| x.is_finite()).ok_or("type")?;
            if in_range(x) { Ok(InputValue::Float(x)) } else { Err("range") }
        }
        InputKind::Bool => raw.as_bool().map(InputValue::Bool).ok_or("type"),
        InputKind::Source => match raw.as_str() {
            Some(s) if SOURCES.contains(&s) => Ok(InputValue::Source(s.to_string())),
            _ => Err("option"),
        },
        InputKind::Color => raw.as_str().and_then(hex_color).map(InputValue::Color).ok_or("type"),
        InputKind::Str | InputKind::Session => {
            let s = raw.as_str().ok_or("type")?;
            if !m.options.is_empty() && !m.options.iter().any(|o| o == s) {
                return Err("option");
            }
            Ok(InputValue::Str(s.to_string()))
        }
    }
}

/// `#rrggbb` or `#rrggbbaa`.
fn hex_color(s: &str) -> Option<Rgba> {
    let h = s.trim().strip_prefix('#')?;
    if !(h.len() == 6 || h.len() == 8) || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: if h.len() == 8 { byte(6)? } else { 255 } })
}

/// Bar columns as hosts hold them: times in epoch ms as f64 (what JS has), prices.
/// An empty `volume` means the host has none.
#[derive(Debug, Clone, Copy)]
pub struct BarCols<'a> {
    pub time: &'a [f64],
    pub open: &'a [f64],
    pub high: &'a [f64],
    pub low: &'a [f64],
    pub close: &'a [f64],
    pub volume: &'a [f64],
}

/// Everything but the bars: instrument facts, OTW timeframe string, input values by key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCtx {
    pub syminfo: SymInfo,
    pub timeframe: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputValue>,
}

/// Close time of each bar: open + the timeframe, calendar months for `M` (UTC).
pub fn close_times(time: &[i64], tf: Timeframe) -> Vec<i64> {
    let m = tf.multiplier as i64;
    time.iter()
        .map(|&t| match tf.unit {
            TfUnit::Second => t + m * 1000,
            TfUnit::Minute => t + m * 60_000,
            TfUnit::Day => t + m * 86_400_000,
            TfUnit::Week => t + m * 604_800_000,
            TfUnit::Month => add_months(t, m).unwrap_or(t),
        })
        .collect()
}

fn add_months(ms: i64, months: i64) -> Option<i64> {
    let d = OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000).ok()?;
    let idx = d.year() as i64 * 12 + d.month() as u8 as i64 - 1 + months;
    let month = Month::try_from((idx.rem_euclid(12) + 1) as u8).ok()?;
    let date = Date::from_calendar_date(idx.div_euclid(12) as i32, month, d.day().min(month.length(idx.div_euclid(12) as i32))).ok()?;
    let out = PrimitiveDateTime::new(date, d.time()).assume_utc();
    Some((out.unix_timestamp_nanos() / 1_000_000) as i64)
}

/// `{"ok":true,"meta":…,"warnings":[…]}` or `{"ok":false,"diagnostics":[…]}`.
pub fn compile_json(source: &str) -> String {
    match crate::compile(source) {
        Ok(p) => json!({ "ok": true, "meta": p.meta(), "warnings": p.warnings() }),
        Err(d) => json!({ "ok": false, "diagnostics": d }),
    }
    .to_string()
}

/// The run's context and bars, or the JSON answer refusing them.
fn prepare(cols: BarCols, ctx_json: &str) -> Result<(RunCtx, Timeframe, Bars), String> {
    let ctx: RunCtx = serde_json::from_str(ctx_json).map_err(|e| json!({ "ok": false, "context": e.to_string() }).to_string())?;
    let Some(timeframe) = Timeframe::parse(&ctx.timeframe) else {
        return Err(json!({ "ok": false, "context": format!("unknown timeframe {}", ctx.timeframe) }).to_string());
    };
    let bars = bars_of(cols, timeframe)?;
    Ok((ctx, timeframe, bars))
}

fn bars_of(cols: BarCols, timeframe: Timeframe) -> Result<Bars, String> {
    if cols.time.iter().any(|t| !t.is_finite()) {
        return Err(json!({ "ok": false, "error": RuntimeError { code: RtCode::BadBars, span: Span::default(), bar: 0, args: vec!["time".into()] } }).to_string());
    }
    let time: Vec<i64> = cols.time.iter().map(|t| *t as i64).collect();
    Ok(Bars {
        time_close: close_times(&time, timeframe),
        time,
        open: cols.open.to_vec(),
        high: cols.high.to_vec(),
        low: cols.low.to_vec(),
        close: cols.close.to_vec(),
        volume: cols.volume.to_vec(),
    })
}

/// `{"ok":true,"output":…}`, `{"ok":false,"error":…}` or `{"ok":false,"context":"…"}`.
pub fn run_json(program: &Program, cols: BarCols, ctx_json: &str, limits: Limits) -> String {
    let (ctx, timeframe, bars) = match prepare(cols, ctx_json) {
        Ok(v) => v,
        Err(answer) => return answer,
    };
    let input = RunInput { bars, syminfo: ctx.syminfo, timeframe, inputs: ctx.inputs, limits };
    match program.run(&input) {
        Ok(output) => json!({ "ok": true, "output": output }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
    .to_string()
}

/// A live script (spec §4.2) behind the same JSON as `run_json`: the chart opens it once,
/// then sends the bars from the last committed one on each update.
pub struct HostSession {
    session: Session,
    timeframe: Timeframe,
}

impl HostSession {
    /// Compile and run every bar, the last as the forming one when `forming`. `Err` is the
    /// answer to show: the compile JSON with its diagnostics, or a run refusal.
    pub fn open(source: &str, cols: BarCols, ctx_json: &str, forming: bool, limits: Limits) -> Result<HostSession, String> {
        let program = crate::compile(source).map_err(|_| compile_json(source))?;
        let (ctx, timeframe, bars) = prepare(cols, ctx_json)?;
        let input = RunInput { bars, syminfo: ctx.syminfo, timeframe, inputs: ctx.inputs, limits };
        let session = Session::open(Rc::new(program), input, forming).map_err(|error| json!({ "ok": false, "error": error }).to_string())?;
        Ok(HostSession { session, timeframe })
    }

    /// Run `cols` (the bars from `committed()` on). Answers like `output_json(committed())`
    /// before the call, or `{"ok":false,"error":…}` with the session unchanged.
    pub fn advance(&mut self, cols: BarCols, forming: bool) -> String {
        let from = self.session.committed();
        let tail = match bars_of(cols, self.timeframe) {
            Ok(b) => b,
            Err(answer) => return answer,
        };
        match self.session.advance(tail, forming) {
            Ok(()) => self.output_json(from),
            Err(error) => json!({ "ok": false, "error": error }).to_string(),
        }
    }

    /// `{"ok":true,"from":…,"committed":…,"output":…}`, per-bar columns from bar `from` on.
    pub fn output_json(&self, from: usize) -> String {
        json!({ "ok": true, "from": from, "committed": self.session.committed(), "output": self.session.output_from(from) }).to_string()
    }

    pub fn committed(&self) -> usize {
        self.session.committed()
    }
}
