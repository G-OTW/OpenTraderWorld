//! Native implementations of the registry's builtins: bar variables, constants, functions
//! (stateless or with per-call-site state) and outputs.

use std::any::Any;

use crate::diag::RtCode;
use crate::output::Rgba;
use crate::vm::value::Value;
use crate::vm::{BarMode, Ctx};

pub mod array;
pub mod draw;
pub mod color;
pub mod input;
pub mod math;
pub mod plot;
pub mod strs;
pub mod ta;
mod ta_more;
pub mod time;

use crate::input::TfUnit;
use time::Part;

/// Per-call-site state of a stateful builtin (`ta.sma`'s window…). Cloneable, so a session
/// can snapshot the VM after the last confirmed bar and roll the forming bar back to it.
/// Reached as `&mut dyn Any` by upcasting (`state_mut`), one indirect call per downcast.
pub trait StateObj: Any {
    fn clone_box(&self) -> Box<dyn StateObj>;
}

// `Box<dyn StateObj>` is deliberately not `Clone`: it would fall under this impl, and a
// method call on a `&mut State` would then reach the box instead of the state inside it.
impl<T: Any + Clone> StateObj for T {
    fn clone_box(&self) -> Box<dyn StateObj> {
        Box::new(self.clone())
    }
}

/// The state of a call site as its concrete type.
pub fn state_mut<T: Any>(s: Option<&mut State>) -> &mut T {
    let any: &mut dyn Any = &mut **s.expect("state");
    any.downcast_mut::<T>().expect("state type")
}

pub type State = Box<dyn StateObj>;
pub type NResult = Result<Value, (RtCode, Vec<String>)>;
pub type NativeFn = for<'r> fn(&mut Ctx<'r>, &[Value], Option<&mut State>) -> NResult;

#[derive(Clone, Copy)]
pub struct Native {
    pub f: NativeFn,
    pub init: Option<fn() -> State>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutKind {
    Plot,
    PlotShape,
    PlotChar,
    PlotArrow,
    PlotCandle,
    PlotBar,
    HLine,
    Fill,
    BgColor,
    BarColor,
}

impl OutKind {
    pub const GROUPS: usize = 7;

    /// Output vector a call lands in: shapes (plotshape, plotchar, plotarrow) share one,
    /// candles (plotcandle, plotbar) another, in source order.
    pub fn group(self) -> usize {
        match self {
            OutKind::Plot => 0,
            OutKind::HLine => 1,
            OutKind::Fill => 2,
            OutKind::BgColor => 3,
            OutKind::BarColor => 4,
            OutKind::PlotShape | OutKind::PlotChar | OutKind::PlotArrow => 5,
            OutKind::PlotCandle | OutKind::PlotBar => 6,
        }
    }
}

pub fn output(name: &str) -> Option<OutKind> {
    Some(match name {
        "plot" => OutKind::Plot,
        "plotshape" => OutKind::PlotShape,
        "plotchar" => OutKind::PlotChar,
        "plotarrow" => OutKind::PlotArrow,
        "plotcandle" => OutKind::PlotCandle,
        "plotbar" => OutKind::PlotBar,
        "hline" => OutKind::HLine,
        "fill" => OutKind::Fill,
        "bgcolor" => OutKind::BgColor,
        "barcolor" => OutKind::BarColor,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymField {
    Ticker,
    TickerId,
    Prefix,
    Root,
    Mintick,
    PointValue,
    Timezone,
    Session,
    Type,
    Currency,
    BaseCurrency,
    Description,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TfField {
    Period,
    Multiplier,
    IsSeconds,
    IsMinutes,
    IsIntraday,
    IsDaily,
    IsWeekly,
    IsMonthly,
    IsDwm,
}

/// Variables read from the bars and the run (history `x[k]` reads them at bar - k).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BVar {
    Open,
    High,
    Low,
    Close,
    Volume,
    Hl2,
    Hlc3,
    Ohlc4,
    Hlcc4,
    BarIndex,
    LastBarIndex,
    LastBarTime,
    Time,
    TimeClose,
    TimeNow,
    TimeTradingDay,
    Part(Part),
    IsFirst,
    IsLast,
    IsHistory,
    IsRealtime,
    IsNew,
    IsConfirmed,
    IsLastConfirmedHistory,
    Sym(SymField),
    Tf(TfField),
    /// `ta.tr` used as a variable (`ta.tr(false)`); history reads are not supported.
    Tr,
}

pub fn bar_var(name: &str) -> Option<BVar> {
    use BVar::*;
    Some(match name {
        "open" => Open,
        "high" => High,
        "low" => Low,
        "close" => Close,
        "volume" => Volume,
        "hl2" => Hl2,
        "hlc3" => Hlc3,
        "ohlc4" => Ohlc4,
        "hlcc4" => Hlcc4,
        "bar_index" => BarIndex,
        "last_bar_index" => LastBarIndex,
        "last_bar_time" => LastBarTime,
        "time" => Time,
        "time_close" => TimeClose,
        "timenow" => TimeNow,
        "time_tradingday" => TimeTradingDay,
        "year" => Part(time::Part::Year),
        "month" => Part(time::Part::Month),
        "weekofyear" => Part(time::Part::WeekOfYear),
        "dayofmonth" => Part(time::Part::DayOfMonth),
        "dayofweek" => Part(time::Part::DayOfWeek),
        "hour" => Part(time::Part::Hour),
        "minute" => Part(time::Part::Minute),
        "second" => Part(time::Part::Second),
        "barstate.isfirst" => IsFirst,
        "barstate.islast" => IsLast,
        "barstate.ishistory" => IsHistory,
        "barstate.isrealtime" => IsRealtime,
        "barstate.isnew" => IsNew,
        "barstate.isconfirmed" => IsConfirmed,
        "barstate.islastconfirmedhistory" => IsLastConfirmedHistory,
        "syminfo.ticker" => Sym(SymField::Ticker),
        "syminfo.tickerid" => Sym(SymField::TickerId),
        "syminfo.prefix" => Sym(SymField::Prefix),
        "syminfo.root" => Sym(SymField::Root),
        "syminfo.mintick" => Sym(SymField::Mintick),
        "syminfo.pointvalue" => Sym(SymField::PointValue),
        "syminfo.timezone" => Sym(SymField::Timezone),
        "syminfo.session" => Sym(SymField::Session),
        "syminfo.type" => Sym(SymField::Type),
        "syminfo.currency" => Sym(SymField::Currency),
        "syminfo.basecurrency" => Sym(SymField::BaseCurrency),
        "syminfo.description" => Sym(SymField::Description),
        "timeframe.period" | "timeframe.main_period" => Tf(TfField::Period),
        "timeframe.multiplier" => Tf(TfField::Multiplier),
        "timeframe.isseconds" => Tf(TfField::IsSeconds),
        "timeframe.isminutes" => Tf(TfField::IsMinutes),
        "timeframe.isintraday" => Tf(TfField::IsIntraday),
        "timeframe.isdaily" => Tf(TfField::IsDaily),
        "timeframe.isweekly" => Tf(TfField::IsWeekly),
        "timeframe.ismonthly" => Tf(TfField::IsMonthly),
        "timeframe.isdwm" => Tf(TfField::IsDwm),
        "ta.tr" => Tr,
        _ => return None,
    })
}

/// Volume of bar `i`; an empty volume column means the host has none.
pub fn volume_at(ctx: &Ctx, i: usize) -> Result<f64, (RtCode, Vec<String>)> {
    let v = &ctx.input.bars.volume;
    if v.is_empty() && !ctx.input.bars.is_empty() {
        return Err((RtCode::VolumeUnknown, Vec::new()));
    }
    Ok(v.get(i).copied().unwrap_or(f64::NAN))
}

fn unknown(field: &str) -> (RtCode, Vec<String>) {
    (RtCode::SyminfoUnknown, vec![field.to_string()])
}

fn sym_str(v: &Option<String>, field: &str) -> NResult {
    v.as_deref().map(Value::str).ok_or_else(|| unknown(field))
}

/// Value of a bar variable `k` bars back from the current bar.
pub fn bar_value(ctx: &Ctx, v: BVar, k: usize) -> NResult {
    if k > ctx.bar {
        return Ok(Value::Na);
    }
    let i = ctx.bar - k;
    let b = &ctx.input.bars;
    let n = b.len();
    let f = |x: &[f64]| x.get(i).copied().unwrap_or(f64::NAN);
    let t = b.time.get(i).copied();
    let s = &ctx.input.syminfo;
    let tf = ctx.input.timeframe;
    Ok(match v {
        BVar::Open => Value::float(f(&b.open)),
        BVar::High => Value::float(f(&b.high)),
        BVar::Low => Value::float(f(&b.low)),
        BVar::Close => Value::float(f(&b.close)),
        BVar::Volume => Value::float(volume_at(ctx, i)?),
        BVar::Hl2 => Value::float((f(&b.high) + f(&b.low)) / 2.0),
        BVar::Hlc3 => Value::float((f(&b.high) + f(&b.low) + f(&b.close)) / 3.0),
        BVar::Ohlc4 => Value::float((f(&b.open) + f(&b.high) + f(&b.low) + f(&b.close)) / 4.0),
        BVar::Hlcc4 => Value::float((f(&b.high) + f(&b.low) + 2.0 * f(&b.close)) / 4.0),
        BVar::BarIndex => Value::Int(i as i64),
        BVar::LastBarIndex => Value::Int(n as i64 - 1),
        BVar::LastBarTime => b.time.last().map(|t| Value::Int(*t)).unwrap_or(Value::Na),
        BVar::Time => t.map(Value::Int).unwrap_or(Value::Na),
        BVar::TimeClose => b.time_close.get(i).map(|t| Value::Int(*t)).unwrap_or(Value::Na),
        // No wall clock inside the engine (determinism): the close of the last bar.
        BVar::TimeNow => b.time_close.last().map(|t| Value::Int(*t)).unwrap_or(Value::Na),
        BVar::TimeTradingDay => {
            let z = time::exchange_zone(ctx)?;
            let day = crate::input::Timeframe { multiplier: 1, unit: TfUnit::Day };
            t.and_then(|t| time::period_start(t, day, z)).map(Value::Int).unwrap_or(Value::Na)
        }
        BVar::Part(p) => {
            let z = time::exchange_zone(ctx)?;
            t.map(|t| time::part(t, z, p)).unwrap_or(Value::Na)
        }
        BVar::IsFirst => Value::Bool(i == 0),
        BVar::IsLast => Value::Bool(i + 1 == n),
        BVar::IsLastConfirmedHistory => Value::Bool(ctx.mode == BarMode::History && ctx.last_history == Some(i)),
        BVar::IsHistory => Value::Bool(ctx.mode == BarMode::History),
        BVar::IsRealtime => Value::Bool(ctx.mode != BarMode::History),
        BVar::IsNew => Value::Bool(match ctx.mode {
            BarMode::History => true,
            BarMode::Forming { first } => first,
            BarMode::Commit { was_forming } => !was_forming,
        }),
        BVar::IsConfirmed => Value::Bool(!matches!(ctx.mode, BarMode::Forming { .. })),
        BVar::Sym(field) => match field {
            SymField::Ticker => Value::str(&s.ticker),
            SymField::TickerId => Value::str(&s.tickerid),
            SymField::Prefix => Value::str(s.tickerid.split_once(':').map(|(p, _)| p).unwrap_or("")),
            SymField::Root => Value::str(&s.ticker),
            SymField::Mintick => s.mintick.map(Value::float).ok_or_else(|| unknown("mintick"))?,
            SymField::PointValue => s.pointvalue.map(Value::float).ok_or_else(|| unknown("pointvalue"))?,
            SymField::Timezone => sym_str(&s.timezone, "timezone")?,
            SymField::Session => sym_str(&s.session, "session")?,
            SymField::Type => sym_str(&s.kind, "type")?,
            SymField::Currency => sym_str(&s.currency, "currency")?,
            SymField::BaseCurrency => sym_str(&s.basecurrency, "basecurrency")?,
            SymField::Description => sym_str(&s.description, "description")?,
        },
        BVar::Tr if k == 0 => ta::tr_value(ctx),
        BVar::Tr => Value::Na,
        BVar::Tf(field) => match field {
            TfField::Period => Value::str(&time::period_str(tf)),
            TfField::Multiplier => Value::Int(tf.multiplier as i64),
            TfField::IsSeconds => Value::Bool(tf.unit == TfUnit::Second),
            TfField::IsMinutes => Value::Bool(tf.unit == TfUnit::Minute),
            TfField::IsIntraday => Value::Bool(matches!(tf.unit, TfUnit::Second | TfUnit::Minute)),
            TfField::IsDaily => Value::Bool(tf.unit == TfUnit::Day),
            TfField::IsWeekly => Value::Bool(tf.unit == TfUnit::Week),
            TfField::IsMonthly => Value::Bool(tf.unit == TfUnit::Month),
            TfField::IsDwm => Value::Bool(matches!(tf.unit, TfUnit::Day | TfUnit::Week | TfUnit::Month)),
        },
    })
}

/// `#RRGGBBAA` literal → color.
pub fn rgba_from_u32(c: u32) -> Rgba {
    Rgba { r: (c >> 24) as u8, g: (c >> 16) as u8, b: (c >> 8) as u8, a: c as u8 }
}

/// Named constants (`color.red`, `plot.style_line`, `math.pi`…).
pub fn constant(name: &str) -> Option<Value> {
    if let Some(v) = color::constant(name).or_else(|| math::constant(name)) {
        return Some(v);
    }
    let (ns, member) = name.split_once('.')?;
    if ns == "dayofweek" {
        let days = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
        return days.iter().position(|d| *d == member).map(|i| Value::Int(i as i64 + 1));
    }
    if ns == "display" {
        // Bit flags, so `display.all - display.status_line` works.
        return Some(Value::Int(match member {
            "none" => 0,
            "data_window" => 1,
            "pane" => 2,
            "price_scale" => 4,
            "status_line" => 8,
            _ => 15,
        }));
    }
    if matches!(ns, "label" | "line") && member.starts_with("style_") {
        return Some(Value::str(member));
    }
    match ns {
        "plot" | "hline" | "shape" | "location" | "size" | "format" | "scale" | "position" | "text" | "xloc" | "yloc" | "extend" | "font" | "order" => {
            Some(Value::str(member))
        }
        _ => None,
    }
}

/// Builtin variables computed with per-site state (`ta.obv`), compiled like a call.
pub fn state_var(name: &str) -> Option<Native> {
    match name {
        "ta.obv" => Some(ta_more::obv()),
        // `label.all` and the like: the live drawings, read each time.
        "label.all" | "line.all" | "box.all" | "table.all" | "linefill.all" | "polyline.all" => draw::native(name),
        _ => None,
    }
}

/// Stateless and stateful functions by name.
pub fn native(name: &str) -> Option<Native> {
    math::native(name)
        .or_else(|| color::native(name))
        .or_else(|| input::native(name))
        .or_else(|| time::native(name))
        .or_else(|| ta::native(name))
        .or_else(|| array::native(name))
        .or_else(|| strs::native(name))
        .or_else(|| draw::native(name))
}
