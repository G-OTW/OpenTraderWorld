//! `color.*`. Pine transparency is 0..100; a color keeps an alpha byte
//! `a = round(255 * (100 - transp) / 100)`.

use super::{Native, NativeFn};
use crate::output::Rgba;
use crate::vm::value::Value;

pub fn alpha(transp: f64) -> u8 {
    libm::round(255.0 * (100.0 - transp.clamp(0.0, 100.0)) / 100.0) as u8
}

pub fn transp(a: u8) -> f64 {
    libm::round((255.0 - a as f64) * 100.0 / 255.0)
}

fn hex(c: u32) -> Value {
    Value::Color(Rgba { r: (c >> 16) as u8, g: (c >> 8) as u8, b: c as u8, a: 255 })
}

/// The v6 Reference Manual's named colors.
pub fn constant(name: &str) -> Option<Value> {
    Some(hex(match name {
        "color.aqua" => 0x00BCD4,
        "color.black" => 0x363A45,
        "color.blue" => 0x2196F3,
        "color.fuchsia" => 0xE040FB,
        "color.gray" => 0x787B86,
        "color.green" => 0x4CAF50,
        "color.lime" => 0x00E676,
        "color.maroon" => 0x880E4F,
        "color.navy" => 0x311B92,
        "color.olive" => 0x808000,
        "color.orange" => 0xFF9800,
        "color.purple" => 0x9C27B0,
        "color.red" => 0xFF5252,
        "color.silver" => 0xB2B5BE,
        "color.teal" => 0x00897B,
        "color.white" => 0xFFFFFF,
        "color.yellow" => 0xFFEB3B,
        _ => return None,
    }))
}

fn channel(v: &Value) -> Option<u8> {
    v.num().map(|x| libm::round(x.clamp(0.0, 255.0)) as u8)
}

fn lerp(a: u8, b: u8, t: f64) -> u8 {
    libm::round(a as f64 + (b as f64 - a as f64) * t) as u8
}

pub fn native(name: &str) -> Option<Native> {
    let s = |f: NativeFn| Some(Native { f, init: None });
    match name {
        "color.new" => s(|_, a, _| {
            Ok(match (&a[0], a[1].num()) {
                (Value::Color(c), Some(t)) => Value::Color(Rgba { a: alpha(t), ..*c }),
                _ => Value::Na,
            })
        }),
        "color.rgb" => s(|_, a, _| {
            let (Some(r), Some(g), Some(b)) = (channel(&a[0]), channel(&a[1]), channel(&a[2])) else { return Ok(Value::Na) };
            Ok(Value::Color(Rgba { r, g, b, a: alpha(a[3].num().unwrap_or(0.0)) }))
        }),
        "color.from_gradient" => s(|_, a, _| {
            let (Some(v), Some(lo), Some(hi)) = (a[0].num(), a[1].num(), a[2].num()) else { return Ok(Value::Na) };
            let (Value::Color(c0), Value::Color(c1)) = (&a[3], &a[4]) else { return Ok(Value::Na) };
            let t = if hi == lo { 0.0 } else { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) };
            Ok(Value::Color(Rgba { r: lerp(c0.r, c1.r, t), g: lerp(c0.g, c1.g, t), b: lerp(c0.b, c1.b, t), a: lerp(c0.a, c1.a, t) }))
        }),
        "color.r" => s(|_, a, _| Ok(if let Value::Color(c) = &a[0] { Value::Float(c.r as f64) } else { Value::Na })),
        "color.g" => s(|_, a, _| Ok(if let Value::Color(c) = &a[0] { Value::Float(c.g as f64) } else { Value::Na })),
        "color.b" => s(|_, a, _| Ok(if let Value::Color(c) = &a[0] { Value::Float(c.b as f64) } else { Value::Na })),
        "color.t" => s(|_, a, _| Ok(if let Value::Color(c) = &a[0] { Value::Float(transp(c.a)) } else { Value::Na })),
        _ => None,
    }
}
