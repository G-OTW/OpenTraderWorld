//! Rows + mapping → trades.
//!
//! Two shapes, because trade books come in two kinds:
//!   - **roundtrip** — one row is one finished (or still open) trade. Straight mapping.
//!   - **executions** — one row is one fill. Fills are grouped per instrument in time
//!     order and folded into positions: same-direction fills add entry legs, opposite
//!     ones add exit legs, and the trade closes when the net quantity returns to zero.
//!     An over-closing fill closes the position and opens a new one the other way.
//!
//! Nothing here writes: it produces `TradeInput`s the caller previews or commits, each
//! carrying the source rows it came from and a hash that makes re-importing a no-op.

use std::collections::HashMap;

use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::journal::{Leg, TradeInput};

use crate::import::rowhash::Hasher;

use super::detect::Profile;
use super::dict;
use super::parse::{self, DateOrder, Grid};
use super::Mapping;

/// Names the import resolves against the journal's own objects.
pub struct BuildCtx {
    /// (id, name, is_default)
    pub categories: Vec<(Uuid, String, bool)>,
    pub strategies: Vec<(Uuid, String)>,
    pub default_category: Uuid,
}

#[derive(Debug, Serialize, Clone)]
pub struct Warning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct RowError {
    /// 1-based line number in the source file (header row included).
    pub row: usize,
    pub message: String,
    pub raw: String,
}

pub struct BuiltTrade {
    pub input: TradeInput,
    pub row_hash: String,
    pub source_rows: Vec<usize>,
    pub warnings: Vec<Warning>,
    /// PnL as written in the file, when a PnL column was mapped — cross-checked against
    /// what the journal computes, which is the fastest way to spot a wrong mapping.
    pub pnl_source: Option<f64>,
}

pub struct BuildResult {
    pub trades: Vec<BuiltTrade>,
    pub errors: Vec<RowError>,
}

// ── Row reader ───────────────────────────────────────────────────────────────

/// Reads one grid row through the mapping: which column holds which field, and how the
/// values of that column are to be parsed.
struct Reader<'a> {
    grid: &'a Grid,
    mapping: &'a Mapping,
    profiles: &'a [Profile],
    /// field id → column index
    fields: HashMap<String, usize>,
    /// custom fields: label → column index
    custom: Vec<(String, usize)>,
    side_map: Vec<(String, String)>,
    /// Point value per ticker, keyed on the normalized ticker.
    multipliers: Vec<(String, f64)>,
}

impl<'a> Reader<'a> {
    fn new(grid: &'a Grid, mapping: &'a Mapping, profiles: &'a [Profile]) -> Self {
        let mut fields = HashMap::new();
        let mut custom = Vec::new();
        for (key, target) in &mapping.columns {
            let Ok(idx) = key.parse::<usize>() else { continue };
            if idx >= grid.width() || target == "ignore" || target.is_empty() {
                continue;
            }
            match target.strip_prefix("field:") {
                Some(label) => {
                    let label = if label.is_empty() {
                        grid.headers.get(idx).cloned().unwrap_or_default()
                    } else {
                        label.to_string()
                    };
                    custom.push((label, idx));
                }
                None => {
                    fields.insert(target.clone(), idx);
                }
            }
        }
        custom.sort_by_key(|(_, idx)| *idx);
        let multipliers = mapping
            .multipliers
            .iter()
            .map(|(t, m)| (parse::normalize(t), *m))
            .filter(|(t, m)| !t.is_empty() && *m > 0.0)
            .collect();
        Self { grid, mapping, profiles, fields, custom, side_map: mapping.side_map(), multipliers }
    }

    fn raw(&self, row: &'a [String], field: &str) -> Option<&'a str> {
        let idx = *self.fields.get(field)?;
        let v = self.grid.cell(row, idx);
        (!v.is_empty()).then_some(v)
    }

    fn text(&self, row: &[String], field: &str) -> Option<String> {
        self.raw(row, field).map(|v| v.to_string())
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

    fn date(&self, row: &[String], field: &str) -> Option<OffsetDateTime> {
        let idx = *self.fields.get(field)?;
        let raw = self.raw(row, field)?;
        let order = match DateOrder::from_str(&self.mapping.date_order) {
            DateOrder::Auto => self.profiles[idx].date_order.unwrap_or(DateOrder::Auto),
            explicit => explicit,
        };
        parse::parse_datetime(raw, order, self.mapping.tz_offset)
    }

    fn side(&self, row: &[String], warnings: &mut Vec<Warning>) -> String {
        match self.raw(row, "side") {
            Some(raw) => match parse::side_of(raw, &self.side_map) {
                Some(s) => s.to_string(),
                None => {
                    warnings.push(Warning {
                        code: "side".into(),
                        message: format!("unrecognized side \"{raw}\" — using {}", self.mapping.defaults.side),
                    });
                    self.mapping.defaults.side.clone()
                }
            },
            None => self.mapping.defaults.side.clone(),
        }
    }

    fn currency(&self, row: &[String], warnings: &mut Vec<Warning>) -> String {
        let Some(raw) = self.raw(row, "currency") else {
            return self.mapping.defaults.currency.clone();
        };
        let code = raw.trim().to_uppercase();
        if dict::CURRENCIES.contains(&code.as_str()) {
            return code;
        }
        warnings.push(Warning {
            code: "currency".into(),
            message: format!("unsupported currency \"{raw}\" — using {}", self.mapping.defaults.currency),
        });
        self.mapping.defaults.currency.clone()
    }

    fn asset_class(&self, row: &[String]) -> String {
        self.raw(row, "asset_class")
            .and_then(|v| parse::lookup_enum(v, dict::ASSET_MAP))
            .map(str::to_string)
            .unwrap_or_else(|| self.mapping.defaults.asset_class.clone())
    }

    fn unit_type(&self, row: &[String]) -> String {
        self.raw(row, "unit_type")
            .and_then(|v| parse::lookup_enum(v, dict::UNIT_MAP))
            .map(str::to_string)
            .unwrap_or_else(|| self.mapping.defaults.unit_type.clone())
    }

    /// What one point of this instrument is worth. The value the user set for that
    /// ticker wins — they answered a question the file does not: a contract's point value
    /// is not written anywhere in an export, and it differs per contract (MNQ 2, MES 5).
    /// Failing that, a mapped multiplier column, then the mapping's default.
    fn multiplier(&self, ticker: &str, row: &[String]) -> f64 {
        let key = parse::normalize(ticker);
        if let Some((_, m)) = self.multipliers.iter().find(|(t, _)| *t == key) {
            return *m;
        }
        self.num(row, "multiplier").filter(|m| *m > 0.0).unwrap_or(self.mapping.defaults.multiplier)
    }

    fn fields_json(&self, row: &[String]) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (label, idx) in &self.custom {
            let v = self.grid.cell(row, *idx);
            if !v.is_empty() {
                map.insert(label.clone(), serde_json::Value::String(v.to_string()));
            }
        }
        serde_json::Value::Object(map)
    }

    /// Resolve a category/strategy name written in the file against the journal's own.
    fn category(&self, row: &[String], ctx: &BuildCtx, warnings: &mut Vec<Warning>) -> Uuid {
        let fallback = self.mapping.defaults.category_id.unwrap_or(ctx.default_category);
        let Some(name) = self.raw(row, "category") else { return fallback };
        let n = parse::normalize(name);
        match ctx.categories.iter().find(|(_, cn, _)| parse::normalize(cn) == n) {
            Some((id, _, _)) => *id,
            None => {
                warnings.push(Warning {
                    code: "category".into(),
                    message: format!("no category named \"{name}\" — filed in the selected one"),
                });
                fallback
            }
        }
    }

    fn strategy(&self, row: &[String], ctx: &BuildCtx, warnings: &mut Vec<Warning>) -> Option<Uuid> {
        let Some(name) = self.raw(row, "strategy") else {
            return self.mapping.defaults.strategy_id;
        };
        let n = parse::normalize(name);
        match ctx.strategies.iter().find(|(_, sn)| parse::normalize(sn) == n) {
            Some((id, _)) => Some(*id),
            None => {
                warnings.push(Warning {
                    code: "strategy".into(),
                    message: format!("no strategy named \"{name}\" — left unset"),
                });
                self.mapping.defaults.strategy_id
            }
        }
    }
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub fn run(grid: &Grid, mapping: &Mapping, profiles: &[Profile], ctx: &BuildCtx) -> BuildResult {
    let reader = Reader::new(grid, mapping, profiles);
    if mapping.shape == "executions" {
        build_executions(grid, &reader, mapping, ctx)
    } else {
        build_roundtrip(grid, &reader, mapping, ctx)
    }
}

/// Source line number shown to the user (1-based, blank rows accounted for).
fn line_no(grid: &Grid, row_idx: usize) -> usize {
    grid.row_lines.get(row_idx).copied().unwrap_or(row_idx + 1)
}

fn build_roundtrip(grid: &Grid, r: &Reader, mapping: &Mapping, ctx: &BuildCtx) -> BuildResult {
    let mut trades = Vec::new();
    let mut errors = Vec::new();
    let mut hasher = Hasher::new();

    for (i, row) in grid.rows.iter().enumerate() {
        let mut warnings: Vec<Warning> = Vec::new();
        let line = line_no(grid, i);

        let qty_raw = r.num(row, "quantity");
        let mut side = r.side(row, &mut warnings);
        if mapping.conventions.qty_sign_is_side {
            if let Some(q) = qty_raw {
                if q < 0.0 {
                    side = "short".into();
                }
            }
        }
        let qty = qty_raw.map(f64::abs).filter(|q| *q > 0.0);
        let (mut entry_price, mut exit_price) = (r.num(row, "entry_price"), r.num(row, "exit_price"));
        let (mut entry_at, mut exit_at) = (r.date(row, "entry_at"), r.date(row, "exit_at"));
        if mapping.conventions.swap_entry_exit {
            std::mem::swap(&mut entry_price, &mut exit_price);
            std::mem::swap(&mut entry_at, &mut exit_at);
        }

        let Some(quantity) = qty else {
            errors.push(RowError {
                row: line,
                message: "no quantity — map a quantity column or drop this row".into(),
                raw: row.join(" · "),
            });
            continue;
        };
        let Some(entry) = entry_price else {
            errors.push(RowError {
                row: line,
                message: "no entry price — map an entry price column or drop this row".into(),
                raw: row.join(" · "),
            });
            continue;
        };
        if entry_at.is_none() && exit_at.is_none() {
            warnings.push(Warning {
                code: "date".into(),
                message: "no date could be read — the trade lands on its creation date".into(),
            });
        }

        let fees = r.num(row, "fees").map(|f| if mapping.conventions.fees_abs { f.abs() } else { f });
        let ticker = r.text(row, "ticker").unwrap_or_default();
        if ticker.trim().is_empty() {
            warnings.push(Warning { code: "ticker".into(), message: "no ticker".into() });
        }
        let ticker_for_mult = ticker.clone();

        let input = TradeInput {
            category_id: r.category(row, ctx, &mut warnings),
            template_id: mapping.defaults.template_id,
            strategy_id: r.strategy(row, ctx, &mut warnings),
            ticker,
            asset_class: r.asset_class(row),
            exchange: r.text(row, "exchange"),
            side,
            currency: r.currency(row, &mut warnings),
            unit_type: r.unit_type(row),
            fee_schedule_id: None,
            entry_at,
            exit_at,
            entry_price: Some(entry),
            exit_price,
            quantity: Some(quantity),
            fees: fees.unwrap_or(0.0),
            leverage: r.num(row, "leverage").unwrap_or(mapping.defaults.leverage),
            multiplier: r.multiplier(&ticker_for_mult, row),
            signal_name: r.text(row, "signal_name"),
            feedback: r.text(row, "feedback"),
            images: serde_json::Value::Array(vec![]),
            fields: r.fields_json(row),
            advanced: false,
            cost_basis_method: "avg".into(),
            entries: vec![],
            exits: vec![],
            brackets: vec![],
        };

        trades.push(BuiltTrade {
            row_hash: hasher.row(row, r.raw(row, "external_id")),
            input,
            source_rows: vec![line],
            warnings,
            pnl_source: r.num(row, "pnl_check"),
        });
    }

    BuildResult { trades, errors }
}

// ── Executions → positions ───────────────────────────────────────────────────

struct Fill {
    line: usize,
    at: Option<OffsetDateTime>,
    ticker: String,
    side: String, // "long" = buy, "short" = sell
    price: f64,
    qty: f64,
    fees: f64,
    hash_key: String,
    warnings: Vec<Warning>,
    // Position-level attributes, taken from the fill that opened it.
    currency: String,
    asset_class: String,
    unit_type: String,
    exchange: Option<String>,
    category_id: Uuid,
    strategy_id: Option<Uuid>,
    signal_name: Option<String>,
    feedback: Option<String>,
    fields: serde_json::Value,
    signal: Option<String>,
    multiplier: f64,
}

fn build_executions(grid: &Grid, r: &Reader, mapping: &Mapping, ctx: &BuildCtx) -> BuildResult {
    let mut errors = Vec::new();
    let mut fills: Vec<Fill> = Vec::new();

    for (i, row) in grid.rows.iter().enumerate() {
        let line = line_no(grid, i);
        let mut warnings: Vec<Warning> = Vec::new();
        let qty_raw = r.num(row, "quantity");
        let mut side = r.side(row, &mut warnings);
        if mapping.conventions.qty_sign_is_side {
            if let Some(q) = qty_raw {
                if q < 0.0 {
                    side = "short".into();
                }
            }
        }
        let (Some(qty), Some(price)) = (qty_raw.map(f64::abs).filter(|q| *q > 0.0), r.num(row, "entry_price"))
        else {
            errors.push(RowError {
                row: line,
                message: "a fill needs a price and a quantity".into(),
                raw: row.join(" · "),
            });
            continue;
        };
        let at = r.date(row, "entry_at").or_else(|| r.date(row, "exit_at"));
        if at.is_none() {
            warnings.push(Warning {
                code: "date".into(),
                message: "no date — fills without a date are grouped in file order".into(),
            });
        }
        let external = r.raw(row, "external_id").map(|s| format!("id:{s}"));
        fills.push(Fill {
            line,
            at,
            ticker: r.text(row, "ticker").unwrap_or_default(),
            side,
            price,
            qty,
            fees: r.num(row, "fees").map(|f| if mapping.conventions.fees_abs { f.abs() } else { f }).unwrap_or(0.0),
            hash_key: external.unwrap_or_else(|| row.join("\u{1}")),
            warnings,
            currency: r.currency(row, &mut Vec::new()),
            asset_class: r.asset_class(row),
            unit_type: r.unit_type(row),
            exchange: r.text(row, "exchange"),
            category_id: r.category(row, ctx, &mut Vec::new()),
            strategy_id: r.strategy(row, ctx, &mut Vec::new()),
            signal_name: r.text(row, "signal_name"),
            feedback: r.text(row, "feedback"),
            fields: r.fields_json(row),
            signal: r.text(row, "signal_name"),
            multiplier: r.multiplier(&r.text(row, "ticker").unwrap_or_default(), row),
        });
    }

    // Group per instrument, chronologically. Fills without a date keep file order.
    fills.sort_by(|a, b| {
        a.ticker
            .to_lowercase()
            .cmp(&b.ticker.to_lowercase())
            .then(a.at.cmp(&b.at))
            .then(a.line.cmp(&b.line))
    });

    let mut hasher = Hasher::new();
    let mut trades = Vec::new();
    let mut open: Option<Position> = None;
    let mut current_ticker: Option<String> = None;

    for fill in fills {
        let same_instrument = current_ticker.as_deref() == Some(fill.ticker.as_str());
        if !same_instrument {
            if let Some(pos) = open.take() {
                trades.push(pos.finish(&mut hasher, mapping));
            }
            current_ticker = Some(fill.ticker.clone());
        }

        let mut remaining = fill.qty;
        while remaining > 1e-9 {
            match open.as_mut() {
                None => {
                    open = Some(Position::open(&fill, remaining));
                    remaining = 0.0;
                }
                Some(pos) if pos.dir == fill.side => {
                    pos.add_entry(&fill, remaining);
                    remaining = 0.0;
                }
                Some(pos) => {
                    let closed = remaining.min(pos.open_qty);
                    pos.add_exit(&fill, closed, closed / fill.qty);
                    remaining -= closed;
                    if pos.open_qty <= 1e-9 {
                        let done = open.take().expect("position present");
                        trades.push(done.finish(&mut hasher, mapping));
                    }
                }
            }
        }
    }
    if let Some(pos) = open.take() {
        trades.push(pos.finish(&mut hasher, mapping));
    }

    BuildResult { trades, errors }
}

/// A position being assembled from fills.
struct Position {
    dir: String,
    open_qty: f64,
    entries: Vec<Leg>,
    exits: Vec<Leg>,
    lines: Vec<usize>,
    hash_keys: Vec<String>,
    warnings: Vec<Warning>,
    meta: TradeMeta,
}

/// Attributes a position inherits from the fill that opened it.
struct TradeMeta {
    ticker: String,
    currency: String,
    asset_class: String,
    unit_type: String,
    exchange: Option<String>,
    category_id: Uuid,
    strategy_id: Option<Uuid>,
    signal_name: Option<String>,
    feedback: Option<String>,
    fields: serde_json::Value,
    multiplier: f64,
}

impl Position {
    fn open(fill: &Fill, qty: f64) -> Self {
        let mut pos = Self {
            dir: fill.side.clone(),
            open_qty: 0.0,
            entries: vec![],
            exits: vec![],
            lines: vec![],
            hash_keys: vec![],
            warnings: vec![],
            meta: TradeMeta {
                ticker: fill.ticker.clone(),
                currency: fill.currency.clone(),
                asset_class: fill.asset_class.clone(),
                unit_type: fill.unit_type.clone(),
                exchange: fill.exchange.clone(),
                category_id: fill.category_id,
                strategy_id: fill.strategy_id,
                signal_name: fill.signal_name.clone(),
                feedback: fill.feedback.clone(),
                fields: fill.fields.clone(),
                multiplier: fill.multiplier,
            },
        };
        pos.add_entry(fill, qty);
        pos
    }

    fn leg(fill: &Fill, qty: f64, fee_share: f64) -> Leg {
        Leg {
            id: format!("row{}", fill.line),
            price: fill.price,
            qty,
            at: fill.at,
            fees: fill.fees * fee_share,
            signal: fill.signal.clone(),
        }
    }

    fn add_entry(&mut self, fill: &Fill, qty: f64) {
        let share = qty / fill.qty;
        self.entries.push(Self::leg(fill, qty, share));
        self.open_qty += qty;
        self.track(fill);
    }

    fn add_exit(&mut self, fill: &Fill, qty: f64, fee_share: f64) {
        self.exits.push(Self::leg(fill, qty, fee_share));
        self.open_qty -= qty;
        self.track(fill);
    }

    fn track(&mut self, fill: &Fill) {
        if !self.lines.contains(&fill.line) {
            self.lines.push(fill.line);
            self.hash_keys.push(fill.hash_key.clone());
            self.warnings.extend(fill.warnings.iter().cloned());
        }
    }

    fn finish(mut self, hasher: &mut Hasher, mapping: &Mapping) -> BuiltTrade {
        let entry_at = self.entries.iter().filter_map(|l| l.at).min();
        let exit_at = self.exits.iter().filter_map(|l| l.at).max();
        if self.open_qty > 1e-9 {
            self.warnings.push(Warning {
                code: "open".into(),
                message: format!("{} left open at the end of the file", crate::journal_import::fmt_qty(self.open_qty)),
            });
        }
        self.lines.sort_unstable();
        let input = TradeInput {
            category_id: self.meta.category_id,
            template_id: mapping.defaults.template_id,
            strategy_id: self.meta.strategy_id,
            ticker: self.meta.ticker,
            asset_class: self.meta.asset_class,
            exchange: self.meta.exchange,
            side: self.dir,
            currency: self.meta.currency,
            unit_type: self.meta.unit_type,
            fee_schedule_id: None,
            entry_at,
            exit_at,
            entry_price: None,
            exit_price: None,
            quantity: None,
            fees: 0.0, // every fill's fee rides on its own leg
            leverage: mapping.defaults.leverage,
            multiplier: self.meta.multiplier,
            signal_name: self.meta.signal_name,
            feedback: self.meta.feedback,
            images: serde_json::Value::Array(vec![]),
            fields: self.meta.fields,
            advanced: true,
            cost_basis_method: "avg".into(),
            entries: self.entries,
            exits: self.exits,
            brackets: vec![],
        };
        BuiltTrade {
            row_hash: hasher.group(&self.hash_keys),
            input,
            source_rows: self.lines,
            warnings: self.warnings,
            pnl_source: None,
        }
    }
}
