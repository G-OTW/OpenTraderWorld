//! Drawing objects (phase 5): `label.*`, `line.*`, `box.*`, `table.*`, `linefill.*`,
//! `polyline.*` and `chart.point.*`. A drawing lives on the heap until deleted or pushed out by
//! its kind's count limit (`indicator(max_labels_count = …)`, Pine's default 50); the run's
//! output is the state of every live drawing after the last bar. A setter on a deleted drawing
//! does nothing and a getter gives `na`, as in Pine.

use super::{array, Native, NativeFn, NResult};
use crate::diag::RtCode;
use crate::output::{BoxOut, CellOut, LabelOut, LineOut, PolylineOut, Rgba, TableOut};
use crate::vm::heap::{Drawing, Elem, Obj, Object, MAX_OBJECTS};
use crate::vm::value::Value;
use crate::vm::Ctx;

type E = (RtCode, Vec<String>);

/// Type index of `chart.point` in every program (`compile` puts it first).
pub const CHART_POINT_TY: usize = 0;
/// `chart.point` fields, in layout order.
pub const CHART_POINT_FIELDS: [&str; 3] = ["time", "index", "price"];

fn full() -> E {
    (RtCode::MemoryLimit, vec![MAX_OBJECTS.to_string()])
}

fn s(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    }
}

/// An enum argument's member without Pine's `style_` prefix (`label_down`, `dashed`).
fn member(v: &Value) -> String {
    let t = s(v);
    t.strip_prefix("style_").map(str::to_string).unwrap_or(t)
}

fn color(v: &Value) -> Option<Rgba> {
    match v {
        Value::Color(c) => Some(*c),
        _ => None,
    }
}

fn width(v: &Value) -> u32 {
    v.int().unwrap_or(1).clamp(0, 1000) as u32
}

fn idx(v: &Value) -> usize {
    v.int().unwrap_or(0).max(0) as usize
}

fn id(v: &Value) -> Option<u32> {
    match v {
        Value::Ref(i) => Some(*i),
        _ => None,
    }
}

/// The current bar's index (or open time for `xloc.bar_time`) when `x` is `na`.
fn x_of(ctx: &Ctx, v: &Value, xloc: &str) -> i64 {
    v.int().unwrap_or_else(|| if xloc == "bar_time" { ctx.input.bars.time[ctx.bar] } else { ctx.bar as i64 })
}

/// `(time, index, price)` of a `chart.point`.
fn point(ctx: &Ctx, v: &Value) -> Option<(Option<i64>, Option<i64>, Option<f64>)> {
    let o = ctx.heap.object(id(v)?)?;
    (o.ty == CHART_POINT_TY).then(|| (o.fields[0].int(), o.fields[1].int(), o.fields[2].num()))
}

/// A point's x for `xloc`, and its price.
fn point_xy(ctx: &Ctx, v: &Value, xloc: &str) -> (Value, Value) {
    match point(ctx, v) {
        Some((t, i, p)) => {
            let x = if xloc == "bar_time" { t } else { i };
            (x.map(Value::Int).unwrap_or(Value::Na), p.map(Value::float).unwrap_or(Value::Na))
        }
        None => (Value::Na, Value::Na),
    }
}

fn limit(ctx: &Ctx, kind: usize) -> usize {
    match kind {
        0 => ctx.draw_limits[0],
        1 => ctx.draw_limits[1],
        2 => ctx.draw_limits[2],
        5 => ctx.draw_limits[3],
        _ => usize::MAX,
    }
}

fn add(ctx: &mut Ctx, d: Drawing) -> NResult {
    let l = limit(ctx, d.kind());
    ctx.heap.draw(d, l).ok_or_else(full)
}

fn label(ctx: &Ctx, a: &[Value]) -> LabelOut {
    let xloc = s(&a[3]);
    LabelOut {
        x: x_of(ctx, &a[0], &xloc),
        y: a[1].num(),
        xloc,
        yloc: s(&a[4]),
        text: s(&a[2]),
        color: color(&a[5]),
        style: member(&a[6]),
        textcolor: color(&a[7]),
        size: s(&a[8]),
        textalign: s(&a[9]),
        tooltip: s(&a[10]),
        font: s(&a[11]),
    }
}

fn line(ctx: &Ctx, a: &[Value]) -> LineOut {
    let xloc = s(&a[4]);
    LineOut {
        x1: x_of(ctx, &a[0], &xloc),
        y1: a[1].num(),
        x2: x_of(ctx, &a[2], &xloc),
        y2: a[3].num(),
        xloc,
        extend: s(&a[5]),
        color: color(&a[6]),
        style: member(&a[7]),
        width: width(&a[8]),
    }
}

fn boxd(ctx: &Ctx, a: &[Value]) -> BoxOut {
    let xloc = s(&a[8]);
    BoxOut {
        left: x_of(ctx, &a[0], &xloc),
        top: a[1].num(),
        right: x_of(ctx, &a[2], &xloc),
        bottom: a[3].num(),
        xloc,
        extend: s(&a[7]),
        border_color: color(&a[4]),
        border_width: width(&a[5]),
        border_style: member(&a[6]),
        bgcolor: color(&a[9]),
        text: s(&a[10]),
        text_size: s(&a[11]),
        text_color: color(&a[12]),
        text_halign: s(&a[13]),
        text_valign: s(&a[14]),
        text_wrap: s(&a[15]),
        font: s(&a[16]),
    }
}

/// `[x, y]` of every point argument (first `n` arguments), then the rest: the point form of a
/// constructor as its coordinate form. `xloc_at` = index of `xloc` in the point form.
fn from_points(ctx: &Ctx, a: &[Value], n: usize, xloc_at: usize) -> Vec<Value> {
    let xloc = s(&a[xloc_at]);
    let mut out = Vec::with_capacity(a.len() + n);
    for p in &a[..n] {
        let (x, y) = point_xy(ctx, p, &xloc);
        out.push(x);
        out.push(y);
    }
    out.extend_from_slice(&a[n..]);
    out
}

/// Run `f` on drawing `v` when it is one of the right kind; nothing otherwise.
fn edit(ctx: &mut Ctx, v: &Value, f: impl FnOnce(&mut Drawing)) -> NResult {
    if let Some(d) = id(v).and_then(|i| ctx.heap.drawing_mut(i)) {
        f(d);
    }
    Ok(Value::Na)
}

fn get(ctx: &Ctx, v: &Value, f: impl FnOnce(&Drawing) -> Option<Value>) -> NResult {
    Ok(id(v).and_then(|i| ctx.heap.drawing(i)).and_then(f).unwrap_or(Value::Na))
}

fn copy(ctx: &mut Ctx, v: &Value) -> NResult {
    match id(v).and_then(|i| ctx.heap.drawing(i)).cloned() {
        Some(d) => add(ctx, d),
        None => Ok(Value::Na),
    }
}

/// Every live drawing of kind `k`, oldest first, as a new array.
fn all(ctx: &mut Ctx, k: usize) -> NResult {
    let items = ctx.heap.drawings_of(k).map(Value::Ref).collect();
    array::alloc(ctx, Elem::Any, items)
}

fn opt_f(v: Option<f64>) -> Value {
    v.map(Value::float).unwrap_or(Value::Na)
}

macro_rules! set {
    ($kind:ident, |$d:ident, $a:ident| $body:expr) => {
        |ctx, $a, _| {
            let a2 = $a.to_vec();
            edit(ctx, &$a[0], move |d| {
                if let Drawing::$kind($d) = d {
                    let $a = &a2;
                    $body;
                }
            })
        }
    };
}

/// The cell at `(column, row)` of table `t`, created with Pine's defaults when absent; an
/// index outside the table stops the run.
fn cell<'t>(t: &'t mut TableOut, a: &[Value]) -> Result<&'t mut CellOut, E> {
    let (c, r) = (a[1].int().unwrap_or(-1), a[2].int().unwrap_or(-1));
    if c < 0 || r < 0 || c as usize >= t.columns || r as usize >= t.rows {
        return Err((RtCode::TableCell, vec![c.to_string(), r.to_string(), t.columns.to_string(), t.rows.to_string()]));
    }
    let (c, r) = (c as usize, r as usize);
    let at = match t.cells.binary_search_by_key(&(r, c), |x| (x.row, x.column)) {
        Ok(i) => i,
        Err(i) => {
            t.cells.insert(
                i,
                CellOut {
                    column: c,
                    row: r,
                    text: String::new(),
                    width: 0.0,
                    height: 0.0,
                    text_color: Some(Rgba { r: 0, g: 0, b: 0, a: 255 }),
                    text_halign: "align_center".into(),
                    text_valign: "align_center".into(),
                    text_size: "normal".into(),
                    bgcolor: None,
                    tooltip: String::new(),
                    font: "family_default".into(),
                },
            );
            i
        }
    };
    Ok(&mut t.cells[at])
}

/// `table.cell_set_*(table_id, column, row, value)`: `f` on the cell.
fn cell_set(ctx: &mut Ctx, a: &[Value], f: fn(&mut CellOut, &Value)) -> NResult {
    let Some(Drawing::Table(t)) = id(&a[0]).and_then(|i| ctx.heap.drawing_mut(i)) else { return Ok(Value::Na) };
    f(cell(t, a)?, &a[3]);
    Ok(Value::Na)
}

fn new_point(ctx: &mut Ctx, time: Value, index: Value, price: Value) -> NResult {
    ctx.heap.alloc(Obj::Object(Object { ty: CHART_POINT_TY, fields: vec![time, index, price] })).ok_or_else(full)
}

pub fn native(name: &str) -> Option<Native> {
    let n = |f: NativeFn| Some(Native { f, init: None });
    match name {
        "label" | "line" | "box" | "table" | "linefill" | "polyline" => n(|ctx, a, _| {
            // A cast keeps a drawing of that kind (or `na`).
            Ok(match id(&a[0]).and_then(|i| ctx.heap.drawing(i)) {
                Some(_) => a[0].clone(),
                None => Value::Na,
            })
        }),
        // ── label ──
        "label.new" => n(|ctx, a, _| {
            let d = label(ctx, a);
            add(ctx, Drawing::Label(d))
        }),
        "label.new#point" => n(|ctx, a, _| {
            let full = from_points(ctx, a, 1, 2);
            let d = label(ctx, &full);
            add(ctx, Drawing::Label(d))
        }),
        "label.set_x" => n(set!(Label, |l, a| l.x = a[1].int().unwrap_or(l.x))),
        "label.set_y" => n(set!(Label, |l, a| l.y = a[1].num())),
        "label.set_xy" => n(set!(Label, |l, a| {
            l.x = a[1].int().unwrap_or(l.x);
            l.y = a[2].num();
        })),
        "label.set_point" => n(|ctx, a, _| {
            let Some(Drawing::Label(l)) = id(&a[0]).and_then(|i| ctx.heap.drawing(i)) else { return Ok(Value::Na) };
            let (x, y) = point_xy(ctx, &a[1], &l.xloc.clone());
            edit(ctx, &a[0], |d| {
                if let Drawing::Label(l) = d {
                    l.x = x.int().unwrap_or(l.x);
                    l.y = y.num();
                }
            })
        }),
        "label.set_text" => n(set!(Label, |l, a| l.text = s(&a[1]))),
        "label.set_xloc" => n(set!(Label, |l, a| {
            l.x = a[1].int().unwrap_or(l.x);
            l.xloc = s(&a[2]);
        })),
        "label.set_yloc" => n(set!(Label, |l, a| l.yloc = s(&a[1]))),
        "label.set_color" => n(set!(Label, |l, a| l.color = color(&a[1]))),
        "label.set_style" => n(set!(Label, |l, a| l.style = member(&a[1]))),
        "label.set_textcolor" => n(set!(Label, |l, a| l.textcolor = color(&a[1]))),
        "label.set_size" => n(set!(Label, |l, a| l.size = s(&a[1]))),
        "label.set_textalign" => n(set!(Label, |l, a| l.textalign = s(&a[1]))),
        "label.set_tooltip" => n(set!(Label, |l, a| l.tooltip = s(&a[1]))),
        "label.set_text_font_family" => n(set!(Label, |l, a| l.font = s(&a[1]))),
        "label.set_text_formatting" => n(|_, _, _| Ok(Value::Na)),
        "label.get_x" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Label(l) = d { Some(Value::Int(l.x)) } else { None })),
        "label.get_y" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Label(l) = d { Some(opt_f(l.y)) } else { None })),
        "label.get_text" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Label(l) = d { Some(Value::str(&l.text)) } else { None })),
        "label.copy" | "line.copy" | "box.copy" | "polyline.copy" => n(|ctx, a, _| copy(ctx, &a[0])),
        "label.delete" | "line.delete" | "box.delete" | "table.delete" | "linefill.delete" | "polyline.delete" => n(|ctx, a, _| {
            if let Some(i) = id(&a[0]) {
                ctx.heap.delete(i);
            }
            Ok(Value::Na)
        }),
        "label.all" => n(|ctx, _, _| all(ctx, 0)),
        "line.all" => n(|ctx, _, _| all(ctx, 1)),
        "box.all" => n(|ctx, _, _| all(ctx, 2)),
        "table.all" => n(|ctx, _, _| all(ctx, 3)),
        "linefill.all" => n(|ctx, _, _| all(ctx, 4)),
        "polyline.all" => n(|ctx, _, _| all(ctx, 5)),

        // ── line ──
        "line.new" => n(|ctx, a, _| {
            let d = line(ctx, a);
            add(ctx, Drawing::Line(d))
        }),
        "line.new#point" => n(|ctx, a, _| {
            let full = from_points(ctx, a, 2, 2);
            let d = line(ctx, &full);
            add(ctx, Drawing::Line(d))
        }),
        "line.set_x1" => n(set!(Line, |l, a| l.x1 = a[1].int().unwrap_or(l.x1))),
        "line.set_x2" => n(set!(Line, |l, a| l.x2 = a[1].int().unwrap_or(l.x2))),
        "line.set_y1" => n(set!(Line, |l, a| l.y1 = a[1].num())),
        "line.set_y2" => n(set!(Line, |l, a| l.y2 = a[1].num())),
        "line.set_xy1" => n(set!(Line, |l, a| {
            l.x1 = a[1].int().unwrap_or(l.x1);
            l.y1 = a[2].num();
        })),
        "line.set_xy2" => n(set!(Line, |l, a| {
            l.x2 = a[1].int().unwrap_or(l.x2);
            l.y2 = a[2].num();
        })),
        "line.set_first_point" | "line.set_second_point" => {
            let f: NativeFn = if name.ends_with("first_point") {
                |ctx, a, _| line_point(ctx, a, true)
            } else {
                |ctx, a, _| line_point(ctx, a, false)
            };
            n(f)
        }
        "line.set_xloc" => n(set!(Line, |l, a| {
            l.x1 = a[1].int().unwrap_or(l.x1);
            l.x2 = a[2].int().unwrap_or(l.x2);
            l.xloc = s(&a[3]);
        })),
        "line.set_extend" => n(set!(Line, |l, a| l.extend = s(&a[1]))),
        "line.set_color" => n(set!(Line, |l, a| l.color = color(&a[1]))),
        "line.set_style" => n(set!(Line, |l, a| l.style = member(&a[1]))),
        "line.set_width" => n(set!(Line, |l, a| l.width = width(&a[1]))),
        "line.get_x1" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Line(l) = d { Some(Value::Int(l.x1)) } else { None })),
        "line.get_x2" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Line(l) = d { Some(Value::Int(l.x2)) } else { None })),
        "line.get_y1" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Line(l) = d { Some(opt_f(l.y1)) } else { None })),
        "line.get_y2" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Line(l) = d { Some(opt_f(l.y2)) } else { None })),
        // The price on the (infinite) line at bar `x`; Pine reads it for `xloc.bar_index` only.
        "line.get_price" => n(|ctx, a, _| {
            let x = a[1].num();
            get(ctx, &a[0], |d| {
                let Drawing::Line(l) = d else { return None };
                let (x, y1, y2) = (x?, l.y1?, l.y2?);
                if l.xloc != "bar_index" {
                    return Some(Value::Na);
                }
                if l.x2 == l.x1 {
                    return Some(Value::float(y1));
                }
                Some(Value::float(y1 + (y2 - y1) * (x - l.x1 as f64) / (l.x2 - l.x1) as f64))
            })
        }),

        // ── box ──
        "box.new" => n(|ctx, a, _| {
            let d = boxd(ctx, a);
            add(ctx, Drawing::Box(d))
        }),
        "box.new#point" => n(|ctx, a, _| {
            let full = from_points(ctx, a, 2, 6);
            // The point form is (top_left, bottom_right): left, top, right, bottom.
            let d = boxd(ctx, &full);
            add(ctx, Drawing::Box(d))
        }),
        "box.set_left" => n(set!(Box, |b, a| b.left = a[1].int().unwrap_or(b.left))),
        "box.set_right" => n(set!(Box, |b, a| b.right = a[1].int().unwrap_or(b.right))),
        "box.set_top" => n(set!(Box, |b, a| b.top = a[1].num())),
        "box.set_bottom" => n(set!(Box, |b, a| b.bottom = a[1].num())),
        "box.set_lefttop" => n(set!(Box, |b, a| {
            b.left = a[1].int().unwrap_or(b.left);
            b.top = a[2].num();
        })),
        "box.set_rightbottom" => n(set!(Box, |b, a| {
            b.right = a[1].int().unwrap_or(b.right);
            b.bottom = a[2].num();
        })),
        "box.set_top_left_point" | "box.set_bottom_right_point" => {
            let f: NativeFn = if name.contains("top_left") {
                |ctx, a, _| box_point(ctx, a, true)
            } else {
                |ctx, a, _| box_point(ctx, a, false)
            };
            n(f)
        }
        "box.set_xloc" => n(set!(Box, |b, a| {
            b.left = a[1].int().unwrap_or(b.left);
            b.right = a[2].int().unwrap_or(b.right);
            b.xloc = s(&a[3]);
        })),
        "box.set_border_color" => n(set!(Box, |b, a| b.border_color = color(&a[1]))),
        "box.set_border_width" => n(set!(Box, |b, a| b.border_width = width(&a[1]))),
        "box.set_border_style" => n(set!(Box, |b, a| b.border_style = member(&a[1]))),
        "box.set_extend" => n(set!(Box, |b, a| b.extend = s(&a[1]))),
        "box.set_bgcolor" => n(set!(Box, |b, a| b.bgcolor = color(&a[1]))),
        "box.set_text" => n(set!(Box, |b, a| b.text = s(&a[1]))),
        "box.set_text_size" => n(set!(Box, |b, a| b.text_size = s(&a[1]))),
        "box.set_text_color" => n(set!(Box, |b, a| b.text_color = color(&a[1]))),
        "box.set_text_halign" => n(set!(Box, |b, a| b.text_halign = s(&a[1]))),
        "box.set_text_valign" => n(set!(Box, |b, a| b.text_valign = s(&a[1]))),
        "box.set_text_wrap" => n(set!(Box, |b, a| b.text_wrap = s(&a[1]))),
        "box.set_text_font_family" => n(set!(Box, |b, a| b.font = s(&a[1]))),
        "box.set_text_formatting" => n(|_, _, _| Ok(Value::Na)),
        "box.get_left" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Box(b) = d { Some(Value::Int(b.left)) } else { None })),
        "box.get_right" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Box(b) = d { Some(Value::Int(b.right)) } else { None })),
        "box.get_top" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Box(b) = d { Some(opt_f(b.top)) } else { None })),
        "box.get_bottom" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Box(b) = d { Some(opt_f(b.bottom)) } else { None })),

        // ── table ──
        "table.new" => n(|ctx, a, _| {
            let t = TableOut {
                position: s(&a[0]),
                columns: idx(&a[1]),
                rows: idx(&a[2]),
                bgcolor: color(&a[3]),
                frame_color: color(&a[4]),
                frame_width: width(&a[5]),
                border_color: color(&a[6]),
                border_width: width(&a[7]),
                cells: Vec::new(),
                merges: Vec::new(),
            };
            add(ctx, Drawing::Table(t))
        }),
        "table.cell" => n(|ctx, a, _| {
            let Some(Drawing::Table(t)) = id(&a[0]).and_then(|i| ctx.heap.drawing_mut(i)) else { return Ok(Value::Na) };
            let c = cell(t, a)?;
            c.text = s(&a[3]);
            c.width = a[4].num().unwrap_or(0.0);
            c.height = a[5].num().unwrap_or(0.0);
            c.text_color = color(&a[6]);
            c.text_halign = s(&a[7]);
            c.text_valign = s(&a[8]);
            c.text_size = s(&a[9]);
            c.bgcolor = color(&a[10]);
            c.tooltip = s(&a[11]);
            c.font = s(&a[12]);
            Ok(Value::Na)
        }),
        "table.cell_set_text" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.text = s(v))),
        "table.cell_set_bgcolor" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.bgcolor = color(v))),
        "table.cell_set_text_color" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.text_color = color(v))),
        "table.cell_set_text_size" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.text_size = s(v))),
        "table.cell_set_text_halign" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.text_halign = s(v))),
        "table.cell_set_text_valign" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.text_valign = s(v))),
        "table.cell_set_width" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.width = v.num().unwrap_or(0.0))),
        "table.cell_set_height" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.height = v.num().unwrap_or(0.0))),
        "table.cell_set_tooltip" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.tooltip = s(v))),
        "table.cell_set_text_font_family" => n(|ctx, a, _| cell_set(ctx, a, |c, v| c.font = s(v))),
        "table.cell_set_text_formatting" => n(|_, _, _| Ok(Value::Na)),
        "table.set_position" => n(set!(Table, |t, a| t.position = s(&a[1]))),
        "table.set_bgcolor" => n(set!(Table, |t, a| t.bgcolor = color(&a[1]))),
        "table.set_frame_color" => n(set!(Table, |t, a| t.frame_color = color(&a[1]))),
        "table.set_frame_width" => n(set!(Table, |t, a| t.frame_width = width(&a[1]))),
        "table.set_border_color" => n(set!(Table, |t, a| t.border_color = color(&a[1]))),
        "table.set_border_width" => n(set!(Table, |t, a| t.border_width = width(&a[1]))),
        "table.clear" => n(set!(Table, |t, a| {
            let (c0, r0) = (idx(&a[1]), idx(&a[2]));
            let (c1, r1) = (a[3].int().map_or(c0, |x| x.max(0) as usize), a[4].int().map_or(r0, |x| x.max(0) as usize));
            t.cells.retain(|c| !(c.column >= c0 && c.column <= c1 && c.row >= r0 && c.row <= r1));
            t.merges.retain(|m| !(m[0] >= c0 && m[2] <= c1 && m[1] >= r0 && m[3] <= r1));
        })),
        "table.merge_cells" => n(set!(Table, |t, a| t.merges.push([idx(&a[1]), idx(&a[2]), idx(&a[3]), idx(&a[4])]))),

        // ── linefill ──
        "linefill.new" => n(|ctx, a, _| {
            let (Some(l1), Some(l2)) = (id(&a[0]), id(&a[1])) else { return Ok(Value::Na) };
            let is_line = |i| matches!(ctx.heap.drawing(i), Some(Drawing::Line(_)));
            if !is_line(l1) || !is_line(l2) {
                return Ok(Value::Na);
            }
            // Pine keeps one linefill per pair of lines: a new one replaces it.
            let same: Vec<u32> = ctx
                .heap
                .drawings_of(4)
                .filter(|f| matches!(ctx.heap.drawing(*f), Some(Drawing::Linefill { line1, line2, .. }) if (*line1, *line2) == (l1, l2) || (*line1, *line2) == (l2, l1)))
                .collect();
            for f in same {
                ctx.heap.delete(f);
            }
            add(ctx, Drawing::Linefill { line1: l1, line2: l2, color: color(&a[2]) })
        }),
        "linefill.set_color" => n(|ctx, a, _| {
            let c = color(&a[1]);
            edit(ctx, &a[0], |d| {
                if let Drawing::Linefill { color, .. } = d {
                    *color = c;
                }
            })
        }),
        "linefill.get_line1" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Linefill { line1, .. } = d { Some(Value::Ref(*line1)) } else { None })),
        "linefill.get_line2" => n(|ctx, a, _| get(ctx, &a[0], |d| if let Drawing::Linefill { line2, .. } = d { Some(Value::Ref(*line2)) } else { None })),

        // ── polyline ──
        "polyline.new" => n(|ctx, a, _| {
            let xloc = s(&a[3]);
            let ids: Vec<Value> = match id(&a[0]).and_then(|i| ctx.heap.array(i)) {
                Some(arr) => arr.items.clone(),
                None => return Ok(Value::Na),
            };
            // The points as they are now: later changes to the array do not move the line.
            let points = ids
                .iter()
                .filter_map(|p| {
                    let (x, y) = point_xy(ctx, p, &xloc);
                    Some((x.int()?, y.num()?))
                })
                .collect();
            let d = PolylineOut {
                points,
                xloc,
                curved: a[1].truthy(),
                closed: a[2].truthy(),
                line_color: color(&a[4]),
                fill_color: color(&a[5]),
                line_style: member(&a[6]),
                line_width: width(&a[7]),
            };
            add(ctx, Drawing::Polyline(d))
        }),

        // ── chart.point ──
        "chart.point.new" => n(|ctx, a, _| new_point(ctx, int_or_na(&a[0]), int_or_na(&a[1]), Value::float(a[2].num().unwrap_or(f64::NAN)))),
        "chart.point.from_index" => n(|ctx, a, _| new_point(ctx, Value::Na, int_or_na(&a[0]), Value::float(a[1].num().unwrap_or(f64::NAN)))),
        "chart.point.from_time" => n(|ctx, a, _| new_point(ctx, int_or_na(&a[0]), Value::Na, Value::float(a[1].num().unwrap_or(f64::NAN)))),
        "chart.point.now" => n(|ctx, a, _| {
            let (t, i) = (ctx.input.bars.time[ctx.bar], ctx.bar as i64);
            new_point(ctx, Value::Int(t), Value::Int(i), Value::float(a[0].num().unwrap_or(f64::NAN)))
        }),
        "chart.point.copy" => n(|ctx, a, _| {
            let Some(o) = id(&a[0]).and_then(|i| ctx.heap.object(i)).cloned() else { return Ok(Value::Na) };
            ctx.heap.alloc(Obj::Object(o)).ok_or_else(full)
        }),
        _ => None,
    }
}

/// `recv.name(…)` on a value of unknown type that several drawing kinds answer: the
/// drawing's kind picks the builtin. The method name comes last.
pub fn dispatch() -> Native {
    Native {
        f: |ctx, a, st| {
            let (name, args) = a.split_last().expect("method name");
            let kind = id(&args[0]).and_then(|i| ctx.heap.drawing(i)).map(|d| crate::vm::heap::DRAW_KINDS[d.kind()]);
            let (Some(kind), Value::Str(name)) = (kind, name) else { return Ok(Value::Na) };
            match native(&format!("{kind}.{name}")) {
                Some(n) => (n.f)(ctx, args, st),
                None => Ok(Value::Na),
            }
        },
        init: None,
    }
}

fn int_or_na(v: &Value) -> Value {
    v.int().map(Value::Int).unwrap_or(Value::Na)
}

fn line_point(ctx: &mut Ctx, a: &[Value], first: bool) -> NResult {
    let Some(Drawing::Line(l)) = id(&a[0]).and_then(|i| ctx.heap.drawing(i)) else { return Ok(Value::Na) };
    let (x, y) = point_xy(ctx, &a[1], &l.xloc.clone());
    edit(ctx, &a[0], |d| {
        if let Drawing::Line(l) = d {
            if first {
                l.x1 = x.int().unwrap_or(l.x1);
                l.y1 = y.num();
            } else {
                l.x2 = x.int().unwrap_or(l.x2);
                l.y2 = y.num();
            }
        }
    })
}

fn box_point(ctx: &mut Ctx, a: &[Value], top_left: bool) -> NResult {
    let Some(Drawing::Box(b)) = id(&a[0]).and_then(|i| ctx.heap.drawing(i)) else { return Ok(Value::Na) };
    let (x, y) = point_xy(ctx, &a[1], &b.xloc.clone());
    edit(ctx, &a[0], |d| {
        if let Drawing::Box(b) = d {
            if top_left {
                b.left = x.int().unwrap_or(b.left);
                b.top = y.num();
            } else {
                b.right = x.int().unwrap_or(b.right);
                b.bottom = y.num();
            }
        }
    })
}
