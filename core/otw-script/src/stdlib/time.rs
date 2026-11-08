//! Time, sessions and timeframes. Calendar values are read in the exchange timezone
//! (`syminfo.timezone`), never the host's; an unknown timezone refuses the script.

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};
use time_tz::{timezones, OffsetDateTimeExt, PrimitiveDateTimeExt, Tz};

use super::{Native, NativeFn};
use crate::diag::RtCode;
use crate::input::{TfUnit, Timeframe};
use crate::vm::value::Value;
use crate::vm::Ctx;

type Fail = (RtCode, Vec<String>);

#[derive(Clone, Copy)]
pub enum Zone {
    Named(&'static Tz),
    Fixed(UtcOffset),
}

/// `"UTC"`, `"GMT+2"`, `"UTC-5:30"`, or an IANA name.
pub fn zone(name: &str) -> Option<Zone> {
    let n = name.trim();
    for prefix in ["UTC", "GMT"] {
        if let Some(rest) = n.strip_prefix(prefix) {
            if rest.is_empty() {
                return Some(Zone::Fixed(UtcOffset::UTC));
            }
            let (sign, rest) = match rest.as_bytes()[0] {
                b'+' => (1, &rest[1..]),
                b'-' => (-1, &rest[1..]),
                _ => break,
            };
            let (h, m) = rest.split_once(':').unwrap_or((rest, "0"));
            let (h, m): (i8, i8) = (h.parse().ok()?, m.parse().ok()?);
            return UtcOffset::from_hms(sign * h, sign * m, 0).ok().map(Zone::Fixed);
        }
    }
    timezones::get_by_name(n).map(Zone::Named)
}

/// The exchange timezone, or the refusal naming the missing field.
pub fn exchange_zone(ctx: &Ctx) -> Result<Zone, Fail> {
    ctx.input.syminfo.timezone.as_deref().and_then(zone).ok_or((RtCode::SyminfoUnknown, vec!["timezone".into()]))
}

/// Timezone argument: `""` = exchange.
pub(super) fn zone_arg(ctx: &Ctx, v: &Value) -> Result<Zone, Fail> {
    match v {
        Value::Str(s) if !s.is_empty() => zone(s).ok_or((RtCode::SyminfoUnknown, vec!["timezone".into()])),
        _ => exchange_zone(ctx),
    }
}

pub fn local(ms: i64, z: Zone) -> Option<OffsetDateTime> {
    let utc = OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000).ok()?;
    Some(match z {
        Zone::Named(tz) => utc.to_timezone(tz),
        Zone::Fixed(off) => utc.to_offset(off),
    })
}

fn to_utc_ms(p: PrimitiveDateTime, z: Zone) -> Option<i64> {
    let odt = match z {
        Zone::Named(tz) => p.assume_timezone(tz).take_first()?,
        Zone::Fixed(off) => p.assume_offset(off),
    };
    Some((odt.unix_timestamp_nanos() / 1_000_000) as i64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Year,
    Month,
    WeekOfYear,
    DayOfMonth,
    DayOfWeek,
    Hour,
    Minute,
    Second,
}

pub fn part(ms: i64, z: Zone, p: Part) -> Value {
    let Some(d) = local(ms, z) else { return Value::Na };
    Value::Int(match p {
        Part::Year => d.year() as i64,
        Part::Month => d.month() as u8 as i64,
        Part::WeekOfYear => d.iso_week() as i64,
        Part::DayOfMonth => d.day() as i64,
        Part::DayOfWeek => d.weekday().number_from_sunday() as i64,
        Part::Hour => d.hour() as i64,
        Part::Minute => d.minute() as i64,
        Part::Second => d.second() as i64,
    })
}

/// `"60"`, `"D"`, `"2D"`, `"W"`, `"3M"`, `"30S"`; `""` = the chart's.
pub fn parse_tf(s: &str, chart: Timeframe) -> Option<Timeframe> {
    let s = s.trim();
    if s.is_empty() {
        return Some(chart);
    }
    let (num, unit) = match s.chars().last()? {
        'S' => (&s[..s.len() - 1], TfUnit::Second),
        'D' => (&s[..s.len() - 1], TfUnit::Day),
        'W' => (&s[..s.len() - 1], TfUnit::Week),
        'M' => (&s[..s.len() - 1], TfUnit::Month),
        c if c.is_ascii_digit() => (s, TfUnit::Minute),
        _ => return None,
    };
    let multiplier = if num.is_empty() { 1 } else { num.parse().ok()? };
    Some(Timeframe { multiplier, unit })
}

pub fn period_str(tf: Timeframe) -> String {
    let m = tf.multiplier;
    let n = |suffix: &str| if m == 1 { suffix.to_string() } else { format!("{m}{suffix}") };
    match tf.unit {
        TfUnit::Second => format!("{m}S"),
        TfUnit::Minute => m.to_string(),
        TfUnit::Day => n("D"),
        TfUnit::Week => n("W"),
        TfUnit::Month => n("M"),
    }
}

pub fn in_seconds(tf: Timeframe) -> i64 {
    tf.multiplier as i64
        * match tf.unit {
            TfUnit::Second => 1,
            TfUnit::Minute => 60,
            TfUnit::Day => 86_400,
            TfUnit::Week => 604_800,
            TfUnit::Month => 2_628_003,
        }
}

/// Start (epoch ms) of the `tf` period holding `ms`, in zone `z`.
pub fn period_start(ms: i64, tf: Timeframe, z: Zone) -> Option<i64> {
    let d = local(ms, z)?;
    let n = tf.multiplier.max(1) as i64;
    let offset_s = d.offset().whole_seconds() as i64;
    let local_s = ms.div_euclid(1000) + offset_s;
    let start_local_s = match tf.unit {
        TfUnit::Second => local_s - local_s.rem_euclid(n),
        TfUnit::Minute => local_s - local_s.rem_euclid(60 * n),
        TfUnit::Day => {
            let day = local_s.div_euclid(86_400);
            (day - day.rem_euclid(n)) * 86_400
        }
        TfUnit::Week => {
            // 1970-01-01 is a Thursday: weeks start on Monday.
            let day = local_s.div_euclid(86_400);
            let week = (day + 3).div_euclid(7);
            ((week - week.rem_euclid(n)) * 7 - 3) * 86_400
        }
        TfUnit::Month => {
            let idx = d.year() as i64 * 12 + d.month() as u8 as i64 - 1;
            let idx = idx - idx.rem_euclid(n);
            let date = Date::from_calendar_date((idx / 12) as i32, Month::try_from((idx % 12 + 1) as u8).ok()?, 1).ok()?;
            return to_utc_ms(PrimitiveDateTime::new(date, Time::MIDNIGHT), z);
        }
    };
    Some((start_local_s - offset_s) * 1000)
}

/// Is the local time of `ms` inside a session string (`"0930-1600"`, `"2200-0200:23456"`,
/// several comma separated)? `"24x7"` and empty sessions are always open.
pub fn in_session(ms: i64, session: &str, z: Zone) -> Option<bool> {
    let s = session.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("24x7") {
        return Some(true);
    }
    let d = local(ms, z)?;
    let minute = d.hour() as i32 * 60 + d.minute() as i32;
    let weekday = d.weekday().number_from_sunday();
    for part in s.split(',') {
        let (range, days) = part.split_once(':').unwrap_or((part, "1234567"));
        let (a, b) = range.split_once('-')?;
        let hm = |x: &str| -> Option<i32> { Some(x.get(0..2)?.parse::<i32>().ok()? * 60 + x.get(2..4)?.parse::<i32>().ok()?) };
        let (start, end) = (hm(a.trim())?, hm(b.trim())?);
        let open = if start < end { minute >= start && minute < end } else { minute >= start || minute < end };
        if open && days.contains(char::from(b'0' + weekday)) {
            return Some(true);
        }
    }
    Some(false)
}

fn str_arg(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    }
}

fn bar_time(ctx: &Ctx, k: i64) -> Option<i64> {
    let i = (ctx.bar as i64).checked_sub(k.max(0))?;
    ctx.input.bars.time.get(usize::try_from(i).ok()?).copied()
}

/// `time(tf, session, tz, bars_back)` and `time_close(…)`.
fn time_fn(ctx: &mut Ctx, a: &[Value], close: bool) -> Result<Value, Fail> {
    let Some(t) = bar_time(ctx, a[3].int().unwrap_or(0)) else { return Ok(Value::Na) };
    let z = zone_arg(ctx, &a[2])?;
    let Some(tf) = parse_tf(&str_arg(&a[0]), ctx.input.timeframe) else { return Ok(Value::Na) };
    let session = str_arg(&a[1]);
    if in_session(t, &session, z) != Some(true) {
        return Ok(Value::Na);
    }
    let chart = str_arg(&a[0]).is_empty() || tf == ctx.input.timeframe;
    let v = match (chart, close) {
        (true, false) => Some(t),
        (true, true) => ctx.input.bars.time_close.get(ctx.bar).copied(),
        (false, false) => period_start(t, tf, z),
        (false, true) => period_start(t, tf, z).and_then(|s| {
            // The next period's start: probe 1.1 periods ahead (months and DST days vary).
            period_start(s + in_seconds(tf) * 1100, tf, z)
        }),
    };
    Ok(v.map(Value::Int).unwrap_or(Value::Na))
}

fn part_fn(ctx: &mut Ctx, a: &[Value], p: Part) -> Result<Value, Fail> {
    let Some(t) = a[0].int() else { return Ok(Value::Na) };
    let z = zone_arg(ctx, &a[1])?;
    Ok(part(t, z, p))
}

fn timestamp(ctx: &mut Ctx, a: &[Value]) -> Result<Value, Fail> {
    let (z, nums) = match &a[0] {
        Value::Str(s) => (zone(s).ok_or((RtCode::SyminfoUnknown, vec!["timezone".into()]))?, &a[1..]),
        _ => (exchange_zone(ctx)?, a),
    };
    let n = |i: usize| nums.get(i).and_then(Value::int).unwrap_or(0);
    let (Ok(month), true) = (Month::try_from(n(1) as u8), nums.len() >= 3) else { return Ok(Value::Na) };
    let Ok(date) = Date::from_calendar_date(n(0) as i32, month, n(2) as u8) else { return Ok(Value::Na) };
    let Ok(time) = Time::from_hms(n(3) as u8, n(4) as u8, n(5) as u8) else { return Ok(Value::Na) };
    Ok(to_utc_ms(PrimitiveDateTime::new(date, time), z).map(Value::Int).unwrap_or(Value::Na))
}

pub fn native(name: &str) -> Option<Native> {
    let s = |f: NativeFn| Some(Native { f, init: None });
    match name {
        "time" => s(|c, a, _| time_fn(c, a, false)),
        "time_close" => s(|c, a, _| time_fn(c, a, true)),
        "year" => s(|c, a, _| part_fn(c, a, Part::Year)),
        "month" => s(|c, a, _| part_fn(c, a, Part::Month)),
        "weekofyear" => s(|c, a, _| part_fn(c, a, Part::WeekOfYear)),
        "dayofmonth" => s(|c, a, _| part_fn(c, a, Part::DayOfMonth)),
        "dayofweek" => s(|c, a, _| part_fn(c, a, Part::DayOfWeek)),
        "hour" => s(|c, a, _| part_fn(c, a, Part::Hour)),
        "minute" => s(|c, a, _| part_fn(c, a, Part::Minute)),
        "second" => s(|c, a, _| part_fn(c, a, Part::Second)),
        "timestamp" => s(|c, a, _| timestamp(c, a)),
        "timeframe.in_seconds" => s(|c, a, _| {
            Ok(match parse_tf(&str_arg(&a[0]), c.input.timeframe) {
                Some(tf) => Value::Int(in_seconds(tf)),
                None => Value::Na,
            })
        }),
        "timeframe.change" => s(|c, a, _| {
            let z = exchange_zone(c)?;
            let Some(tf) = parse_tf(&str_arg(&a[0]), c.input.timeframe) else { return Ok(Value::Na) };
            let (Some(now), prev) = (bar_time(c, 0), bar_time(c, 1)) else { return Ok(Value::Bool(false)) };
            Ok(Value::Bool(match prev {
                None => true,
                Some(p) => period_start(now, tf, z) != period_start(p, tf, z),
            }))
        }),
        _ => None,
    }
}
