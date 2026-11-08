//! Shared report engine — one document model, three renderers (Markdown, PDF, PNG).
//!
//! Consumers (Trading Journal periodic reports, Backtest run reports) build a
//! [`Report`] out of neutral blocks — stat cards, tables, bullets, a time-series
//! chart — and pick a renderer. Neither renderer knows anything about trading;
//! all domain logic stays in the builders.
//!
//! The palette and the axis helpers below live here rather than in one renderer
//! because a chart must read the same whichever renderer drew it: the PDF page
//! and the PNG a chat channel receives are the same picture at two resolutions.

// Nothing in the binary draws a bitmap chart yet: the renderer exists for the
// external-control channels, which attach one to a chat reply. Its own tests
// exercise it; the allow keeps the build quiet until that caller lands.
#[allow(dead_code)]
pub mod font;
pub mod markdown;
pub mod pdf;
#[allow(dead_code)]
pub mod png;

/// A complete report document.
pub struct Report {
    pub title: String,
    /// One line under the title (period, scope…).
    pub subtitle: Option<String>,
    /// Machine-readable header pairs — YAML front matter in Markdown, a meta
    /// line in PDF. Keys should be lower_snake_case.
    pub meta: Vec<(String, String)>,
    pub sections: Vec<Section>,
    /// Small closing note (generation context, caveats).
    pub footer_note: Option<String>,
}

impl Report {
    pub fn new(title: impl Into<String>) -> Self {
        Report {
            title: title.into(),
            subtitle: None,
            meta: Vec::new(),
            sections: Vec::new(),
            footer_note: None,
        }
    }
}

/// A titled group of blocks. `level` 2 renders as `##` (major), 3 as `###`.
pub struct Section {
    pub heading: String,
    pub level: u8,
    pub blocks: Vec<Block>,
}

impl Section {
    pub fn new(heading: impl Into<String>) -> Self {
        Section { heading: heading.into(), level: 2, blocks: Vec::new() }
    }
    pub fn sub(heading: impl Into<String>) -> Self {
        Section { heading: heading.into(), level: 3, blocks: Vec::new() }
    }
}

pub enum Block {
    Paragraph(String),
    Bullets(Vec<String>),
    /// Headline figures — a card grid in PDF, a Metric/Value table in Markdown.
    Stats(Vec<Stat>),
    Table(Table),
    /// Time-series line chart — vector-drawn in PDF, a sparkline in Markdown.
    Chart(Chart),
}

/// Semantic color of a value (PDF renders green/red/grey, Markdown ignores it).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Neutral,
    Positive,
    Negative,
    Muted,
}

pub struct Stat {
    pub label: String,
    pub value: String,
    /// Small secondary text next to the value ("10 closed · 2 open").
    pub hint: Option<String>,
    pub tone: Tone,
}

impl Stat {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Stat { label: label.into(), value: value.into(), hint: None, tone: Tone::Neutral }
    }
    pub fn toned(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    /// Tone from the sign of a monetary value: positive green, negative red.
    pub fn signed(self, v: f64) -> Self {
        let tone = if v > 0.0 {
            Tone::Positive
        } else if v < 0.0 {
            Tone::Negative
        } else {
            Tone::Neutral
        };
        self.toned(tone)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Right,
}

pub struct Table {
    pub headers: Vec<String>,
    /// Per-column alignment; shorter than `headers` means the rest default Left.
    pub aligns: Vec<Align>,
    pub rows: Vec<Vec<Cell>>,
}

pub struct Cell {
    pub text: String,
    pub tone: Tone,
}

impl Cell {
    pub fn new(text: impl Into<String>) -> Self {
        Cell { text: text.into(), tone: Tone::Neutral }
    }
    pub fn toned(text: impl Into<String>, tone: Tone) -> Self {
        Cell { text: text.into(), tone }
    }
    /// Tone from the sign of the numeric value the cell shows.
    pub fn signed(text: impl Into<String>, v: f64) -> Self {
        let tone = if v > 0.0 {
            Tone::Positive
        } else if v < 0.0 {
            Tone::Negative
        } else {
            Tone::Neutral
        };
        Cell { text: text.into(), tone }
    }
}

/// A single-series line chart. `x` values are unix seconds when `time_axis`
/// (labels formatted as dates), otherwise plain numbers.
pub struct Chart {
    /// What the y axis measures ("Cumulative net PnL (USD)").
    pub y_label: String,
    pub points: Vec<(f64, f64)>,
    /// Optional horizontal reference line (e.g. 0 for a PnL curve).
    pub baseline: Option<f64>,
    pub time_axis: bool,
}

/// Format a float with `d` decimals and thousands separators ("–" if not finite).
pub fn fmt_num(x: f64, d: usize) -> String {
    if !x.is_finite() {
        return "–".into();
    }
    let mut s = format!("{x:.*}", d);
    // Never print negative zero ("-0.00").
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
        s.remove(0);
    }
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') { ("-", r) } else { ("", s.as_str()) };
    let (int_part, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let mut grouped = String::new();
    let bytes = int_part.as_bytes();
    for (i, ch) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*ch as char);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

// ── Shared chart palette ────────────────────────────────────────────────────
// Normalised sRGB, because PDF wants floats; the PNG rasteriser scales to u8.

pub(crate) type Rgb = (f64, f64, f64);

pub(crate) const TEXT: Rgb = (0.10, 0.12, 0.16);
pub(crate) const MUTED: Rgb = (0.44, 0.47, 0.53);
pub(crate) const BORDER: Rgb = (0.84, 0.86, 0.89);
pub(crate) const STRIPE: Rgb = (0.955, 0.962, 0.972);
pub(crate) const CARD_BG: Rgb = (0.965, 0.970, 0.978);
pub(crate) const ACCENT: Rgb = (0.23, 0.47, 0.93);
pub(crate) const ACCENT_SOFT: Rgb = (0.875, 0.915, 0.985);
pub(crate) const GREEN: Rgb = (0.075, 0.55, 0.32);
pub(crate) const RED: Rgb = (0.83, 0.24, 0.24);
/// Page/plot ground. Only the bitmap renderer needs it (a PDF page is already white).
pub(crate) const PAPER: Rgb = (1.0, 1.0, 1.0);

// ── Shared axis helpers ─────────────────────────────────────────────────────

/// Round a raw axis step up to a human one (1, 2, 2.5, 5, 10 × a power of ten).
pub(crate) fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 || !raw.is_finite() {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    let nice = if n <= 1.0 {
        1.0
    } else if n <= 2.0 {
        2.0
    } else if n <= 2.5 {
        2.5
    } else if n <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

/// Compact axis number: 12.3k / 4.5M below/above thousand-scale.
pub(crate) fn compact_num(v: f64) -> String {
    let a = v.abs();
    if a >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if a >= 10_000.0 {
        format!("{:.1}k", v / 1_000.0)
    } else if a >= 100.0 {
        format!("{v:.0}")
    } else {
        fmt_num(v, 2)
    }
}

/// Format a unix-seconds x value as a date label; include the year on long spans.
pub(crate) fn fmt_ts(secs: f64, span_secs: f64) -> String {
    use time::macros::format_description;
    let short = format_description!("[month repr:short] [day padding:none]");
    let long = format_description!("[month repr:short] [day padding:none], [year]");
    match time::OffsetDateTime::from_unix_timestamp(secs as i64) {
        Ok(dt) => {
            let f = if span_secs > 300.0 * 86_400.0 { long } else { short };
            dt.format(f).unwrap_or_else(|_| String::from("?"))
        }
        Err(_) => String::from("?"),
    }
}
