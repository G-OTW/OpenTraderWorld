//! Rows + mapping → operations.
//!
//! One row is one buy or one sell. Nothing is folded, netted or grouped: a portfolio's
//! ledger *is* the list of operations, and the position derives from it later. A row the
//! mapping cannot turn into an operation becomes an error the user can see and fix — it
//! is never silently dropped, and never guessed into shape.

use serde::Serialize;
use time::{Date, OffsetDateTime};

use crate::import::detect::Profile;
use crate::import::dict;
use crate::import::parse::{self, DateOrder, Grid};
use crate::import::rowhash::Hasher;

use super::Mapping;

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

/// One operation the file would create, before it is tied to an asset.
pub struct BuiltOp {
    /// The symbol exactly as the file wrote it.
    pub symbol: String,
    pub side: &'static str,
    pub op_date: Date,
    pub quantity: f64,
    pub price: f64,
    pub fee: f64,
    pub note: String,
    /// Currency the amounts are written in, when the file says so.
    pub currency: Option<String>,
    pub row_hash: String,
    pub source_row: usize,
    pub warnings: Vec<Warning>,
}

pub struct BuildResult {
    pub ops: Vec<BuiltOp>,
    pub errors: Vec<RowError>,
}

// ── Row reader ───────────────────────────────────────────────────────────────

struct Reader<'a> {
    grid: &'a Grid,
    mapping: &'a Mapping,
    profiles: &'a [Profile],
    /// field id → column index
    fields: std::collections::HashMap<String, usize>,
    side_map: Vec<(String, String)>,
}

impl<'a> Reader<'a> {
    fn new(grid: &'a Grid, mapping: &'a Mapping, profiles: &'a [Profile]) -> Self {
        let mut fields = std::collections::HashMap::new();
        for (key, target) in &mapping.columns {
            let Ok(idx) = key.parse::<usize>() else { continue };
            if idx >= grid.width() || target == "ignore" || target.is_empty() {
                continue;
            }
            // A portfolio operation has no free-form fields: anything not reserved is
            // ignored rather than carried along.
            if target.starts_with("field:") {
                continue;
            }
            fields.insert(target.clone(), idx);
        }
        Self { grid, mapping, profiles, fields, side_map: mapping.side_map() }
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

    fn date(&self, row: &[String]) -> Option<OffsetDateTime> {
        let idx = *self.fields.get("op_date")?;
        let raw = self.raw(row, "op_date")?;
        let order = match DateOrder::from_str(&self.mapping.date_order) {
            DateOrder::Auto => self.profiles[idx].date_order.unwrap_or(DateOrder::Auto),
            explicit => explicit,
        };
        parse::parse_datetime(raw, order, self.mapping.tz_offset)
    }

    /// buy / sell, or `None` when the file said something this import cannot read as a
    /// direction — a dividend, a deposit, a transfer. Those are not operations here.
    fn side(&self, row: &[String], qty: Option<f64>) -> Option<&'static str> {
        if self.mapping.conventions.qty_sign_is_side {
            if let Some(q) = qty {
                if q != 0.0 {
                    return Some(if q < 0.0 { "sell" } else { "buy" });
                }
            }
        }
        match self.raw(row, "side") {
            Some(raw) => parse::side_of(raw, &self.side_map).map(|s| match s {
                "short" => "sell",
                _ => "buy",
            }),
            // No side column at all: the user says once what the file is a list of.
            None => match self.mapping.defaults.side.as_str() {
                "buy" => Some("buy"),
                "sell" => Some("sell"),
                _ => None,
            },
        }
    }

    fn currency(&self, row: &[String]) -> Option<String> {
        let raw = self.raw(row, "currency")?;
        let code = raw.trim().to_uppercase();
        dict::CURRENCIES.contains(&code.as_str()).then_some(code)
    }
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub fn run(grid: &Grid, mapping: &Mapping, profiles: &[Profile]) -> BuildResult {
    let r = Reader::new(grid, mapping, profiles);
    let mut hasher = Hasher::new();
    let mut ops = Vec::new();
    let mut errors = Vec::new();

    for (i, row) in grid.rows.iter().enumerate() {
        let line = grid.row_lines.get(i).copied().unwrap_or(i + 1);
        let fail = |message: &str| RowError {
            row: line,
            message: message.to_string(),
            raw: row.join(" · "),
        };
        let mut warnings: Vec<Warning> = Vec::new();

        let symbol = r.text(row, "ticker").unwrap_or_default().trim().to_string();
        if symbol.is_empty() {
            errors.push(fail("no symbol — map a symbol column or drop this row"));
            continue;
        }

        let qty_raw = r.num(row, "quantity");
        let Some(side) = r.side(row, qty_raw) else {
            errors.push(fail(
                "not a buy or a sell — map a direction column, or set what rows without one are",
            ));
            continue;
        };

        let Some(quantity) = qty_raw.map(f64::abs).filter(|q| *q > 0.0) else {
            errors.push(fail("no quantity — map a quantity column or drop this row"));
            continue;
        };

        // A ledger often states the cash moved and leaves the unit price implicit; with a
        // quantity beside it, one gives the other.
        let price = match r.num(row, "price").map(f64::abs).filter(|p| *p > 0.0) {
            Some(p) => p,
            None => match r.num(row, "amount").map(f64::abs).filter(|a| *a > 0.0) {
                Some(amount) => {
                    warnings.push(Warning {
                        code: "price".into(),
                        message: "unit price computed from the amount".into(),
                    });
                    amount / quantity
                }
                None => {
                    errors.push(fail("no price — map a price or an amount column"));
                    continue;
                }
            },
        };

        let Some(at) = r.date(row) else {
            errors.push(fail("no date could be read — map a date column"));
            continue;
        };

        let fee = r
            .num(row, "fee")
            .map(|f| if mapping.conventions.fees_abs { f.abs() } else { f })
            .unwrap_or(0.0);

        ops.push(BuiltOp {
            symbol,
            side,
            op_date: at.date(),
            quantity,
            price,
            fee,
            note: r.text(row, "note").unwrap_or_default(),
            currency: r.currency(row),
            row_hash: hasher.row(row, r.raw(row, "external_id")),
            source_row: line,
            warnings,
        });
    }

    BuildResult { ops, errors }
}
