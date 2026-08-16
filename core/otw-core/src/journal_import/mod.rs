//! Generic trade-book import for the Trading Journal.
//!
//! Any broker, any language, any locale, without a per-broker parser: the file is read
//! into a grid, each column is scored against a multilingual dictionary **and** against
//! its own values, and the result is proposed as a mapping the user validates before a
//! single row is written. Validating saves the mapping, so the next export from the same
//! source maps itself; correcting one teaches the alias table.
//!
//! An *import mapping* is not a journal *template*: the template is the form used to log
//! a trade by hand, the mapping says which column of a foreign file is which trade field.
//!
//! Nothing is written until `commit`, and every commit is tagged with a batch, so a bad
//! import is reverted whole instead of being picked out of the journal by hand.

pub mod build;

/// Grid reading, vocabulary and column detection are shared with the other modules that
/// import a foreign file (`crate::import`); re-exported so the paths inside this module
/// — and the API layer above it — read as they always did.
pub use crate::import::{detect, dict, parse};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use otw_store::{journal, journal_import as store};

use build::{BuildCtx, RowError, Warning};
use detect::Profile;

/// Learned header aliases are per target set: "Date" means `entry_at` to a trade book
/// and `op_date` to an operations ledger, so the two vocabularies are kept apart.
const ALIAS_SCOPE: &str = "journal";

// ── The mapping document ─────────────────────────────────────────────────────

/// Everything needed to replay an import deterministically. Saved as-is in
/// `journal_import_mappings.mapping`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
    /// "roundtrip" (one row = one trade) | "executions" (one row = one fill).
    #[serde(default = "default_shape")]
    pub shape: String,
    /// 0-based index of the header row inside the file (skips broker preambles).
    #[serde(default)]
    pub header_row: Option<usize>,
    #[serde(default)]
    pub delimiter: Option<String>,
    /// Which table of a stacked statement to read. `None` = pick the biggest,
    /// `Some("")` = read the file flat, ignoring the sections.
    #[serde(default)]
    pub section: Option<String>,
    /// "auto" | "." | "," — decimal separator, forced for the whole file.
    #[serde(default = "default_auto")]
    pub decimal: String,
    /// "auto" | "dmy" | "mdy" | "ymd".
    #[serde(default = "default_auto")]
    pub date_order: String,
    /// Minutes to apply to timestamps that carry no zone of their own.
    #[serde(default)]
    pub tz_offset: i32,
    /// column index (as a string) → target field id, "field:<label>", or "ignore".
    #[serde(default)]
    pub columns: BTreeMap<String, String>,
    /// Per-field value overrides, e.g. value_maps.side = { "compra": "long" }.
    #[serde(default)]
    pub value_maps: BTreeMap<String, BTreeMap<String, String>>,
    /// Point value per ticker (futures/CFD contract size). A file names its contract but
    /// almost never says what a point is worth, and no detection can invent it — so it is
    /// asked per ticker, where the answer differs (MNQ 2, MES 5, shares 1).
    #[serde(default)]
    pub multipliers: BTreeMap<String, f64>,
    #[serde(default)]
    pub conventions: Conventions,
    #[serde(default)]
    pub defaults: Defaults,
}

fn default_shape() -> String {
    "roundtrip".to_string()
}
fn default_auto() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conventions {
    /// A negative quantity means a short/sell instead of a direction column.
    #[serde(default)]
    pub qty_sign_is_side: bool,
    /// Fees arrive as negative amounts in most exports; the journal stores a cost.
    #[serde(default = "yes")]
    pub fees_abs: bool,
    /// The file labels its columns the other way round.
    #[serde(default)]
    pub swap_entry_exit: bool,
}

fn yes() -> bool {
    true
}

impl Default for Conventions {
    fn default() -> Self {
        Self { qty_sign_is_side: false, fees_abs: true, swap_entry_exit: false }
    }
}

/// Values used for every row the file does not carry itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    pub category_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
    pub strategy_id: Option<Uuid>,
    #[serde(default = "usd")]
    pub currency: String,
    /// Currency the broker bills its commissions in, when that is not the one the
    /// instrument is quoted in — IBKR charges a USD contract in the account's base
    /// currency. `None` = fees are in the trade's own currency. Import-wide, not a
    /// column: a statement states it once, in its header, if at all.
    #[serde(default)]
    pub fee_currency: Option<String>,
    #[serde(default = "stock")]
    pub asset_class: String,
    #[serde(default = "unit")]
    pub unit_type: String,
    #[serde(default = "long")]
    pub side: String,
    #[serde(default = "one")]
    pub leverage: f64,
    #[serde(default = "one")]
    pub multiplier: f64,
}

fn usd() -> String {
    "USD".to_string()
}
fn stock() -> String {
    "stock".to_string()
}
fn unit() -> String {
    "unit".to_string()
}
fn long() -> String {
    "long".to_string()
}
fn one() -> f64 {
    1.0
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            category_id: None,
            template_id: None,
            strategy_id: None,
            currency: usd(),
            fee_currency: None,
            asset_class: stock(),
            unit_type: unit(),
            side: long(),
            leverage: 1.0,
            multiplier: 1.0,
        }
    }
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            shape: default_shape(),
            header_row: None,
            delimiter: None,
            section: None,
            decimal: default_auto(),
            date_order: default_auto(),
            tz_offset: 0,
            columns: BTreeMap::new(),
            value_maps: BTreeMap::new(),
            multipliers: BTreeMap::new(),
            conventions: Conventions::default(),
            defaults: Defaults::default(),
        }
    }
}

impl Mapping {
    /// The side overrides, normalized for matching.
    fn side_map(&self) -> Vec<(String, String)> {
        self.value_maps
            .get("side")
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (parse::normalize(k), v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn delimiter_char(&self) -> Option<char> {
        match self.delimiter.as_deref() {
            Some("\\t") | Some("\t") => Some('\t'),
            Some(s) => s.chars().next(),
            None => None,
        }
    }
}

// ── Analysis payload ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PreviewTrade {
    pub index: usize,
    pub source_rows: Vec<usize>,
    pub trade: journal::TradeInput,
    pub computed: journal::PnlPreview,
    pub warnings: Vec<Warning>,
    /// Already in the journal from an earlier import of the same row.
    pub duplicate: bool,
    /// PnL as written in the file, when a PnL column was mapped.
    pub pnl_source: Option<f64>,
    /// The file's PnL and the journal's disagree by more than a rounding error.
    pub pnl_mismatch: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct Stats {
    pub rows: usize,
    pub trades: usize,
    pub closed: usize,
    pub open: usize,
    pub duplicates: usize,
    pub errors: usize,
    pub warnings: usize,
    pub pnl_mismatches: usize,
    /// Net PnL per currency over the trades that would be imported.
    pub net_by_currency: Vec<(String, f64)>,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
}

/// A ticker found in the file, with how many rows carry it — the list the per-ticker
/// point values are filled in against.
#[derive(Debug, Serialize)]
pub struct TickerCount {
    pub ticker: String,
    pub rows: usize,
}

#[derive(Debug, Serialize)]
pub struct MappingMatch {
    pub id: Uuid,
    pub name: String,
    pub score: f64,
    pub exact: bool,
}

#[derive(Debug, Serialize)]
pub struct Analysis {
    pub filename: String,
    pub delimiter: String,
    pub header_row: usize,
    pub preamble: Vec<String>,
    /// Tables found in a stacked statement (empty for an ordinary flat file).
    pub sections: Vec<String>,
    pub section: Option<String>,
    pub headers: Vec<String>,
    pub row_count: usize,
    pub mapping: Mapping,
    pub columns: Vec<detect::ColumnInfo>,
    /// Distinct values of the mapped ticker column, most frequent first.
    pub tickers: Vec<TickerCount>,
    pub preview: Vec<PreviewTrade>,
    pub errors: Vec<RowError>,
    pub stats: Stats,
    pub mapping_match: Option<MappingMatch>,
    /// True when the mapping came from the detector (not from the client).
    pub detected: bool,
}

#[derive(Debug, Serialize)]
pub struct CommitReport {
    pub batch_id: Uuid,
    pub imported: usize,
    pub duplicates: usize,
    pub failed: usize,
    pub mapping_id: Option<Uuid>,
}

// ── Pipeline ─────────────────────────────────────────────────────────────────

/// Read the file, decide (or accept) a mapping, and build every trade it would import.
/// Writes nothing. `preview_limit` caps how many built trades come back to the UI; the
/// statistics always cover the whole file.
pub async fn analyze(
    pool: &PgPool,
    filename: &str,
    bytes: &[u8],
    supplied: Option<Mapping>,
    section: Option<String>,
    preview_limit: usize,
) -> anyhow::Result<Analysis> {
    let detected = supplied.is_none();
    let mut base = supplied.unwrap_or_default();
    // Pointing a fresh detection at another table of a statement: its columns are
    // different ones entirely, so this is a re-detection, not an edit of the mapping.
    if detected {
        base.section = section;
    }
    let grid = parse::read_grid(filename, bytes, base.header_row, base.delimiter_char(), base.section.as_deref())?;
    let profiles: Vec<Profile> = (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();

    let (mapping, confidence) = if detected {
        let aliases = otw_store::imports::load_aliases(pool, ALIAS_SCOPE).await.unwrap_or_default();
        let s = detect::suggest(&grid, &profiles, &aliases, &dict::JOURNAL);
        let mut m = Mapping {
            shape: shape_of(&s).to_string(),
            header_row: Some(grid.header_row),
            delimiter: Some(grid.delimiter.to_string()),
            decimal: s.decimal.to_string(),
            date_order: s.date_order.as_str().to_string(),
            section: grid.section.clone(),
            ..base
        };
        m.conventions.qty_sign_is_side = s.qty_sign_is_side;
        m.columns = (0..grid.width())
            .map(|i| {
                let target = s.columns.get(&i).cloned().unwrap_or_else(|| "ignore".to_string());
                (i.to_string(), target)
            })
            .collect();
        (m, s.confidence)
    } else {
        let mut m = base;
        m.header_row = Some(grid.header_row);
        m.delimiter = Some(grid.delimiter.to_string());
        m.section = m.section.or_else(|| grid.section.clone());
        (m, Default::default())
    };

    let ctx = build_ctx(pool).await?;
    let mut built = build::run(&grid, &mapping, &profiles, &ctx);
    convert_fees(pool, &mut built, &mapping).await;

    // Which rows are already in the journal (same file imported twice).
    let hashes: Vec<String> = built.trades.iter().map(|t| t.row_hash.clone()).collect();
    let known = store::existing_hashes(pool, &hashes).await.unwrap_or_default();

    // Columns, for the mapping table in the UI.
    let columns = detect::describe(&grid, &profiles, &mapping.columns, &confidence);

    // Distinct tickers, for the per-ticker point values. Read from the column rather
    // than from the built trades, so rows that failed to build still contribute.
    let tickers = mapping
        .columns
        .iter()
        .find(|(_, target)| *target == "ticker")
        .and_then(|(key, _)| key.parse::<usize>().ok())
        .map(|idx| {
            let mut counts: Vec<(String, usize)> = Vec::new();
            for row in &grid.rows {
                let v = grid.cell(row, idx);
                if v.is_empty() || is_placeholder(v) {
                    continue;
                }
                match counts.iter_mut().find(|(t, _)| t == v) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((v.to_string(), 1)),
                }
            }
            counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            counts.truncate(300);
            counts.into_iter().map(|(ticker, rows)| TickerCount { ticker, rows }).collect()
        })
        .unwrap_or_default();

    // Stats over the whole file, preview over the first `preview_limit` trades.
    let mut stats = Stats {
        rows: grid.rows.len(),
        trades: built.trades.len(),
        errors: built.errors.len(),
        ..Default::default()
    };
    let mut net: BTreeMap<String, f64> = BTreeMap::new();
    let mut preview = Vec::new();
    for (i, t) in built.trades.iter().enumerate() {
        let computed = journal::preview_pnl(&t.input);
        // Same row, same category — the pair is the identity, not the hash alone.
        let duplicate = known.contains(&(t.input.category_id, t.row_hash.clone()));
        let mismatch = match (t.pnl_source, computed.net_pnl) {
            (Some(src), Some(got)) => (src - got).abs() > 0.01 + src.abs() * 0.01,
            _ => false,
        };
        if duplicate {
            stats.duplicates += 1;
        }
        if computed.net_pnl.is_some() {
            stats.closed += 1;
        } else {
            stats.open += 1;
        }
        if mismatch {
            stats.pnl_mismatches += 1;
        }
        stats.warnings += t.warnings.len();
        if !duplicate {
            if let Some(n) = computed.net_pnl {
                *net.entry(t.input.currency.clone()).or_insert(0.0) += n;
            }
        }
        for d in [t.input.entry_at, t.input.exit_at].into_iter().flatten() {
            let iso = d.format(&Rfc3339).unwrap_or_default();
            if stats.first_date.as_ref().is_none_or(|f| iso < *f) {
                stats.first_date = Some(iso.clone());
            }
            if stats.last_date.as_ref().is_none_or(|l| iso > *l) {
                stats.last_date = Some(iso);
            }
        }
        if i < preview_limit {
            preview.push(PreviewTrade {
                index: i,
                source_rows: t.source_rows.clone(),
                trade: clone_input(&t.input),
                computed,
                warnings: t.warnings.clone(),
                duplicate,
                pnl_source: t.pnl_source,
                pnl_mismatch: mismatch,
            });
        }
    }
    stats.net_by_currency = net.into_iter().map(|(c, v)| (c, journal::round_dp(v, 2))).collect();

    let normalized_headers: Vec<String> = grid
        .headers
        .iter()
        .map(|h| parse::normalize(h))
        .filter(|h| !h.is_empty())
        .collect();
    let mapping_match = if detected {
        match_saved_mapping(pool, &normalized_headers).await
    } else {
        None
    };

    Ok(Analysis {
        filename: filename.to_string(),
        delimiter: grid.delimiter.to_string(),
        header_row: grid.header_row,
        preamble: grid.preamble.clone(),
        sections: grid.sections.clone(),
        section: grid.section.clone(),
        headers: grid.headers.clone(),
        row_count: grid.rows.len(),
        mapping,
        columns,
        tickers,
        preview,
        errors: built.errors,
        stats,
        mapping_match,
        detected,
    })
}

/// Import for real. Every trade lands in one batch, so the whole import can be undone.
pub async fn commit(
    pool: &PgPool,
    filename: &str,
    bytes: &[u8],
    mapping: Mapping,
    save_as: Option<String>,
) -> anyhow::Result<CommitReport> {
    let grid = parse::read_grid(
        filename,
        bytes,
        mapping.header_row,
        mapping.delimiter_char(),
        mapping.section.as_deref(),
    )?;
    let profiles: Vec<Profile> = (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
    let ctx = build_ctx(pool).await?;
    let mut built = build::run(&grid, &mapping, &profiles, &ctx);
    convert_fees(pool, &mut built, &mapping).await;

    let normalized_headers: Vec<String> = grid
        .headers
        .iter()
        .map(|h| parse::normalize(h))
        .filter(|h| !h.is_empty())
        .collect();

    // Save the mapping first so the batch can point at it.
    let mut mapping_id = None;
    let mut mapping_name = String::new();
    if let Some(name) = save_as.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        let doc = serde_json::to_value(&mapping)?;
        let headers = serde_json::to_value(&normalized_headers)?;
        let fp = detect::fingerprint(&grid.headers);
        let saved = store::upsert_mapping(pool, &name, &fp, &headers, &doc).await?;
        mapping_name = saved.name.clone();
        mapping_id = Some(saved.id);
    }

    let batch = store::add_batch(
        pool,
        mapping_id,
        &mapping_name,
        filename,
        mapping.defaults.category_id.or(Some(ctx.default_category)),
        &mapping.shape,
        grid.rows.len() as i32,
    )
    .await?;

    let (mut imported, mut duplicates) = (0usize, 0usize);
    let mut insert_error = None;
    for t in &built.trades {
        match store::add_imported_trade(pool, &t.input, batch, &t.row_hash).await {
            Ok(true) => imported += 1,
            Ok(false) => duplicates += 1,
            // Stop, but keep the batch: whatever landed is still tagged, so the user can
            // revert the half-import in one click instead of hunting for its trades.
            Err(e) => {
                insert_error = Some(e);
                break;
            }
        }
    }
    let failed = built.errors.len();
    store::finish_batch(pool, batch, imported as i32, duplicates as i32, failed as i32).await?;
    if let Some(e) = insert_error {
        return Err(e.context(format!(
            "import stopped after {imported} trades — revert this import to undo them"
        )));
    }
    if let Some(id) = mapping_id {
        store::touch_mapping(pool, id).await.ok();
    }

    // Learn the vocabulary of this file: every column the user kept teaches its header.
    for (key, target) in &mapping.columns {
        if target == "ignore" || target.is_empty() {
            continue;
        }
        let Ok(idx) = key.parse::<usize>() else { continue };
        let Some(header) = grid.headers.get(idx) else { continue };
        let norm = parse::normalize(header);
        if norm.is_empty() {
            continue;
        }
        otw_store::imports::record_alias(pool, ALIAS_SCOPE, &norm, target).await.ok();
    }

    Ok(CommitReport { batch_id: batch, imported, duplicates, failed, mapping_id })
}

/// Convert every fee into its trade's currency when the broker bills in another one.
///
/// A trade carries a single currency and its PnL is netted in it (`net = gross − fees`),
/// so a fee in a foreign currency cannot travel with the trade — it is converted here,
/// at the trade's own date with the journal's FX rates, before it reaches the journal.
/// Runs on the built trades of both `analyze` and `commit`, so the preview shows exactly
/// what will be written. A row whose rate is missing keeps its raw amount and says so;
/// nothing is written to the FX pending list (analyze must stay read-only).
async fn convert_fees(pool: &PgPool, built: &mut build::BuildResult, mapping: &Mapping) {
    let Some(from) = mapping
        .defaults
        .fee_currency
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    else {
        return;
    };
    let mut fx = otw_store::journal_fx::FxCache::new();
    for t in &mut built.trades {
        let to = t.input.currency.clone();
        if from == to {
            continue;
        }
        let date = t
            .input
            .exit_at
            .or(t.input.entry_at)
            .map(|d| d.date())
            .unwrap_or_else(|| time::OffsetDateTime::now_utc().date());
        // One unit tells us the rate; applying it per amount keeps the arithmetic in
        // one place and costs one lookup per trade (the cache makes it one per date).
        let rate = match fx.convert(pool, 1.0, from, &to, date).await {
            Ok(Some(r)) => r,
            _ => {
                t.warnings.push(Warning {
                    code: "fee_currency".into(),
                    message: format!(
                        "no {from}→{to} rate for {date} — fees left in {from}"
                    ),
                });
                continue;
            }
        };
        if rate == 1.0 {
            continue;
        }
        t.input.fees *= rate;
        for leg in t.input.entries.iter_mut().chain(t.input.exits.iter_mut()) {
            leg.fees *= rate;
        }
        t.warnings.push(Warning {
            code: "fee_currency".into(),
            message: format!("fees converted {from} → {to} at {rate:.4} ({date})"),
        });
    }
}

/// The journal objects an import resolves names against.
async fn build_ctx(pool: &PgPool) -> anyhow::Result<BuildCtx> {
    let categories = journal::list_categories(pool).await?;
    let default_category = categories
        .iter()
        .find(|c| c.is_default)
        .or_else(|| categories.first())
        .map(|c| c.id)
        .ok_or_else(|| anyhow::anyhow!("the journal has no category to import into"))?;
    let strategies = journal::list_strategies(pool).await?;
    Ok(BuildCtx {
        categories: categories.into_iter().map(|c| (c.id, c.name, c.is_default)).collect(),
        strategies: strategies.into_iter().map(|s| (s.id, s.name)).collect(),
        default_category,
    })
}

/// The saved mapping that best covers this file's headers, if any is close enough.
async fn match_saved_mapping(pool: &PgPool, headers: &[String]) -> Option<MappingMatch> {
    let saved = store::mapping_headers(pool).await.ok()?;
    let mut best: Option<MappingMatch> = None;
    for (id, name, _fp, saved_headers) in saved {
        let score = detect::similarity(headers, &saved_headers);
        if score < 0.7 {
            continue;
        }
        let m = MappingMatch { id, name, score: journal::round_dp(score, 3), exact: score >= 0.999 };
        if best.as_ref().is_none_or(|b| m.score > b.score) {
            best = Some(m);
        }
    }
    best
}

/// `TradeInput` is not `Clone` (it is a request payload); the preview needs a copy.
fn clone_input(t: &journal::TradeInput) -> journal::TradeInput {
    journal::TradeInput {
        category_id: t.category_id,
        template_id: t.template_id,
        strategy_id: t.strategy_id,
        ticker: t.ticker.clone(),
        asset_class: t.asset_class.clone(),
        exchange: t.exchange.clone(),
        side: t.side.clone(),
        currency: t.currency.clone(),
        unit_type: t.unit_type.clone(),
        fee_schedule_id: t.fee_schedule_id,
        entry_at: t.entry_at,
        exit_at: t.exit_at,
        entry_price: t.entry_price,
        exit_price: t.exit_price,
        quantity: t.quantity,
        fees: t.fees,
        leverage: t.leverage,
        multiplier: t.multiplier,
        signal_name: t.signal_name.clone(),
        feedback: t.feedback.clone(),
        images: t.images.clone(),
        fields: t.fields.clone(),
        advanced: t.advanced,
        cost_basis_method: t.cost_basis_method.clone(),
        entries: t.entries.clone(),
        exits: t.exits.clone(),
        brackets: t.brackets.clone(),
    }
}

/// A cell that stands for "nothing here" rather than naming something. Statements write
/// `-` in every column of a cash movement (deposit, fee, tax), and those rows are not
/// trades — offering to set a point value for `-` would be asking about a bank transfer.
fn is_placeholder(v: &str) -> bool {
    let t = v.trim();
    !t.is_empty()
        && (t.chars().all(|c| matches!(c, '-' | '–' | '—' | '.' | '_'))
            || matches!(parse::normalize(t).as_str(), "n a" | "na" | "none" | "null" | "nil"))
}

/// Quantity without float noise, for warning messages.
pub fn fmt_qty(q: f64) -> String {
    journal::round_dp(q, 8).to_string()
}

/// One row = one trade, or one row = one execution? An exit column settles it; a book
/// with only a direction, a price and a quantity is a list of fills — which is how every
/// execution report writes itself.
fn shape_of(s: &detect::Suggestion) -> &'static str {
    if s.has("exit_price") || s.has("exit_at") {
        "roundtrip"
    } else if (s.has("side") || s.qty_sign_is_side) && s.has("entry_price") && s.has("quantity") {
        "executions"
    } else {
        "roundtrip"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Detect + build the way `analyze` does, without a database.
    fn run(file: &str) -> (Mapping, build::BuildResult) {
        let grid = parse::read_grid("book.csv", file.as_bytes(), None, None, None).expect("grid");
        let profiles: Vec<Profile> =
            (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
        let s = detect::suggest(&grid, &profiles, &Default::default(), &dict::JOURNAL);
        let mut mapping = Mapping {
            shape: shape_of(&s).to_string(),
            decimal: s.decimal.to_string(),
            date_order: s.date_order.as_str().to_string(),
            columns: (0..grid.width())
                .map(|i| (i.to_string(), s.columns.get(&i).cloned().unwrap_or("ignore".into())))
                .collect(),
            ..Default::default()
        };
        mapping.conventions.qty_sign_is_side = s.qty_sign_is_side;
        let ctx = BuildCtx {
            categories: vec![],
            strategies: vec![],
            default_category: Uuid::nil(),
        };
        let built = build::run(&grid, &mapping, &profiles, &ctx);
        (mapping, built)
    }

    fn target_of(m: &Mapping, idx: usize) -> &str {
        m.columns.get(&idx.to_string()).map(|s| s.as_str()).unwrap_or("")
    }

    #[test]
    fn ibkr_activity_statement_padded_by_a_spreadsheet() {
        // An IBKR activity statement that went through a spreadsheet in a semicolon
        // locale: every record came back quoted whole and padded with empty cells, so
        // the file reads as one column until the padding is peeled off. Once peeled it
        // is an ordinary stacked statement — and its Trades table names no side, the
        // sign of the quantity carries it.
        let file = concat!(
            "Statement,Header,Field Name,Field Value;;;;;;;;;;;;;\n",
            "Statement,Data,BrokerName,Interactive Brokers (U.K.) Limited;;;;;;;;;;;;;\n",
            "Account Information,Data,Base Currency,CHF;;;;;;;;;;;;;\n",
            // A summary line with no tag at all, and prose holding the outer delimiter:
            // both appear in a real statement and neither must stop the file being read.
            "Total P/L for Statement Period,,,,,,,,,,,,78.355887,;;;;;;;;;;;;;\n",
            "Codes,Header,Code,Meaning;;;;;;;;;;;;;\n",
            "Codes,Data,IM,A portion of the order was executed against IB or an affiliate; IB acted as agent on a portion.;;;;;;;;;;;;\n",
            "Trades,Header,DataDiscriminator,Asset Category,Currency,Symbol,Date/Time,Quantity,T. Price,C. Price,Notional Value,Comm/Fee,Basis,Realized P/L,Realized P/L %,MTM P/L,Code;;;;;;;;;;;;;\n",
            "\"Trades,Data,Order,Futures,USD,MESU6,\"\"2026-08-04, 07:01:59\"\",1,7644,7765.5,-38220,-0.61,38220.61,0,0,607.5,O\";;;;;;;;;;;;;\n",
            "\"Trades,Data,Order,Futures,USD,MESU6,\"\"2026-08-04, 07:27:50\"\",-1,7647,7765.5,38235,-0.61,-38220.61,13.78,0.036053846,-592.5,C\";;;;;;;;;;;;;\n",
            "\"Trades,Data,Order,Futures,USD,MESU6,\"\"2026-08-05, 05:36:01\"\",-1,7791.25,7749.5,38956.25,-0.61,-38955.64,0,0,208.75,O\";;;;;;;;;;;;;\n",
            "\"Trades,Data,Order,Futures,USD,MESU6,\"\"2026-08-05, 05:40:37\"\",1,7789.25,7749.5,-38946.25,-0.61,38955.64,8.78,0.022538457,-198.75,C\";;;;;;;;;;;;;\n",
            "Trades,SubTotal,,Futures,USD,MESU6,,0,,,-8.75,-39.04,0,-47.79,-2.99,-8.75,;;;;;;;;;;;;;\n",
            "Trades,Total,,Futures,USD,,,,,,231.25,-131.76,0.000000001,99.49,,231.25, ;;;;;;;;;;;;;\n",
            "Fees,Header,Subtitle,Currency,Date,Description,Amount;;;;;;;;;;;;;\n",
            "Fees,Data,Other Fees,CHF,2026-08-03,CME Level I,-1.25;;;;;;;;;;;;;\n",
        );
        let grid = parse::read_grid("stmt.csv", file.as_bytes(), None, None, None).expect("grid");
        assert_eq!(grid.section.as_deref(), Some("Trades"));
        assert_eq!(grid.headers[3], "Symbol");
        // SubTotal / Total rows carry another tag and never reach the table.
        assert_eq!(grid.rows.len(), 4);

        let (m, built) = run(file);
        assert_eq!(target_of(&m, 2), "currency");
        assert_eq!(target_of(&m, 3), "ticker");
        assert_eq!(target_of(&m, 4), "entry_at");
        assert_eq!(target_of(&m, 5), "quantity");
        assert_eq!(target_of(&m, 6), "entry_price");
        assert_eq!(target_of(&m, 9), "fees");
        // No side column: the signed quantity is the direction, which also makes this
        // a list of fills rather than a list of round trips.
        assert!(m.conventions.qty_sign_is_side);
        assert_eq!(m.shape, "executions");
        assert!(built.errors.is_empty(), "{:?}", built.errors);

        // Two positions: bought then sold, then sold then bought back.
        assert_eq!(built.trades.len(), 2);
        let long = &built.trades[0].input;
        assert_eq!(long.ticker, "MESU6");
        assert_eq!(long.side, "long");
        assert_eq!(long.asset_class, "future");
        assert_eq!(long.currency, "USD");
        assert_eq!(long.entries.len(), 1);
        assert_eq!(long.exits.len(), 1);
        assert_eq!(long.entries[0].fees, 0.61); // per fill, on its own leg
        let short = &built.trades[1].input;
        assert_eq!(short.side, "short");
        assert_eq!(short.entries[0].price, 7791.25);
        assert_eq!(short.exits[0].price, 7789.25);
    }

    #[test]
    fn french_semicolon_book_maps_and_parses() {
        // Semicolon-delimited, decimal commas, day-first dates, French headers, and two
        // preamble lines above the header — a very ordinary European export.
        let file = "Relevé de compte;;;;;;\n\
                    Compte 12345;;;;;;\n\
                    Symbole;Sens;Quantité;Prix d'entrée;Prix de sortie;Date d'entrée;Frais\n\
                    AAPL;Achat;10;150,25;160,50;15/01/2024 09:30;-1,50\n\
                    TSLA;Vente;5;250,00;240,00;03/02/2024 14:00;-2,00\n";
        let (m, built) = run(file);
        assert_eq!(target_of(&m, 0), "ticker");
        assert_eq!(target_of(&m, 1), "side");
        assert_eq!(target_of(&m, 2), "quantity");
        assert_eq!(target_of(&m, 3), "entry_price");
        assert_eq!(target_of(&m, 4), "exit_price");
        assert_eq!(target_of(&m, 5), "entry_at");
        assert_eq!(target_of(&m, 6), "fees");
        assert_eq!(m.shape, "roundtrip");
        assert!(built.errors.is_empty(), "{:?}", built.errors);

        let t = &built.trades[0].input;
        assert_eq!(t.ticker, "AAPL");
        assert_eq!(t.side, "long");
        assert_eq!(t.entry_price, Some(150.25));
        assert_eq!(t.exit_price, Some(160.50));
        assert_eq!(t.quantity, Some(10.0));
        assert_eq!(t.fees, 1.50); // stored as a cost, whatever sign the file used
        let at = t.entry_at.expect("entry date");
        assert_eq!(at.day(), 15);
        assert_eq!(u8::from(at.month()), 1);
        assert_eq!(built.trades[1].input.side, "short");
        // 15/01 is unambiguous, so the whole column is read day-first.
        assert_eq!(built.trades[1].input.entry_at.unwrap().day(), 3);
    }

    #[test]
    fn english_book_with_month_names_and_thousands() {
        let file = "Symbol,Side,Qty,Entry Price,Exit Price,Open Time,Close Time,Commission,P&L\n\
                    ES,Sell,2,\"4,512.75\",\"4,480.25\",\"Jan 15, 2024 09:30\",\"Jan 15, 2024 15:45\",4.20,64.99\n";
        let (m, built) = run(file);
        assert_eq!(target_of(&m, 1), "side");
        assert_eq!(target_of(&m, 7), "fees");
        assert_eq!(target_of(&m, 8), "pnl_check");
        let t = &built.trades[0];
        assert_eq!(t.input.entry_price, Some(4512.75));
        assert_eq!(t.input.side, "short");
        assert_eq!(t.input.exit_at.unwrap().hour(), 15);
        // The PnL column is kept for cross-checking, not stored.
        assert_eq!(t.pnl_source, Some(64.99));
        let pnl = journal::preview_pnl(&t.input);
        assert_eq!(journal::round_dp(pnl.net_pnl.unwrap(), 2), 60.8); // (4512.75−4480.25)×2 − 4.20
    }

    #[test]
    fn executions_fold_into_positions() {
        // One row per fill: two buys then one sell that closes the whole position.
        let file = "Date,Symbol,Action,Quantity,Price,Fee\n\
                    2024-01-15T09:30:00Z,BTCUSDT,BUY,1,40000,10\n\
                    2024-01-15T10:00:00Z,BTCUSDT,BUY,1,42000,10\n\
                    2024-01-16T11:00:00Z,BTCUSDT,SELL,2,45000,20\n";
        let (m, built) = run(file);
        assert_eq!(m.shape, "executions");
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        assert_eq!(built.trades.len(), 1);
        let t = &built.trades[0].input;
        assert!(t.advanced);
        assert_eq!(t.entries.len(), 2);
        assert_eq!(t.exits.len(), 1);
        assert_eq!(t.side, "long");
        assert_eq!(built.trades[0].source_rows, vec![2, 3, 4]);
        let pnl = journal::preview_pnl(t);
        // (45000 − 41000) × 2 − 40 of fees.
        assert_eq!(pnl.net_pnl, Some(7960.0));
        assert_eq!(pnl.open_qty, 0.0);
    }

    #[test]
    fn an_over_closing_fill_flips_the_position() {
        let file = "Date,Symbol,Action,Quantity,Price,Fee\n\
                    2024-01-15,AAPL,BUY,10,100,0\n\
                    2024-01-16,AAPL,SELL,15,110,0\n";
        let (_, built) = run(file);
        assert_eq!(built.trades.len(), 2);
        assert_eq!(built.trades[0].input.side, "long");
        assert_eq!(journal::preview_pnl(&built.trades[0].input).net_pnl, Some(100.0));
        // The 5 left over open a short that the file never closes.
        let open = &built.trades[1];
        assert_eq!(open.input.side, "short");
        assert_eq!(journal::preview_pnl(&open.input).open_qty, 5.0);
        assert!(open.warnings.iter().any(|w| w.code == "open"));
    }

    #[test]
    fn unmappable_rows_are_reported_not_dropped_silently() {
        let file = "Symbol,Side,Qty,Entry Price,Exit Price\n\
                    AAPL,Buy,10,150,160\n\
                    ,,,,\n\
                    MSFT,Buy,,400,410\n";
        let (_, built) = run(file);
        assert_eq!(built.trades.len(), 1);
        assert_eq!(built.errors.len(), 1);
        assert_eq!(built.errors[0].row, 4); // 1-based, header included
        assert!(built.errors[0].message.contains("quantity"));
    }

    #[test]
    fn the_same_row_twice_hashes_differently_than_the_same_file_twice() {
        let file = "Symbol,Side,Qty,Entry Price,Exit Price\n\
                    AAPL,Buy,10,150,160\n\
                    AAPL,Buy,10,150,160\n";
        let (_, first) = run(file);
        let (_, second) = run(file);
        // Two identical trades in one file stay two distinct rows…
        assert_ne!(first.trades[0].row_hash, first.trades[1].row_hash);
        // …and re-importing the file produces the very same hashes (so it is a no-op).
        assert_eq!(first.trades[0].row_hash, second.trades[0].row_hash);
        assert_eq!(first.trades[1].row_hash, second.trades[1].row_hash);
    }

    #[test]
    fn json_exports_are_read_too() {
        let file = r#"[{"symbol":"AAPL","side":"buy","quantity":3,"entryPrice":100,"exitPrice":110}]"#;
        let grid = parse::read_grid("book.json", file.as_bytes(), None, None, None).expect("grid");
        assert_eq!(grid.headers.len(), 5);
        assert_eq!(grid.rows.len(), 1);
        let profiles: Vec<Profile> =
            (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
        let s = detect::suggest(&grid, &profiles, &Default::default(), &dict::JOURNAL);
        // JSON keys come back sorted, so look each column up by its header.
        let of = |name: &str| {
            let i = grid.headers.iter().position(|h| h == name).expect(name);
            s.columns.get(&i).map(String::as_str)
        };
        assert_eq!(of("symbol"), Some("ticker"));
        assert_eq!(of("entryPrice"), Some("entry_price"));
        assert_eq!(of("exitPrice"), Some("exit_price"));
        assert_eq!(of("side"), Some("side"));
    }

    /// A statement that stacks several tables, each row prefixed with its section name
    /// and a role tag. Shape reproduced from a real futures transaction statement.
    const STATEMENT: &str = "\
Statement,Header,Field Name,Field Value
Statement,Data,Title,Transaction History
Statement,Data,Period,\"July 7, 2026 - August 7, 2026\"
Summary,Header,Field Name,Field Value
Summary,Data,Base Currency,CHF
Summary,Data,Ending Cash,4075.85934
Transaction History,Header,Date,Account,Description,Transaction Type,Symbol,Quantity,Price,Price Currency,Gross Amount ,Commission,Net Amount
Transaction History,Data,2026-08-07,Trading,MNQ 18SEP26,Buy,MNQU6,1.0,29717.0,USD,-48022.672(1),-0.49288,189.79112
Transaction History,Data,2026-08-07,Trading,MNQ 18SEP26,Sell,MNQU6,-1.0,29797.25,USD,48152.356(1),-0.49288,-61.09288
Transaction History,Data,2026-08-07,Trading,MNQ 18SEP26,Buy,MNQU6,1.0,29762.0,USD,-48095.392(1),-0.49288,117.07112
Transaction History,Data,2026-08-07,Trading,MNQ 18SEP26,Sell,MNQU6,-1.0,29739.25,USD,48058.628(1),-0.49288,-154.82088
Transaction History,Notes,\"1. Values shown are in notional terms.\"
";

    #[test]
    fn a_stacked_statement_reads_its_biggest_table() {
        let grid = parse::read_grid("stmt.csv", STATEMENT.as_bytes(), None, None, None).expect("grid");
        assert_eq!(grid.sections, ["Statement", "Summary", "Transaction History"]);
        assert_eq!(grid.section.as_deref(), Some("Transaction History"));
        // The two routing columns are gone, and so is the trailing Notes row.
        assert_eq!(grid.headers[0], "Date");
        assert_eq!(grid.headers[5], "Quantity");
        assert_eq!(grid.headers.len(), 11);
        assert_eq!(grid.rows.len(), 4);
        assert_eq!(grid.cell(&grid.rows[0], 4), "MNQU6");

        let (m, built) = run(STATEMENT);
        assert_eq!(target_of(&m, 0), "entry_at");
        assert_eq!(target_of(&m, 3), "side"); // "Transaction Type"
        assert_eq!(target_of(&m, 4), "ticker"); // "Symbol"
        assert_eq!(target_of(&m, 5), "quantity");
        assert_eq!(target_of(&m, 6), "entry_price");
        assert_eq!(target_of(&m, 7), "currency"); // "Price Currency"
        assert_eq!(target_of(&m, 9), "fees"); // "Commission"
        // Gross/Net Amount are notional totals, not trade fields — left alone.
        assert_eq!(target_of(&m, 8), "ignore");
        assert_eq!(m.shape, "executions");
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        assert_eq!(built.trades.len(), 2);
        assert_eq!(built.trades[0].input.ticker, "MNQU6");
        assert_eq!(built.trades[0].input.side, "long");
    }

    #[test]
    fn a_stacked_statement_can_be_read_flat_or_by_section() {
        let by_name =
            parse::read_grid("stmt.csv", STATEMENT.as_bytes(), None, None, Some("Summary")).expect("grid");
        assert_eq!(by_name.headers, ["Field Name", "Field Value"]);
        assert_eq!(by_name.rows.len(), 2);
        // Some("") escapes the section machinery entirely.
        let flat = parse::read_grid("stmt.csv", STATEMENT.as_bytes(), None, None, Some("")).expect("grid");
        assert!(flat.sections.is_empty());
        assert_eq!(flat.headers[0], "Statement");
    }

    #[test]
    fn flat_files_keep_the_header_they_had_before_sections_existed() {
        // The two shapes that already imported correctly must not move an inch.
        let paper = "Symbol,Side,Type,Quantity,Fill price,Status,Commission,Closing time\n\
                     NASDAQ:MIDD,Sell,Market,17,138.04,Filled,1,2026-07-07 20:31:12\n\
                     NASDAQ:KLIC,Sell,Market,23,107.49,Filled,1,2026-07-07 20:31:12\n";
        let g = parse::read_grid("p.csv", paper.as_bytes(), None, None, None).expect("grid");
        assert_eq!(g.header_row, 0);
        assert_eq!(g.headers[0], "Symbol");
        assert!(g.sections.is_empty());

        let ibkr = "Symbol,Side,Quantity,Fill price,Time,Net amount,Commission\n\
                    Sep18 '26,Sell,1,29840,2026-08-07 21:55:13,59680,0.61\n\
                    Sep18 '26,Buy,1,29761,2026-08-07 15:49:33,59522,0.61\n";
        let g = parse::read_grid("i.csv", ibkr.as_bytes(), None, None, None).expect("grid");
        assert_eq!(g.header_row, 0);
        assert_eq!(g.headers[0], "Symbol");
        assert!(g.sections.is_empty());

        // A preamble narrower than the table still resolves to the real header.
        let with_preamble = "Relevé de compte\nCompte 12345\n\nSymbole;Sens;Quantité;Prix\nAAPL;Achat;10;150,25\n";
        let g = parse::read_grid("f.csv", with_preamble.as_bytes(), None, None, None).expect("grid");
        assert_eq!(g.headers[0], "Symbole");
        assert_eq!(g.rows.len(), 1);
    }

    #[test]
    fn a_footnote_marker_is_not_part_of_the_number() {
        assert_eq!(parse::parse_number("-48022.672(1)", '.'), Some(-48022.672));
        assert_eq!(parse::parse_number("48152.356(1)", '.'), Some(48152.356));
        // The accounting negative still works.
        assert_eq!(parse::parse_number("(1234.50)", '.'), Some(-1234.50));
        assert_eq!(parse::parse_number("(12)", '.'), Some(-12.0));
    }

    #[test]
    fn each_ticker_carries_its_own_point_value() {
        // Two contracts in one file, worth 2 and 5 a point — the file says neither.
        let file = "Date,Symbol,Action,Quantity,Price\n\
                    2026-08-04,MNQU6,Buy,1,29717\n\
                    2026-08-04,MNQU6,Sell,1,29727\n\
                    2026-08-05,MESU6,Buy,1,7760\n\
                    2026-08-05,MESU6,Sell,1,7770\n";
        let grid = parse::read_grid("f.csv", file.as_bytes(), None, None, None).expect("grid");
        let profiles: Vec<Profile> =
            (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
        let s = detect::suggest(&grid, &profiles, &Default::default(), &dict::JOURNAL);
        let mut mapping = Mapping {
            shape: shape_of(&s).to_string(),
            columns: (0..grid.width())
                .map(|i| (i.to_string(), s.columns.get(&i).cloned().unwrap_or("ignore".into())))
                .collect(),
            ..Default::default()
        };
        mapping.multipliers.insert("MNQU6".into(), 2.0);
        mapping.multipliers.insert("mesu6".into(), 5.0); // matching ignores case
        let ctx = BuildCtx { categories: vec![], strategies: vec![], default_category: Uuid::nil() };
        let built = build::run(&grid, &mapping, &profiles, &ctx);

        assert_eq!(built.trades.len(), 2);
        let by = |t: &str| {
            built
                .trades
                .iter()
                .find(|b| b.input.ticker == t)
                .map(|b| journal::preview_pnl(&b.input).net_pnl.unwrap())
                .unwrap()
        };
        assert_eq!(by("MNQU6"), 20.0); // 10 points × 2
        assert_eq!(by("MESU6"), 50.0); // 10 points × 5
    }

    #[test]
    fn index_future_prices_are_numbers_not_excel_dates() {
        // 20 000–80 000 is also the Excel serial-day window: futures and crypto prices
        // live there, and a bare number carries nothing that says "day".
        for v in ["29717.0", "7766.75", "29840", "68450.25"] {
            assert_eq!(parse::value_kind(v, '.'), parse::ValueKind::Num, "{v}");
        }
        // Written as a date, the dot form still reads as one.
        assert_eq!(parse::value_kind("12.05.2024", '.'), parse::ValueKind::Date);
        // And a column explicitly mapped to a date still accepts Excel serials.
        assert!(parse::parse_datetime("45000.5", parse::DateOrder::Auto, 0).is_some());
    }

    #[test]
    fn decimal_convention_is_decided_per_column() {
        // "1.234" is a decimal price in a column that elsewhere writes "1.2".
        assert_eq!(parse::column_decimal(&["1.234", "1.2"]), '.');
        // A comma column: "1,234" is 1.234 because another row writes "1,25".
        assert_eq!(parse::column_decimal(&["1,234", "1,25"]), ',');
        // Mixed separators settle it outright.
        assert_eq!(parse::column_decimal(&["1.234,56"]), ',');
        assert_eq!(parse::column_decimal(&["1,234.56"]), '.');
        assert_eq!(parse::parse_number("(1 234,50)", ','), Some(-1234.50));
        assert_eq!(parse::parse_number("$1,234.50", '.'), Some(1234.50));
    }
}
