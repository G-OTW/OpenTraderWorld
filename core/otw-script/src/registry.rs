//! Every builtin the engine knows, with the phase that supports it. Single source for the
//! checker ("supported?"), the editor's autocomplete and the corpus metric. Names absent from
//! the table resolve through `PREFIXES` to the phase planned for their family.
//! Signatures follow the Pine v6 Reference Manual.

/// Phase currently shipped: a builtin with `phase <= CURRENT_PHASE` runs.
pub const CURRENT_PHASE: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Func,
    Var,
    Const,
}

/// Parameter type, as the checker sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PTy {
    /// int or float.
    Num,
    Int,
    Bool,
    Color,
    Str,
    Any,
    /// A `plot(...)` or `hline(...)` result.
    Ref,
}

/// Return type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RTy {
    Float,
    Int,
    Bool,
    Color,
    Str,
    Void,
    /// Same type as the first argument (`nz`, `math.abs`, `ta.change`…).
    Arg0,
    /// Same type as the second argument (`ta.valuewhen`).
    Arg1,
    /// `ta.vwap`: float, or `[vwap, upper, lower]` when `stdev_mult` is given.
    Vwap,
    /// `[float, float, float]` (`ta.macd`, `ta.bb`, `ta.dmi`).
    Tuple3,
    /// `[float, int]` (`ta.supertrend`).
    FloatInt,
    /// An array (`array.new_float`, `array.copy`…).
    Array(Elt),
    /// The element type of the array in the first argument (`array.get`).
    Elem,
    PlotRef,
    HLineRef,
    /// `math.round(x)` is int, `math.round(x, precision)` is float.
    Round,
    /// A drawing (`label`, `line`…) or a `chart.point`.
    Draw(&'static str),
}

/// Element type of an array result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elt {
    Int,
    Float,
    Bool,
    Str,
    Color,
    /// The type of the first argument (`array.from`).
    Arg0,
    /// The array in the first argument's own type (`array.copy`).
    Same,
    /// `array.new<T>`: the call's type argument.
    Generic,
    /// A drawing kind (`array.new_line`).
    Draw(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    pub name: &'static str,
    pub ty: PTy,
    /// `None` = required.
    pub default: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builtin {
    pub name: &'static str,
    pub kind: Kind,
    /// 0 = not planned.
    pub phase: u8,
    pub visual_only: bool,
    /// False when the value depends on where the data ends (spec §4.4).
    pub causal: bool,
    pub params: &'static [Param],
    pub ret: RTy,
    /// Accepts any number of trailing arguments of the last parameter's type (`math.max`).
    pub variadic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Supported(&'static Builtin),
    NotYet { phase: u8, visual_only: bool },
    Unknown,
}

const fn p(name: &'static str, ty: PTy) -> Param {
    Param { name, ty, default: None }
}
const fn d(name: &'static str, ty: PTy, default: &'static str) -> Param {
    Param { name, ty, default: Some(default) }
}
const fn f(name: &'static str, params: &'static [Param], ret: RTy) -> Builtin {
    Builtin { name, kind: Kind::Func, phase: 1, visual_only: false, causal: true, params, ret, variadic: false }
}
const fn f4(name: &'static str, params: &'static [Param], ret: RTy) -> Builtin {
    Builtin { phase: 4, ..f(name, params, ret) }
}
const fn fv4(name: &'static str, params: &'static [Param], ret: RTy) -> Builtin {
    Builtin { variadic: true, ..f4(name, params, ret) }
}
/// A drawing builtin (phase 5).
const fn f5(name: &'static str, params: &'static [Param], ret: RTy) -> Builtin {
    Builtin { phase: 5, visual_only: true, ..f(name, params, ret) }
}
const fn v5(name: &'static str, ret: RTy) -> Builtin {
    Builtin { phase: 5, visual_only: true, ..v(name, ret) }
}
const fn c5(name: &'static str) -> Builtin {
    Builtin { phase: 5, ..c(name, RTy::Str) }
}
const fn fv(name: &'static str, params: &'static [Param], ret: RTy) -> Builtin {
    Builtin { variadic: true, ..f(name, params, ret) }
}
const fn v(name: &'static str, ret: RTy) -> Builtin {
    Builtin { name, kind: Kind::Var, phase: 1, visual_only: false, causal: true, params: &[], ret, variadic: false }
}
const fn nc(name: &'static str, ret: RTy) -> Builtin {
    Builtin { causal: false, ..v(name, ret) }
}
const fn c(name: &'static str, ret: RTy) -> Builtin {
    Builtin { kind: Kind::Const, ..v(name, ret) }
}

use PTy::{Any, Bool as B, Color as C, Int as I, Num as N, Ref as R, Str as S};

const SRC_LEN: &[Param] = &[p("source", N), p("length", I)];
const INPUT_TAIL: [Param; 6] = [d("tooltip", S, ""), d("inline", S, ""), d("group", S, ""), d("confirm", B, "false"), d("display", Any, "display.all"), d("active", B, "true")];
const DISPLAY_TAIL: [Param; 6] = [d("editable", B, "true"), d("show_last", I, "na"), d("display", Any, "display.all"), d("format", S, ""), d("precision", I, "na"), d("force_overlay", B, "false")];

macro_rules! cat {
    ($($a:expr),* $(,)?) => {{
        const LEN: usize = 0 $(+ $a.len())*;
        const OUT: [Param; LEN] = {
            let mut out = [p("", Any); LEN];
            let mut n = 0;
            $(
                let a: &[Param] = &$a;
                let mut i = 0;
                while i < a.len() {
                    out[n] = a[i];
                    n += 1;
                    i += 1;
                }
            )*
            let _ = n;
            out
        };
        &OUT
    }};
}

const LABEL_NEW: &[Param] = &[
    p("x", I),
    p("y", N),
    d("text", S, ""),
    d("xloc", Any, "xloc.bar_index"),
    d("yloc", Any, "yloc.price"),
    d("color", C, "color.blue"),
    d("style", Any, "label.style_label_down"),
    d("textcolor", C, "color.white"),
    d("size", Any, "size.normal"),
    d("textalign", Any, "text.align_center"),
    d("tooltip", S, ""),
    d("text_font_family", Any, "font.family_default"),
    d("force_overlay", B, "false"),
    d("text_formatting", Any, "text.format_none"),
];
/// `label.new(point, …)`: chosen by the checker when the first argument is a `chart.point`.
const LABEL_NEW_POINT: &[Param] = &[
    p("point", Any),
    d("text", S, ""),
    d("xloc", Any, "xloc.bar_index"),
    d("yloc", Any, "yloc.price"),
    d("color", C, "color.blue"),
    d("style", Any, "label.style_label_down"),
    d("textcolor", C, "color.white"),
    d("size", Any, "size.normal"),
    d("textalign", Any, "text.align_center"),
    d("tooltip", S, ""),
    d("text_font_family", Any, "font.family_default"),
    d("force_overlay", B, "false"),
    d("text_formatting", Any, "text.format_none"),
];
const LINE_TAIL: [Param; 6] = [
    d("xloc", Any, "xloc.bar_index"),
    d("extend", Any, "extend.none"),
    d("color", C, "color.blue"),
    d("style", Any, "line.style_solid"),
    d("width", I, "1"),
    d("force_overlay", B, "false"),
];
const LINE_NEW: &[Param] = cat!([p("x1", I), p("y1", N), p("x2", I), p("y2", N)], LINE_TAIL);
const LINE_NEW_POINT: &[Param] = cat!([p("first_point", Any), p("second_point", Any)], LINE_TAIL);
const BOX_TAIL: [Param; 15] = [
    d("border_color", C, "color.blue"),
    d("border_width", I, "1"),
    d("border_style", Any, "line.style_solid"),
    d("extend", Any, "extend.none"),
    d("xloc", Any, "xloc.bar_index"),
    d("bgcolor", C, "color.blue"),
    d("text", S, ""),
    d("text_size", Any, "size.auto"),
    d("text_color", C, "color.black"),
    d("text_halign", Any, "text.align_center"),
    d("text_valign", Any, "text.align_center"),
    d("text_wrap", Any, "text.wrap_none"),
    d("text_font_family", Any, "font.family_default"),
    d("force_overlay", B, "false"),
    d("text_formatting", Any, "text.format_none"),
];
const BOX_NEW: &[Param] = cat!([p("left", I), p("top", N), p("right", I), p("bottom", N)], BOX_TAIL);
const BOX_NEW_POINT: &[Param] = cat!([p("top_left", Any), p("bottom_right", Any)], BOX_TAIL);

static TABLE: &[Builtin] = &[
    // ── Price and bar series ──
    v("open", RTy::Float),
    v("high", RTy::Float),
    v("low", RTy::Float),
    v("close", RTy::Float),
    v("volume", RTy::Float),
    v("hl2", RTy::Float),
    v("hlc3", RTy::Float),
    v("ohlc4", RTy::Float),
    v("hlcc4", RTy::Float),
    v("bar_index", RTy::Int),
    nc("last_bar_index", RTy::Int),
    nc("last_bar_time", RTy::Int),
    v("time", RTy::Int),
    v("time_close", RTy::Int),
    v("time_tradingday", RTy::Int),
    nc("timenow", RTy::Int),
    v("year", RTy::Int),
    v("month", RTy::Int),
    v("weekofyear", RTy::Int),
    v("dayofmonth", RTy::Int),
    v("dayofweek", RTy::Int),
    v("hour", RTy::Int),
    v("minute", RTy::Int),
    v("second", RTy::Int),
    v("ta.tr", RTy::Float),
    Builtin { phase: 4, ..v("ta.obv", RTy::Float) },
    // ── barstate ──
    v("barstate.isfirst", RTy::Bool),
    nc("barstate.islast", RTy::Bool),
    v("barstate.ishistory", RTy::Bool),
    v("barstate.isrealtime", RTy::Bool),
    v("barstate.isnew", RTy::Bool),
    v("barstate.isconfirmed", RTy::Bool),
    nc("barstate.islastconfirmedhistory", RTy::Bool),
    // ── timeframe ──
    v("timeframe.period", RTy::Str),
    v("timeframe.main_period", RTy::Str),
    v("timeframe.multiplier", RTy::Int),
    v("timeframe.isseconds", RTy::Bool),
    v("timeframe.isminutes", RTy::Bool),
    v("timeframe.isintraday", RTy::Bool),
    v("timeframe.isdaily", RTy::Bool),
    v("timeframe.isweekly", RTy::Bool),
    v("timeframe.ismonthly", RTy::Bool),
    v("timeframe.isdwm", RTy::Bool),
    f("timeframe.in_seconds", &[d("timeframe", S, "")], RTy::Int),
    f("timeframe.change", &[p("timeframe", S)], RTy::Bool),
    // ── syminfo ──
    v("syminfo.ticker", RTy::Str),
    v("syminfo.tickerid", RTy::Str),
    v("syminfo.prefix", RTy::Str),
    v("syminfo.root", RTy::Str),
    v("syminfo.mintick", RTy::Float),
    v("syminfo.pointvalue", RTy::Float),
    v("syminfo.timezone", RTy::Str),
    v("syminfo.session", RTy::Str),
    v("syminfo.type", RTy::Str),
    v("syminfo.currency", RTy::Str),
    v("syminfo.basecurrency", RTy::Str),
    v("syminfo.description", RTy::Str),
    // ── Declaration and outputs ──
    f(
        "indicator",
        &[
            p("title", S),
            d("shorttitle", S, ""),
            d("overlay", B, "false"),
            d("format", S, "format.inherit"),
            d("precision", I, "na"),
            d("scale", Any, "na"),
            d("max_bars_back", I, "na"),
            d("timeframe", S, ""),
            d("timeframe_gaps", B, "true"),
            d("explicit_plot_zorder", B, "false"),
            d("max_lines_count", I, "50"),
            d("max_labels_count", I, "50"),
            d("max_boxes_count", I, "50"),
            d("calc_bars_count", I, "na"),
            d("max_polylines_count", I, "50"),
            d("dynamic_requests", B, "false"),
            d("behind_chart", B, "true"),
        ],
        RTy::Void,
    ),
    f(
        "plot",
        cat!(
            [p("series", N), d("title", S, "Plot"), d("color", C, "#2962FF"), d("linewidth", I, "1"), d("style", Any, "plot.style_line"), d("trackprice", B, "false"), d("histbase", N, "0"), d("offset", I, "0"), d("join", B, "false"), d("linestyle", Any, "plot.linestyle_solid")],
            DISPLAY_TAIL
        ),
        RTy::PlotRef,
    ),
    f(
        "plotshape",
        cat!(
            [p("series", Any), d("title", S, ""), d("style", Any, "shape.xcross"), d("location", Any, "location.abovebar"), d("color", C, "#2962FF"), d("offset", I, "0"), d("text", S, ""), d("textcolor", C, "na"), d("size", Any, "size.auto")],
            DISPLAY_TAIL
        ),
        RTy::Void,
    ),
    f(
        "plotchar",
        cat!(
            [p("series", Any), d("title", S, ""), d("char", S, "★"), d("location", Any, "location.abovebar"), d("color", C, "#2962FF"), d("offset", I, "0"), d("text", S, ""), d("textcolor", C, "na"), d("size", Any, "size.auto")],
            DISPLAY_TAIL
        ),
        RTy::Void,
    ),
    f(
        "plotarrow",
        cat!(
            [p("series", N), d("title", S, ""), d("colorup", C, "#4CAF50"), d("colordown", C, "#FF5252"), d("offset", I, "0"), d("minheight", I, "5"), d("maxheight", I, "100")],
            DISPLAY_TAIL
        ),
        RTy::Void,
    ),
    f(
        "plotcandle",
        cat!(
            [p("open", N), p("high", N), p("low", N), p("close", N), d("title", S, ""), d("color", C, "na"), d("wickcolor", C, "na"), d("bordercolor", C, "na")],
            DISPLAY_TAIL
        ),
        RTy::Void,
    ),
    f(
        "plotbar",
        cat!([p("open", N), p("high", N), p("low", N), p("close", N), d("title", S, ""), d("color", C, "na")], DISPLAY_TAIL),
        RTy::Void,
    ),
    f(
        "hline",
        &[p("price", N), d("title", S, ""), d("color", C, "#787B86"), d("linestyle", Any, "hline.style_solid"), d("linewidth", I, "1"), d("editable", B, "true"), d("display", Any, "display.all")],
        RTy::HLineRef,
    ),
    f(
        "fill",
        &[
            p("plot1", R),
            p("plot2", R),
            d("color", C, "na"),
            d("title", S, ""),
            d("editable", B, "true"),
            d("show_last", I, "na"),
            d("fillgaps", B, "false"),
            d("display", Any, "display.all"),
            d("top_value", N, "na"),
            d("bottom_value", N, "na"),
            d("top_color", C, "na"),
            d("bottom_color", C, "na"),
        ],
        RTy::Void,
    ),
    f(
        "bgcolor",
        &[p("color", C), d("offset", I, "0"), d("editable", B, "true"), d("show_last", I, "na"), d("title", S, ""), d("display", Any, "display.all"), d("force_overlay", B, "false")],
        RTy::Void,
    ),
    f(
        "barcolor",
        &[p("color", C), d("offset", I, "0"), d("editable", B, "true"), d("show_last", I, "na"), d("title", S, ""), d("display", Any, "display.all")],
        RTy::Void,
    ),
    f("runtime.error", &[p("message", S)], RTy::Void),
    f("max_bars_back", &[p("var", Any), p("num", I)], RTy::Void),
    // ── na handling and casts ──
    f("na", &[p("x", Any)], RTy::Bool),
    f("nz", &[p("source", Any), d("replacement", Any, "0")], RTy::Arg0),
    f("fixnan", &[p("source", Any)], RTy::Arg0),
    f("int", &[p("x", Any)], RTy::Int),
    f("float", &[p("x", Any)], RTy::Float),
    f("bool", &[p("x", Any)], RTy::Bool),
    f("color", &[p("x", Any)], RTy::Color),
    f("string", &[p("x", Any)], RTy::Str),
    // ── Inputs ──
    f("input", &[p("defval", Any), d("title", S, ""), d("tooltip", S, ""), d("inline", S, ""), d("group", S, ""), d("display", Any, "display.all")], RTy::Arg0),
    f(
        "input.int",
        cat!([p("defval", I), d("title", S, ""), d("minval", I, "na"), d("maxval", I, "na"), d("step", I, "1"), d("options", Any, "na")], INPUT_TAIL),
        RTy::Int,
    ),
    f(
        "input.float",
        cat!([p("defval", N), d("title", S, ""), d("minval", N, "na"), d("maxval", N, "na"), d("step", N, "na"), d("options", Any, "na")], INPUT_TAIL),
        RTy::Float,
    ),
    f("input.bool", cat!([p("defval", B), d("title", S, "")], INPUT_TAIL), RTy::Bool),
    f("input.string", cat!([p("defval", S), d("title", S, ""), d("options", Any, "na")], INPUT_TAIL), RTy::Str),
    f("input.source", cat!([p("defval", N), d("title", S, "")], INPUT_TAIL), RTy::Float),
    f("input.color", cat!([p("defval", C), d("title", S, "")], INPUT_TAIL), RTy::Color),
    f("input.session", cat!([p("defval", S), d("title", S, ""), d("options", Any, "na")], INPUT_TAIL), RTy::Str),
    // ── math ──
    f("math.abs", &[p("number", N)], RTy::Arg0),
    f("math.acos", &[p("angle", N)], RTy::Float),
    f("math.asin", &[p("angle", N)], RTy::Float),
    f("math.atan", &[p("angle", N)], RTy::Float),
    f("math.cos", &[p("angle", N)], RTy::Float),
    f("math.sin", &[p("angle", N)], RTy::Float),
    f("math.tan", &[p("angle", N)], RTy::Float),
    f("math.exp", &[p("number", N)], RTy::Float),
    f("math.log", &[p("number", N)], RTy::Float),
    f("math.log10", &[p("number", N)], RTy::Float),
    f("math.sqrt", &[p("number", N)], RTy::Float),
    f("math.pow", &[p("base", N), p("exponent", N)], RTy::Float),
    f("math.ceil", &[p("number", N)], RTy::Int),
    f("math.floor", &[p("number", N)], RTy::Int),
    f("math.round", &[p("number", N), d("precision", I, "na")], RTy::Round),
    f("math.round_to_mintick", &[p("number", N)], RTy::Float),
    f("math.sign", &[p("number", N)], RTy::Float),
    fv("math.max", &[p("number0", N), p("number1", N)], RTy::Arg0),
    fv("math.min", &[p("number0", N), p("number1", N)], RTy::Arg0),
    fv("math.avg", &[p("number0", N), p("number1", N)], RTy::Float),
    f("math.sum", SRC_LEN, RTy::Float),
    f("math.todegrees", &[p("radians", N)], RTy::Float),
    f("math.toradians", &[p("degrees", N)], RTy::Float),
    c("math.pi", RTy::Float),
    c("math.e", RTy::Float),
    c("math.phi", RTy::Float),
    c("math.rphi", RTy::Float),
    // ── color ──
    f("color.new", &[p("color", C), p("transp", N)], RTy::Color),
    f("color.rgb", &[p("red", N), p("green", N), p("blue", N), d("transp", N, "0")], RTy::Color),
    f("color.from_gradient", &[p("value", N), p("bottom_value", N), p("top_value", N), p("bottom_color", C), p("top_color", C)], RTy::Color),
    f("color.r", &[p("color", C)], RTy::Float),
    f("color.g", &[p("color", C)], RTy::Float),
    f("color.b", &[p("color", C)], RTy::Float),
    f("color.t", &[p("color", C)], RTy::Float),
    c("color.aqua", RTy::Color),
    c("color.black", RTy::Color),
    c("color.blue", RTy::Color),
    c("color.fuchsia", RTy::Color),
    c("color.gray", RTy::Color),
    c("color.green", RTy::Color),
    c("color.lime", RTy::Color),
    c("color.maroon", RTy::Color),
    c("color.navy", RTy::Color),
    c("color.olive", RTy::Color),
    c("color.orange", RTy::Color),
    c("color.purple", RTy::Color),
    c("color.red", RTy::Color),
    c("color.silver", RTy::Color),
    c("color.teal", RTy::Color),
    c("color.white", RTy::Color),
    c("color.yellow", RTy::Color),
    // ── Time functions ──
    f("time", &[d("timeframe", S, ""), d("session", S, ""), d("timezone", S, ""), d("bars_back", I, "0")], RTy::Int),
    f("time_close", &[d("timeframe", S, ""), d("session", S, ""), d("timezone", S, ""), d("bars_back", I, "0")], RTy::Int),
    f("year", &[p("time", I), d("timezone", S, "")], RTy::Int),
    f("month", &[p("time", I), d("timezone", S, "")], RTy::Int),
    f("weekofyear", &[p("time", I), d("timezone", S, "")], RTy::Int),
    f("dayofmonth", &[p("time", I), d("timezone", S, "")], RTy::Int),
    f("dayofweek", &[p("time", I), d("timezone", S, "")], RTy::Int),
    f("hour", &[p("time", I), d("timezone", S, "")], RTy::Int),
    f("minute", &[p("time", I), d("timezone", S, "")], RTy::Int),
    f("second", &[p("time", I), d("timezone", S, "")], RTy::Int),
    fv("timestamp", &[p("a0", Any)], RTy::Int),
    c("dayofweek.sunday", RTy::Int),
    c("dayofweek.monday", RTy::Int),
    c("dayofweek.tuesday", RTy::Int),
    c("dayofweek.wednesday", RTy::Int),
    c("dayofweek.thursday", RTy::Int),
    c("dayofweek.friday", RTy::Int),
    c("dayofweek.saturday", RTy::Int),
    // ── ta (phase 1: the 20 most used) ──
    f("ta.sma", SRC_LEN, RTy::Float),
    f("ta.ema", SRC_LEN, RTy::Float),
    f("ta.rma", SRC_LEN, RTy::Float),
    f("ta.wma", SRC_LEN, RTy::Float),
    f("ta.hma", SRC_LEN, RTy::Float),
    f("ta.vwma", SRC_LEN, RTy::Float),
    f("ta.stdev", &[p("source", N), p("length", I), d("biased", B, "true")], RTy::Float),
    f("ta.change", &[p("source", Any), d("length", I, "1")], RTy::Arg0),
    f("ta.highest", &[d("source", N, "high"), p("length", I)], RTy::Float),
    f("ta.lowest", &[d("source", N, "low"), p("length", I)], RTy::Float),
    f("ta.atr", &[p("length", I)], RTy::Float),
    f("ta.tr", &[d("handle_na", B, "false")], RTy::Float),
    f("ta.rsi", SRC_LEN, RTy::Float),
    f("ta.macd", &[p("source", N), p("fastlen", I), p("slowlen", I), p("siglen", I)], RTy::Tuple3),
    f("ta.crossover", &[p("source1", N), p("source2", N)], RTy::Bool),
    f("ta.crossunder", &[p("source1", N), p("source2", N)], RTy::Bool),
    f("ta.pivothigh", &[d("source", N, "high"), p("leftbars", I), p("rightbars", I)], RTy::Float),
    f("ta.pivotlow", &[d("source", N, "low"), p("leftbars", I), p("rightbars", I)], RTy::Float),
    f("ta.valuewhen", &[p("condition", B), p("source", Any), p("occurrence", I)], RTy::Arg1),
    f("ta.vwap", &[d("source", N, "hlc3"), d("anchor", B, "na"), d("stdev_mult", N, "na")], RTy::Vwap),
    // ── ta (phase 4: breadth) ──
    f4("ta.cum", &[p("source", N)], RTy::Float),
    f4("ta.barssince", &[p("condition", B)], RTy::Int),
    f4("ta.linreg", &[p("source", N), p("length", I), p("offset", I)], RTy::Float),
    f4("ta.stoch", &[p("source", N), p("high", N), p("low", N), p("length", I)], RTy::Float),
    f4("ta.cross", &[p("source1", N), p("source2", N)], RTy::Bool),
    f4("ta.median", SRC_LEN, RTy::Float),
    f4("ta.supertrend", &[p("factor", N), p("atrPeriod", I)], RTy::FloatInt),
    f4("ta.highestbars", &[d("source", N, "high"), p("length", I)], RTy::Int),
    f4("ta.lowestbars", &[d("source", N, "low"), p("length", I)], RTy::Int),
    f4("ta.percentrank", SRC_LEN, RTy::Float),
    f4("ta.roc", SRC_LEN, RTy::Float),
    f4("ta.mom", SRC_LEN, RTy::Float),
    f4("ta.sar", &[p("start", N), p("inc", N), p("max", N)], RTy::Float),
    f4("ta.percentile_linear_interpolation", &[p("source", N), p("length", I), p("percentage", N)], RTy::Float),
    f4("ta.percentile_nearest_rank", &[p("source", N), p("length", I), p("percentage", N)], RTy::Float),
    f4("ta.dev", SRC_LEN, RTy::Float),
    f4("ta.variance", &[p("source", N), p("length", I), d("biased", B, "true")], RTy::Float),
    f4("ta.correlation", &[p("source1", N), p("source2", N), p("length", I)], RTy::Float),
    f4("ta.rising", SRC_LEN, RTy::Bool),
    f4("ta.falling", SRC_LEN, RTy::Bool),
    f4("ta.cci", SRC_LEN, RTy::Float),
    f4("ta.mfi", &[p("series", N), p("length", I)], RTy::Float),
    f4("ta.bb", &[p("series", N), p("length", I), p("mult", N)], RTy::Tuple3),
    f4("ta.dmi", &[p("diLength", I), p("adxSmoothing", I)], RTy::Tuple3),
    f4("ta.range", SRC_LEN, RTy::Float),
    f4("ta.wpr", &[p("length", I)], RTy::Float),
    f4("ta.tsi", &[p("source", N), p("short_length", I), p("long_length", I)], RTy::Float),
    // ── array (phase 4) ──
    f4("array.new_float", &[d("size", I, "0"), d("initial_value", N, "na")], RTy::Array(Elt::Float)),
    f4("array.new_int", &[d("size", I, "0"), d("initial_value", I, "na")], RTy::Array(Elt::Int)),
    f4("array.new_bool", &[d("size", I, "0"), d("initial_value", B, "false")], RTy::Array(Elt::Bool)),
    f4("array.new_string", &[d("size", I, "0"), d("initial_value", S, "na")], RTy::Array(Elt::Str)),
    f4("array.new_color", &[d("size", I, "0"), d("initial_value", C, "na")], RTy::Array(Elt::Color)),
    f4("array.new", &[d("size", I, "0"), d("initial_value", Any, "na")], RTy::Array(Elt::Generic)),
    fv4("array.from", &[p("arg0", Any)], RTy::Array(Elt::Arg0)),
    f4("array.size", &[p("id", Any)], RTy::Int),
    f4("array.get", &[p("id", Any), p("index", I)], RTy::Elem),
    f4("array.set", &[p("id", Any), p("index", I), p("value", Any)], RTy::Void),
    f4("array.first", &[p("id", Any)], RTy::Elem),
    f4("array.last", &[p("id", Any)], RTy::Elem),
    f4("array.push", &[p("id", Any), p("value", Any)], RTy::Void),
    f4("array.unshift", &[p("id", Any), p("value", Any)], RTy::Void),
    f4("array.insert", &[p("id", Any), p("index", I), p("value", Any)], RTy::Void),
    f4("array.pop", &[p("id", Any)], RTy::Elem),
    f4("array.shift", &[p("id", Any)], RTy::Elem),
    f4("array.remove", &[p("id", Any), p("index", I)], RTy::Elem),
    f4("array.clear", &[p("id", Any)], RTy::Void),
    f4("array.fill", &[p("id", Any), p("value", Any), d("index_from", I, "0"), d("index_to", I, "na")], RTy::Void),
    f4("array.reverse", &[p("id", Any)], RTy::Void),
    f4("array.sort", &[p("id", Any), d("order", Any, "order.ascending")], RTy::Void),
    f4("array.sort_indices", &[p("id", Any), d("order", Any, "order.ascending")], RTy::Array(Elt::Int)),
    f4("array.copy", &[p("id", Any)], RTy::Array(Elt::Same)),
    f4("array.slice", &[p("id", Any), p("index_from", I), p("index_to", I)], RTy::Array(Elt::Same)),
    f4("array.concat", &[p("id1", Any), p("id2", Any)], RTy::Array(Elt::Same)),
    f4("array.abs", &[p("id", Any)], RTy::Array(Elt::Same)),
    f4("array.includes", &[p("id", Any), p("value", Any)], RTy::Bool),
    f4("array.indexof", &[p("id", Any), p("value", Any)], RTy::Int),
    f4("array.lastindexof", &[p("id", Any), p("value", Any)], RTy::Int),
    f4("array.binary_search", &[p("id", Any), p("val", Any)], RTy::Int),
    f4("array.binary_search_leftmost", &[p("id", Any), p("val", Any)], RTy::Int),
    f4("array.binary_search_rightmost", &[p("id", Any), p("val", Any)], RTy::Int),
    f4("array.sum", &[p("id", Any)], RTy::Elem),
    f4("array.avg", &[p("id", Any)], RTy::Float),
    f4("array.max", &[p("id", Any), d("nth", I, "0")], RTy::Elem),
    f4("array.min", &[p("id", Any), d("nth", I, "0")], RTy::Elem),
    f4("array.range", &[p("id", Any)], RTy::Elem),
    f4("array.median", &[p("id", Any)], RTy::Elem),
    f4("array.mode", &[p("id", Any)], RTy::Elem),
    f4("array.stdev", &[p("id", Any), d("biased", B, "true")], RTy::Float),
    f4("array.variance", &[p("id", Any), d("biased", B, "true")], RTy::Float),
    f4("array.covariance", &[p("id1", Any), p("id2", Any), d("biased", B, "true")], RTy::Float),
    f4("array.standardize", &[p("id", Any)], RTy::Array(Elt::Float)),
    f4("array.percentile_linear_interpolation", &[p("id", Any), p("percentage", N)], RTy::Float),
    f4("array.percentile_nearest_rank", &[p("id", Any), p("percentage", N)], RTy::Float),
    f4("array.percentrank", &[p("id", Any), p("index", I)], RTy::Float),
    f4("array.join", &[p("id", Any), d("separator", S, "")], RTy::Str),
    f4("array.every", &[p("id", Any)], RTy::Bool),
    f4("array.some", &[p("id", Any)], RTy::Bool),
    // ── str (phase 4) ──
    f4("str.tostring", &[p("value", Any), d("format", S, "")], RTy::Str),
    f4("str.tonumber", &[p("string", S)], RTy::Float),
    f4("str.length", &[p("string", S)], RTy::Int),
    f4("str.contains", &[p("source", S), p("str", S)], RTy::Bool),
    f4("str.startswith", &[p("source", S), p("str", S)], RTy::Bool),
    f4("str.endswith", &[p("source", S), p("str", S)], RTy::Bool),
    f4("str.pos", &[p("source", S), p("str", S)], RTy::Int),
    f4("str.substring", &[p("source", S), p("begin_pos", I), d("end_pos", I, "na")], RTy::Str),
    f4("str.replace", &[p("source", S), p("target", S), p("replacement", S), d("occurrence", I, "0")], RTy::Str),
    f4("str.replace_all", &[p("source", S), p("target", S), p("replacement", S)], RTy::Str),
    f4("str.lower", &[p("source", S)], RTy::Str),
    f4("str.upper", &[p("source", S)], RTy::Str),
    f4("str.trim", &[p("source", S)], RTy::Str),
    f4("str.repeat", &[p("source", S), p("repeat", I), d("separator", S, "")], RTy::Str),
    f4("str.split", &[p("string", S), p("separator", S)], RTy::Array(Elt::Str)),
    fv4("str.format", &[p("formatString", S)], RTy::Str),
    // Pine Logs pane entries: accepted, nothing shown.
    fv4("log.info", &[p("message", S)], RTy::Void),
    fv4("log.warning", &[p("message", S)], RTy::Void),
    fv4("log.error", &[p("message", S)], RTy::Void),
    f4("str.format_time", &[p("time", I), d("format", S, "yyyy-MM-dd'T'HH:mm:ssZ"), d("timezone", S, "")], RTy::Str),
    c("order.ascending", RTy::Str),
    c("order.descending", RTy::Str),
    // ── Drawings (phase 5) ──
    f5("array.new_line", &[d("size", I, "0"), d("initial_value", Any, "na")], RTy::Array(Elt::Draw("line"))),
    f5("array.new_label", &[d("size", I, "0"), d("initial_value", Any, "na")], RTy::Array(Elt::Draw("label"))),
    f5("array.new_box", &[d("size", I, "0"), d("initial_value", Any, "na")], RTy::Array(Elt::Draw("box"))),
    f5("array.new_table", &[d("size", I, "0"), d("initial_value", Any, "na")], RTy::Array(Elt::Draw("table"))),
    f5("array.new_linefill", &[d("size", I, "0"), d("initial_value", Any, "na")], RTy::Array(Elt::Draw("linefill"))),
    // `label(na)`: a typed na (or the drawing itself).
    f5("label", &[p("x", Any)], RTy::Draw("label")),
    f5("line", &[p("x", Any)], RTy::Draw("line")),
    f5("box", &[p("x", Any)], RTy::Draw("box")),
    f5("table", &[p("x", Any)], RTy::Draw("table")),
    f5("linefill", &[p("x", Any)], RTy::Draw("linefill")),
    f5("polyline", &[p("x", Any)], RTy::Draw("polyline")),
    f5("label.new", LABEL_NEW, RTy::Draw("label")),
    f5("label.new#point", LABEL_NEW_POINT, RTy::Draw("label")),
    f5("label.set_x", &[p("id", Any), p("x", I)], RTy::Void),
    f5("label.set_y", &[p("id", Any), p("y", N)], RTy::Void),
    f5("label.set_xy", &[p("id", Any), p("x", I), p("y", N)], RTy::Void),
    f5("label.set_point", &[p("id", Any), p("point", Any)], RTy::Void),
    f5("label.set_text", &[p("id", Any), p("text", S)], RTy::Void),
    f5("label.set_xloc", &[p("id", Any), p("x", I), p("xloc", Any)], RTy::Void),
    f5("label.set_yloc", &[p("id", Any), p("yloc", Any)], RTy::Void),
    f5("label.set_color", &[p("id", Any), p("color", C)], RTy::Void),
    f5("label.set_style", &[p("id", Any), p("style", Any)], RTy::Void),
    f5("label.set_textcolor", &[p("id", Any), p("textcolor", C)], RTy::Void),
    f5("label.set_size", &[p("id", Any), p("size", Any)], RTy::Void),
    f5("label.set_textalign", &[p("id", Any), p("textalign", Any)], RTy::Void),
    f5("label.set_tooltip", &[p("id", Any), p("tooltip", S)], RTy::Void),
    f5("label.set_text_font_family", &[p("id", Any), p("text_font_family", Any)], RTy::Void),
    f5("label.set_text_formatting", &[p("id", Any), p("text_formatting", Any)], RTy::Void),
    f5("label.get_x", &[p("id", Any)], RTy::Int),
    f5("label.get_y", &[p("id", Any)], RTy::Float),
    f5("label.get_text", &[p("id", Any)], RTy::Str),
    f5("label.copy", &[p("id", Any)], RTy::Draw("label")),
    f5("label.delete", &[p("id", Any)], RTy::Void),
    v5("label.all", RTy::Array(Elt::Draw("label"))),
    f5("line.new", LINE_NEW, RTy::Draw("line")),
    f5("line.new#point", LINE_NEW_POINT, RTy::Draw("line")),
    f5("line.set_x1", &[p("id", Any), p("x", I)], RTy::Void),
    f5("line.set_x2", &[p("id", Any), p("x", I)], RTy::Void),
    f5("line.set_y1", &[p("id", Any), p("y", N)], RTy::Void),
    f5("line.set_y2", &[p("id", Any), p("y", N)], RTy::Void),
    f5("line.set_xy1", &[p("id", Any), p("x", I), p("y", N)], RTy::Void),
    f5("line.set_xy2", &[p("id", Any), p("x", I), p("y", N)], RTy::Void),
    f5("line.set_first_point", &[p("id", Any), p("point", Any)], RTy::Void),
    f5("line.set_second_point", &[p("id", Any), p("point", Any)], RTy::Void),
    f5("line.set_xloc", &[p("id", Any), p("x1", I), p("x2", I), p("xloc", Any)], RTy::Void),
    f5("line.set_extend", &[p("id", Any), p("extend", Any)], RTy::Void),
    f5("line.set_color", &[p("id", Any), p("color", C)], RTy::Void),
    f5("line.set_style", &[p("id", Any), p("style", Any)], RTy::Void),
    f5("line.set_width", &[p("id", Any), p("width", I)], RTy::Void),
    f5("line.get_x1", &[p("id", Any)], RTy::Int),
    f5("line.get_x2", &[p("id", Any)], RTy::Int),
    f5("line.get_y1", &[p("id", Any)], RTy::Float),
    f5("line.get_y2", &[p("id", Any)], RTy::Float),
    f5("line.get_price", &[p("id", Any), p("x", I)], RTy::Float),
    f5("line.copy", &[p("id", Any)], RTy::Draw("line")),
    f5("line.delete", &[p("id", Any)], RTy::Void),
    v5("line.all", RTy::Array(Elt::Draw("line"))),
    f5("box.new", BOX_NEW, RTy::Draw("box")),
    f5("box.new#point", BOX_NEW_POINT, RTy::Draw("box")),
    f5("box.set_left", &[p("id", Any), p("left", I)], RTy::Void),
    f5("box.set_right", &[p("id", Any), p("right", I)], RTy::Void),
    f5("box.set_top", &[p("id", Any), p("top", N)], RTy::Void),
    f5("box.set_bottom", &[p("id", Any), p("bottom", N)], RTy::Void),
    f5("box.set_lefttop", &[p("id", Any), p("left", I), p("top", N)], RTy::Void),
    f5("box.set_rightbottom", &[p("id", Any), p("right", I), p("bottom", N)], RTy::Void),
    f5("box.set_top_left_point", &[p("id", Any), p("point", Any)], RTy::Void),
    f5("box.set_bottom_right_point", &[p("id", Any), p("point", Any)], RTy::Void),
    f5("box.set_xloc", &[p("id", Any), p("left", I), p("right", I), p("xloc", Any)], RTy::Void),
    f5("box.set_border_color", &[p("id", Any), p("color", C)], RTy::Void),
    f5("box.set_border_width", &[p("id", Any), p("width", I)], RTy::Void),
    f5("box.set_border_style", &[p("id", Any), p("style", Any)], RTy::Void),
    f5("box.set_extend", &[p("id", Any), p("extend", Any)], RTy::Void),
    f5("box.set_bgcolor", &[p("id", Any), p("color", C)], RTy::Void),
    f5("box.set_text", &[p("id", Any), p("text", S)], RTy::Void),
    f5("box.set_text_size", &[p("id", Any), p("text_size", Any)], RTy::Void),
    f5("box.set_text_color", &[p("id", Any), p("text_color", C)], RTy::Void),
    f5("box.set_text_halign", &[p("id", Any), p("text_halign", Any)], RTy::Void),
    f5("box.set_text_valign", &[p("id", Any), p("text_valign", Any)], RTy::Void),
    f5("box.set_text_wrap", &[p("id", Any), p("text_wrap", Any)], RTy::Void),
    f5("box.set_text_font_family", &[p("id", Any), p("text_font_family", Any)], RTy::Void),
    f5("box.set_text_formatting", &[p("id", Any), p("text_formatting", Any)], RTy::Void),
    f5("box.get_left", &[p("id", Any)], RTy::Int),
    f5("box.get_right", &[p("id", Any)], RTy::Int),
    f5("box.get_top", &[p("id", Any)], RTy::Float),
    f5("box.get_bottom", &[p("id", Any)], RTy::Float),
    f5("box.copy", &[p("id", Any)], RTy::Draw("box")),
    f5("box.delete", &[p("id", Any)], RTy::Void),
    v5("box.all", RTy::Array(Elt::Draw("box"))),
    f5(
        "table.new",
        &[
            p("position", Any),
            p("columns", I),
            p("rows", I),
            d("bgcolor", C, "na"),
            d("frame_color", C, "na"),
            d("frame_width", I, "0"),
            d("border_color", C, "na"),
            d("border_width", I, "0"),
            d("force_overlay", B, "false"),
        ],
        RTy::Draw("table"),
    ),
    f5(
        "table.cell",
        &[
            p("table_id", Any),
            p("column", I),
            p("row", I),
            d("text", S, ""),
            d("width", N, "0"),
            d("height", N, "0"),
            d("text_color", C, "color.black"),
            d("text_halign", Any, "text.align_center"),
            d("text_valign", Any, "text.align_center"),
            d("text_size", Any, "size.normal"),
            d("bgcolor", C, "na"),
            d("tooltip", S, ""),
            d("text_font_family", Any, "font.family_default"),
            d("text_formatting", Any, "text.format_none"),
        ],
        RTy::Void,
    ),
    f5("table.cell_set_text", &[p("table_id", Any), p("column", I), p("row", I), p("text", S)], RTy::Void),
    f5("table.cell_set_bgcolor", &[p("table_id", Any), p("column", I), p("row", I), p("bgcolor", C)], RTy::Void),
    f5("table.cell_set_text_color", &[p("table_id", Any), p("column", I), p("row", I), p("text_color", C)], RTy::Void),
    f5("table.cell_set_text_size", &[p("table_id", Any), p("column", I), p("row", I), p("text_size", Any)], RTy::Void),
    f5("table.cell_set_text_halign", &[p("table_id", Any), p("column", I), p("row", I), p("text_halign", Any)], RTy::Void),
    f5("table.cell_set_text_valign", &[p("table_id", Any), p("column", I), p("row", I), p("text_valign", Any)], RTy::Void),
    f5("table.cell_set_width", &[p("table_id", Any), p("column", I), p("row", I), p("width", N)], RTy::Void),
    f5("table.cell_set_height", &[p("table_id", Any), p("column", I), p("row", I), p("height", N)], RTy::Void),
    f5("table.cell_set_tooltip", &[p("table_id", Any), p("column", I), p("row", I), p("tooltip", S)], RTy::Void),
    f5("table.cell_set_text_font_family", &[p("table_id", Any), p("column", I), p("row", I), p("text_font_family", Any)], RTy::Void),
    f5("table.cell_set_text_formatting", &[p("table_id", Any), p("column", I), p("row", I), p("text_formatting", Any)], RTy::Void),
    f5("table.set_position", &[p("table_id", Any), p("position", Any)], RTy::Void),
    f5("table.set_bgcolor", &[p("table_id", Any), p("bgcolor", C)], RTy::Void),
    f5("table.set_frame_color", &[p("table_id", Any), p("frame_color", C)], RTy::Void),
    f5("table.set_frame_width", &[p("table_id", Any), p("frame_width", I)], RTy::Void),
    f5("table.set_border_color", &[p("table_id", Any), p("border_color", C)], RTy::Void),
    f5("table.set_border_width", &[p("table_id", Any), p("border_width", I)], RTy::Void),
    f5("table.clear", &[p("table_id", Any), p("start_column", I), p("start_row", I), d("end_column", I, "na"), d("end_row", I, "na")], RTy::Void),
    f5("table.merge_cells", &[p("table_id", Any), p("start_column", I), p("start_row", I), p("end_column", I), p("end_row", I)], RTy::Void),
    f5("table.delete", &[p("table_id", Any)], RTy::Void),
    v5("table.all", RTy::Array(Elt::Draw("table"))),
    f5("linefill.new", &[p("line1", Any), p("line2", Any), p("color", C)], RTy::Draw("linefill")),
    f5("linefill.set_color", &[p("id", Any), p("color", C)], RTy::Void),
    f5("linefill.get_line1", &[p("id", Any)], RTy::Draw("line")),
    f5("linefill.get_line2", &[p("id", Any)], RTy::Draw("line")),
    f5("linefill.delete", &[p("id", Any)], RTy::Void),
    v5("linefill.all", RTy::Array(Elt::Draw("linefill"))),
    f5(
        "polyline.new",
        &[
            p("points", Any),
            d("curved", B, "false"),
            d("closed", B, "false"),
            d("xloc", Any, "xloc.bar_index"),
            d("line_color", C, "color.blue"),
            d("fill_color", C, "na"),
            d("line_style", Any, "line.style_solid"),
            d("line_width", I, "1"),
            d("force_overlay", B, "false"),
        ],
        RTy::Draw("polyline"),
    ),
    f5("polyline.delete", &[p("id", Any)], RTy::Void),
    v5("polyline.all", RTy::Array(Elt::Draw("polyline"))),
    f5("chart.point.new", &[p("time", I), p("index", I), p("price", N)], RTy::Draw("chart.point")),
    f5("chart.point.from_index", &[p("index", I), p("price", N)], RTy::Draw("chart.point")),
    f5("chart.point.from_time", &[p("time", I), p("price", N)], RTy::Draw("chart.point")),
    f5("chart.point.now", &[d("price", N, "close")], RTy::Draw("chart.point")),
    f5("chart.point.copy", &[p("id", Any)], RTy::Draw("chart.point")),
    c5("label.style_none"),
    c5("label.style_xcross"),
    c5("label.style_cross"),
    c5("label.style_triangleup"),
    c5("label.style_triangledown"),
    c5("label.style_flag"),
    c5("label.style_circle"),
    c5("label.style_arrowup"),
    c5("label.style_arrowdown"),
    c5("label.style_label_up"),
    c5("label.style_label_down"),
    c5("label.style_label_left"),
    c5("label.style_label_right"),
    c5("label.style_label_lower_left"),
    c5("label.style_label_lower_right"),
    c5("label.style_label_upper_left"),
    c5("label.style_label_upper_right"),
    c5("label.style_label_center"),
    c5("label.style_square"),
    c5("label.style_diamond"),
    c5("label.style_text_outline"),
    c5("line.style_solid"),
    c5("line.style_dotted"),
    c5("line.style_dashed"),
    c5("line.style_arrow_left"),
    c5("line.style_arrow_right"),
    c5("line.style_arrow_both"),
    c5("text.format_none"),
    c5("text.format_bold"),
    c5("text.format_italic"),
    // ── Enum constants taken by the output and input functions ──
    c("plot.style_line", RTy::Str),
    c("plot.style_linebr", RTy::Str),
    c("plot.style_stepline", RTy::Str),
    c("plot.style_stepline_diamond", RTy::Str),
    c("plot.style_steplinebr", RTy::Str),
    c("plot.style_histogram", RTy::Str),
    c("plot.style_columns", RTy::Str),
    c("plot.style_area", RTy::Str),
    c("plot.style_areabr", RTy::Str),
    c("plot.style_circles", RTy::Str),
    c("plot.style_cross", RTy::Str),
    c("plot.linestyle_solid", RTy::Str),
    c("plot.linestyle_dashed", RTy::Str),
    c("plot.linestyle_dotted", RTy::Str),
    c("hline.style_solid", RTy::Str),
    c("hline.style_dotted", RTy::Str),
    c("hline.style_dashed", RTy::Str),
    c("shape.xcross", RTy::Str),
    c("shape.cross", RTy::Str),
    c("shape.triangleup", RTy::Str),
    c("shape.triangledown", RTy::Str),
    c("shape.flag", RTy::Str),
    c("shape.circle", RTy::Str),
    c("shape.arrowup", RTy::Str),
    c("shape.arrowdown", RTy::Str),
    c("shape.labelup", RTy::Str),
    c("shape.labeldown", RTy::Str),
    c("shape.square", RTy::Str),
    c("shape.diamond", RTy::Str),
    c("location.abovebar", RTy::Str),
    c("location.belowbar", RTy::Str),
    c("location.top", RTy::Str),
    c("location.bottom", RTy::Str),
    c("location.absolute", RTy::Str),
    c("size.auto", RTy::Str),
    c("size.tiny", RTy::Str),
    c("size.small", RTy::Str),
    c("size.normal", RTy::Str),
    c("size.large", RTy::Str),
    c("size.huge", RTy::Str),
    c("display.all", RTy::Int),
    c("display.none", RTy::Int),
    c("display.data_window", RTy::Int),
    c("display.pane", RTy::Int),
    c("display.price_scale", RTy::Int),
    c("display.status_line", RTy::Int),
    c("format.inherit", RTy::Str),
    c("format.price", RTy::Str),
    c("format.volume", RTy::Str),
    c("format.percent", RTy::Str),
    c("format.mintick", RTy::Str),
    c("scale.right", RTy::Str),
    c("scale.left", RTy::Str),
    c("scale.none", RTy::Str),
    c("position.top_left", RTy::Str),
    c("position.top_center", RTy::Str),
    c("position.top_right", RTy::Str),
    c("position.middle_left", RTy::Str),
    c("position.middle_center", RTy::Str),
    c("position.middle_right", RTy::Str),
    c("position.bottom_left", RTy::Str),
    c("position.bottom_center", RTy::Str),
    c("position.bottom_right", RTy::Str),
    c("text.align_left", RTy::Str),
    c("text.align_center", RTy::Str),
    c("text.align_right", RTy::Str),
    c("text.align_top", RTy::Str),
    c("text.align_bottom", RTy::Str),
    c("text.wrap_auto", RTy::Str),
    c("text.wrap_none", RTy::Str),
    c("xloc.bar_index", RTy::Str),
    c("xloc.bar_time", RTy::Str),
    c("yloc.price", RTy::Str),
    c("yloc.abovebar", RTy::Str),
    c("yloc.belowbar", RTy::Str),
    c("extend.none", RTy::Str),
    c("extend.left", RTy::Str),
    c("extend.right", RTy::Str),
    c("extend.both", RTy::Str),
    c("font.family_default", RTy::Str),
    c("font.family_monospace", RTy::Str),
];

/// Names absent from `TABLE`: (prefix or exact name, phase, visual_only). First match wins.
const PREFIXES: &[(&str, u8, bool)] = &[
    ("request.security_lower_tf", 6, false),
    ("request.security", 6, false),
    ("input.timeframe", 6, false),
    ("input.symbol", 6, false),
    ("ticker.", 6, false),
    ("alertcondition", 0, true),
    ("alert", 0, true),
    ("alert.", 0, true),
    ("log.", 0, true),
    ("label", 5, true),
    ("line", 5, true),
    ("box", 5, true),
    ("table", 5, true),
    ("linefill", 5, true),
    ("polyline", 5, true),
    ("label.", 5, true),
    ("line.", 5, true),
    ("box.", 5, true),
    ("table.", 5, true),
    ("linefill.", 5, true),
    ("polyline.", 5, true),
    ("chart.point", 5, true),
    ("ta.", 4, false),
    ("array.", 4, false),
    ("str.", 4, false),
    ("math.random", 0, false),
    ("input.time", 0, false),
    ("input.text_area", 0, false),
    ("input.price", 0, false),
    ("input.enum", 0, false),
    ("request.", 0, false),
    ("matrix.", 0, false),
    ("map.", 0, false),
    ("chart.", 0, false),
    ("session.", 0, false),
    ("strategy.", 0, false),
    ("currency.", 0, false),
    ("earnings.", 0, false),
    ("dividends.", 0, false),
    ("splits.", 0, false),
    ("barmerge.", 6, false),
    ("order.", 0, false),
    ("adjustment.", 0, false),
    ("backadjustment.", 0, false),
    ("settlement_as_close.", 0, false),
    ("xloc.", 5, true),
    ("yloc.", 5, true),
    ("syminfo.", 0, false),
    ("timeframe.", 0, false),
    ("barstate.", 0, false),
    ("color.", 0, false),
    ("math.", 0, false),
    ("input.", 0, false),
    ("plot.", 0, false),
    ("shape.", 0, false),
    ("location.", 0, false),
    ("size.", 0, false),
    ("display.", 0, false),
    ("format.", 0, false),
    ("hline.", 0, false),
    ("position.", 0, false),
    ("text.", 0, false),
    ("extend.", 0, false),
    ("font.", 0, false),
    ("dayofweek.", 0, false),
    ("runtime.", 0, false),
];

pub fn all() -> &'static [Builtin] {
    TABLE
}

/// A name used as a value (variable or constant first, then a function).
pub fn lookup(name: &str) -> Lookup {
    resolve(name, TABLE.iter().find(|b| b.name == name && b.kind != Kind::Func).or_else(|| TABLE.iter().find(|b| b.name == name)))
}

/// A name used as a call (`hour(t)` vs the `hour` variable).
pub fn lookup_call(name: &str) -> Lookup {
    resolve(name, TABLE.iter().find(|b| b.name == name && b.kind == Kind::Func))
}

fn resolve(name: &str, found: Option<&'static Builtin>) -> Lookup {
    if let Some(b) = found {
        return if b.phase != 0 && b.phase <= CURRENT_PHASE {
            Lookup::Supported(b)
        } else {
            Lookup::NotYet { phase: b.phase, visual_only: b.visual_only }
        };
    }
    for &(prefix, phase, visual_only) in PREFIXES {
        let hit = if prefix.ends_with('.') { name.starts_with(prefix) } else { name == prefix };
        if hit {
            return Lookup::NotYet { phase, visual_only };
        }
    }
    Lookup::Unknown
}

/// One builtin as the editor shows it (autocomplete, signature help).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BuiltinDoc {
    pub name: &'static str,
    /// "func" | "var" | "const"
    pub kind: &'static str,
    pub params: Vec<ParamDoc>,
    pub ret: &'static str,
    pub phase: u8,
    pub supported: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ParamDoc {
    pub name: &'static str,
    pub ty: &'static str,
    pub default: Option<&'static str>,
}

/// The whole table, for the editor.
pub fn docs() -> Vec<BuiltinDoc> {
    TABLE
        .iter()
        .map(|b| BuiltinDoc {
            name: b.name,
            kind: match b.kind {
                Kind::Func => "func",
                Kind::Var => "var",
                Kind::Const => "const",
            },
            params: b.params.iter().map(|p| ParamDoc { name: p.name, ty: pty_word(p.ty), default: p.default }).collect(),
            ret: rty_word(b.ret),
            phase: b.phase,
            supported: b.phase != 0 && b.phase <= CURRENT_PHASE,
        })
        .collect()
}

fn pty_word(t: PTy) -> &'static str {
    match t {
        PTy::Num => "float",
        PTy::Int => "int",
        PTy::Bool => "bool",
        PTy::Color => "color",
        PTy::Str => "string",
        PTy::Any => "any",
        PTy::Ref => "plot",
    }
}

fn rty_word(t: RTy) -> &'static str {
    match t {
        RTy::Float | RTy::Vwap => "float",
        RTy::Int => "int",
        RTy::Bool => "bool",
        RTy::Color => "color",
        RTy::Str => "string",
        RTy::Void => "void",
        RTy::Arg0 | RTy::Arg1 | RTy::Round => "series",
        RTy::Tuple3 => "[float, float, float]",
        RTy::FloatInt => "[float, int]",
        RTy::Array(_) => "array",
        RTy::Draw(k) => k,
        RTy::Elem => "series",
        RTy::PlotRef => "plot",
        RTy::HLineRef => "hline",
    }
}
