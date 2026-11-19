//! Output calls: they write into the run's `RunOutput`, one slot per bar. Arguments arrive in
//! registry parameter order (`na` for an optional parameter not given and without default).

use super::OutKind;
use crate::output::{BarColorOut, CandleOut, FillOut, FillRef, HLineOut, PlotOut, Rgba, RunOutput, ShapeOut};
use crate::vm::value::Value;
use crate::vm::Ctx;

fn number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        other => other.num().filter(|x| x.is_finite()),
    }
}

fn color(v: Option<&Value>) -> Option<Rgba> {
    match v? {
        Value::Color(c) => Some(*c),
        _ => None,
    }
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn fill_ref(v: Option<&Value>) -> FillRef {
    match v {
        Some(Value::HLineRef(i)) => FillRef::HLine(*i),
        Some(Value::PlotRef(i)) => FillRef::Plot(*i),
        _ => FillRef::Plot(usize::MAX),
    }
}

/// Fresh outputs for `counts[group]` calls of each output group over `n` bars.
pub fn empty(counts: &[usize; OutKind::GROUPS], n: usize) -> RunOutput {
    let c = |k: OutKind| counts[k.group()];
    let series = || vec![None; n];
    let colors = || vec![None; n];
    RunOutput {
        plots: (0..c(OutKind::Plot))
            .map(|index| PlotOut { index, values: series(), colors: colors(), linewidth: 1, offset: 0, histbase: 0.0, hidden: false })
            .collect(),
        hlines: (0..c(OutKind::HLine)).map(|_| HLineOut { price: 0.0, title: String::new(), color: None, style: String::new(), linewidth: 1 }).collect(),
        fills: (0..c(OutKind::Fill)).map(|_| FillOut { a: FillRef::Plot(0), b: FillRef::Plot(0), colors: colors() }).collect(),
        bgcolor: (0..c(OutKind::BgColor)).map(|_| BarColorOut { colors: colors() }).collect(),
        barcolor: (0..c(OutKind::BarColor)).map(|_| BarColorOut { colors: colors() }).collect(),
        shapes: (0..c(OutKind::PlotShape))
            .map(|_| ShapeOut { kind: String::new(), when: series(), style: String::new(), location: String::new(), color: colors(), text: String::new() })
            .collect(),
        candles: (0..c(OutKind::PlotCandle))
            .map(|_| CandleOut { kind: String::new(), o: series(), h: series(), l: series(), c: series(), colors: colors() })
            .collect(),
        drawings: Default::default(),
    }
}

/// Does `record` read parameter `pi` of this output? Others are never evaluated.
pub fn reads(kind: OutKind, pi: usize) -> bool {
    match kind {
        OutKind::Plot => matches!(pi, 0 | 2 | 3 | 6 | 7 | 12),
        OutKind::HLine => pi <= 4,
        OutKind::Fill => pi <= 2,
        OutKind::BgColor | OutKind::BarColor => pi == 0,
        OutKind::PlotShape | OutKind::PlotChar => matches!(pi, 0 | 2 | 3 | 4 | 6),
        OutKind::PlotArrow => pi <= 3,
        OutKind::PlotCandle | OutKind::PlotBar => pi <= 3 || pi == 5,
    }
}

/// Record one output call on the current bar.
pub fn record(ctx: &mut Ctx, kind: OutKind, index: usize, args: &[Value]) {
    let bar = ctx.bar;
    let a = |i: usize| args.get(i).filter(|v| !v.is_na());
    let out = &mut ctx.out;
    match kind {
        OutKind::Plot => {
            let Some(p) = out.plots.get_mut(index) else { return };
            p.values[bar] = number(a(0));
            p.colors[bar] = color(a(2));
            p.linewidth = a(3).and_then(Value::int).unwrap_or(1).clamp(1, 255) as u8;
            p.histbase = number(a(6)).unwrap_or(0.0);
            p.offset = a(7).and_then(Value::int).unwrap_or(0) as i32;
            p.hidden = matches!(a(12), Some(Value::Int(0)));
        }
        OutKind::HLine => {
            let Some(h) = out.hlines.get_mut(index) else { return };
            h.price = number(a(0)).unwrap_or(f64::NAN);
            h.title = text(a(1));
            h.color = color(a(2));
            h.style = text(a(3));
            h.linewidth = a(4).and_then(Value::int).unwrap_or(1).clamp(1, 255) as u8;
        }
        OutKind::Fill => {
            let Some(f) = out.fills.get_mut(index) else { return };
            f.a = fill_ref(a(0));
            f.b = fill_ref(a(1));
            f.colors[bar] = color(a(2));
        }
        OutKind::BgColor => {
            if let Some(b) = out.bgcolor.get_mut(index) {
                b.colors[bar] = color(a(0));
            }
        }
        OutKind::BarColor => {
            if let Some(b) = out.barcolor.get_mut(index) {
                b.colors[bar] = color(a(0));
            }
        }
        OutKind::PlotShape | OutKind::PlotChar | OutKind::PlotArrow => {
            let Some(s) = out.shapes.get_mut(index) else { return };
            s.when[bar] = number(a(0));
            match kind {
                OutKind::PlotShape => {
                    s.kind = "plotshape".into();
                    s.style = text(a(2));
                    s.location = text(a(3));
                    s.color[bar] = color(a(4));
                    s.text = text(a(6));
                }
                OutKind::PlotChar => {
                    s.kind = "plotchar".into();
                    s.style = text(a(2));
                    s.location = text(a(3));
                    s.color[bar] = color(a(4));
                    s.text = text(a(6));
                }
                _ => {
                    s.kind = "plotarrow".into();
                    let up = number(a(0)).is_some_and(|x| x >= 0.0);
                    s.color[bar] = color(if up { a(2) } else { a(3) });
                }
            }
        }
        OutKind::PlotCandle | OutKind::PlotBar => {
            let Some(c) = out.candles.get_mut(index) else { return };
            c.kind = if kind == OutKind::PlotCandle { "plotcandle" } else { "plotbar" }.into();
            c.o[bar] = number(a(0));
            c.h[bar] = number(a(1));
            c.l[bar] = number(a(2));
            c.c[bar] = number(a(3));
            c.colors[bar] = color(a(5));
        }
    }
}
