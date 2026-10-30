//! Candlestick patterns and price structures, judged on closed bars.
//!
//! Each pattern is a pure function of the bars up to `i`: no trend filter, no volume, no
//! tolerance hidden in a setting. The thresholds are written here, one place, and each is
//! pinned by a fixture test with a bar that matches and one that does not.
//!
//! - `doji`: body at most 10 % of the range.
//! - `hammer`: lower wick at least twice the body, upper wick at most 10 % of the range,
//!   body at least 5 % of the range (a flat body is a doji, not a hammer).
//! - `shooting_star`: the hammer upside down.
//! - `pin_bar`: one wick at least two thirds of the range, body at most a third.
//! - `engulfing`: the body covers the previous body and is bigger, colors opposite.
//! - `inside_bar`: inside the previous bar's range and narrower. `outside_bar`: past both
//!   ends of it.
//! - `three_bar_reversal`: the middle bar holds the extreme, the third closes past the
//!   middle bar's other end in the new direction.
//! - `breakout{bars}`: close past the highest high (or lowest low) of the `bars` before.
//! - `consolidation{bars, max_range_pct}`: the last `bars` bars span at most that percent.
//! - `hh_hl{count}` / `lh_ll{count}`: `count` bars in a row with a higher high and a higher
//!   low (lower high and lower low) than the bar before.

use serde::{Deserialize, Serialize};

use super::{Direction, Ohlcv};

/// Longest window a structure may ask for.
pub const MAX_BARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pattern {
    Engulfing {
        #[serde(default)]
        direction: Direction,
    },
    PinBar {
        #[serde(default)]
        direction: Direction,
    },
    InsideBar,
    OutsideBar,
    Doji,
    Hammer,
    ShootingStar,
    ThreeBarReversal {
        #[serde(default)]
        direction: Direction,
    },
    Breakout {
        bars: usize,
        #[serde(default)]
        direction: Direction,
    },
    Consolidation {
        bars: usize,
        max_range_pct: f64,
    },
    HhHl {
        count: usize,
    },
    LhLl {
        count: usize,
    },
}

impl Pattern {
    /// Why this pattern cannot be judged, in the user's terms.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Pattern::Breakout { bars, .. } if !(1..=MAX_BARS).contains(bars) => {
                Err(format!("a breakout looks back 1 to {MAX_BARS} bars"))
            }
            Pattern::Consolidation { bars, max_range_pct } => {
                if !(2..=MAX_BARS).contains(bars) {
                    return Err(format!("a consolidation spans 2 to {MAX_BARS} bars"));
                }
                if !max_range_pct.is_finite() || *max_range_pct <= 0.0 {
                    return Err("the consolidation range must be a percent above 0".into());
                }
                Ok(())
            }
            Pattern::HhHl { count } | Pattern::LhLl { count } if !(1..=MAX_BARS).contains(count) => {
                Err(format!("the count of bars goes from 1 to {MAX_BARS}"))
            }
            _ => Ok(()),
        }
    }

    /// Closed bars needed before the newest can be judged.
    pub fn lookback(&self) -> usize {
        match self {
            Pattern::PinBar { .. } | Pattern::Doji | Pattern::Hammer | Pattern::ShootingStar => 1,
            Pattern::Engulfing { .. } | Pattern::InsideBar | Pattern::OutsideBar => 2,
            Pattern::ThreeBarReversal { .. } => 3,
            Pattern::Breakout { bars, .. } => bars + 1,
            Pattern::Consolidation { bars, .. } => *bars,
            Pattern::HhHl { count } | Pattern::LhLl { count } => count + 1,
        }
    }

    /// Whether the pattern holds on each bar.
    pub fn judge(&self, s: &Ohlcv) -> Vec<bool> {
        (0..s.len()).map(|i| self.holds(s, i)).collect()
    }

    /// Whether the pattern holds on bar `i`.
    pub fn holds(&self, s: &Ohlcv, i: usize) -> bool {
        if i + 1 < self.lookback() || i >= s.len() {
            return false;
        }
        match self {
            Pattern::Doji => doji(s, i),
            Pattern::Hammer => hammer(s, i),
            Pattern::ShootingStar => shooting_star(s, i),
            Pattern::PinBar { direction } => pin_bar(s, i, *direction),
            Pattern::Engulfing { direction } => engulfing(s, i, *direction),
            Pattern::InsideBar => inside_bar(s, i),
            Pattern::OutsideBar => outside_bar(s, i),
            Pattern::ThreeBarReversal { direction } => three_bar_reversal(s, i, *direction),
            Pattern::Breakout { bars, direction } => breakout(s, i, *bars, *direction),
            Pattern::Consolidation { bars, max_range_pct } => consolidation(s, i, *bars, *max_range_pct),
            Pattern::HhHl { count } => staircase(s, i, *count, true),
            Pattern::LhLl { count } => staircase(s, i, *count, false),
        }
    }

    pub fn describe(&self) -> String {
        let side = |d: &Direction| match d {
            Direction::Up => "bullish ",
            Direction::Down => "bearish ",
            Direction::Either => "",
        };
        match self {
            Pattern::Engulfing { direction } => format!("{}engulfing", side(direction)),
            Pattern::PinBar { direction } => format!("{}pin bar", side(direction)),
            Pattern::InsideBar => "inside bar".into(),
            Pattern::OutsideBar => "outside bar".into(),
            Pattern::Doji => "doji".into(),
            Pattern::Hammer => "hammer".into(),
            Pattern::ShootingStar => "shooting star".into(),
            Pattern::ThreeBarReversal { direction } => format!("{}three bar reversal", side(direction)),
            Pattern::Breakout { bars, direction } => match direction {
                Direction::Down => format!("breakdown under the {bars} bar low"),
                Direction::Up => format!("breakout over the {bars} bar high"),
                Direction::Either => format!("breakout of the {bars} bar range"),
            },
            Pattern::Consolidation { bars, max_range_pct } => {
                format!("{bars} bars within {max_range_pct}%")
            }
            Pattern::HhHl { count } => format!("{count} higher highs and higher lows"),
            Pattern::LhLl { count } => format!("{count} lower highs and lower lows"),
        }
    }
}

pub fn doji(s: &Ohlcv, i: usize) -> bool {
    let r = s.range(i);
    r > 0.0 && s.body(i) <= 0.1 * r
}

pub fn hammer(s: &Ohlcv, i: usize) -> bool {
    let (r, b) = (s.range(i), s.body(i));
    r > 0.0 && b >= 0.05 * r && s.lower_wick(i) >= 2.0 * b && s.upper_wick(i) <= 0.1 * r
}

pub fn shooting_star(s: &Ohlcv, i: usize) -> bool {
    let (r, b) = (s.range(i), s.body(i));
    r > 0.0 && b >= 0.05 * r && s.upper_wick(i) >= 2.0 * b && s.lower_wick(i) <= 0.1 * r
}

pub fn pin_bar(s: &Ohlcv, i: usize, d: Direction) -> bool {
    let r = s.range(i);
    if r <= 0.0 || s.body(i) > r / 3.0 {
        return false;
    }
    let bull = s.lower_wick(i) >= r * 2.0 / 3.0;
    let bear = s.upper_wick(i) >= r * 2.0 / 3.0;
    (bull && d.admits(true)) || (bear && d.admits(false))
}

pub fn engulfing(s: &Ohlcv, i: usize, d: Direction) -> bool {
    if i == 0 {
        return false;
    }
    let p = i - 1;
    let bigger = s.body(i) > s.body(p);
    let bull = s.bearish(p) && s.bullish(i) && s.open[i] <= s.close[p] && s.close[i] >= s.open[p];
    let bear = s.bullish(p) && s.bearish(i) && s.open[i] >= s.close[p] && s.close[i] <= s.open[p];
    bigger && ((bull && d.admits(true)) || (bear && d.admits(false)))
}

pub fn inside_bar(s: &Ohlcv, i: usize) -> bool {
    i > 0 && s.high[i] <= s.high[i - 1] && s.low[i] >= s.low[i - 1] && s.range(i) < s.range(i - 1)
}

pub fn outside_bar(s: &Ohlcv, i: usize) -> bool {
    i > 0 && s.high[i] > s.high[i - 1] && s.low[i] < s.low[i - 1]
}

pub fn three_bar_reversal(s: &Ohlcv, i: usize, d: Direction) -> bool {
    if i < 2 {
        return false;
    }
    let (a, m) = (i - 2, i - 1);
    let bull = s.bearish(a)
        && s.low[m] < s.low[a]
        && s.low[m] < s.low[i]
        && s.bullish(i)
        && s.close[i] > s.high[m];
    let bear = s.bullish(a)
        && s.high[m] > s.high[a]
        && s.high[m] > s.high[i]
        && s.bearish(i)
        && s.close[i] < s.low[m];
    (bull && d.admits(true)) || (bear && d.admits(false))
}

pub fn breakout(s: &Ohlcv, i: usize, bars: usize, d: Direction) -> bool {
    if bars == 0 || i < bars {
        return false;
    }
    let prior = i - bars..i;
    let top = prior.clone().map(|k| s.high[k]).fold(f64::MIN, f64::max);
    let bottom = prior.map(|k| s.low[k]).fold(f64::MAX, f64::min);
    let up = s.close[i] > top;
    let down = s.close[i] < bottom;
    (up && d.admits(true)) || (down && d.admits(false))
}

pub fn consolidation(s: &Ohlcv, i: usize, bars: usize, max_range_pct: f64) -> bool {
    if bars == 0 || i + 1 < bars {
        return false;
    }
    let span = i + 1 - bars..=i;
    let top = span.clone().map(|k| s.high[k]).fold(f64::MIN, f64::max);
    let bottom = span.map(|k| s.low[k]).fold(f64::MAX, f64::min);
    bottom > 0.0 && (top - bottom) / bottom * 100.0 <= max_range_pct
}

/// `count` bars in a row, ending at `i`, each with a higher high and higher low than the one
/// before (`up`), or a lower high and lower low.
pub fn staircase(s: &Ohlcv, i: usize, count: usize, up: bool) -> bool {
    if count == 0 || i < count {
        return false;
    }
    (i + 1 - count..=i).all(|k| {
        if up {
            s.high[k] > s.high[k - 1] && s.low[k] > s.low[k - 1]
        } else {
            s.high[k] < s.high[k - 1] && s.low[k] < s.low[k - 1]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::fixtures::bars;

    fn last(p: &Pattern, rows: &[(f64, f64, f64, f64)]) -> bool {
        let s = bars(rows);
        p.holds(&s, s.len() - 1)
    }

    #[test]
    fn scanner_pattern_doji() {
        assert!(last(&Pattern::Doji, &[(10.0, 11.0, 9.0, 10.05)]));
        assert!(!last(&Pattern::Doji, &[(10.0, 11.0, 9.0, 10.8)]));
        // A flat bar has no range to judge.
        assert!(!last(&Pattern::Doji, &[(10.0, 10.0, 10.0, 10.0)]));
    }

    #[test]
    fn scanner_pattern_hammer() {
        // Body 0.5 at the top, lower wick 2.0, no upper wick.
        assert!(last(&Pattern::Hammer, &[(10.0, 10.5, 8.0, 10.5)]));
        // A long upper wick is not a hammer.
        assert!(!last(&Pattern::Hammer, &[(10.0, 11.5, 8.0, 10.5)]));
        // A flat body is a doji.
        assert!(!last(&Pattern::Hammer, &[(10.5, 10.5, 8.0, 10.5)]));
    }

    #[test]
    fn scanner_pattern_shooting_star() {
        assert!(last(&Pattern::ShootingStar, &[(10.5, 13.0, 10.0, 10.0)]));
        assert!(!last(&Pattern::ShootingStar, &[(10.0, 10.5, 8.0, 10.5)]));
    }

    #[test]
    fn scanner_pattern_pin_bar() {
        let bull = [(10.0, 10.3, 7.0, 10.2)];
        assert!(last(&Pattern::PinBar { direction: Direction::Up }, &bull));
        assert!(last(&Pattern::PinBar { direction: Direction::Either }, &bull));
        assert!(!last(&Pattern::PinBar { direction: Direction::Down }, &bull));
        let bear = [(10.0, 13.0, 9.8, 9.9)];
        assert!(last(&Pattern::PinBar { direction: Direction::Down }, &bear));
        // A full body is no pin.
        assert!(!last(&Pattern::PinBar { direction: Direction::Either }, &[(8.0, 10.2, 7.9, 10.0)]));
    }

    #[test]
    fn scanner_pattern_engulfing() {
        let bull = [(10.0, 10.1, 8.9, 9.0), (8.9, 10.6, 8.8, 10.5)];
        assert!(last(&Pattern::Engulfing { direction: Direction::Up }, &bull));
        assert!(!last(&Pattern::Engulfing { direction: Direction::Down }, &bull));
        let bear = [(9.0, 10.1, 8.9, 10.0), (10.1, 10.2, 8.5, 8.6)];
        assert!(last(&Pattern::Engulfing { direction: Direction::Either }, &bear));
        // Same color twice is not engulfing.
        let same = [(9.0, 10.1, 8.9, 10.0), (8.9, 11.0, 8.8, 10.9)];
        assert!(!last(&Pattern::Engulfing { direction: Direction::Either }, &same));
        // A single bar has nothing to engulf.
        assert!(!last(&Pattern::Engulfing { direction: Direction::Either }, &bull[1..]));
    }

    #[test]
    fn scanner_pattern_inside_and_outside_bar() {
        let inside = [(10.0, 12.0, 8.0, 11.0), (10.5, 11.5, 9.0, 11.0)];
        assert!(last(&Pattern::InsideBar, &inside));
        assert!(!last(&Pattern::OutsideBar, &inside));
        let outside = [(10.0, 11.0, 9.0, 10.5), (10.5, 11.5, 8.5, 9.0)];
        assert!(last(&Pattern::OutsideBar, &outside));
        assert!(!last(&Pattern::InsideBar, &outside));
    }

    #[test]
    fn scanner_pattern_three_bar_reversal() {
        let bull = [(10.0, 10.2, 9.0, 9.2), (9.2, 9.4, 8.5, 9.0), (9.0, 10.0, 8.9, 9.8)];
        assert!(last(&Pattern::ThreeBarReversal { direction: Direction::Up }, &bull));
        assert!(!last(&Pattern::ThreeBarReversal { direction: Direction::Down }, &bull));
        // The third bar does not close over the middle bar's high.
        let weak = [(10.0, 10.2, 9.0, 9.2), (9.2, 9.4, 8.5, 9.0), (9.0, 9.5, 8.9, 9.3)];
        assert!(!last(&Pattern::ThreeBarReversal { direction: Direction::Either }, &weak));
        let bear = [(9.0, 10.0, 8.9, 9.8), (9.8, 10.5, 9.7, 10.0), (10.0, 10.1, 9.0, 9.2)];
        assert!(last(&Pattern::ThreeBarReversal { direction: Direction::Down }, &bear));
    }

    #[test]
    fn scanner_pattern_breakout() {
        let rows = [
            (10.0, 10.5, 9.5, 10.0),
            (10.0, 10.6, 9.6, 10.2),
            (10.2, 10.4, 9.8, 10.1),
            (10.1, 11.0, 10.0, 10.9),
        ];
        let up = Pattern::Breakout { bars: 3, direction: Direction::Up };
        assert!(last(&up, &rows));
        assert!(!last(&Pattern::Breakout { bars: 3, direction: Direction::Down }, &rows));
        // Not past the high of the three bars before.
        assert!(!last(&up, &rows[..3]));
        let down = [(10.0, 10.5, 9.5, 10.0), (10.0, 10.2, 9.6, 9.8), (9.8, 9.9, 9.0, 9.1)];
        assert!(last(&Pattern::Breakout { bars: 2, direction: Direction::Either }, &down));
    }

    #[test]
    fn scanner_pattern_consolidation() {
        let tight = [(10.0, 10.1, 9.95, 10.0), (10.0, 10.05, 9.9, 10.0), (10.0, 10.1, 9.95, 10.05)];
        let p = Pattern::Consolidation { bars: 3, max_range_pct: 2.5 };
        assert!(last(&p, &tight));
        let wide = [(10.0, 10.1, 9.95, 10.0), (10.0, 11.0, 9.9, 10.9), (10.9, 11.1, 10.8, 11.0)];
        assert!(!last(&p, &wide));
        // Fewer bars than the span: not judged.
        assert!(!last(&p, &tight[..2]));
    }

    #[test]
    fn scanner_pattern_hh_hl_and_lh_ll() {
        let up = [(10.0, 10.5, 9.5, 10.2), (10.2, 10.8, 9.8, 10.6), (10.6, 11.2, 10.1, 11.0)];
        assert!(last(&Pattern::HhHl { count: 2 }, &up));
        assert!(!last(&Pattern::LhLl { count: 2 }, &up));
        // Only two rising bars: three asked.
        assert!(!last(&Pattern::HhHl { count: 3 }, &up));
        let down = [(11.0, 11.2, 10.1, 10.6), (10.6, 10.8, 9.8, 10.2), (10.2, 10.5, 9.5, 9.7)];
        assert!(last(&Pattern::LhLl { count: 2 }, &down));
        assert!(!last(&Pattern::HhHl { count: 1 }, &down));
    }

    #[test]
    fn scanner_pattern_config_reads_and_refuses() {
        let p: Pattern = serde_json::from_value(serde_json::json!({ "kind": "inside_bar" })).unwrap();
        assert_eq!(p, Pattern::InsideBar);
        let p: Pattern = serde_json::from_value(serde_json::json!({ "kind": "engulfing" })).unwrap();
        assert_eq!(p, Pattern::Engulfing { direction: Direction::Either });
        assert!(Pattern::Breakout { bars: 0, direction: Direction::Up }.validate().is_err());
        assert!(Pattern::Consolidation { bars: 5, max_range_pct: 0.0 }.validate().is_err());
        assert!(Pattern::HhHl { count: 3 }.validate().is_ok());
        assert_eq!(Pattern::Breakout { bars: 20, direction: Direction::Up }.lookback(), 21);
    }
}
