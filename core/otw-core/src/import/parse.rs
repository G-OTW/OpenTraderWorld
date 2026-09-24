//! Reading a trade book: bytes → a grid of cells, and cells → typed values.
//!
//! Everything here is locale-defensive. Broker exports arrive as CSV with any of four
//! delimiters, in UTF-8 or Latin-1, with a preamble above the header row, numbers in
//! either decimal convention, and dates in every order. The rules are decided **per
//! column** (never per cell) so "1,234" reads the same way in every row of a column.

use anyhow::Context;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use super::dict;

// ── Text normalization ───────────────────────────────────────────────────────

/// Lowercase, strip accents, and reduce anything that isn't a letter or digit to a
/// single space. "Prix d'entrée (€)" → "prix d entree". camelCase is split first, so a
/// JSON export's `exitPrice` reads as the two words it is.
pub fn normalize(s: &str) -> String {
    let mut split = String::with_capacity(s.len() + 4);
    let mut prev_lower = false;
    for ch in s.chars() {
        if prev_lower && ch.is_uppercase() {
            split.push(' ');
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        split.push(ch);
    }
    let lowered = split.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut space = true; // leading spaces are dropped
    for ch in lowered.chars() {
        let ch = deaccent(ch);
        if ch.is_alphanumeric() {
            out.push(ch);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

fn deaccent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ß' => 's',
        _ => c,
    }
}

// ── Grid ─────────────────────────────────────────────────────────────────────

/// A source file reduced to a header row plus data rows, all padded to one width.
pub struct Grid {
    pub delimiter: char,
    pub header_row: usize,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// 1-based source line of each kept row. Blank rows are dropped, so a row's index
    /// is not its line number — and an error has to point at the line the user can see.
    pub row_lines: Vec<usize>,
    /// Rows above the header (title/account preamble), kept to show in the UI.
    pub preamble: Vec<String>,
    /// Section names when the file stacks several tables (statement exports). Empty for
    /// an ordinary flat CSV.
    pub sections: Vec<String>,
    /// Which of them this grid was cut from.
    pub section: Option<String>,
}

impl Grid {
    pub fn width(&self) -> usize {
        self.headers.len()
    }
    pub fn cell<'a>(&'a self, row: &'a [String], idx: usize) -> &'a str {
        row.get(idx).map(|s| s.trim()).unwrap_or("")
    }
    /// Up to `n` non-empty sample values for a column.
    pub fn samples(&self, idx: usize, n: usize) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|r| {
                let v = self.cell(r, idx);
                (!v.is_empty()).then(|| v.to_string())
            })
            .take(n)
            .collect()
    }
    /// Every non-empty value of a column (used for per-column format decisions).
    pub fn column(&self, idx: usize) -> Vec<&str> {
        self.rows
            .iter()
            .map(|r| self.cell(r, idx))
            .filter(|v| !v.is_empty())
            .collect()
    }
}

/// Decode bytes as UTF-8, falling back to Latin-1 (never fails, never panics).
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes); // UTF-8 BOM
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

const DELIMITERS: [char; 4] = [',', ';', '\t', '|'];

/// Pick the delimiter that yields the most consistent, widest table over the first
/// lines. A file with no delimiter at all still parses (one column).
fn sniff_delimiter(text: &str) -> char {
    let head: String = text.lines().take(40).collect::<Vec<_>>().join("\n");
    let mut best = (',', 0f64);
    for d in DELIMITERS {
        let rows = read_records(&head, d);
        if rows.is_empty() {
            continue;
        }
        // Modal field count, and how many rows agree with it.
        let mut counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for r in &rows {
            *counts.entry(r.len()).or_insert(0) += 1;
        }
        let (width, agree) = counts.into_iter().max_by_key(|&(w, c)| (c, w)).unwrap_or((1, 0));
        if width < 2 {
            continue;
        }
        let score = (width - 1) as f64 * agree as f64 / rows.len() as f64;
        if score > best.1 {
            best = (d, score);
        }
    }
    best.0
}

/// Peel a spreadsheet's padding off a wrapped file, returning the real CSV inside it.
///
/// A statement opened and re-saved by a spreadsheet in another locale comes back as one
/// column: every line holds the original record, quoted whole because it is full of the
/// other delimiter, followed by a run of empty cells (`…,MESU6,…,O";;;;;;;;;;;;;`).
/// Splitting on the outer delimiter therefore yields exactly one filled cell per line —
/// that cell *is* the file. Reading it back out (the outer parse already un-escaped the
/// doubled quotes) hands us the record the broker actually wrote.
///
/// Only fires when every line is shaped that way and the recovered text is a real table,
/// so an ordinary one-column file is left alone.
fn peel_padding(text: &str) -> Option<String> {
    let outer = sniff_delimiter(text);
    let records = read_records(text, outer);
    if records.len() < 2 || !records.iter().any(|r| r.len() > 1) {
        return None; // nothing was padded
    }
    // A statement carries prose (legal notes, code glossaries) that contains the outer
    // delimiter, so those lines do split in two. Judge the file as a whole: a strong
    // majority of single-cell lines means the column structure is the padding's, not the
    // file's. A genuine two-column table has none.
    let singles = records.iter().filter(|r| eff_width(r) <= 1).count();
    if singles * 10 < records.len() * 9 {
        return None;
    }
    // Put each line back together (dropping only the trailing empties) so a line the
    // outer delimiter cut inside a sentence is restored whole.
    let inner = records
        .iter()
        .map(|r| r[..eff_width(r)].join(&outer.to_string()))
        .collect::<Vec<_>>()
        .join("\n");
    let d = sniff_delimiter(&inner);
    let rows = read_records(&inner, d);
    let widest = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    (widest >= 2).then_some(inner)
}

fn read_records(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter as u8)
        .flexible(true)
        .has_headers(false)
        .from_reader(text.as_bytes());
    rdr.records()
        .filter_map(|r| r.ok())
        .map(|r| r.iter().map(|c| c.trim().to_string()).collect())
        .collect()
}

/// True when a record reads like a header: mostly filled, mostly non-numeric.
fn looks_like_header(row: &[String]) -> bool {
    let filled: Vec<&String> = row.iter().filter(|c| !c.is_empty()).collect();
    if filled.len() < 2 {
        return false;
    }
    let wordy = filled
        .iter()
        .filter(|c| parse_number(c, '.').is_none() && parse_datetime(c, DateOrder::Auto, 0).is_none())
        .count();
    wordy * 2 >= filled.len()
}

/// Number of cells up to the last non-empty one (spreadsheets pad rows with commas).
fn eff_width(row: &[String]) -> usize {
    row.iter().rposition(|c| !c.is_empty()).map(|i| i + 1).unwrap_or(0)
}

// ── Stacked sections (statement exports) ─────────────────────────────────────

/// Several tables in one file, each row prefixed with its section name and a role tag:
///
/// ```text
/// Statement,Header,Field Name,Field Value
/// Statement,Data,Title,Transaction History
/// Transaction History,Header,Date,Symbol,Quantity,Price
/// Transaction History,Data,2026-08-07,MNQU6,-1.0,29840
/// ```
///
/// The tell is structural, not a vocabulary: **a flat CSV has exactly one header row and
/// it is the first one**. A file where several header-shaped rows appear further down,
/// each opening a run of same-tagged rows, is a stack of tables. Nothing here matches on
/// "Header"/"Data" as words, so a localized statement is read the same way.
struct Layout {
    tag_col: usize,
    sections: Vec<Section>,
}

struct Section {
    name: String,
    /// Record index of this section's header row.
    header: usize,
    /// Record indexes of its data rows.
    data: Vec<usize>,
}

fn detect_sections(records: &[Vec<String>]) -> Option<Layout> {
    const NAME_COL: usize = 0;
    const TAG_COL: usize = 1;
    if records.len() < 6 || records.iter().map(|r| r.len()).max().unwrap_or(0) < 4 {
        return None;
    }
    let at = |r: &Vec<String>, i: usize| r.get(i).map(|s| s.trim()).unwrap_or("").to_string();

    // 1. The section column repeats in contiguous runs, and there are several of them.
    let mut runs = 1usize;
    for w in records.windows(2) {
        if at(&w[0], NAME_COL) != at(&w[1], NAME_COL) {
            runs += 1;
        }
    }
    if runs < 2 || runs * 2 > records.len() || records.iter().any(|r| at(r, NAME_COL).is_empty()) {
        return None;
    }

    // 2. The tag column is a small closed vocabulary. A stray summary line carries no tag
    //    at all ("Total P/L for Statement Period,,,,,…"): tolerate a few of those — they
    //    belong to no section and are dropped below — but a file whose second column is
    //    largely empty is not a stack of tables.
    let tags: Vec<String> = records.iter().map(|r| at(r, TAG_COL)).collect();
    if tags.iter().filter(|t| t.is_empty()).count() * 10 > records.len() {
        return None;
    }
    let mut distinct: Vec<&String> = tags.iter().filter(|t| !t.is_empty()).collect();
    distinct.sort();
    distinct.dedup();
    if distinct.len() < 2 || distinct.len() > 8 {
        return None;
    }

    // 3. One tag opens each section with a header-shaped row. Pick the tag that does it
    //    in the most sections; it must work in at least two (one is just a flat file).
    let body = |r: &Vec<String>| r.iter().skip(TAG_COL + 1).cloned().collect::<Vec<_>>();
    let header_tag = distinct
        .iter()
        .map(|t| {
            let hits = records
                .iter()
                .enumerate()
                .filter(|(i, r)| {
                    at(r, TAG_COL) == ***t
                        && looks_like_header(&body(r))
                        // …and something tagged differently follows it in the same section.
                        && records.get(i + 1).is_some_and(|n| {
                            at(n, NAME_COL) == at(r, NAME_COL) && at(n, TAG_COL) != ***t
                        })
                })
                .count();
            ((*t).clone(), hits)
        })
        .max_by_key(|(_, hits)| *hits)
        .filter(|(_, hits)| *hits >= 2)?
        .0;

    // 4. Cut the sections. Rows tagged anything else (Notes, Total, SubTotal) are dropped.
    let mut sections: Vec<Section> = Vec::new();
    for (i, r) in records.iter().enumerate() {
        let name = at(r, NAME_COL);
        let tag = at(r, TAG_COL);
        if tag == header_tag && looks_like_header(&body(r)) {
            sections.push(Section { name, header: i, data: vec![] });
        } else if let Some(s) = sections.iter_mut().rev().find(|s| s.name == name) {
            // The data tag is whatever the section's own rows carry under its header.
            let data_tag = records
                .get(s.header + 1)
                .map(|n| at(n, TAG_COL))
                .unwrap_or_default();
            if tag == data_tag {
                s.data.push(i);
            }
        }
    }
    sections.retain(|s| !s.data.is_empty());
    (sections.len() >= 1).then_some(Layout { tag_col: TAG_COL, sections })
}

/// Turn a file into a grid. `header_row`/`delimiter`/`section` override what was
/// detected (the user can correct all three in the mapping step). `section` is
/// `Some("")` to read a stacked file flat, `None` to let the best one be picked.
pub fn read_grid(
    filename: &str,
    bytes: &[u8],
    header_row: Option<usize>,
    delimiter: Option<char>,
    section: Option<&str>,
) -> anyhow::Result<Grid> {
    // Parquet is binary: it has to be recognized before anything tries to decode it as
    // text. What comes back is the same grid, so nothing downstream knows the difference.
    if super::parquet::looks_parquet(filename, bytes) {
        return super::parquet::read_grid(bytes);
    }

    let text = decode(bytes);
    if text.trim().is_empty() {
        anyhow::bail!("the file is empty");
    }
    // JSON export (array of objects) — some brokers and most APIs hand this out.
    let looks_json = filename.to_lowercase().ends_with(".json")
        || text.trim_start().starts_with('[')
        || text.trim_start().starts_with('{');
    if looks_json {
        if let Some(grid) = read_json(&text) {
            return Ok(grid);
        }
    }

    // Unwrap a padded file before anything else, so the delimiter (sniffed or forced by
    // a saved mapping) applies to the record the broker wrote, not to the padding.
    let text = peel_padding(&text).unwrap_or(text);
    let delimiter = delimiter.unwrap_or_else(|| sniff_delimiter(&text));
    let records = read_records(&text, delimiter);
    if records.is_empty() {
        anyhow::bail!("no rows found in the file");
    }

    // A stacked statement: cut the wanted section out, unless the user asked for flat.
    if section != Some("") {
        if let Some(layout) = detect_sections(&records) {
            return Ok(cut_section(&records, &layout, delimiter, section));
        }
    }

    // Header row: the first plausible one in the first 20 records (brokers put a title,
    // an account number and a blank line above it).
    let hidx = header_row.unwrap_or_else(|| pick_header_row(&records));
    let hidx = hidx.min(records.len() - 1);

    // Preamble lines are shown back to the user, so drop the empty cells a spreadsheet
    // pads them with ("Statement;;;;;" reads as "Statement").
    let preamble = records[..hidx]
        .iter()
        .map(|r| {
            r.iter()
                .filter(|c| !c.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .filter(|s| !s.is_empty())
        .collect();

    let mut headers = records[hidx].clone();
    let kept: Vec<(usize, Vec<String>)> = records[hidx + 1..]
        .iter()
        .enumerate()
        .filter(|(_, r)| r.iter().any(|c| !c.is_empty()))
        .map(|(i, r)| (hidx + i + 2, r.clone())) // 1-based line in the source file
        .collect();

    let width = kept.iter().map(|(_, r)| r.len()).chain([headers.len()]).max().unwrap_or(0);
    headers.resize(width, String::new());
    let (row_lines, rows) = kept
        .into_iter()
        .map(|(line, mut r)| {
            r.resize(width, String::new());
            (line, r)
        })
        .unzip();

    Ok(Grid {
        delimiter,
        header_row: hidx,
        headers,
        rows,
        row_lines,
        preamble,
        sections: vec![],
        section: None,
    })
}

/// Where the table starts in a flat file.
///
/// The long-standing rule — first header-shaped record in the first 20 — is kept as-is
/// whenever it lands on a row as wide as the file's modal row, because that is what an
/// ordinary CSV looks like and changing it there would only invite regressions. It is
/// only when the pick is *narrower* than the body (a title block above a wider table)
/// that the two structural signals are consulted:
///
///   - **width stability**: the real table is the longest run of consecutive rows of the
///     same width;
///   - **type stability**: its header is the row above that run which reads as words
///     where the run reads as numbers and dates.
fn pick_header_row(records: &[Vec<String>]) -> usize {
    let first = records
        .iter()
        .take(20)
        .position(|r| looks_like_header(r))
        .unwrap_or(0);

    let mut freq: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for r in records {
        *freq.entry(eff_width(r)).or_insert(0) += 1;
    }
    let modal = freq.into_iter().max_by_key(|&(w, c)| (c, w)).map(|(w, _)| w).unwrap_or(0);
    if eff_width(&records[first]) >= modal {
        return first; // ordinary CSV — untouched
    }

    // Longest run of same-width records, ignoring runs narrower than the modal width.
    let (mut best_start, mut best_len, mut i) = (first, 0usize, 0usize);
    while i < records.len() {
        let w = eff_width(&records[i]);
        let mut j = i;
        while j < records.len() && eff_width(&records[j]) == w {
            j += 1;
        }
        if w >= modal && j - i > best_len {
            best_start = i;
            best_len = j - i;
        }
        i = j;
    }
    if best_len == 0 {
        return first;
    }
    // The run either opens with its own header, or is introduced by the row above it.
    if looks_like_header(&records[best_start]) {
        best_start
    } else if best_start > 0 && looks_like_header(&records[best_start - 1]) {
        best_start - 1
    } else {
        first
    }
}

/// Build a grid from one section of a stacked file, dropping the two routing columns.
fn cut_section(
    records: &[Vec<String>],
    layout: &Layout,
    delimiter: char,
    wanted: Option<&str>,
) -> Grid {
    let names: Vec<String> = layout.sections.iter().map(|s| s.name.clone()).collect();
    // The asked-for section, else the one carrying the most rows.
    let chosen = wanted
        .and_then(|w| layout.sections.iter().find(|s| s.name == w))
        .or_else(|| layout.sections.iter().max_by_key(|s| s.data.len()))
        .expect("detect_sections returns at least one section");

    let strip = |r: &Vec<String>| r.iter().skip(layout.tag_col + 1).cloned().collect::<Vec<String>>();
    let mut headers = strip(&records[chosen.header]);
    let kept: Vec<(usize, Vec<String>)> = chosen
        .data
        .iter()
        .map(|&i| (i + 1, strip(&records[i]))) // 1-based line in the source file
        .filter(|(_, r)| r.iter().any(|c| !c.is_empty()))
        .collect();

    let width = kept.iter().map(|(_, r)| r.len()).chain([headers.len()]).max().unwrap_or(0);
    headers.resize(width, String::new());
    let (row_lines, rows) = kept
        .into_iter()
        .map(|(line, mut r)| {
            r.resize(width, String::new());
            (line, r)
        })
        .unzip();

    Grid {
        delimiter,
        header_row: chosen.header,
        headers,
        rows,
        row_lines,
        // The other sections are the context this table was pulled out of.
        preamble: names
            .iter()
            .filter(|n| **n != chosen.name)
            .map(|n| n.clone())
            .collect(),
        sections: names,
        section: Some(chosen.name.clone()),
    }
}

/// JSON array of flat objects → grid. Keys of the first object set the column order;
/// keys seen later are appended.
fn read_json(text: &str) -> Option<Grid> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    // Accept a bare array, or an object with exactly one array field ({ "trades": [...] }).
    let array = match &value {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(o) => o.values().find_map(|v| v.as_array().cloned())?,
        _ => return None,
    };
    let mut headers: Vec<String> = Vec::new();
    for item in &array {
        for k in item.as_object()?.keys() {
            if !headers.iter().any(|h| h == k) {
                headers.push(k.clone());
            }
        }
    }
    if headers.is_empty() {
        return None;
    }
    let rows: Vec<Vec<String>> = array
        .iter()
        .map(|item| {
            headers
                .iter()
                .map(|h| match item.get(h) {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(serde_json::Value::Null) | None => String::new(),
                    Some(v) => v.to_string(),
                })
                .collect()
        })
        .collect();
    // No lines in a JSON array — number the entries instead.
    let row_lines = (1..=rows.len()).collect();
    Some(Grid {
        delimiter: ',',
        header_row: 0,
        headers,
        rows,
        row_lines,
        preamble: vec![],
        sections: vec![],
        section: None,
    })
}

/// Decode a base64 upload with a sane size cap.
pub fn decode_upload(b64: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    // Accept a data: URI too — the browser's FileReader hands one out for free.
    let payload = b64.rsplit_once("base64,").map(|(_, p)| p).unwrap_or(b64);
    let cleaned: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .context("the upload is not valid base64")?;
    anyhow::ensure!(bytes.len() <= 20 * 1024 * 1024, "file too large (20 MB max)");
    Ok(bytes)
}

// ── Numbers ──────────────────────────────────────────────────────────────────

/// Parse a number written in either decimal convention. `decimal` says which character
/// separates the fractional part in *this column*; the other one is a group separator.
/// Understands parentheses for negatives, a leading/trailing sign, currency symbols,
/// spaces (incl. NBSP) and a trailing %.
pub fn parse_number(raw: &str, decimal: char) -> Option<f64> {
    let s = strip_footnote(raw.trim());
    if s.is_empty() {
        return None;
    }
    let negative_paren = s.starts_with('(') && s.ends_with(')');
    let mut cleaned = String::with_capacity(s.len());
    let mut negative = negative_paren;
    for ch in s.chars() {
        match ch {
            '0'..='9' => cleaned.push(ch),
            '-' | '−' if cleaned.is_empty() => negative = true,
            '+' => {}
            c if c == decimal => cleaned.push('.'),
            _ => {} // group separators, currency symbols, spaces, %, letters
        }
    }
    if cleaned.is_empty() || !cleaned.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    // A trailing sign ("1 234,50-", common in German exports).
    if s.ends_with('-') {
        negative = true;
    }
    let n: f64 = cleaned.parse().ok()?;
    // Guard against reading a date ("2024-01-15" → "20240115") as a number: a value
    // that still holds two separators of the same kind is not a number.
    Some(if negative { -n } else { n })
}

/// Drop a trailing footnote reference: statements write `31358.48(1)` where `(1)` points
/// at a note under the table. Left in, its digit would be read as part of the number.
/// A value that *starts* with '(' is the accounting negative and is left alone.
fn strip_footnote(s: &str) -> &str {
    if s.starts_with('(') || !s.ends_with(')') {
        return s;
    }
    match s.rfind('(') {
        Some(open) if open > 0 && s[open + 1..s.len() - 1].chars().all(|c| c.is_ascii_digit()) => {
            s[..open].trim_end()
        }
        _ => s,
    }
}

/// Decide which character is the decimal separator for a whole column.
///
/// A value carrying both separators settles it (the *last* one is the decimal). Failing
/// that, a comma followed by anything other than exactly three digits is a decimal
/// comma. Otherwise the column is read as English.
pub fn column_decimal(values: &[&str]) -> char {
    for v in values {
        let last_dot = v.rfind('.');
        let last_comma = v.rfind(',');
        if let (Some(d), Some(c)) = (last_dot, last_comma) {
            return if c > d { ',' } else { '.' };
        }
    }
    for v in values {
        if let Some(pos) = v.rfind(',') {
            let tail = &v[pos + 1..];
            if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) && tail.len() != 3 {
                return ',';
            }
        }
    }
    '.'
}

// ── Dates ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    Auto,
    Dmy,
    Mdy,
    Ymd,
}

impl DateOrder {
    pub fn from_str(s: &str) -> Self {
        match s {
            "dmy" => Self::Dmy,
            "mdy" => Self::Mdy,
            "ymd" => Self::Ymd,
            _ => Self::Auto,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dmy => "dmy",
            Self::Mdy => "mdy",
            Self::Ymd => "ymd",
            Self::Auto => "auto",
        }
    }
}

/// Resolve the day/month order for a whole column: a component > 12 anywhere in the
/// column decides it. `None` means every row was ambiguous (e.g. only 01/02/2024) —
/// the caller warns and falls back to the mapping's setting.
pub fn column_date_order(values: &[&str]) -> Option<DateOrder> {
    for v in values {
        let Some((a, b, _c)) = numeric_date_parts(v) else { continue };
        if a > 12 && b <= 12 {
            return Some(DateOrder::Dmy);
        }
        if b > 12 && a <= 12 {
            return Some(DateOrder::Mdy);
        }
    }
    None
}

/// The three numeric components of a separator-style date, in written order.
fn numeric_date_parts(raw: &str) -> Option<(u32, u32, u32)> {
    let date_part = raw.trim().split(['T', ' ', ';']).next()?;
    let parts: Vec<&str> = date_part.split(['-', '/', '.']).filter(|p| !p.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let nums: Vec<u32> = parts.iter().filter_map(|p| p.parse::<u32>().ok()).collect();
    if nums.len() != 3 {
        return None;
    }
    // A 4-digit leading component is a year — not a day/month ambiguity.
    if parts[0].len() == 4 {
        return None;
    }
    Some((nums[0], nums[1], nums[2]))
}

/// True when a value is worth handing to the date parser (keeps prices out).
pub fn looks_dateish(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    if s.chars().all(|c| c.is_ascii_digit()) {
        return matches!(s.len(), 8 | 10 | 13);
    }
    if s.chars().any(|c| dict::MONTHS.iter().any(|(m, _)| m.starts_with(c.to_ascii_lowercase())))
        && s.chars().any(|c| c.is_alphabetic())
        && digits >= 3
    {
        return true;
    }
    // A dot-separated date has three parts ("12.05.2024"). One dot is a decimal point,
    // and a price like 29717.0 must not be offered to the parser — it would come back a
    // valid Excel serial day and the whole column would read as dates.
    (digits >= 3 && s.contains(['-', '/', ':'])) || (digits >= 6 && s.matches('.').count() >= 2)
}

/// Parse a timestamp. Naive values take `tz_offset_min`; values carrying their own
/// offset (Z, +02:00) keep it. Understands ISO, YYYYMMDD[;HHMMSS], d/m/y and m/d/y with
/// any separator, short month names, Unix epochs and Excel serial days.
pub fn parse_datetime(raw: &str, order: DateOrder, tz_offset_min: i32) -> Option<OffsetDateTime> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let offset = UtcOffset::from_whole_seconds(tz_offset_min * 60).unwrap_or(UtcOffset::UTC);

    // Pure digits: epoch seconds / milliseconds, or YYYYMMDD.
    if s.chars().all(|c| c.is_ascii_digit()) {
        return match s.len() {
            13 => s.parse::<i64>().ok().and_then(|ms| {
                OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000).ok()
            }),
            10 => s.parse::<i64>().ok().and_then(|sec| OffsetDateTime::from_unix_timestamp(sec).ok()),
            8 => {
                let y: i32 = s[0..4].parse().ok()?;
                let m: u8 = s[4..6].parse().ok()?;
                let d: u8 = s[6..8].parse().ok()?;
                build(y, m, d, 0, 0, 0, offset)
            }
            _ => None,
        };
    }

    // Excel serial day (a bare decimal number in a date column).
    if let Ok(serial) = s.replace(',', ".").parse::<f64>() {
        if (20_000.0..80_000.0).contains(&serial) {
            let days = serial.trunc() as i64;
            let secs = ((serial - serial.trunc()) * 86_400.0).round() as i64;
            // Excel's epoch is 1899-12-30 (its 1900 leap-year bug included).
            let epoch = Date::from_calendar_date(1899, Month::December, 30).ok()?;
            let date = epoch.checked_add(time::Duration::days(days))?;
            return Some(
                date.midnight().assume_offset(offset) + time::Duration::seconds(secs),
            );
        }
        return None;
    }

    // Split date / time. IBKR uses "20240115;093000"; ISO uses 'T'.
    let (date_str, rest) = match s.split_once(['T', ';']) {
        Some((d, r)) => (d, r),
        None => match s.split_once(' ') {
            // "Jan 15, 2024 09:30" — the first space is inside the date.
            Some((d, r)) if r.contains(':') || r.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
                if d.chars().any(|c| c.is_alphabetic()) {
                    // Month-name form: the date runs until the token containing ':'
                    let idx = s.find(|c: char| c == ':').and_then(|i| s[..i].rfind(' ')).unwrap_or(s.len());
                    (&s[..idx], if idx < s.len() { &s[idx + 1..] } else { "" })
                } else {
                    (d, r)
                }
            }
            _ => (s, ""),
        },
    };

    let (y, m, d) = parse_date_part(date_str, order)?;
    let (hh, mm, ss, tz) = parse_time_part(rest);
    build(y, m, d, hh, mm, ss, tz.unwrap_or(offset))
}

fn build(y: i32, m: u8, d: u8, hh: u8, mm: u8, ss: u8, offset: UtcOffset) -> Option<OffsetDateTime> {
    let date = Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()?;
    let time = Time::from_hms(hh.min(23), mm.min(59), ss.min(59)).ok()?;
    Some(PrimitiveDateTime::new(date, time).assume_offset(offset))
}

fn parse_date_part(raw: &str, order: DateOrder) -> Option<(i32, u8, u8)> {
    let cleaned = raw.trim().trim_end_matches(',');
    let parts: Vec<String> = cleaned
        .split(['-', '/', '.', ' ', ','])
        .filter(|p| !p.is_empty())
        .map(|p| normalize(p))
        .collect();
    if parts.len() < 3 {
        return None;
    }
    // Month written as a name, in any of the three positions.
    let month_at = parts.iter().position(|p| month_number(p).is_some());
    if let Some(mi) = month_at {
        let m = month_number(&parts[mi])?;
        let nums: Vec<u32> = parts
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != mi)
            .filter_map(|(_, p)| p.parse::<u32>().ok())
            .collect();
        if nums.len() < 2 {
            return None;
        }
        let (day, year) = if nums[0] > 31 { (nums[1], nums[0]) } else { (nums[0], nums[1]) };
        return Some((full_year(year), m, day as u8));
    }

    let nums: Vec<u32> = parts.iter().filter_map(|p| p.parse::<u32>().ok()).collect();
    if nums.len() < 3 {
        return None;
    }
    let (y, m, d) = if parts[0].len() == 4 || nums[0] > 31 || order == DateOrder::Ymd {
        (nums[0], nums[1], nums[2])
    } else {
        let dmy = match order {
            DateOrder::Mdy => false,
            DateOrder::Dmy => true,
            // Ambiguous and nothing decided it: a component > 12 in this very value,
            // else day-first (the majority convention outside the US).
            _ => !(nums[0] <= 12 && nums[1] > 12),
        };
        if dmy { (nums[2], nums[1], nums[0]) } else { (nums[2], nums[0], nums[1]) }
    };
    Some((full_year(y), m as u8, d as u8))
}

fn full_year(y: u32) -> i32 {
    match y {
        0..=68 => 2000 + y as i32,
        69..=99 => 1900 + y as i32,
        _ => y as i32,
    }
}

fn month_number(token: &str) -> Option<u8> {
    if token.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    dict::MONTHS
        .iter()
        .filter(|(name, _)| token.starts_with(*name) || name.starts_with(token))
        .map(|(_, n)| *n)
        .next()
}

/// "09:30:15 PM +02:00" → (21, 30, 15, Some(+2h)).
fn parse_time_part(raw: &str) -> (u8, u8, u8, Option<UtcOffset>) {
    let s = raw.trim();
    if s.is_empty() {
        return (0, 0, 0, None);
    }
    // Trailing timezone.
    let mut tz = None;
    let mut body = s.to_string();
    if let Some(stripped) = body.strip_suffix('Z').or_else(|| body.strip_suffix('z')) {
        tz = Some(UtcOffset::UTC);
        body = stripped.trim().to_string();
    } else if let Some(pos) = body.rfind(['+', '-']) {
        if pos > 0 {
            let (head, off) = body.split_at(pos);
            let sign = if off.starts_with('-') { -1 } else { 1 };
            let digits: Vec<u32> = off[1..]
                .split(':')
                .filter_map(|p| p.trim().parse::<u32>().ok())
                .collect();
            if !digits.is_empty() {
                let (h, m) = if off.len() >= 5 && !off.contains(':') && digits.len() == 1 {
                    (digits[0] / 100, digits[0] % 100)
                } else {
                    (digits[0], *digits.get(1).unwrap_or(&0))
                };
                tz = UtcOffset::from_hms(sign * h as i8, sign * m as i8, 0).ok();
                body = head.trim().to_string();
            }
        }
    }

    let lower = body.to_lowercase();
    let pm = lower.contains("pm");
    let am = lower.contains("am");
    let digits: String = body.chars().filter(|c| c.is_ascii_digit() || *c == ':').collect();
    let parts: Vec<u32> = digits.split(':').filter_map(|p| p.parse().ok()).collect();
    let (mut h, m, sec) = match parts.len() {
        0 => (0, 0, 0),
        1 if digits.len() >= 4 => (parts[0] / 10000 % 100, parts[0] / 100 % 100, parts[0] % 100),
        1 => (parts[0], 0, 0),
        2 => (parts[0], parts[1], 0),
        _ => (parts[0], parts[1], parts[2]),
    };
    if pm && h < 12 {
        h += 12;
    }
    if am && h == 12 {
        h = 0;
    }
    (h as u8, m as u8, sec as u8, tz)
}

// ── Cell classification ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Empty,
    Num,
    Date,
    Side,
    Currency,
    Text,
}

pub fn value_kind(raw: &str, decimal: char) -> ValueKind {
    let s = raw.trim();
    if s.is_empty() {
        return ValueKind::Empty;
    }
    if s.len() == 3 && dict::CURRENCIES.contains(&s.to_uppercase().as_str()) {
        return ValueKind::Currency;
    }
    if looks_dateish(s) && parse_datetime(s, DateOrder::Auto, 0).is_some() {
        return ValueKind::Date;
    }
    if parse_number(s, decimal).is_some() && s.chars().any(|c| c.is_ascii_digit()) {
        // "1" / "-1" can also be a side; the caller's Side gate accepts numbers.
        return ValueKind::Num;
    }
    if side_of(s, &[]).is_some() {
        return ValueKind::Side;
    }
    ValueKind::Text
}

/// Read a cell as a direction. `extra` holds the mapping's own value overrides
/// (normalized source value → "long" | "short"), which win over the shipped words.
/// The non-trade ledger kind a cell names, or `None` when it is not one.
///
/// Checked **before** the direction vocabulary: a statement writes "Dividend" and "Buy" in
/// one column, and `side_of` would read the word "credit" in a deposit line as a direction.
/// A whole-cell match only, since a word-level fallback would turn "Sell to cover tax" into
/// a tax row.
pub fn kind_of(raw: &str) -> Option<&'static str> {
    let v = normalize(raw);
    if v.is_empty() {
        return None;
    }
    dict::KIND_MAP
        .iter()
        .find(|(_, words)| words.iter().any(|w| normalize(w) == v))
        .map(|(kind, _)| *kind)
}

pub fn side_of(raw: &str, extra: &[(String, String)]) -> Option<&'static str> {
    let v = normalize(raw);
    if v.is_empty() {
        return None;
    }
    if let Some((_, target)) = extra.iter().find(|(k, _)| *k == v) {
        return match target.as_str() {
            "short" => Some("short"),
            _ => Some("long"),
        };
    }
    if dict::SIDE_LONG.iter().any(|w| normalize(w) == v) {
        return Some("long");
    }
    if dict::SIDE_SHORT.iter().any(|w| normalize(w) == v) {
        return Some("short");
    }
    // "Buy to open" / "Vente à découvert" — fall back to a word-level match.
    let words: Vec<&str> = v.split(' ').collect();
    if words.iter().any(|w| dict::SIDE_SHORT.contains(w)) {
        return Some("short");
    }
    if words.iter().any(|w| dict::SIDE_LONG.contains(w)) {
        return Some("long");
    }
    None
}

/// Map a free-text asset/unit label onto the journal's whitelist.
pub fn lookup_enum(raw: &str, table: &[(&'static str, &[&str])]) -> Option<&'static str> {
    let v = normalize(raw);
    if v.is_empty() {
        return None;
    }
    for (id, words) in table {
        if *id == v || words.iter().any(|w| normalize(w) == v) {
            return Some(id);
        }
    }
    for (id, words) in table {
        if words.iter().any(|w| v.split(' ').any(|part| part == normalize(w))) {
            return Some(id);
        }
    }
    None
}
