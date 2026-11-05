//! What a compiled script declares (`ScriptMeta`) and what a run produces (`RunOutput`).

use serde::{Deserialize, Serialize};

use crate::input::InputValue;

/// A color. `a` = round(255 * (100 - transp) / 100), Pine transparency being 0..100.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputKind {
    Int,
    Float,
    Bool,
    Str,
    Source,
    Color,
    Session,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputMeta {
    /// The declared variable name: the stable key hosts pass values under.
    pub key: String,
    pub title: String,
    pub kind: InputKind,
    pub default: InputValue,
    pub minval: Option<f64>,
    pub maxval: Option<f64>,
    pub step: Option<f64>,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlotStyle {
    Line,
    LineBr,
    StepLine,
    Histogram,
    Columns,
    Area,
    AreaBr,
    Circles,
    Cross,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlotMeta {
    pub index: usize,
    pub title: String,
    pub style: PlotStyle,
    /// False when the plot reads data a backtest cannot know yet (see spec §4.4).
    pub causal: bool,
    /// What makes it non-causal (`None` when causal). Absent from metadata stored before it
    /// existed.
    #[serde(default)]
    pub taint: Option<Taint>,
}

/// The root of a plot's non-causality: the builtin that knows where the data ends (or
/// `"offset"` for a negative plot offset) and its line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taint {
    pub feature: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptMeta {
    pub version: u8,
    pub title: String,
    pub shorttitle: Option<String>,
    pub overlay: bool,
    pub precision: Option<u8>,
    pub max_bars_back: Option<usize>,
    pub inputs: Vec<InputMeta>,
    pub plots: Vec<PlotMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlotOut {
    pub index: usize,
    /// Unshifted values; `offset` says how the chart moves them.
    pub values: Vec<Option<f64>>,
    pub colors: Vec<Option<Rgba>>,
    pub linewidth: u8,
    pub offset: i32,
    pub histbase: f64,
    /// `display = display.none`: computed (usable by a backtest) but not drawn.
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HLineOut {
    pub price: f64,
    pub title: String,
    pub color: Option<Rgba>,
    pub style: String,
    pub linewidth: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillRef {
    Plot(usize),
    HLine(usize),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillOut {
    pub a: FillRef,
    pub b: FillRef,
    pub colors: Vec<Option<Rgba>>,
}

/// `bgcolor` and `barcolor`: one color per bar, `None` = untouched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarColorOut {
    pub colors: Vec<Option<Rgba>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeOut {
    /// plotshape | plotchar | plotarrow
    pub kind: String,
    pub when: Vec<Option<f64>>,
    pub style: String,
    pub location: String,
    pub color: Vec<Option<Rgba>>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandleOut {
    /// plotcandle | plotbar
    pub kind: String,
    pub o: Vec<Option<f64>>,
    pub h: Vec<Option<f64>>,
    pub l: Vec<Option<f64>>,
    pub c: Vec<Option<f64>>,
    pub colors: Vec<Option<Rgba>>,
}

// ── Drawing objects (phase 5): their state after the last bar run ──
//
// An `x` is a bar index (`xloc = "bar_index"`, counted from the first bar the script ran on)
// or an open time in ms (`"bar_time"`). Enum arguments keep Pine's member name without its
// namespace (`label.style_label_down` is `"label_down"`, `size.small` is `"small"`).

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelOut {
    pub x: i64,
    pub y: Option<f64>,
    pub xloc: String,
    /// price | abovebar | belowbar
    pub yloc: String,
    pub text: String,
    pub color: Option<Rgba>,
    pub style: String,
    pub textcolor: Option<Rgba>,
    pub size: String,
    pub textalign: String,
    pub tooltip: String,
    pub font: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineOut {
    pub x1: i64,
    pub y1: Option<f64>,
    pub x2: i64,
    pub y2: Option<f64>,
    pub xloc: String,
    /// none | left | right | both
    pub extend: String,
    pub color: Option<Rgba>,
    pub style: String,
    pub width: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoxOut {
    pub left: i64,
    pub top: Option<f64>,
    pub right: i64,
    pub bottom: Option<f64>,
    pub xloc: String,
    pub extend: String,
    pub border_color: Option<Rgba>,
    pub border_width: u32,
    pub border_style: String,
    pub bgcolor: Option<Rgba>,
    pub text: String,
    pub text_size: String,
    pub text_color: Option<Rgba>,
    pub text_halign: String,
    pub text_valign: String,
    pub text_wrap: String,
    pub font: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellOut {
    pub column: usize,
    pub row: usize,
    pub text: String,
    /// Share of the pane in percent; 0 = fit the text.
    pub width: f64,
    pub height: f64,
    pub text_color: Option<Rgba>,
    pub text_halign: String,
    pub text_valign: String,
    pub text_size: String,
    pub bgcolor: Option<Rgba>,
    pub tooltip: String,
    pub font: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableOut {
    /// top_left … bottom_right
    pub position: String,
    pub columns: usize,
    pub rows: usize,
    pub bgcolor: Option<Rgba>,
    pub frame_color: Option<Rgba>,
    pub frame_width: u32,
    pub border_color: Option<Rgba>,
    pub border_width: u32,
    /// Cells set so far, by row then column.
    pub cells: Vec<CellOut>,
    /// Merged ranges `[start_column, start_row, end_column, end_row]`.
    pub merges: Vec<[usize; 4]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinefillOut {
    /// Indexes into `Drawings::lines`.
    pub line1: usize,
    pub line2: usize,
    pub color: Option<Rgba>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolylineOut {
    /// `(x, price)`, `x` per `xloc`.
    pub points: Vec<(i64, f64)>,
    pub xloc: String,
    pub curved: bool,
    pub closed: bool,
    pub line_color: Option<Rgba>,
    pub fill_color: Option<Rgba>,
    pub line_style: String,
    pub line_width: u32,
}

/// Every drawing alive after the last bar, oldest first per kind.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Drawings {
    pub labels: Vec<LabelOut>,
    pub lines: Vec<LineOut>,
    pub boxes: Vec<BoxOut>,
    pub tables: Vec<TableOut>,
    pub linefills: Vec<LinefillOut>,
    pub polylines: Vec<PolylineOut>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunOutput {
    pub plots: Vec<PlotOut>,
    pub hlines: Vec<HLineOut>,
    pub fills: Vec<FillOut>,
    pub bgcolor: Vec<BarColorOut>,
    pub barcolor: Vec<BarColorOut>,
    pub shapes: Vec<ShapeOut>,
    pub candles: Vec<CandleOut>,
    #[serde(default)]
    pub drawings: Drawings,
}

/// Apply `num` to every per-bar value column and `col` to every per-bar color column.
fn each_column(o: &mut RunOutput, mut num: impl FnMut(&mut Vec<Option<f64>>), mut col: impl FnMut(&mut Vec<Option<Rgba>>)) {
    for p in &mut o.plots {
        num(&mut p.values);
        col(&mut p.colors);
    }
    for f in &mut o.fills {
        col(&mut f.colors);
    }
    for b in o.bgcolor.iter_mut().chain(o.barcolor.iter_mut()) {
        col(&mut b.colors);
    }
    for s in &mut o.shapes {
        num(&mut s.when);
        col(&mut s.color);
    }
    for c in &mut o.candles {
        for v in [&mut c.o, &mut c.h, &mut c.l, &mut c.c] {
            num(v);
        }
        col(&mut c.colors);
    }
}

impl RunOutput {
    /// Keep bars `..keep` of every per-bar column and pad them with `None` to `len` bars.
    pub fn reset_from(&mut self, keep: usize, len: usize) {
        each_column(
            self,
            |v| {
                v.truncate(keep);
                v.resize(len, None);
            },
            |v| {
                v.truncate(keep);
                v.resize(len, None);
            },
        );
    }

    /// Bars `from..` of every per-bar column; the other fields whole.
    /// Bars `from..` only: `columns_from`, plus drawings placed by bar index moved back by
    /// `from` (bar_time ones keep their time). What a host shows after cutting a warmup off.
    pub fn cut(mut self, from: usize) -> RunOutput {
        if from == 0 {
            return self;
        }
        each_column(&mut self, |v| drop(v.drain(..from.min(v.len()))), |v| drop(v.drain(..from.min(v.len()))));
        let d = &mut self.drawings;
        let by_index = |xloc: &str| xloc != "bar_time";
        let f = from as i64;
        for l in d.labels.iter_mut().filter(|l| by_index(&l.xloc)) {
            l.x -= f;
        }
        for l in d.lines.iter_mut().filter(|l| by_index(&l.xloc)) {
            l.x1 -= f;
            l.x2 -= f;
        }
        for b in d.boxes.iter_mut().filter(|b| by_index(&b.xloc)) {
            b.left -= f;
            b.right -= f;
        }
        for p in d.polylines.iter_mut().filter(|p| by_index(&p.xloc)) {
            for pt in &mut p.points {
                pt.0 -= f;
            }
        }
        self
    }

    pub fn columns_from(&self, from: usize) -> RunOutput {
        let mut o = self.clone();
        each_column(&mut o, |v| drop(v.drain(..from.min(v.len()))), |v| drop(v.drain(..from.min(v.len()))));
        o
    }

    /// Replace bars `from..` of every per-bar column with `tail`'s columns (an output of the
    /// same script, cut by `columns_from`). Other fields stay.
    pub fn splice_columns(&mut self, from: usize, mut tail: RunOutput) {
        let mut nums: Vec<Vec<Option<f64>>> = Vec::new();
        let mut cols: Vec<Vec<Option<Rgba>>> = Vec::new();
        each_column(&mut tail, |v| nums.push(std::mem::take(v)), |v| cols.push(std::mem::take(v)));
        let (mut ni, mut ci) = (nums.into_iter(), cols.into_iter());
        each_column(
            self,
            |v| {
                v.truncate(from);
                v.extend(ni.next().unwrap_or_default());
            },
            |v| {
                v.truncate(from);
                v.extend(ci.next().unwrap_or_default());
            },
        );
    }
}
