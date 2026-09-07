//! Auto-discovery: which source column is which field of the module's target set.
//!
//! Two independent signals, because either alone is wrong often enough to matter:
//! the **header** (matched against the multilingual dictionary and against the aliases
//! the user taught us) and the **values** (a column of `BUY`/`SELL`, of ISO currency
//! codes, of timestamps). A header hit whose column obviously holds something else is
//! rejected. Whatever survives is assigned one-to-one, best score first, and anything
//! under the threshold is left for the user — an honest "not sure" beats a wrong guess
//! the user has to notice.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use super::dict::{Kind, TargetSet};
use super::parse::{self, DateOrder, Grid, ValueKind};

/// Below this, a candidate is not proposed at all.
const THRESHOLD: u32 = 50;

/// What a column looks like, decided over the whole column (never one cell).
pub struct Profile {
    pub decimal: char,
    pub date_order: Option<DateOrder>,
    pub counts: HashMap<&'static str, usize>,
    pub filled: usize,
    /// The column holds numbers on both sides of zero. On a quantity column with no
    /// side column beside it, that sign *is* the direction (IBKR writes `-1` for a sell).
    pub signed: bool,
}

impl Profile {
    fn ratio(&self, kind: &str) -> f64 {
        if self.filled == 0 {
            return 0.0;
        }
        *self.counts.get(kind).unwrap_or(&0) as f64 / self.filled as f64
    }
    /// The single label shown next to the column in the mapping step.
    pub fn dominant(&self) -> &'static str {
        let mut best = ("text", 0usize);
        for (k, c) in &self.counts {
            if *c > best.1 {
                best = (k, *c);
            }
        }
        if self.filled == 0 {
            "empty"
        } else {
            best.0
        }
    }
}

pub fn profile_column(grid: &Grid, idx: usize) -> Profile {
    let values = grid.column(idx);
    let decimal = parse::column_decimal(&values);
    let date_order = parse::column_date_order(&values);
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    let (mut neg, mut pos) = (false, false);
    // Sampling 400 rows is plenty to characterize a column and keeps big files snappy.
    for v in values.iter().take(400) {
        let key = match parse::value_kind(v, decimal) {
            ValueKind::Num => {
                match parse::parse_number(v, decimal) {
                    Some(n) if n < 0.0 => neg = true,
                    Some(n) if n > 0.0 => pos = true,
                    _ => {}
                }
                "num"
            }
            ValueKind::Date => "date",
            ValueKind::Side => "side",
            ValueKind::Currency => "currency",
            ValueKind::Text => "text",
            ValueKind::Empty => continue,
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    let filled = counts.values().sum();
    Profile { decimal, date_order, counts, filled, signed: neg && pos }
}

/// Does this column's content allow it to be that field?
fn kind_ok(kind: Kind, p: &Profile) -> bool {
    if p.filled == 0 {
        return true; // an all-empty column can't contradict anything
    }
    match kind {
        Kind::Num => p.ratio("num") >= 0.6,
        Kind::Date => p.ratio("date") >= 0.6,
        Kind::Currency => p.ratio("currency") >= 0.5,
        // Side columns come as words, as 1/-1, or as free text the user will map.
        Kind::Side => p.ratio("side") + p.ratio("num") + p.ratio("text") >= 0.5,
        Kind::Text | Kind::Enum => true,
    }
}

/// How strongly a header names a field. 0 = no relation.
fn header_score(
    header: &str,
    target: &str,
    aliases: &HashMap<String, String>,
    set: &TargetSet,
) -> u32 {
    if header.is_empty() {
        return 0;
    }
    // What the user taught us beats anything shipped.
    if aliases.get(header).map(|f| f == target).unwrap_or(false) {
        return 130;
    }
    let Some(t) = set.get(target) else { return 0 };
    let mut best = 0;
    for syn in t.synonyms {
        let syn = parse::normalize(syn);
        if syn.is_empty() {
            continue;
        }
        let score = if header == syn {
            100
        } else if header.starts_with(&format!("{syn} ")) || header.ends_with(&format!(" {syn}")) {
            78
        } else if syn.len() >= 4 && header.contains(&syn) {
            62
        } else if header.len() >= 4 && syn.contains(header) {
            56
        } else {
            0
        };
        best = best.max(score);
    }
    best
}

pub struct Suggestion {
    /// column index → target field id
    pub columns: HashMap<usize, String>,
    pub confidence: HashMap<usize, &'static str>,
    pub decimal: char,
    pub date_order: DateOrder,
    /// Proposed `conventions.qty_sign_is_side` (a signed quantity, no side column).
    pub qty_sign_is_side: bool,
}

impl Suggestion {
    /// Was a column proposed for this field?
    pub fn has(&self, field: &str) -> bool {
        self.columns.values().any(|v| v == field)
    }
}

/// Propose a mapping for a freshly read grid, over the targets `set` offers.
pub fn suggest(
    grid: &Grid,
    profiles: &[Profile],
    aliases: &HashMap<String, String>,
    set: &TargetSet,
) -> Suggestion {
    let headers: Vec<String> = grid.headers.iter().map(|h| parse::normalize(h)).collect();

    // Score every (column, field) pair that passes the content gate.
    let mut candidates: Vec<(u32, usize, &'static str)> = Vec::new();
    for (idx, header) in headers.iter().enumerate() {
        let p = &profiles[idx];
        for t in set.iter() {
            let mut score = header_score(header, t.id, aliases, set);
            if score == 0 {
                continue;
            }
            if !kind_ok(t.kind, p) {
                // An exact header match survives a content mismatch (the user will see
                // the rows fail and can fix it); a fuzzy one does not.
                if score < 100 {
                    continue;
                }
                score -= 45;
            }
            candidates.push((score, idx, t.id));
        }
    }

    // Value-only inference for columns whose header said nothing (or nothing legible).
    // A date column is offered to the set's date fields in declaration order (a trade
    // book's entry before its exit), one point apart so the first one wins the column.
    let date_fields: Vec<&'static str> = set.of_kind(Kind::Date).collect();
    for (idx, p) in profiles.iter().enumerate() {
        if candidates.iter().any(|(s, i, _)| *i == idx && *s >= 62) {
            continue;
        }
        if p.ratio("side") >= 0.8 && set.has("side") {
            candidates.push((70, idx, "side"));
        }
        if p.ratio("currency") >= 0.8 && set.has("currency") {
            candidates.push((70, idx, "currency"));
        }
        if p.ratio("date") >= 0.9 {
            for (n, field) in date_fields.iter().enumerate() {
                candidates.push((52u32.saturating_sub(n as u32), idx, field));
            }
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut columns: HashMap<usize, String> = HashMap::new();
    let mut confidence: HashMap<usize, &'static str> = HashMap::new();
    let mut used_fields: Vec<&str> = Vec::new();
    for (score, idx, field) in candidates {
        if score < THRESHOLD || columns.contains_key(&idx) || used_fields.contains(&field) {
            continue;
        }
        columns.insert(idx, field.to_string());
        confidence.insert(idx, if score >= 95 { "high" } else if score >= 65 { "medium" } else { "low" });
        used_fields.push(field);
    }

    // File-level formats: take the decision of the columns that carry numbers/dates.
    let decimal = profiles
        .iter()
        .filter(|p| p.ratio("num") >= 0.6)
        .map(|p| p.decimal)
        .find(|c| *c == ',')
        .unwrap_or('.');
    let date_order = columns
        .iter()
        .filter(|(_, f)| set.kind(f) == Kind::Date)
        .find_map(|(i, _)| profiles[*i].date_order)
        .unwrap_or(DateOrder::Auto);

    // No side column, but the quantity swings through zero: the sign is the direction.
    // A statement that lists fills (IBKR, and most execution reports) writes it that way
    // and names no side at all.
    let qty_sign_is_side = !columns.values().any(|v| v == "side")
        && columns
            .iter()
            .find(|(_, f)| *f == "quantity")
            .is_some_and(|(i, _)| profiles[*i].signed);

    Suggestion { columns, confidence, decimal, date_order, qty_sign_is_side }
}

// ── What the mapping step is shown ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ColumnInfo {
    pub index: usize,
    pub header: String,
    pub samples: Vec<String>,
    /// Dominant value kind: num | date | side | currency | text | empty.
    pub kind: &'static str,
    pub target: String,
    /// high | medium | low | manual | none — how sure the detector is about `target`.
    pub confidence: &'static str,
    pub decimal: String,
    pub date_order: Option<String>,
    /// A date column where every value could be read either way (01/02/2024).
    pub ambiguous_dates: bool,
}

/// Describe every column of a grid for the mapping step: what it holds, what it is
/// currently mapped to, and how sure the detector was about that. `confidence` is empty
/// when the mapping came from the client — an edited mapping has nothing to be sure of.
pub fn describe(
    grid: &Grid,
    profiles: &[Profile],
    columns: &BTreeMap<String, String>,
    confidence: &HashMap<usize, &'static str>,
) -> Vec<ColumnInfo> {
    (0..grid.width())
        .map(|i| {
            let p = &profiles[i];
            let target = columns.get(&i.to_string()).cloned().unwrap_or_else(|| "ignore".into());
            ColumnInfo {
                index: i,
                header: grid.headers[i].clone(),
                samples: grid.samples(i, 4),
                kind: p.dominant(),
                confidence: if target == "ignore" {
                    "none"
                } else {
                    confidence.get(&i).copied().unwrap_or("manual")
                },
                target,
                decimal: p.decimal.to_string(),
                date_order: p.date_order.map(|o| o.as_str().to_string()),
                ambiguous_dates: p.date_order.is_none() && p.dominant() == "date",
            }
        })
        .collect()
}

/// sha256 of the normalized, sorted header set — the identity of a source format.
pub fn fingerprint(headers: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut norm: Vec<String> = headers
        .iter()
        .map(|h| parse::normalize(h))
        .filter(|h| !h.is_empty())
        .collect();
    norm.sort();
    norm.dedup();
    let mut hasher = Sha256::new();
    hasher.update(norm.join("|").as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Jaccard similarity between two normalized header sets (0..1).
pub fn similarity(a: &[String], b: &[String]) -> f64 {
    let sa: std::collections::HashSet<&String> = a.iter().collect();
    let sb: std::collections::HashSet<&String> = b.iter().collect();
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    inter / union
}
