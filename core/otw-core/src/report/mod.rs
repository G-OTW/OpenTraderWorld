//! Shared report engine — one document model, two renderers (Markdown + PDF).
//!
//! Consumers (Trading Journal periodic reports, Backtest run reports) build a
//! [`Report`] out of neutral blocks — stat cards, tables, bullets, a time-series
//! chart — and pick a renderer. Neither renderer knows anything about trading;
//! all domain logic stays in the builders.

pub mod markdown;
pub mod pdf;

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
