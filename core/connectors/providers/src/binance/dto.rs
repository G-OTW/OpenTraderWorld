//! Wire shapes, one per endpoint. Tolerant to new fields; required fields are not `Option`.

use otw_connector_sdk::parse::{Dec, Int, OptDec};
use serde::Deserialize;
use serde_json::value::RawValue;

/// `GET /api/v3/klines`: an array of positional arrays
/// `[openTime, open, high, low, close, volume, closeTime, quoteVolume, trades, takerBase, takerQuote, ignore]`.
pub type Klines = Vec<Vec<Box<RawValue>>>;

/// The fields of one kline row, read by position.
pub struct KlineRow {
    pub open_ms: i64,
    pub open: Dec,
    pub high: Dec,
    pub low: Dec,
    pub close: Dec,
    pub volume: Dec,
    pub close_ms: i64,
    pub quote_volume: Option<Dec>,
    pub trades: Option<i64>,
}

fn at<'a, T: Deserialize<'a>>(row: &'a [Box<RawValue>], i: usize) -> Option<T> {
    row.get(i).and_then(|v| serde_json::from_str(v.get()).ok())
}

impl KlineRow {
    /// `None` when one of the first seven fields is missing or not a number.
    pub fn read(row: &[Box<RawValue>]) -> Option<KlineRow> {
        Some(KlineRow {
            open_ms: at::<Int>(row, 0)?.0,
            open: at(row, 1)?,
            high: at(row, 2)?,
            low: at(row, 3)?,
            close: at(row, 4)?,
            volume: at(row, 5)?,
            close_ms: at::<Int>(row, 6)?.0,
            quote_volume: at(row, 7),
            trades: at::<Int>(row, 8).map(|i| i.0),
        })
    }
}

/// `GET /api/v3/exchangeInfo`.
#[derive(Deserialize)]
pub struct ExchangeInfo {
    pub symbols: Vec<SymbolInfo>,
}

/// The symbols alone, with their filters (the order checks' tests read them so).
#[cfg(test)]
#[derive(Deserialize)]
pub struct SymbolFilters {
    pub symbols: Vec<SymbolFiltersRow>,
}

#[cfg(test)]
#[derive(Deserialize)]
pub struct SymbolFiltersRow {
    pub symbol: String,
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfo {
    pub symbol: String,
    pub status: String,
    pub base_asset: String,
    pub quote_asset: String,
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// Futures only: `PERPETUAL`, `CURRENT_QUARTER`, `NEXT_QUARTER`, ...
    #[serde(default)]
    pub contract_type: Option<String>,
    /// Futures only: delivery (ms); far in the future for a perpetual.
    #[serde(default)]
    pub delivery_date: Option<Int>,
    #[serde(default)]
    pub margin_asset: Option<String>,
}

/// One symbol filter; each type fills its own fields (filters.md on spot, the futures common
/// definitions on USDⓈ-M, where counts and the minimum notional are spelled `limit` and
/// `notional`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub filter_type: String,
    #[serde(default)]
    pub tick_size: OptDec,
    #[serde(default)]
    pub min_price: OptDec,
    #[serde(default)]
    pub max_price: OptDec,
    #[serde(default)]
    pub step_size: OptDec,
    #[serde(default)]
    pub min_qty: OptDec,
    #[serde(default)]
    pub max_qty: OptDec,
    #[serde(default)]
    pub min_notional: OptDec,
    #[serde(default)]
    pub max_notional: OptDec,
    #[serde(default)]
    pub notional: OptDec,
    #[serde(default)]
    pub apply_to_market: Option<bool>,
    #[serde(default)]
    pub apply_min_to_market: Option<bool>,
    #[serde(default)]
    pub apply_max_to_market: Option<bool>,
    #[serde(default)]
    pub avg_price_mins: OptDec,
    #[serde(default)]
    pub multiplier_up: OptDec,
    #[serde(default)]
    pub multiplier_down: OptDec,
    #[serde(default)]
    pub bid_multiplier_up: OptDec,
    #[serde(default)]
    pub bid_multiplier_down: OptDec,
    #[serde(default)]
    pub ask_multiplier_up: OptDec,
    #[serde(default)]
    pub ask_multiplier_down: OptDec,
    #[serde(default)]
    pub max_num_orders: OptDec,
    #[serde(default)]
    pub max_num_algo_orders: OptDec,
    #[serde(default)]
    pub limit: OptDec,
}

/// `GET /api/v3/ticker/bookTicker?symbol=`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookTicker {
    pub symbol: String,
    pub bid_price: Dec,
    pub bid_qty: OptDec,
    pub ask_price: Dec,
    pub ask_qty: OptDec,
}

/// Every error body: `{"code": -1121, "msg": "Invalid symbol."}`.
#[derive(Deserialize)]
pub struct ApiError {
    pub code: i64,
    #[serde(default)]
    pub msg: String,
}
