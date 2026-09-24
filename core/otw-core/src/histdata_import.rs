//! Reading an OHLCV series out of a file the user already has.
//!
//! Same front half as every other import in the app (`crate::import`): bytes become a
//! grid, columns are scored against the shared multilingual dictionary and against their
//! own values, and the proposed mapping is *shown* before a single bar is written. What
//! differs is the far end: a row here is a **period**, not a transaction, and it lands in
//! `histdata_bars` under a dataset filed against the reserved `import` provider.
//!
//! Three things this import will not do:
//!   - **guess the instrument.** A file says "Close"; it does not say the bars are AAPL
//!     daily. Ticker, timeframe and asset type are asked for, and a timeframe that is not
//!     a timeframe is an error naming the vocabulary, never a string written to the
//!     catalog for the alignment layer to choke on later.
//!   - **guess the clock.** A naive timestamp is read in the offset the user names; an
//!     epoch integer is read at the precision the column actually carries. Both are shown
//!     back as real dates in the preview, which is the only way to notice a wrong one.
//!   - **average two tapes.** The source (the broker or vendor the file came from) is part
//!     of an imported dataset's identity, so the same ticker exported by two brokers stays
//!     two series. Re-importing the same file, though, lands on the same dataset and
//!     overwrites bar for bar: an import is idempotent, never additive.
//!
//! The file rides in the request body as base64 on every call, like the journal and
//! portfolio imports: no server-side upload state, no temp file, nothing to clean up.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::histdata::{self as store, Bar};

use crate::import::detect::{self, Profile};
use crate::import::dict;
use crate::import::parse::{self, DateOrder, Grid};
use crate::timeframe::Timeframe;

/// Learned header aliases are per target set (see `crate::import::dict`).
const ALIAS_SCOPE: &str = "histdata";

/// Bars one commit may write. A decade of 1m bars is ~3.5M rows and belongs in a download,
/// not in a browser round trip; the cap is what a 20 MB file can plausibly hold anyway.
pub const MAX_BARS: usize = 1_000_000;

/// Timestamps outside this are a misread column (a price read as an epoch, a two-digit
/// year), not history. Bounds are deliberately wide: some indices publish back to 1900.
const TS_MIN_YEAR: i32 = 1800;
const TS_MAX_YEAR: i32 = 2200;

// ── The mapping document ─────────────────────────────────────────────────────

/// How to read the file. Everything about the *file*; nothing about the instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
    /// 0-based index of the header row inside the file (skips export preambles).
    #[serde(default)]
    pub header_row: Option<usize>,
    #[serde(default)]
    pub delimiter: Option<String>,
    /// Which table of a stacked export to read. `None` = pick the biggest,
    /// `Some("")` = read the file flat, ignoring the sections.
    #[serde(default)]
    pub section: Option<String>,
    /// Decimal separator, forced for the whole file: "auto" | "." | ",".
    #[serde(default = "auto")]
    pub decimal: String,
    /// "auto" | "dmy" | "mdy" | "ymd".
    #[serde(default = "auto")]
    pub date_order: String,
    /// Minutes to apply to timestamps that carry no zone of their own. A bar is a period,
    /// so this is not cosmetic: it decides which period every row in the file belongs to.
    #[serde(default)]
    pub tz_offset: i32,
    /// How to read the timestamp column: "auto" | "text" | "s" | "ms" | "us" | "ns".
    #[serde(default = "auto")]
    pub ts_unit: String,
    /// column index (as a string) → target field id, or "ignore".
    #[serde(default)]
    pub columns: BTreeMap<String, String>,
}

fn auto() -> String {
    "auto".to_string()
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            header_row: None,
            delimiter: None,
            section: None,
            decimal: auto(),
            date_order: auto(),
            tz_offset: 0,
            ts_unit: auto(),
            columns: BTreeMap::new(),
        }
    }
}

impl Mapping {
    fn delimiter_char(&self) -> Option<char> {
        match self.delimiter.as_deref() {
            Some("\\t") | Some("\t") => Some('\t'),
            Some(s) => s.chars().next(),
            None => None,
        }
    }
}

// ── What the bars are ────────────────────────────────────────────────────────

/// The instrument the file is bars *of*, and how the user files it. None of it can be
/// read from the file, so all of it is asked.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Destination {
    /// Free text (crypto, equity, etf, fx, index, …): it only groups the catalog.
    #[serde(default)]
    pub asset_type: String,
    #[serde(default)]
    pub ticker: String,
    /// Canonical timeframe (`1m`, `4h`, `1d`, `1w`, `1M`).
    #[serde(default)]
    pub timeframe: String,
    /// Where the file came from: a broker, a venue, a data vendor. Free text, and part of
    /// the dataset's identity.
    #[serde(default)]
    pub source: String,
    /// The user's own name for the series.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Destination {
    fn asset_type(&self) -> String {
        self.asset_type.trim().to_lowercase()
    }
    fn ticker(&self) -> String {
        self.ticker.trim().to_string()
    }
    fn timeframe(&self) -> String {
        self.timeframe.trim().to_string()
    }
    fn source(&self) -> String {
        self.source.trim().to_string()
    }
    /// Tags, trimmed, de-duplicated, order kept.
    fn tags(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in &self.tags {
            let t = t.trim();
            if !t.is_empty() && !out.iter().any(|k| k.eq_ignore_ascii_case(t)) {
                out.push(t.to_string());
            }
        }
        out
    }

    /// Everything a commit needs, or the first thing missing said as a fixable sentence.
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.ticker().is_empty(), "a ticker is required: the file does not say which instrument these bars are");
        anyhow::ensure!(!self.asset_type().is_empty(), "an asset type is required (crypto, equity, etf, fx, index, …)");
        let tf = self.timeframe();
        anyhow::ensure!(!tf.is_empty(), "a timeframe is required: 1m, 5m, 15m, 1h, 4h, 1d, 1w or 1M");
        Timeframe::parse(&tf)
            .map_err(|e| anyhow::anyhow!("{tf:?} is not a timeframe ({e}). Use 1m, 5m, 15m, 1h, 4h, 1d, 1w or 1M"))?;
        Ok(())
    }
}

// ── Reading one row ──────────────────────────────────────────────────────────

/// How a numeric timestamp column is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TsUnit {
    Auto,
    Text,
    Secs,
    Millis,
    Micros,
    Nanos,
}

impl TsUnit {
    fn from_str(s: &str) -> Self {
        match s {
            "text" => TsUnit::Text,
            "s" | "sec" | "secs" => TsUnit::Secs,
            "ms" => TsUnit::Millis,
            "us" => TsUnit::Micros,
            "ns" => TsUnit::Nanos,
            _ => TsUnit::Auto,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            TsUnit::Auto => "auto",
            TsUnit::Text => "text",
            TsUnit::Secs => "s",
            TsUnit::Millis => "ms",
            TsUnit::Micros => "us",
            TsUnit::Nanos => "ns",
        }
    }
    fn divisor(&self) -> Option<f64> {
        match self {
            TsUnit::Secs => Some(1.0),
            TsUnit::Millis => Some(1e3),
            TsUnit::Micros => Some(1e6),
            TsUnit::Nanos => Some(1e9),
            _ => None,
        }
    }
}

/// An integer that can only be an epoch, and at which precision.
///
/// Decided by magnitude, not by digit count, so a pre-1970 (negative) second stamp and a
/// 2038 one are read the same way. The windows do not overlap: seconds run out of range
/// four orders of magnitude before milliseconds begin.
fn epoch_unit(n: f64) -> Option<TsUnit> {
    let a = n.abs();
    if (1e8..4e9).contains(&a) {
        Some(TsUnit::Secs)
    } else if (1e11..4e12).contains(&a) {
        Some(TsUnit::Millis)
    } else if (1e14..4e15).contains(&a) {
        Some(TsUnit::Micros)
    } else if (1e17..4e18).contains(&a) {
        Some(TsUnit::Nanos)
    } else {
        None
    }
}

/// Does this column look like a column of epoch integers? Asked of the whole column,
/// never of one cell, so one stray value cannot flip the reading of the rest.
fn looks_epoch(values: &[&str]) -> bool {
    let mut seen = 0usize;
    let mut hits = 0usize;
    for v in values.iter().take(200) {
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        seen += 1;
        if parse::parse_number(v, '.').and_then(epoch_unit).is_some() {
            hits += 1;
        }
    }
    seen > 0 && hits * 10 >= seen * 9
}

struct Reader<'a> {
    grid: &'a Grid,
    mapping: &'a Mapping,
    profiles: &'a [Profile],
    /// field id → column index
    fields: HashMap<String, usize>,
    ts_unit: TsUnit,
}

impl<'a> Reader<'a> {
    fn new(grid: &'a Grid, mapping: &'a Mapping, profiles: &'a [Profile]) -> Self {
        let mut fields = HashMap::new();
        for (key, target) in &mapping.columns {
            let Ok(idx) = key.parse::<usize>() else { continue };
            if idx >= grid.width() || target == "ignore" || target.is_empty() {
                continue;
            }
            // A bar has no free-form fields: there is nowhere to put a custom column.
            if target.starts_with("field:") {
                continue;
            }
            fields.insert(target.clone(), idx);
        }
        let mut ts_unit = TsUnit::from_str(&mapping.ts_unit);
        if ts_unit == TsUnit::Auto {
            // Resolve "auto" once, over the column, rather than per cell.
            if let Some(idx) = fields.get("ts") {
                if looks_epoch(&grid.column(*idx)) {
                    ts_unit = grid
                        .column(*idx)
                        .iter()
                        .find_map(|v| parse::parse_number(v, '.').and_then(epoch_unit))
                        .unwrap_or(TsUnit::Text);
                } else {
                    ts_unit = TsUnit::Text;
                }
            }
        }
        Self { grid, mapping, profiles, fields, ts_unit }
    }

    fn raw(&self, row: &'a [String], field: &str) -> Option<&'a str> {
        let idx = *self.fields.get(field)?;
        let v = self.grid.cell(row, idx);
        (!v.is_empty()).then_some(v)
    }

    fn num(&self, row: &[String], field: &str) -> Option<f64> {
        let idx = *self.fields.get(field)?;
        let raw = self.raw(row, field)?;
        let decimal = match self.mapping.decimal.as_str() {
            "." => '.',
            "," => ',',
            _ => self.profiles[idx].decimal,
        };
        parse::parse_number(raw, decimal)
    }

    fn ts(&self, row: &[String]) -> Option<OffsetDateTime> {
        let idx = *self.fields.get("ts")?;
        let raw = self.raw(row, "ts")?;
        if let Some(div) = self.ts_unit.divisor() {
            let n = parse::parse_number(raw, '.')?;
            let secs = (n / div).trunc() as i64;
            return OffsetDateTime::from_unix_timestamp(secs).ok();
        }
        let order = match DateOrder::from_str(&self.mapping.date_order) {
            DateOrder::Auto => self.profiles[idx].date_order.unwrap_or(DateOrder::Auto),
            explicit => explicit,
        };
        parse::parse_datetime(raw, order, self.mapping.tz_offset)
    }
}

// ── Rows → bars ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RowError {
    /// 1-based line number in the source file (header row included).
    pub row: usize,
    pub message: String,
    pub raw: String,
}

/// A bar that was read, with where it came from and what is odd about it.
pub struct BuiltBar {
    pub bar: Bar,
    pub source_row: usize,
    /// The bar's own high/low do not contain its open/close.
    pub inconsistent: bool,
    /// An earlier row in the same file carried this timestamp too; this one wins.
    pub file_duplicate: bool,
}

pub struct BuildResult {
    pub bars: Vec<BuiltBar>,
    pub errors: Vec<RowError>,
    /// Price fields filled from `close` because the file has no column for them.
    pub synthesized: Vec<&'static str>,
    /// How the timestamp column was actually read.
    pub ts_unit: &'static str,
}

/// Read every row. Bars come back sorted by timestamp, one per timestamp, last row wins.
pub fn build(grid: &Grid, mapping: &Mapping, profiles: &[Profile]) -> BuildResult {
    let r = Reader::new(grid, mapping, profiles);
    let has = |f: &str| r.fields.contains_key(f);
    let synthesized: Vec<&'static str> = ["open", "high", "low"]
        .into_iter()
        .filter(|f| !has(f))
        .collect();

    let mut bars: Vec<BuiltBar> = Vec::new();
    let mut errors: Vec<RowError> = Vec::new();

    if !has("ts") {
        errors.push(RowError {
            row: grid.header_row + 1,
            message: "no column is mapped to the bar timestamp".into(),
            raw: String::new(),
        });
        return BuildResult { bars, errors, synthesized, ts_unit: r.ts_unit.label() };
    }
    if !has("close") {
        errors.push(RowError {
            row: grid.header_row + 1,
            message: "no column is mapped to the close: a bar with no close is not a bar".into(),
            raw: String::new(),
        });
        return BuildResult { bars, errors, synthesized, ts_unit: r.ts_unit.label() };
    }

    for (i, row) in grid.rows.iter().enumerate() {
        let line = grid.row_lines.get(i).copied().unwrap_or(i + 1);
        let fail = |msg: String| RowError { row: line, message: msg, raw: row.join(" | ") };

        let Some(ts) = r.ts(row) else {
            errors.push(fail(match r.raw(row, "ts") {
                Some(v) => format!("cannot read {v:?} as a timestamp"),
                None => "the timestamp cell is empty".into(),
            }));
            continue;
        };
        if ts.year() < TS_MIN_YEAR || ts.year() > TS_MAX_YEAR {
            errors.push(fail(format!(
                "{} is not a plausible bar timestamp. Check the timestamp column and its unit",
                ts.format(&Rfc3339).unwrap_or_else(|_| ts.year().to_string())
            )));
            continue;
        }

        let Some(close) = r.num(row, "close") else {
            errors.push(fail(match r.raw(row, "close") {
                Some(v) => format!("cannot read {v:?} as a price"),
                None => "the close cell is empty".into(),
            }));
            continue;
        };
        // A missing open/high/low column makes a flat bar on the close; a mapped column
        // that holds nothing on this row is a hole in the file, and holes are errors.
        let price = |field: &'static str| -> Result<f64, RowError> {
            if !has(field) {
                return Ok(close);
            }
            r.num(row, field).ok_or_else(|| match r.raw(row, field) {
                Some(v) => fail(format!("cannot read {v:?} as the {field}")),
                None => fail(format!("the {field} cell is empty")),
            })
        };
        let (open, high, low) = match (price("open"), price("high"), price("low")) {
            (Ok(o), Ok(h), Ok(l)) => (o, h, l),
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                errors.push(e);
                continue;
            }
        };
        if [open, high, low, close].iter().any(|p| !p.is_finite() || *p <= 0.0) {
            errors.push(fail("a bar price is zero, negative or not a number".into()));
            continue;
        }
        let volume = r.num(row, "volume").unwrap_or(0.0);
        if volume < 0.0 || !volume.is_finite() {
            errors.push(fail(format!("volume {volume} is not a volume")));
            continue;
        }

        bars.push(BuiltBar {
            bar: Bar {
                ts,
                open,
                high,
                low,
                close,
                volume,
                adj_open: r.num(row, "adj_open"),
                adj_high: r.num(row, "adj_high"),
                adj_low: r.num(row, "adj_low"),
                adj_close: r.num(row, "adj_close"),
            },
            source_row: line,
            inconsistent: high < open.max(close) || low > open.min(close) || high < low,
            file_duplicate: false,
        });
    }

    // One bar per period. The file's own order decides which wins (the last one), and the
    // row that lost is *counted*, not dropped silently: a file that lists the same period
    // twice is either a bad export or two tapes concatenated, and the user has to know.
    bars.sort_by_key(|b| b.bar.ts);
    let mut deduped: Vec<BuiltBar> = Vec::with_capacity(bars.len());
    for b in bars {
        match deduped.last_mut() {
            Some(prev) if prev.bar.ts == b.bar.ts => {
                *prev = BuiltBar { file_duplicate: true, ..b };
            }
            _ => deduped.push(b),
        }
    }

    BuildResult { bars: deduped, errors, synthesized, ts_unit: r.ts_unit.label() }
}

// ── Analysis payload ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PreviewBar {
    pub source_row: usize,
    pub ts: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub adj_close: Option<f64>,
    pub inconsistent: bool,
    /// This period is already stored in the dataset the commit would write to.
    pub existing: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct Stats {
    /// Data rows in the file.
    pub rows: usize,
    /// Columns in the file.
    pub columns: usize,
    /// Bars the commit would write.
    pub bars: usize,
    pub errors: usize,
    /// Rows dropped because another row carried the same period.
    pub file_duplicates: usize,
    /// Bars that would overwrite a period the dataset already holds.
    pub existing: usize,
    /// Bars whose high/low do not contain their open/close.
    pub inconsistent: usize,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    /// The spacing the timestamps actually have, as a timeframe (the most common gap).
    pub detected_timeframe: Option<String>,
    /// Periods missing between the first and the last bar, at that spacing.
    pub missing_periods: Option<i64>,
}

/// The dataset a commit would land in, when one already exists.
#[derive(Debug, Serialize)]
pub struct ExistingDataset {
    pub id: Uuid,
    pub bar_count: i64,
    pub range_from: Option<String>,
    pub range_to: Option<String>,
    pub label: Option<String>,
    pub tags: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct Analysis {
    pub filename: String,
    pub delimiter: String,
    pub header_row: usize,
    pub preamble: Vec<String>,
    pub sections: Vec<String>,
    pub section: Option<String>,
    pub headers: Vec<String>,
    pub mapping: Mapping,
    pub columns: Vec<detect::ColumnInfo>,
    pub destination: Destination,
    pub preview: Vec<PreviewBar>,
    pub errors: Vec<RowError>,
    pub stats: Stats,
    /// Price fields filled from the close because the file carries no column for them.
    pub synthesized: Vec<&'static str>,
    /// How the timestamp column was read: text | s | ms | us | ns.
    pub ts_unit: String,
    pub existing_dataset: Option<ExistingDataset>,
    /// True when the mapping came from the detector rather than from the client.
    pub detected: bool,
}

#[derive(Debug, Serialize)]
pub struct CommitReport {
    pub dataset_id: Uuid,
    pub provider: &'static str,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    pub source: String,
    /// Bars written (inserted plus overwritten).
    pub written: usize,
    pub inserted: u64,
    pub updated: u64,
    pub file_duplicates: usize,
    pub errors: usize,
    pub range_from: Option<String>,
    pub range_to: Option<String>,
    pub bar_count: i64,
}

// ── Pipeline ─────────────────────────────────────────────────────────────────

/// The spacing the file's own timestamps have, and how many periods are missing between
/// its ends at that spacing.
///
/// The *most common* gap, not the smallest and not the average: a daily series has a
/// three-day gap every weekend and a one-day gap the rest of the time, and only the mode
/// answers "what timeframe is this file" correctly for both.
fn spacing(bars: &[BuiltBar]) -> (Option<String>, Option<i64>) {
    if bars.len() < 2 {
        return (None, None);
    }
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for w in bars.windows(2) {
        let d = (w[1].bar.ts - w[0].bar.ts).whole_seconds();
        if d > 0 {
            *counts.entry(d).or_default() += 1;
        }
    }
    let Some((&step, _)) = counts.iter().max_by_key(|(_, n)| **n) else {
        return (None, None);
    };
    let span = (bars[bars.len() - 1].bar.ts - bars[0].bar.ts).whole_seconds();
    let expected = span / step + 1;
    let missing = (expected - bars.len() as i64).max(0);
    (Some(label_secs(step)), Some(missing))
}

/// Seconds as the timeframe string the app writes, when they are one.
fn label_secs(secs: i64) -> String {
    const WEEK: i64 = 604_800;
    const DAY: i64 = 86_400;
    match secs {
        s if s % WEEK == 0 => format!("{}w", s / WEEK),
        s if s % DAY == 0 => format!("{}d", s / DAY),
        s if s % 3_600 == 0 => format!("{}h", s / 3_600),
        s if s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

/// Fill the empty half of the destination from the file's own metadata, when it carries
/// any. Exporting a dataset to Parquet and importing it back should not mean retyping the
/// instrument, and a `source` survives the round trip so provenance is not lost either.
fn adopt_file_metadata(dest: &mut Destination, filename: &str, bytes: &[u8]) {
    if !crate::import::parquet::looks_parquet(filename, bytes) {
        return;
    }
    let meta = crate::import::parquet::file_metadata(bytes);
    if meta.is_empty() {
        return;
    }
    let fill = |field: &mut String, key: &str| {
        if field.trim().is_empty() {
            if let Some(v) = meta.get(key) {
                *field = v.clone();
            }
        }
    };
    fill(&mut dest.ticker, "ticker");
    fill(&mut dest.asset_type, "asset_type");
    fill(&mut dest.timeframe, "timeframe");
    fill(&mut dest.source, "source");
    fill(&mut dest.label, "label");
}

/// Detect a mapping for a freshly read grid: the shared detector, plus the one thing it
/// cannot know here.
fn propose(
    grid: &Grid,
    profiles: &[Profile],
    aliases: &HashMap<String, String>,
    base: Mapping,
) -> (Mapping, HashMap<usize, &'static str>) {
    let s = detect::suggest(grid, profiles, aliases, &dict::HISTDATA);
    let mut columns: BTreeMap<String, String> = (0..grid.width())
        .map(|i| {
            (
                i.to_string(),
                s.columns.get(&i).cloned().unwrap_or_else(|| "ignore".into()),
            )
        })
        .collect();
    // An epoch column profiles as numbers, so neither its header nor its values will have
    // offered it as a date. It is still the timestamp, and a file that writes one writes
    // nothing else shaped like one.
    if !columns.values().any(|v| v == "ts") {
        if let Some(i) = (0..grid.width()).find(|i| {
            columns.get(&i.to_string()).is_some_and(|v| v == "ignore") && looks_epoch(&grid.column(*i))
        }) {
            columns.insert(i.to_string(), "ts".into());
        }
    }
    let m = Mapping {
        header_row: Some(grid.header_row),
        delimiter: Some(grid.delimiter.to_string()),
        decimal: s.decimal.to_string(),
        date_order: s.date_order.as_str().to_string(),
        section: grid.section.clone(),
        columns,
        ..base
    };
    (m, s.confidence)
}

/// Read the file, decide (or accept) a mapping, and build every bar it would write.
/// Writes nothing. `preview_limit` caps how many bars come back; the statistics always
/// cover the whole file.
pub async fn analyze(
    pool: &PgPool,
    filename: &str,
    bytes: &[u8],
    supplied: Option<Mapping>,
    mut destination: Destination,
    section: Option<String>,
    preview_limit: usize,
) -> anyhow::Result<Analysis> {
    let detected = supplied.is_none();
    let mut base = supplied.unwrap_or_default();
    // Pointing a fresh detection at another table of an export: its columns are different
    // ones entirely, so this is a re-detection, not an edit of the mapping.
    if detected {
        base.section = section;
    }
    let grid = parse::read_grid(
        filename,
        bytes,
        base.header_row,
        base.delimiter_char(),
        base.section.as_deref(),
    )?;
    let profiles: Vec<Profile> = (0..grid.width())
        .map(|i| detect::profile_column(&grid, i))
        .collect();

    // A file this app exported says what it is. Only blanks are filled, so anything the
    // user has already typed wins, and a file from anywhere else changes nothing.
    adopt_file_metadata(&mut destination, filename, bytes);

    let (mapping, confidence) = if detected {
        let aliases = otw_store::imports::load_aliases(pool, ALIAS_SCOPE)
            .await
            .unwrap_or_default();
        propose(&grid, &profiles, &aliases, base)
    } else {
        let mut m = base;
        m.header_row = Some(grid.header_row);
        m.delimiter = Some(grid.delimiter.to_string());
        m.section = m.section.or_else(|| grid.section.clone());
        (m, Default::default())
    };

    let built = build(&grid, &mapping, &profiles);
    let (detected_timeframe, missing_periods) = spacing(&built.bars);
    // The file knows its own spacing; the user should not have to type it. Only offered,
    // never forced: a user re-filing 1m bars as an hourly series is entitled to.
    if destination.timeframe.trim().is_empty() {
        if let Some(tf) = detected_timeframe.as_deref() {
            if Timeframe::parse(tf).is_ok() {
                destination.timeframe = tf.to_string();
            }
        }
    }

    // What a commit would land on, and which of its periods it would overwrite.
    let existing_row = if destination.validate().is_ok() {
        store::find_import_dataset(
            pool,
            &destination.asset_type(),
            &destination.ticker(),
            &destination.timeframe(),
            &destination.source(),
        )
        .await?
    } else {
        None
    };
    let existing_ts: std::collections::HashSet<OffsetDateTime> = match &existing_row {
        Some(d) if !built.bars.is_empty() => {
            let all: Vec<OffsetDateTime> = built.bars.iter().map(|b| b.bar.ts).collect();
            store::existing_ts(pool, d.id, &all).await.unwrap_or_default()
        }
        _ => Default::default(),
    };

    let stats = Stats {
        rows: grid.rows.len(),
        columns: grid.width(),
        bars: built.bars.len(),
        errors: built.errors.len(),
        file_duplicates: built.bars.iter().filter(|b| b.file_duplicate).count(),
        existing: built.bars.iter().filter(|b| existing_ts.contains(&b.bar.ts)).count(),
        inconsistent: built.bars.iter().filter(|b| b.inconsistent).count(),
        first_ts: built.bars.first().map(|b| iso(b.bar.ts)),
        last_ts: built.bars.last().map(|b| iso(b.bar.ts)),
        detected_timeframe,
        missing_periods,
    };

    let preview = built
        .bars
        .iter()
        .take(preview_limit)
        .map(|b| PreviewBar {
            source_row: b.source_row,
            ts: iso(b.bar.ts),
            open: b.bar.open,
            high: b.bar.high,
            low: b.bar.low,
            close: b.bar.close,
            volume: b.bar.volume,
            adj_close: b.bar.adj_close,
            inconsistent: b.inconsistent,
            existing: existing_ts.contains(&b.bar.ts),
        })
        .collect();

    let columns = detect::describe(&grid, &profiles, &mapping.columns, &confidence);

    Ok(Analysis {
        filename: filename.to_string(),
        delimiter: grid.delimiter.to_string(),
        header_row: grid.header_row,
        preamble: grid.preamble.clone(),
        sections: grid.sections.clone(),
        section: grid.section.clone(),
        headers: grid.headers.clone(),
        mapping,
        columns,
        destination,
        preview,
        errors: built.errors,
        stats,
        synthesized: built.synthesized,
        ts_unit: built.ts_unit.to_string(),
        existing_dataset: existing_row.map(|d| ExistingDataset {
            id: d.id,
            bar_count: d.bar_count,
            range_from: d.range_from.map(iso),
            range_to: d.range_to.map(iso),
            label: d.label,
            tags: d.tags,
        }),
        detected,
    })
}

/// Write the bars the user validated. Idempotent: the same file imported twice leaves the
/// same dataset, bar for bar.
pub async fn commit(
    pool: &PgPool,
    filename: &str,
    bytes: &[u8],
    mapping: Mapping,
    destination: Destination,
) -> anyhow::Result<CommitReport> {
    destination.validate()?;
    let grid = parse::read_grid(
        filename,
        bytes,
        mapping.header_row,
        mapping.delimiter_char(),
        mapping.section.as_deref(),
    )?;
    let profiles: Vec<Profile> = (0..grid.width())
        .map(|i| detect::profile_column(&grid, i))
        .collect();
    let built = build(&grid, &mapping, &profiles);
    anyhow::ensure!(
        !built.bars.is_empty(),
        "no bar could be read from this file. Check the column mapping"
    );
    anyhow::ensure!(
        built.bars.len() <= MAX_BARS,
        "{} bars is past the {MAX_BARS} an import may write in one go. Split the file",
        built.bars.len()
    );

    let (asset_type, ticker, timeframe, source) = (
        destination.asset_type(),
        destination.ticker(),
        destination.timeframe(),
        destination.source(),
    );
    let tags = serde_json::to_value(destination.tags())?;
    let dataset_id = store::upsert_import_dataset(
        pool,
        &asset_type,
        &ticker,
        &timeframe,
        &source,
        destination.label.trim(),
        &tags,
    )
    .await?;

    let bars: Vec<Bar> = built.bars.iter().map(|b| b.bar.clone()).collect();
    let inserted = store::write_bars(pool, dataset_id, &bars).await?;
    // Learn the header→field pairs the user settled on, so the next file of this shape
    // maps itself. Only the columns that are actually mapped teach anything.
    for (key, target) in &mapping.columns {
        if target == "ignore" || target.is_empty() {
            continue;
        }
        let Ok(idx) = key.parse::<usize>() else { continue };
        let Some(header) = grid.headers.get(idx) else { continue };
        let norm = parse::normalize(header);
        if !norm.is_empty() {
            otw_store::imports::record_alias(pool, ALIAS_SCOPE, &norm, target)
                .await
                .ok();
        }
    }

    let saved = store::get_dataset(pool, dataset_id).await?;
    Ok(CommitReport {
        dataset_id,
        provider: crate::histdata::IMPORT_PROVIDER,
        asset_type,
        ticker,
        timeframe,
        source,
        written: bars.len(),
        inserted,
        updated: (bars.len() as u64).saturating_sub(inserted),
        file_duplicates: built.bars.iter().filter(|b| b.file_duplicate).count(),
        errors: built.errors.len(),
        range_from: saved.as_ref().and_then(|d| d.range_from).map(iso),
        range_to: saved.as_ref().and_then(|d| d.range_to).map(iso),
        bar_count: saved.map(|d| d.bar_count).unwrap_or(bars.len() as i64),
    })
}

fn iso(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Detect + build the way `analyze` does, without a database.
    fn run(name: &str, file: &str) -> (Mapping, BuildResult) {
        run_bytes(name, file.as_bytes())
    }

    /// The same, for a format that is not text.
    fn run_bytes(name: &str, file: &[u8]) -> (Mapping, BuildResult) {
        let grid = parse::read_grid(name, file, None, None, None).expect("grid");
        let profiles: Vec<Profile> = (0..grid.width())
            .map(|i| detect::profile_column(&grid, i))
            .collect();
        let (mapping, _) = propose(&grid, &profiles, &Default::default(), Mapping::default());
        let built = build(&grid, &mapping, &profiles);
        (mapping, built)
    }

    fn target_of(m: &Mapping, idx: usize) -> &str {
        m.columns.get(&idx.to_string()).map(|s| s.as_str()).unwrap_or("ignore")
    }

    /// The shape every equity vendor hands out. The adjusted close must not be read as
    /// the close: they differ by every split the instrument ever had.
    #[test]
    fn reads_a_yahoo_export() {
        let (m, b) = run(
            "AAPL.csv",
            "Date,Open,High,Low,Close,Adj Close,Volume\n\
             2024-01-02,187.15,188.44,183.89,185.64,184.90,82488700\n\
             2024-01-03,184.22,185.88,183.43,184.25,183.51,58414500\n",
        );
        assert_eq!(target_of(&m, 0), "ts");
        assert_eq!(target_of(&m, 4), "close");
        assert_eq!(target_of(&m, 5), "adj_close");
        assert_eq!(target_of(&m, 6), "volume");
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert_eq!(b.bars.len(), 2);
        assert_eq!(b.bars[0].bar.close, 185.64);
        assert_eq!(b.bars[0].bar.adj_close, Some(184.90));
        assert!(b.synthesized.is_empty());
    }

    /// An exchange dump stamps its bars in epoch milliseconds. Nothing about that column
    /// looks like a date, which is exactly why the detector gets a second pass.
    #[test]
    fn reads_epoch_millis() {
        let (m, b) = run(
            "BTCUSDT-1h.csv",
            "open_time,open,high,low,close,volume\n\
             1704067200000,42283.58,42554.57,42120.00,42475.23,1183.4\n\
             1704070800000,42475.23,42690.00,42400.11,42602.87,970.2\n",
        );
        assert_eq!(target_of(&m, 0), "ts");
        assert_eq!(b.ts_unit, "ms");
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert_eq!(b.bars[0].bar.ts.unix_timestamp(), 1_704_067_200);
        assert_eq!(b.bars[1].bar.ts.unix_timestamp(), 1_704_070_800);
    }

    /// Seconds and milliseconds are four orders of magnitude apart; neither window can
    /// swallow the other.
    #[test]
    fn epoch_windows_do_not_overlap() {
        assert_eq!(epoch_unit(1_704_067_200.0), Some(TsUnit::Secs));
        assert_eq!(epoch_unit(1_704_067_200_000.0), Some(TsUnit::Millis));
        assert_eq!(epoch_unit(1_704_067_200_000_000.0), Some(TsUnit::Micros));
        // A price is not an epoch.
        assert_eq!(epoch_unit(185.64), None);
        assert_eq!(epoch_unit(42_475.23), None);
    }

    /// A fund publishes one number a day. That is a close series, not an open series, and
    /// the bar it makes is flat rather than wrong.
    #[test]
    fn a_single_price_column_is_a_close() {
        let (m, b) = run("nav.csv", "Date;Value\n02/01/2024;12,4501\n03/01/2024;12,5130\n");
        assert_eq!(target_of(&m, 0), "ts");
        assert_eq!(target_of(&m, 1), "close");
        assert_eq!(b.synthesized, vec!["open", "high", "low"]);
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        let first = &b.bars[0].bar;
        assert_eq!(first.close, 12.4501);
        assert_eq!(first.open, first.close);
        assert_eq!(first.high, first.close);
        assert!(!b.bars[0].inconsistent);
    }

    /// Two rows for one period: the later one wins and the collision is reported, because
    /// a file that does this is either a bad export or two tapes stapled together.
    #[test]
    fn one_bar_per_period() {
        let (_, b) = run(
            "dup.csv",
            "time,open,high,low,close\n\
             2024-01-02,1,2,0.5,1.5\n\
             2024-01-02,1,3,0.5,2.5\n\
             2024-01-03,2.5,2.6,2.4,2.55\n",
        );
        assert_eq!(b.bars.len(), 2);
        assert_eq!(b.bars[0].bar.close, 2.5);
        assert!(b.bars[0].file_duplicate);
        assert!(!b.bars[1].file_duplicate);
    }

    /// A high under the close is a broken bar, not an unreadable row: it is stored and
    /// flagged, so the user decides whether their source is trustworthy.
    #[test]
    fn a_broken_bar_is_flagged_not_dropped() {
        let (_, b) = run(
            "odd.csv",
            "time,open,high,low,close\n2024-01-02,10,10.5,9.8,11.2\n2024-01-03,11,11.5,10.8,11.2\n",
        );
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert!(b.bars[0].inconsistent);
        assert!(!b.bars[1].inconsistent);
    }

    /// A price that cannot be read is an error naming its line, never a zero.
    #[test]
    fn an_unreadable_price_is_an_error() {
        let (_, b) = run(
            "holes.csv",
            "time,open,high,low,close\n\
             2024-01-02,10,10.5,9.8,10.2\n\
             2024-01-03,11,11.5,10.8,\n\
             2024-01-04,12,12.5,11.8,12.2\n",
        );
        assert_eq!(b.bars.len(), 2);
        assert_eq!(b.errors.len(), 1);
        assert_eq!(b.errors[0].row, 3);
    }

    /// The spacing of a daily series is one day even though every weekend is three.
    #[test]
    fn spacing_is_the_common_gap_not_the_largest() {
        let (_, b) = run(
            "daily.csv",
            "time,close\n\
             2024-01-02,1\n2024-01-03,2\n2024-01-04,3\n2024-01-05,4\n2024-01-08,5\n2024-01-09,6\n",
        );
        let (tf, missing) = spacing(&b.bars);
        assert_eq!(tf.as_deref(), Some("1d"));
        // The weekend: two calendar days with no session.
        assert_eq!(missing, Some(2));
    }

    /// A JSON array of objects is a grid too; every API that is not CSV hands one out.
    #[test]
    fn reads_json() {
        let (m, b) = run(
            "bars.json",
            r#"[{"t":"2024-01-02T00:00:00Z","o":1,"h":2,"l":0.5,"c":1.5,"v":10},
                {"t":"2024-01-03T00:00:00Z","o":1.5,"h":2.5,"l":1.4,"c":2.4,"v":12}]"#,
        );
        assert!(b.errors.is_empty(), "{:?} {:?}", b.errors, m.columns);
        assert_eq!(b.bars.len(), 2);
    }

    /// The destination is the half no file states, so it is the half that is checked.
    #[test]
    fn a_destination_is_validated_not_guessed() {
        let mut d = Destination::default();
        assert!(d.validate().is_err());
        d.ticker = "AAPL".into();
        d.asset_type = "Equity".into();
        assert!(d.validate().is_err(), "a timeframe is still missing");
        d.timeframe = "daily".into();
        assert!(d.validate().is_err(), "'daily' is not the vocabulary");
        d.timeframe = "1d".into();
        assert!(d.validate().is_ok());
        assert_eq!(d.asset_type(), "equity");
    }

    fn dataset(provider: &str, source: &str) -> otw_store::histdata::Dataset {
        otw_store::histdata::Dataset {
            id: Uuid::new_v4(),
            provider: provider.into(),
            asset_type: "equity".into(),
            ticker: "AAPL".into(),
            timeframe: "1d".into(),
            range_from: None,
            range_to: None,
            bar_count: 0,
            size_bytes: 0,
            gaps: serde_json::json!([]),
            status: "complete".into(),
            last_updated: OffsetDateTime::now_utc(),
            label: Some("Apple daily".into()),
            source: source.into(),
            tags: serde_json::json!([]),
        }
    }

    fn bar(ts: i64, close: f64, adj: Option<f64>) -> Bar {
        Bar {
            ts: OffsetDateTime::from_unix_timestamp(ts).unwrap(),
            open: close - 1.0,
            high: close + 1.0,
            low: close - 2.0,
            close,
            volume: 1_000.0,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: adj,
        }
    }

    /// Export to Parquet, import it back: the same bars, through the same reader every
    /// other import uses. The adjusted column is null on one row on purpose, since that is
    /// the shape an equity series with a partial history actually has.
    #[test]
    fn parquet_round_trip() {
        let ds = dataset("yahoo", "");
        let bars = vec![
            bar(1_704_153_600, 185.64, Some(184.90)),
            bar(1_704_240_000, 184.25, None),
        ];
        let file = crate::histdata_export::parquet_bytes(&ds, &bars).expect("write");
        let (m, b) = run_bytes("AAPL_1d.parquet", &file);
        assert_eq!(target_of(&m, 0), "ts");
        assert_eq!(target_of(&m, 4), "close");
        assert_eq!(target_of(&m, 5), "volume");
        assert_eq!(target_of(&m, 9), "adj_close");
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert_eq!(b.bars.len(), 2);
        assert_eq!(b.bars[0].bar.ts, bars[0].ts);
        assert_eq!(b.bars[0].bar.close, 185.64);
        assert_eq!(b.bars[0].bar.adj_close, Some(184.90));
        assert_eq!(b.bars[1].bar.adj_close, None);
        assert!(b.synthesized.is_empty());
    }

    /// The file says what it is, so the form does not have to be retyped. What the user
    /// already filled in is never overwritten.
    #[test]
    fn parquet_metadata_fills_only_the_blanks() {
        let file = crate::histdata_export::parquet_bytes(&dataset("binance", ""), &[bar(1_704_153_600, 1.0, None)])
            .expect("write");

        let mut fresh = Destination::default();
        adopt_file_metadata(&mut fresh, "x.parquet", &file);
        assert_eq!(fresh.ticker, "AAPL");
        assert_eq!(fresh.asset_type, "equity");
        assert_eq!(fresh.timeframe, "1d");
        // No import source on a downloaded dataset, so the provider is the provenance.
        assert_eq!(fresh.source, "binance");
        assert_eq!(fresh.label, "Apple daily");
        assert!(fresh.validate().is_ok());

        let mut typed = Destination { ticker: "MSFT".into(), source: "my csv".into(), ..Default::default() };
        adopt_file_metadata(&mut typed, "x.parquet", &file);
        assert_eq!(typed.ticker, "MSFT");
        assert_eq!(typed.source, "my csv");
        assert_eq!(typed.timeframe, "1d", "the blank half is still filled");
    }

    /// An imported series keeps the source it was imported under, not the reserved
    /// `import` provider id: re-exporting and re-importing must not lose where it is from.
    #[test]
    fn an_imported_series_exports_its_own_source() {
        let file = crate::histdata_export::parquet_bytes(
            &dataset(crate::histdata::IMPORT_PROVIDER, "degiro"),
            &[bar(1_704_153_600, 1.0, None)],
        )
        .expect("write");
        let mut dest = Destination::default();
        adopt_file_metadata(&mut dest, "x.parquet", &file);
        assert_eq!(dest.source, "degiro");
    }

    /// A CSV carries no metadata and must not be probed as if it did.
    #[test]
    fn a_csv_carries_no_metadata() {
        let mut dest = Destination::default();
        adopt_file_metadata(&mut dest, "bars.csv", b"time,close\n2024-01-02,1\n");
        assert_eq!(dest.ticker, "");
    }

    #[test]
    fn tags_are_trimmed_and_deduplicated() {
        let d = Destination {
            tags: vec![" crypto ".into(), "Crypto".into(), "".into(), "backfill".into()],
            ..Default::default()
        };
        assert_eq!(d.tags(), vec!["crypto".to_string(), "backfill".to_string()]);
    }
}
