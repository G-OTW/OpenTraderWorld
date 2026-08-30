//! Generic operations-ledger import for the Portfolio Tracker.
//!
//! Same pipeline as the journal's trade-book import — the file is read into a grid, each
//! column is scored against the shared multilingual dictionary **and** against its own
//! values, and the result is proposed as a mapping the user validates before a single row
//! is written (`crate::import`). What differs is the other end: one row is one
//! **operation**, a buy or a sell against one asset of one portfolio.
//!
//! Two things this import refuses to guess, because no file states them:
//!   - **which asset a symbol is.** "VT" in a broker export is a string; an asset here is
//!     a price source (provider + id). The match against the portfolio's own assets is
//!     offered, everything else is asked, and nothing is created until commit.
//!   - **what a row that is neither a buy nor a sell means.** A dividend, a deposit or a
//!     transfer has no place in an operations ledger, so it is listed as an error rather
//!     than folded into a trade it never was.
//!
//! De-duplication is **per portfolio**: a portfolio is the book being kept, the same
//! statement may legitimately feed two of them, while importing it twice into one stays
//! a no-op.

pub mod build;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use otw_store::{portfolios as store, portfolios_import as istore};

use crate::import::{detect, dict, parse};
use crate::portfolios::prices;
use build::{RowError, Warning};
use detect::Profile;

/// Learned header aliases are per target set (see `crate::import::dict`).
const ALIAS_SCOPE: &str = "portfolios";

/// Providers an asset can be priced from — the same whitelist `add_asset` enforces.
const PROVIDERS: [&str; 2] = ["coingecko", "yahoo"];

// ── The mapping document ─────────────────────────────────────────────────────

/// Everything needed to replay an import deterministically. Saved as-is in
/// `portfolio_import_mappings.mapping`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
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
    /// Minutes to apply to timestamps that carry no zone of their own. An operation
    /// keeps only its day, so this decides which day a late-evening fill lands on.
    #[serde(default)]
    pub tz_offset: i32,
    /// column index (as a string) → target field id, or "ignore".
    #[serde(default)]
    pub columns: BTreeMap<String, String>,
    /// Per-field value overrides, e.g. value_maps.side = { "compra": "long" }.
    #[serde(default)]
    pub value_maps: BTreeMap<String, BTreeMap<String, String>>,
    /// What each symbol of the file is, in this portfolio. Keyed on the upper-cased
    /// symbol. Absent = still to be decided (a symbol that already matches an asset of
    /// the portfolio resolves itself and is deliberately **not** written here, so a saved
    /// mapping stays about the file and can be reused by any portfolio).
    #[serde(default)]
    pub symbols: BTreeMap<String, SymbolChoice>,
    #[serde(default)]
    pub conventions: Conventions,
    #[serde(default)]
    pub defaults: Defaults,
}

/// What the user decided a symbol of the file is.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SymbolChoice {
    /// Leave every row carrying this symbol out of the import.
    Skip,
    /// An asset the portfolio already holds.
    Asset { asset_id: Uuid },
    /// An asset to create on commit, as picked from the symbol search.
    New {
        asset_class: String,
        provider: String,
        provider_id: String,
        symbol: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        currency: Option<String>,
    },
}

fn default_auto() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conventions {
    /// A negative quantity means a sell instead of a direction column.
    #[serde(default)]
    pub qty_sign_is_side: bool,
    /// Fees arrive as negative amounts in most exports; a portfolio stores a cost.
    #[serde(default = "yes")]
    pub fees_abs: bool,
}

fn yes() -> bool {
    true
}

impl Default for Conventions {
    fn default() -> Self {
        Self { qty_sign_is_side: false, fees_abs: true }
    }
}

/// Values used for every row the file does not carry itself.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Defaults {
    /// What a row with no readable direction is: "" (an error — the default), "buy" or
    /// "sell". A file of nothing but purchases says so once, here, instead of having a
    /// direction invented per row.
    #[serde(default)]
    pub side: String,
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            header_row: None,
            delimiter: None,
            section: None,
            decimal: default_auto(),
            date_order: default_auto(),
            tz_offset: 0,
            columns: BTreeMap::new(),
            value_maps: BTreeMap::new(),
            symbols: BTreeMap::new(),
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
            .map(|m| m.iter().map(|(k, v)| (parse::normalize(k), v.clone())).collect())
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

/// Symbols are matched case-insensitively; the file writes "vt", the portfolio "VT".
fn sym_key(symbol: &str) -> String {
    symbol.trim().to_uppercase()
}

// ── Analysis payload ─────────────────────────────────────────────────────────

/// One symbol of the file, and what it resolves to in this portfolio.
#[derive(Debug, Serialize)]
pub struct SymbolInfo {
    /// The symbol exactly as the file writes it.
    pub source: String,
    pub rows: usize,
    /// matched (an asset of the portfolio) | chosen | new | skip | unresolved.
    pub state: &'static str,
    pub asset_id: Option<Uuid>,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub asset_class: Option<String>,
    pub provider: Option<String>,
    pub provider_id: Option<String>,
    /// Currency the operations of this symbol will be entered in.
    pub currency: Option<String>,
    /// The currency the file quotes this symbol in, when it says. An asset created from
    /// here is created in it — the file is the only source there is for that.
    pub file_currency: Option<String>,
    /// The file quotes this symbol in a currency the asset is not kept in.
    pub currency_clash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PreviewOp {
    pub index: usize,
    pub source_row: usize,
    pub symbol: String,
    /// The asset this operation lands on, once resolved.
    pub target: Option<String>,
    pub side: &'static str,
    pub op_date: String,
    pub quantity: f64,
    pub price: f64,
    pub fee: f64,
    /// What the operation moves: quantity × price, fee included.
    pub amount: f64,
    pub currency: String,
    pub note: String,
    /// Already in this portfolio from an earlier import of the same row.
    pub duplicate: bool,
    /// Its symbol is set to be skipped.
    pub skipped: bool,
    /// Its symbol has not been resolved to an asset yet.
    pub unresolved: bool,
    pub warnings: Vec<Warning>,
}

/// Bought and sold per currency, over the operations that would be imported.
#[derive(Debug, Serialize)]
pub struct Flow {
    pub currency: String,
    pub bought: f64,
    pub sold: f64,
}

#[derive(Debug, Serialize, Default)]
pub struct Stats {
    pub rows: usize,
    pub operations: usize,
    pub buys: usize,
    pub sells: usize,
    pub duplicates: usize,
    pub errors: usize,
    pub warnings: usize,
    pub skipped: usize,
    /// Operations whose symbol is still undecided — the commit blocker.
    pub unresolved: usize,
    pub symbols: usize,
    /// Assets the commit would create.
    pub new_assets: usize,
    pub flows: Vec<Flow>,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
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
    pub symbols: Vec<SymbolInfo>,
    pub preview: Vec<PreviewOp>,
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
    pub skipped: usize,
    pub failed: usize,
    pub assets_created: usize,
    pub mapping_id: Option<Uuid>,
}

// ── Symbol resolution ────────────────────────────────────────────────────────

/// What one symbol of the file resolves to, before anything is written.
#[derive(Clone)]
struct Resolution {
    state: &'static str,
    asset_id: Option<Uuid>,
    symbol: Option<String>,
    name: Option<String>,
    asset_class: Option<String>,
    provider: Option<String>,
    provider_id: Option<String>,
    currency: Option<String>,
}

impl Resolution {
    fn unresolved() -> Self {
        Self {
            state: "unresolved",
            asset_id: None,
            symbol: None,
            name: None,
            asset_class: None,
            provider: None,
            provider_id: None,
            currency: None,
        }
    }
    fn writes(&self) -> bool {
        matches!(self.state, "matched" | "chosen" | "new")
    }
}

/// Resolve a symbol against the user's choice first, then against the assets the
/// portfolio already holds. An asset that already carries the symbol (or the provider
/// ticker the price is fetched under) is the obvious answer and is taken without asking;
/// anything else is left to the user.
fn resolve(symbol: &str, mapping: &Mapping, assets: &[store::Asset]) -> Resolution {
    let key = sym_key(symbol);
    let of_asset = |a: &store::Asset, state: &'static str| Resolution {
        state,
        asset_id: Some(a.id),
        symbol: Some(a.symbol.clone()),
        name: Some(a.name.clone()),
        asset_class: Some(a.asset_class.clone()),
        provider: Some(a.provider.clone()),
        provider_id: Some(a.provider_id.clone()),
        currency: Some(a.currency.clone()),
    };

    match mapping.symbols.get(&key) {
        Some(SymbolChoice::Skip) => Resolution { state: "skip", ..Resolution::unresolved() },
        Some(SymbolChoice::Asset { asset_id }) => match assets.iter().find(|a| a.id == *asset_id) {
            Some(a) => of_asset(a, "chosen"),
            // The asset was deleted between two analyses: fall back to asking again.
            None => Resolution::unresolved(),
        },
        Some(SymbolChoice::New { asset_class, provider, provider_id, symbol: sym, name, currency }) => {
            Resolution {
                state: "new",
                asset_id: None,
                symbol: Some(sym.clone()),
                name: Some(name.clone()),
                asset_class: Some(asset_class.clone()),
                provider: Some(provider.clone()),
                provider_id: Some(provider_id.clone()),
                currency: currency.clone(),
            }
        }
        None => match assets
            .iter()
            .find(|a| sym_key(&a.symbol) == key || sym_key(&a.spot_symbol) == key)
        {
            Some(a) => of_asset(a, "matched"),
            None => Resolution::unresolved(),
        },
    }
}

// ── Pipeline ─────────────────────────────────────────────────────────────────

/// Read the file, decide (or accept) a mapping, and build every operation it would
/// import. Writes nothing. `preview_limit` caps how many operations come back to the UI;
/// the statistics always cover the whole file.
pub async fn analyze(
    pool: &PgPool,
    portfolio_id: Uuid,
    filename: &str,
    bytes: &[u8],
    supplied: Option<Mapping>,
    section: Option<String>,
    preview_limit: usize,
) -> anyhow::Result<Analysis> {
    let pf = store::get_portfolio(pool, portfolio_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("portfolio not found"))?;
    let assets = store::list_assets(pool, portfolio_id).await?;

    let detected = supplied.is_none();
    let mut base = supplied.unwrap_or_default();
    // Pointing a fresh detection at another table of a statement: its columns are
    // different ones entirely, so this is a re-detection, not an edit of the mapping.
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
    let profiles: Vec<Profile> = (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();

    let (mapping, confidence) = if detected {
        let aliases = otw_store::imports::load_aliases(pool, ALIAS_SCOPE).await.unwrap_or_default();
        let s = detect::suggest(&grid, &profiles, &aliases, &dict::PORTFOLIO);
        let mut m = Mapping {
            header_row: Some(grid.header_row),
            delimiter: Some(grid.delimiter.to_string()),
            decimal: s.decimal.to_string(),
            date_order: s.date_order.as_str().to_string(),
            section: grid.section.clone(),
            ..base
        };
        m.conventions.qty_sign_is_side = s.qty_sign_is_side;
        m.columns = (0..grid.width())
            .map(|i| (i.to_string(), s.columns.get(&i).cloned().unwrap_or_else(|| "ignore".into())))
            .collect();
        (m, s.confidence)
    } else {
        let mut m = base;
        m.header_row = Some(grid.header_row);
        m.delimiter = Some(grid.delimiter.to_string());
        m.section = m.section.or_else(|| grid.section.clone());
        (m, Default::default())
    };

    let built = build::run(&grid, &mapping, &profiles);

    // Which rows this portfolio already holds (same file imported twice).
    let hashes: Vec<String> = built.ops.iter().map(|o| o.row_hash.clone()).collect();
    let known = istore::existing_hashes(pool, portfolio_id, &hashes).await.unwrap_or_default();

    // One entry per distinct symbol, in the order the file first mentions it.
    let mut symbols: Vec<SymbolInfo> = Vec::new();
    let mut resolutions: BTreeMap<String, Resolution> = BTreeMap::new();
    for op in &built.ops {
        let key = sym_key(&op.symbol);
        if let Some(existing) = symbols.iter_mut().find(|s| sym_key(&s.source) == key) {
            existing.rows += 1;
            if existing.file_currency.is_none() {
                existing.file_currency = op.currency.clone();
            }
            if existing.currency_clash.is_none() {
                existing.currency_clash = clash(op.currency.as_deref(), existing.currency.as_deref());
            }
            continue;
        }
        let r = resolutions
            .entry(key)
            .or_insert_with(|| resolve(&op.symbol, &mapping, &assets))
            .clone();
        symbols.push(SymbolInfo {
            source: op.symbol.clone(),
            rows: 1,
            state: r.state,
            asset_id: r.asset_id,
            symbol: r.symbol.clone(),
            name: r.name.clone(),
            asset_class: r.asset_class.clone(),
            provider: r.provider.clone(),
            provider_id: r.provider_id.clone(),
            currency: r.currency.clone(),
            file_currency: op.currency.clone(),
            currency_clash: clash(op.currency.as_deref(), r.currency.as_deref()),
        });
    }

    // Stats over the whole file, preview over the first `preview_limit` operations.
    let mut stats = Stats {
        rows: grid.rows.len(),
        operations: built.ops.len(),
        errors: built.errors.len(),
        symbols: symbols.len(),
        new_assets: symbols.iter().filter(|s| s.state == "new").count(),
        ..Default::default()
    };
    let mut flows: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    let mut preview = Vec::new();
    for (i, op) in built.ops.iter().enumerate() {
        let r = resolutions.get(&sym_key(&op.symbol)).cloned().unwrap_or_else(Resolution::unresolved);
        let duplicate = known.contains(&op.row_hash);
        let skipped = r.state == "skip";
        let unresolved = r.state == "unresolved";
        let currency = r
            .currency
            .clone()
            .or_else(|| op.currency.clone())
            .unwrap_or_else(|| pf.currency.clone());
        let amount = op.quantity * op.price + if op.side == "buy" { op.fee } else { -op.fee };

        if duplicate {
            stats.duplicates += 1;
        }
        if skipped {
            stats.skipped += 1;
        }
        if unresolved {
            stats.unresolved += 1;
        }
        if op.side == "buy" {
            stats.buys += 1;
        } else {
            stats.sells += 1;
        }
        stats.warnings += op.warnings.len();
        if !duplicate && !skipped && !unresolved {
            let e = flows.entry(currency.clone()).or_insert((0.0, 0.0));
            if op.side == "buy" {
                e.0 += amount;
            } else {
                e.1 += amount;
            }
        }
        let iso = op.op_date.to_string();
        if stats.first_date.as_ref().is_none_or(|f| iso < *f) {
            stats.first_date = Some(iso.clone());
        }
        if stats.last_date.as_ref().is_none_or(|l| iso > *l) {
            stats.last_date = Some(iso.clone());
        }

        if i < preview_limit {
            preview.push(PreviewOp {
                index: i,
                source_row: op.source_row,
                symbol: op.symbol.clone(),
                target: r.symbol.clone(),
                side: op.side,
                op_date: iso,
                quantity: op.quantity,
                price: op.price,
                fee: op.fee,
                amount,
                currency,
                note: op.note.clone(),
                duplicate,
                skipped,
                unresolved,
                warnings: op.warnings.clone(),
            });
        }
    }
    stats.flows = flows
        .into_iter()
        .map(|(currency, (bought, sold))| Flow {
            currency,
            bought: round2(bought),
            sold: round2(sold),
        })
        .collect();

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
        columns: detect::describe(&grid, &profiles, &mapping.columns, &confidence),
        mapping,
        symbols,
        preview,
        errors: built.errors,
        stats,
        mapping_match,
        detected,
    })
}

/// Import for real. Every operation lands in one batch, so the whole import can be
/// undone; the assets the file needed are created first, and only those the user picked.
pub async fn commit(
    pool: &PgPool,
    portfolio_id: Uuid,
    filename: &str,
    bytes: &[u8],
    mapping: Mapping,
    save_as: Option<String>,
) -> anyhow::Result<CommitReport> {
    store::get_portfolio(pool, portfolio_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("portfolio not found"))?;
    let assets = store::list_assets(pool, portfolio_id).await?;

    let grid = parse::read_grid(
        filename,
        bytes,
        mapping.header_row,
        mapping.delimiter_char(),
        mapping.section.as_deref(),
    )?;
    let profiles: Vec<Profile> = (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
    let built = build::run(&grid, &mapping, &profiles);

    // Resolve every symbol before writing anything: an import that would land half its
    // rows on assets and leave the rest hanging is worse than one that does not start.
    let mut resolutions: BTreeMap<String, Resolution> = BTreeMap::new();
    for op in &built.ops {
        resolutions
            .entry(sym_key(&op.symbol))
            .or_insert_with(|| resolve(&op.symbol, &mapping, &assets));
    }
    let pending: Vec<String> = resolutions
        .iter()
        .filter(|(_, r)| r.state == "unresolved")
        .map(|(k, _)| k.clone())
        .collect();
    if !pending.is_empty() {
        anyhow::bail!(
            "{} symbol(s) are not tied to an asset yet: {}",
            pending.len(),
            pending.join(", ")
        );
    }

    // Create what the user picked from the symbol search, and price it right away so the
    // rows it will hold don't show a position with no value until the next refresh.
    let mut assets_created = 0usize;
    for r in resolutions.values_mut() {
        if r.state != "new" {
            continue;
        }
        let provider = r.provider.clone().unwrap_or_default();
        if !PROVIDERS.contains(&provider.as_str()) {
            anyhow::bail!("unknown price source \"{provider}\"");
        }
        let provider_id = r.provider_id.clone().unwrap_or_default();
        anyhow::ensure!(!provider_id.trim().is_empty(), "a new asset needs a price source id");
        let currency = r
            .currency
            .clone()
            .filter(|c| dict::CURRENCIES.contains(&c.as_str()))
            .unwrap_or_else(|| "USD".to_string());
        let asset = store::add_asset(
            pool,
            portfolio_id,
            r.asset_class.as_deref().unwrap_or("stock"),
            &provider,
            provider_id.trim(),
            r.symbol.clone().unwrap_or_default().trim(),
            r.name.clone().unwrap_or_default().trim(),
            &currency,
        )
        .await?;
        if let Err(e) = prices::price_new_asset(pool, &asset).await {
            tracing::warn!("initial price for {} failed: {e:#}", asset.symbol);
        }
        r.asset_id = Some(asset.id);
        r.currency = Some(asset.currency.clone());
        assets_created += 1;
    }

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
        let saved = istore::upsert_mapping(pool, &name, &fp, &headers, &doc).await?;
        mapping_name = saved.name.clone();
        mapping_id = Some(saved.id);
    }

    let batch = istore::add_batch(
        pool,
        portfolio_id,
        mapping_id,
        &mapping_name,
        filename,
        grid.rows.len() as i32,
    )
    .await?;

    let (mut imported, mut duplicates, mut skipped) = (0usize, 0usize, 0usize);
    let mut insert_error = None;
    for op in &built.ops {
        let Some(r) = resolutions.get(&sym_key(&op.symbol)) else { continue };
        if !r.writes() {
            skipped += 1;
            continue;
        }
        let Some(asset_id) = r.asset_id else {
            skipped += 1;
            continue;
        };
        match istore::add_imported_operation(
            pool,
            portfolio_id,
            asset_id,
            op.side,
            op.op_date,
            op.quantity,
            op.price,
            op.fee,
            &op.note,
            batch,
            &op.row_hash,
        )
        .await
        {
            Ok(true) => imported += 1,
            Ok(false) => duplicates += 1,
            // Stop, but keep the batch: whatever landed is still tagged, so the user can
            // revert the half-import in one click instead of hunting for its operations.
            Err(e) => {
                insert_error = Some(e);
                break;
            }
        }
    }
    let failed = built.errors.len();
    istore::finish_batch(
        pool,
        batch,
        imported as i32,
        duplicates as i32,
        failed as i32,
        assets_created as i32,
    )
    .await?;
    if let Some(e) = insert_error {
        return Err(e.context(format!(
            "import stopped after {imported} operations — revert this import to undo them"
        )));
    }
    if let Some(id) = mapping_id {
        istore::touch_mapping(pool, id).await.ok();
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

    Ok(CommitReport {
        batch_id: batch,
        imported,
        duplicates,
        skipped,
        failed,
        assets_created,
        mapping_id,
    })
}

/// The saved mapping that best covers this file's headers, if any is close enough.
async fn match_saved_mapping(pool: &PgPool, headers: &[String]) -> Option<MappingMatch> {
    let saved = istore::mapping_headers(pool).await.ok()?;
    let mut best: Option<MappingMatch> = None;
    for (id, name, _fp, saved_headers) in saved {
        let score = detect::similarity(headers, &saved_headers);
        if score < 0.7 {
            continue;
        }
        let m = MappingMatch { id, name, score: round_dp(score, 3), exact: score >= 0.999 };
        if best.as_ref().is_none_or(|b| m.score > b.score) {
            best = Some(m);
        }
    }
    best
}

/// The currency the file quotes against the one the asset is kept in, when they differ.
/// An operation is stored in the asset's currency, so this is worth saying out loud.
fn clash(file: Option<&str>, asset: Option<&str>) -> Option<String> {
    match (file, asset) {
        (Some(f), Some(a)) if !f.eq_ignore_ascii_case(a) => Some(f.to_string()),
        _ => None,
    }
}

fn round2(v: f64) -> f64 {
    round_dp(v, 2)
}

fn round_dp(v: f64, dp: i32) -> f64 {
    let f = 10f64.powi(dp);
    (v * f).round() / f
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Detect + build the way `analyze` does, without a database.
    fn run(file: &str) -> (Mapping, build::BuildResult) {
        let grid = parse::read_grid("ledger.csv", file.as_bytes(), None, None, None).expect("grid");
        let profiles: Vec<Profile> =
            (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
        let s = detect::suggest(&grid, &profiles, &Default::default(), &dict::PORTFOLIO);
        let mut mapping = Mapping {
            decimal: s.decimal.to_string(),
            date_order: s.date_order.as_str().to_string(),
            columns: (0..grid.width())
                .map(|i| (i.to_string(), s.columns.get(&i).cloned().unwrap_or("ignore".into())))
                .collect(),
            ..Default::default()
        };
        mapping.conventions.qty_sign_is_side = s.qty_sign_is_side;
        let built = build::run(&grid, &mapping, &profiles);
        (mapping, built)
    }

    fn target_of(m: &Mapping, idx: usize) -> &str {
        m.columns.get(&idx.to_string()).map(|s| s.as_str()).unwrap_or("")
    }

    #[test]
    fn a_broker_ledger_maps_and_builds() {
        let file = concat!(
            "Date,Symbol,Type,Quantity,Price,Fees\n",
            "2026-03-04,VT,Buy,10,118.42,1.20\n",
            "2026-04-11,VT,Sell,4,124.05,1.20\n",
        );
        let (m, built) = run(file);
        assert_eq!(target_of(&m, 0), "op_date");
        assert_eq!(target_of(&m, 1), "ticker");
        assert_eq!(target_of(&m, 2), "side");
        assert_eq!(target_of(&m, 3), "quantity");
        assert_eq!(target_of(&m, 4), "price");
        assert_eq!(target_of(&m, 5), "fee");
        assert!(built.errors.is_empty());
        assert_eq!(built.ops.len(), 2);
        assert_eq!(built.ops[0].side, "buy");
        assert_eq!(built.ops[0].quantity, 10.0);
        assert_eq!(built.ops[1].side, "sell");
        assert_eq!(built.ops[1].op_date.to_string(), "2026-04-11");
    }

    /// The exports that name no direction sign their quantity instead.
    #[test]
    fn a_signed_quantity_carries_the_direction() {
        let file = concat!(
            "Trade Date;Ticker;Quantity;Price;Commission\n",
            "04/03/2026;MC.PA;12;712,40;2,50\n",
            "11/04/2026;MC.PA;-5;735,10;2,50\n",
        );
        let (m, built) = run(file);
        assert!(m.conventions.qty_sign_is_side);
        assert_eq!(built.ops.len(), 2);
        assert_eq!(built.ops[0].side, "buy");
        assert_eq!(built.ops[1].side, "sell");
        assert_eq!(built.ops[1].quantity, 5.0);
        // Decimal comma and day-first dates, decided per column.
        assert_eq!(built.ops[0].price, 712.40);
        assert_eq!(built.ops[0].op_date.to_string(), "2026-03-04");
    }

    /// A ledger that states the cash and leaves the unit price implicit.
    #[test]
    fn an_amount_column_gives_the_unit_price() {
        let file = concat!(
            "Date,Symbol,Side,Shares,Amount\n",
            "2026-02-02,AAPL,Bought,4,800.00\n",
        );
        let (_, built) = run(file);
        assert_eq!(built.ops.len(), 1);
        assert_eq!(built.ops[0].price, 200.0);
        assert_eq!(built.ops[0].warnings.len(), 1);
        assert_eq!(built.ops[0].warnings[0].code, "price");
    }

    /// A dividend is not an operation: it is reported, never folded into a buy.
    #[test]
    fn rows_that_are_not_a_buy_or_a_sell_are_reported() {
        let file = concat!(
            "Date,Symbol,Type,Quantity,Price\n",
            "2026-03-04,VT,Buy,10,118.42\n",
            "2026-03-31,VT,Dividend,0,0\n",
        );
        let (_, built) = run(file);
        assert_eq!(built.ops.len(), 1);
        assert_eq!(built.errors.len(), 1);
        assert_eq!(built.errors[0].row, 3);
    }

    /// Re-importing the same file must add nothing, while a file holding the same row
    /// twice keeps both.
    #[test]
    fn the_same_row_twice_hashes_differently_than_the_same_file_twice() {
        let line = "2026-03-04,VT,Buy,10,118.42\n";
        let header = "Date,Symbol,Type,Quantity,Price\n";
        let (_, twice) = run(&format!("{header}{line}{line}"));
        assert_eq!(twice.ops.len(), 2);
        assert_ne!(twice.ops[0].row_hash, twice.ops[1].row_hash);

        let (_, once) = run(&format!("{header}{line}"));
        assert_eq!(once.ops[0].row_hash, twice.ops[0].row_hash);
    }

    /// A symbol the portfolio already holds resolves without asking; anything else waits
    /// for the user, and never invents an asset.
    #[test]
    fn known_symbols_resolve_themselves_and_unknown_ones_wait() {
        let asset = store::Asset {
            id: Uuid::new_v4(),
            portfolio_id: Uuid::new_v4(),
            asset_class: "etf".into(),
            provider: "yahoo".into(),
            provider_id: "VT".into(),
            symbol: "VT".into(),
            name: "Vanguard Total World".into(),
            currency: "USD".into(),
            last_price_usd: None,
            last_price_at: None,
            spot_provider: None,
            spot_symbol: String::new(),
            recon_status: "ok".into(),
            recon_checked_at: None,
            recon_note: String::new(),
        };
        let mut mapping = Mapping::default();
        assert_eq!(resolve("vt", &mapping, &[asset.clone()]).state, "matched");
        assert_eq!(resolve("AAPL", &mapping, &[asset.clone()]).state, "unresolved");

        mapping.symbols.insert("AAPL".into(), SymbolChoice::Skip);
        assert_eq!(resolve("aapl", &mapping, &[asset]).state, "skip");
    }
}
