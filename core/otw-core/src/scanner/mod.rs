//! Scanner: pure judgement of closed bars, shared by the background job
//! (`scanner_job`), the run-now preview and the "test on item" route (`scanner_api`).
//!
//! A signal is one of four builders, all judged bar by bar on closed bars only:
//!
//! - `signal`: a backtest `SignalGroup`, the same operands and ops a strategy uses;
//! - `event`: something that happened on the bar ([`events`]);
//! - `pattern`: a candlestick pattern or a price structure ([`patterns`]);
//! - `time`: a window on the clock, in a timezone or a market session ([`time`]).
//!
//! An alert ANDs its signals on the same closed bar ([`eval`]). Nothing here reads the
//! database or a provider: the job hands in the bars and gets back verdicts.

pub mod eval;
pub mod events;
pub mod patterns;
pub mod time;

use ::time::OffsetDateTime;
use serde::{Deserialize, Serialize};

use otw_store::histdata::Bar;

/// The closed bars of one item, as columns, oldest first.
#[derive(Debug, Clone, Default)]
pub struct Ohlcv {
    pub ts: Vec<OffsetDateTime>,
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub volume: Vec<f64>,
}

impl Ohlcv {
    pub fn of(bars: &[Bar]) -> Ohlcv {
        Ohlcv {
            ts: bars.iter().map(|b| b.ts).collect(),
            open: bars.iter().map(|b| b.open).collect(),
            high: bars.iter().map(|b| b.high).collect(),
            low: bars.iter().map(|b| b.low).collect(),
            close: bars.iter().map(|b| b.close).collect(),
            volume: bars.iter().map(|b| b.volume).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.close.len()
    }

    pub fn is_empty(&self) -> bool {
        self.close.is_empty()
    }

    /// Bar `i`'s range, high to low.
    pub fn range(&self, i: usize) -> f64 {
        self.high[i] - self.low[i]
    }

    /// Bar `i`'s body, open to close, as a size.
    pub fn body(&self, i: usize) -> f64 {
        (self.close[i] - self.open[i]).abs()
    }

    pub fn upper_wick(&self, i: usize) -> f64 {
        self.high[i] - self.open[i].max(self.close[i])
    }

    pub fn lower_wick(&self, i: usize) -> f64 {
        self.open[i].min(self.close[i]) - self.low[i]
    }

    pub fn bullish(&self, i: usize) -> bool {
        self.close[i] > self.open[i]
    }

    pub fn bearish(&self, i: usize) -> bool {
        self.close[i] < self.open[i]
    }
}

/// Which way a move, a gap or a reversal must go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
    #[default]
    Either,
}

impl Direction {
    /// Does a signed value go this way by at least `size` (a positive threshold)?
    pub fn reaches(self, value: f64, size: f64) -> bool {
        match self {
            Direction::Up => value >= size,
            Direction::Down => value <= -size,
            Direction::Either => value.abs() >= size,
        }
    }

    /// Does a bullish (`true`) or bearish (`false`) shape match this direction?
    pub fn admits(self, bullish: bool) -> bool {
        match self {
            Direction::Up => bullish,
            Direction::Down => !bullish,
            Direction::Either => true,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Either => "either way",
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use ::time::macros::datetime;

    /// Hourly bars from `(open, high, low, close)`, volume 100, from 2026-09-07 00:00 UTC.
    pub fn bars(rows: &[(f64, f64, f64, f64)]) -> Ohlcv {
        let start = datetime!(2026-09-07 00:00 UTC);
        let mut s = Ohlcv::default();
        for (i, &(o, h, l, c)) in rows.iter().enumerate() {
            s.ts.push(start + ::time::Duration::hours(i as i64));
            s.open.push(o);
            s.high.push(h);
            s.low.push(l);
            s.close.push(c);
            s.volume.push(100.0);
        }
        s
    }

    /// Bars that only move on the close: open = previous close, wicks of 0.5.
    pub fn closes(px: &[f64]) -> Ohlcv {
        let rows: Vec<(f64, f64, f64, f64)> = px
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let o = if i == 0 { c } else { px[i - 1] };
                (o, o.max(c) + 0.5, o.min(c) - 0.5, c)
            })
            .collect();
        bars(&rows)
    }
}
