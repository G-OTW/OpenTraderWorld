//! `str.*`. Number formats follow the Java patterns Pine exposes: `#.##`-style
//! `DecimalFormat` for `str.tostring`, `MessageFormat` placeholders for `str.format`,
//! `SimpleDateFormat` letters for `str.format_time`; the subset scripts use, the rest is
//! printed as written. Positions count characters.

use super::{array, time, Native, NativeFn, NResult};
use crate::diag::RtCode;
use crate::vm::heap::Elem;
use crate::vm::value::Value;
use crate::vm::Ctx;

fn text(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s),
        _ => None,
    }
}

/// A `DecimalFormat` pattern: `#,##0.00`, `#.##`, `0.0%`, with a literal prefix and suffix.
pub fn decimal(x: f64, pattern: &str) -> String {
    if !x.is_finite() {
        return "NaN".into();
    }
    let pattern = pattern.split(';').next().unwrap_or(pattern);
    let is_num = |c: char| matches!(c, '#' | '0' | ',' | '.');
    let (Some(start), Some(end)) = (pattern.find(is_num), pattern.rfind(is_num)) else { return pattern.to_string() };
    let (prefix, body, suffix) = (&pattern[..start], &pattern[start..=end], &pattern[end + 1..]);
    let x = if suffix.contains('%') || prefix.contains('%') { x * 100.0 } else { x };
    let (int_p, frac_p) = body.split_once('.').unwrap_or((body, ""));
    let min_frac = frac_p.chars().filter(|c| *c == '0').count();
    let max_frac = frac_p.chars().filter(|c| matches!(c, '0' | '#')).count();
    let min_int = int_p.chars().filter(|c| *c == '0').count();
    let group = int_p.rfind(',').map(|i| int_p[i + 1..].chars().filter(|c| matches!(c, '#' | '0')).count()).filter(|g| *g > 0);
    let s = format!("{:.*}", max_frac, x.abs());
    let (ip, fp) = s.split_once('.').unwrap_or((&s, ""));
    let mut fp = fp.to_string();
    while fp.len() > min_frac && fp.ends_with('0') {
        fp.pop();
    }
    let mut ip = ip.trim_start_matches('0').to_string();
    while ip.len() < min_int {
        ip.insert(0, '0');
    }
    if let Some(g) = group {
        let digits: Vec<char> = ip.chars().collect();
        let mut out = String::new();
        for (i, c) in digits.iter().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(g) {
                out.push(',');
            }
            out.push(*c);
        }
        ip = out;
    }
    let num = if fp.is_empty() { ip } else { format!("{ip}.{fp}") };
    let num = if num.is_empty() || num.starts_with('.') { format!("0{num}") } else { num };
    let neg = x < 0.0 && num.chars().any(|c| c.is_ascii_digit() && c != '0');
    format!("{}{prefix}{num}{}", if neg { "-" } else { "" }, suffix.replace('\'', ""))
}

/// `str.tostring` of a number under a `format.*` constant or a pattern.
fn number(ctx: &Ctx, x: f64, format: &str) -> Result<String, (RtCode, Vec<String>)> {
    Ok(match format {
        "mintick" => {
            let tick = ctx.input.syminfo.mintick.ok_or((RtCode::SyminfoUnknown, vec!["mintick".into()]))?;
            let decimals = format!("{tick}").split_once('.').map(|(_, f)| f.len()).unwrap_or(0);
            format!("{:.*}", decimals, (x / tick).round() * tick)
        }
        "percent" => format!("{}%", decimal(x, "#.##")),
        "volume" => {
            let (v, unit) = match x.abs() {
                a if a >= 1e9 => (x / 1e9, "B"),
                a if a >= 1e6 => (x / 1e6, "M"),
                a if a >= 1e3 => (x / 1e3, "K"),
                _ => (x, ""),
            };
            format!("{}{unit}", decimal(v, "#.###"))
        }
        "price" | "inherit" | "" => decimal(x, "#.##########"),
        p => decimal(x, p),
    })
}

/// `str.tostring(value, format)`.
pub fn tostring(ctx: &Ctx, v: &Value, format: &str) -> NResult {
    Ok(Value::str(&match v {
        Value::Na => "NaN".into(),
        Value::Int(i) if format.is_empty() => i.to_string(),
        Value::Int(i) => number(ctx, *i as f64, format)?,
        Value::Float(f) => number(ctx, *f, format)?,
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.to_string(),
        Value::Ref(id) => match ctx.heap.array(*id) {
            Some(a) => {
                let items: Result<Vec<String>, _> = a.items.iter().map(|x| tostring(ctx, x, format).map(|v| text(&v).unwrap_or_default().to_string())).collect();
                format!("[{}]", items?.join(", "))
            }
            None => "NaN".into(),
        },
        _ => String::new(),
    }))
}

/// `MessageFormat`: `{0}`, `{1,number,#.##}`, `{2,number,integer}`, quotes with `'`.
fn message(ctx: &Ctx, pattern: &str, args: &[Value]) -> Result<String, (RtCode, Vec<String>)> {
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                if chars.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    out.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '{' => {
                let Some(close) = chars[i..].iter().position(|c| *c == '}') else {
                    out.extend(&chars[i..]);
                    break;
                };
                let spec: String = chars[i + 1..i + close].iter().collect();
                let mut parts = spec.splitn(3, ',').map(str::trim);
                let idx: Option<usize> = parts.next().and_then(|p| p.parse().ok());
                let (kind, style) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let v = idx.and_then(|k| args.get(k)).cloned().unwrap_or(Value::Na);
                out.push_str(&match (&v, kind) {
                    (Value::Str(s), _) => s.to_string(),
                    (Value::Bool(b), _) => b.to_string(),
                    (Value::Na, _) => "NaN".into(),
                    (v, "number") if v.num().is_some() => {
                        let x = v.num().unwrap_or(f64::NAN);
                        match style {
                            "" => decimal(x, "#,##0.###"),
                            "integer" => decimal(x, "#,##0"),
                            "percent" => decimal(x, "#,##0%"),
                            "currency" => decimal(x, "$#,##0.00"),
                            p => decimal(x, p),
                        }
                    }
                    (v, _) if v.num().is_some() => decimal(v.num().unwrap_or(f64::NAN), "#,##0.###"),
                    (v, _) => text(&tostring(ctx, v, "")?).unwrap_or_default().to_string(),
                });
                i += close + 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// `SimpleDateFormat` letters on a local date-time.
fn format_time(d: ::time::OffsetDateTime, pattern: &str) -> String {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                continue;
            }
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let n = chars[i..].iter().take_while(|x| **x == c).count();
        let pad = |v: i64, w: usize| format!("{v:0w$}");
        let month = d.month() as u8 as usize;
        out.push_str(&match c {
            'y' if n == 2 => pad(d.year() as i64 % 100, 2),
            'y' => pad(d.year() as i64, n),
            'M' if n >= 4 => MONTHS[month - 1].to_string(),
            'M' if n == 3 => MONTHS[month - 1][..3].to_string(),
            'M' => pad(month as i64, n),
            'd' => pad(d.day() as i64, n),
            'H' => pad(d.hour() as i64, n),
            'h' => pad(((d.hour() as i64 + 11) % 12) + 1, n),
            'm' => pad(d.minute() as i64, n),
            's' => pad(d.second() as i64, n),
            'S' => pad(d.millisecond() as i64, n),
            'a' => (if d.hour() < 12 { "AM" } else { "PM" }).to_string(),
            'E' if n >= 4 => DAYS[d.weekday().number_days_from_monday() as usize].to_string(),
            'E' => DAYS[d.weekday().number_days_from_monday() as usize][..3].to_string(),
            'Z' | 'z' | 'X' => {
                let off = d.offset().whole_minutes();
                format!("{}{:02}{:02}", if off < 0 { '-' } else { '+' }, off.abs() / 60, off.abs() % 60)
            }
            _ => chars[i..i + n].iter().collect(),
        });
        i += n;
    }
    out
}

pub fn native(name: &str) -> Option<Native> {
    let s = |f: NativeFn| Some(Native { f, init: None });
    match name {
        "str.tostring" => s(|ctx, a, _| tostring(ctx, &a[0], text(&a[1]).unwrap_or(""))),
        "log.info" | "log.warning" | "log.error" => s(|_, _, _| Ok(Value::Na)),
        "str.tonumber" => s(|_, a, _| Ok(text(&a[0]).and_then(|t| t.trim().parse::<f64>().ok()).map(Value::float).unwrap_or(Value::Na))),
        "str.length" => s(|_, a, _| Ok(text(&a[0]).map(|t| Value::Int(t.chars().count() as i64)).unwrap_or(Value::Na))),
        "str.contains" => s(|_, a, _| Ok(Value::Bool(matches!((text(&a[0]), text(&a[1])), (Some(x), Some(y)) if x.contains(y))))),
        "str.startswith" => s(|_, a, _| Ok(Value::Bool(matches!((text(&a[0]), text(&a[1])), (Some(x), Some(y)) if x.starts_with(y))))),
        "str.endswith" => s(|_, a, _| Ok(Value::Bool(matches!((text(&a[0]), text(&a[1])), (Some(x), Some(y)) if x.ends_with(y))))),
        "str.pos" => s(|_, a, _| {
            let (Some(x), Some(y)) = (text(&a[0]), text(&a[1])) else { return Ok(Value::Na) };
            Ok(x.find(y).map(|b| Value::Int(x[..b].chars().count() as i64)).unwrap_or(Value::Na))
        }),
        "str.substring" => s(|_, a, _| {
            let Some(x) = text(&a[0]) else { return Ok(Value::Na) };
            let n = x.chars().count() as i64;
            let from = a[1].int().unwrap_or(0).clamp(0, n) as usize;
            let to = a[2].int().unwrap_or(n).clamp(0, n) as usize;
            Ok(Value::str(&x.chars().skip(from).take(to.saturating_sub(from)).collect::<String>()))
        }),
        "str.replace" => s(|ctx, a, _| {
            let (Some(x), Some(t), Some(r)) = (text(&a[0]), text(&a[1]), text(&a[2])) else { return Ok(a[0].clone()) };
            let k = a[3].int().unwrap_or(0).max(0) as usize;
            if t.is_empty() {
                return Ok(a[0].clone());
            }
            let out = match x.match_indices(t).nth(k) {
                Some((b, _)) => format!("{}{r}{}", &x[..b], &x[b + t.len()..]),
                None => x.to_string(),
            };
            if out.len() > ctx.input.limits.max_string_len {
                return Err((RtCode::StringTooLong, vec![out.len().to_string()]));
            }
            Ok(Value::str(&out))
        }),
        "str.replace_all" => s(|ctx, a, _| {
            let (Some(x), Some(t), Some(r)) = (text(&a[0]), text(&a[1]), text(&a[2])) else { return Ok(a[0].clone()) };
            if t.is_empty() {
                return Ok(a[0].clone());
            }
            // Sized before it is built: many matches times a long replacement is large.
            let hits = x.matches(t).count();
            let len = (x.len() - hits * t.len()).saturating_add(hits.saturating_mul(r.len()));
            if len > ctx.input.limits.max_string_len {
                return Err((RtCode::StringTooLong, vec![len.to_string()]));
            }
            Ok(Value::str(&x.replace(t, r)))
        }),
        "str.lower" => s(|_, a, _| Ok(text(&a[0]).map(|t| Value::str(&t.to_lowercase())).unwrap_or(Value::Na))),
        "str.upper" => s(|_, a, _| Ok(text(&a[0]).map(|t| Value::str(&t.to_uppercase())).unwrap_or(Value::Na))),
        "str.trim" => s(|_, a, _| Ok(text(&a[0]).map(|t| Value::str(t.trim())).unwrap_or(Value::Na))),
        "str.repeat" => s(|ctx, a, _| {
            let Some(x) = text(&a[0]) else { return Ok(Value::Na) };
            let n = a[1].int().unwrap_or(0).max(0) as usize;
            let sep = text(&a[2]).unwrap_or("");
            let len = (x.len() + sep.len()).saturating_mul(n);
            if len > ctx.input.limits.max_string_len {
                return Err((RtCode::StringTooLong, vec![len.to_string()]));
            }
            Ok(Value::str(&vec![x; n].join(sep)))
        }),
        "str.split" => s(|ctx, a, _| {
            let (Some(x), Some(sep)) = (text(&a[0]), text(&a[1])) else { return array::alloc(ctx, Elem::Str, Vec::new()) };
            let items = if sep.is_empty() { x.chars().map(|c| Value::str(&c.to_string())).collect() } else { x.split(sep).map(Value::str).collect() };
            array::alloc(ctx, Elem::Str, items)
        }),
        "str.format" => s(|ctx, a, _| {
            let Some(p) = text(&a[0]) else { return Ok(Value::Na) };
            let out = message(ctx, p, &a[1..])?;
            if out.len() > ctx.input.limits.max_string_len {
                return Err((RtCode::StringTooLong, vec![out.len().to_string()]));
            }
            Ok(Value::str(&out))
        }),
        "str.format_time" => s(|ctx, a, _| {
            let Some(t) = a[0].int() else { return Ok(Value::Na) };
            let z = time::zone_arg(ctx, &a[2])?;
            let Some(d) = time::local(t, z) else { return Ok(Value::Na) };
            Ok(Value::str(&format_time(d, text(&a[1]).unwrap_or("yyyy-MM-dd'T'HH:mm:ssZ"))))
        }),
        _ => None,
    }
}
