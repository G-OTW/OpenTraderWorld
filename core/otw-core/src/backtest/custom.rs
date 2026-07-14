//! Custom-indicator DAG: a small directed acyclic graph of nodes evaluated into a per-bar
//! series. There is **no text language** — the user never types code (spec §2.1). A definition
//! is an ordered list of nodes where every node that references another does so by an earlier
//! index, so the graph is topologically ordered by construction (and cycle-free by validation).
//! One node index is marked `output`.
//!
//! Leaf nodes (`price`, `const`, `indicator`) reuse the same OHLCV / built-in indicator
//! machinery as `Operand`; transform nodes combine earlier nodes element-wise or with a rolling
//! window. An `indicator` node may also carry a `src` index to chain a single-series indicator
//! (RSI, HullMA, MACD…) onto an earlier node's output instead of the price (see
//! `indicators::is_chainable`). Undefined inputs (warm-up bars, div-by-zero) propagate as `None`.
//!
//! The frontend builder now edits this graph as named steps and text formulas, compiling them to
//! this node list; the stored/evaluated format remains the index DAG below.

use serde::{Deserialize, Serialize};

use super::{price_series, resolve_builtin, Bars};

/// Hard limits so a pathological definition can't blow up the run (validated on save).
pub const MAX_NODES: usize = 64;

/// One node of a custom-indicator graph. Binary/unary transforms reference input nodes by their
/// index in the definition's `nodes` vec; a valid graph only references *earlier* indices.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Node {
    // ── Sources ──
    /// Raw OHLCV field: open/high/low/close/volume.
    Price { field: String },
    /// A constant.
    Const { value: f64 },
    /// A built-in indicator (same catalog + params as `Operand::Indicator`). `src` optionally
    /// chains it onto an earlier step's series instead of the price: `Some(i)` applies a
    /// single-series indicator (RSI, HullMA, MACD…) to node `i`'s output; `None` uses the price
    /// (OHLC), matching legacy definitions. Only `indicators::is_chainable` names honour `src`.
    Indicator {
        indicator: String,
        #[serde(default)]
        period: usize,
        #[serde(default)]
        fast: usize,
        #[serde(default)]
        slow: usize,
        #[serde(default)]
        mult: f64,
        #[serde(default)]
        signal_period: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        src: Option<usize>,
    },

    // ── Binary transforms (element-wise) ──
    Add { a: usize, b: usize },
    Sub { a: usize, b: usize },
    Mul { a: usize, b: usize },
    Div { a: usize, b: usize },
    Min { a: usize, b: usize },
    Max { a: usize, b: usize },

    // ── Unary transforms ──
    Abs { a: usize },
    Neg { a: usize },
    /// Lag by `n` bars (value at i-n).
    Shift { a: usize, n: usize },
    /// Rolling simple / exponential moving average of another node.
    SmaOf { a: usize, period: usize },
    EmaOf { a: usize, period: usize },
    /// Rolling highest / lowest over `period` bars.
    Highest { a: usize, period: usize },
    Lowest { a: usize, period: usize },
    /// Difference from `period` bars ago (a[i] - a[i-period]).
    Change { a: usize, period: usize },
    /// Clamp into [lo, hi].
    Clamp { a: usize, lo: f64, hi: f64 },
}

/// A saved custom indicator: the node list plus which node is the output series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomIndicatorDef {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub output: usize,
}

impl CustomIndicatorDef {
    /// Conservative warm-up (bars before the output series is defined): the cumulative lookback
    /// along the DAG to the output node. Each node adds its own lookback on top of the largest of
    /// its inputs — e.g. `SMA(20)` of `RSI(14)` needs ~14+20 bars, not just 20. Overestimating is
    /// safe (a few extra warm-up bars); underestimating would feed signals `None`/partial values.
    pub fn lookback(&self) -> usize {
        if self.validate().is_some() {
            return 0;
        }
        let mut cum = vec![0usize; self.nodes.len()];
        for (i, node) in self.nodes.iter().enumerate() {
            let inputs = node.refs().into_iter().map(|r| cum.get(r).copied().unwrap_or(0)).max().unwrap_or(0);
            cum[i] = inputs + node.own_lookback();
        }
        cum.get(self.output).copied().unwrap_or(0)
    }

    /// Structural validation: bounded size, output in range, and every reference points to an
    /// *earlier* node (which makes the graph acyclic and topologically ordered). Returns an
    /// error message the API can surface, or None when valid.
    pub fn validate(&self) -> Option<String> {
        if self.nodes.is_empty() {
            return Some("indicator has no steps".into());
        }
        if self.nodes.len() > MAX_NODES {
            return Some(format!("indicator has too many steps (max {MAX_NODES})"));
        }
        if self.output >= self.nodes.len() {
            return Some("output step is out of range".into());
        }
        for (i, node) in self.nodes.iter().enumerate() {
            for r in node.refs() {
                if r >= i {
                    return Some(format!(
                        "step {} references step {} which is not before it (no forward/self refs)",
                        i + 1,
                        r + 1
                    ));
                }
            }
        }
        None
    }
}

impl Node {
    /// Bars this node alone consumes before it produces a value, ignoring input warm-up. A rough
    /// upper bound is fine (warm-up only needs to be safe, not exact). Built-in indicators reuse
    /// the largest configured length; MACD-family uses the slow leg; ATR/DEMA/etc. fold in a bit
    /// of headroom via the max of their lengths.
    fn own_lookback(&self) -> usize {
        match self {
            Node::Price { .. } | Node::Const { .. } => 0,
            Node::Indicator { period, fast, slow, signal_period, .. } => {
                [*period, *fast, *slow, *signal_period].into_iter().max().unwrap_or(0)
            }
            Node::Shift { n, .. } => *n,
            Node::SmaOf { period, .. }
            | Node::EmaOf { period, .. }
            | Node::Highest { period, .. }
            | Node::Lowest { period, .. }
            | Node::Change { period, .. } => *period,
            Node::Add { .. }
            | Node::Sub { .. }
            | Node::Mul { .. }
            | Node::Div { .. }
            | Node::Min { .. }
            | Node::Max { .. }
            | Node::Abs { .. }
            | Node::Neg { .. }
            | Node::Clamp { .. } => 0,
        }
    }

    /// Indices of the nodes this node reads from (empty for sources).
    fn refs(&self) -> Vec<usize> {
        match self {
            Node::Price { .. } | Node::Const { .. } => vec![],
            Node::Indicator { src, .. } => src.iter().copied().collect(),
            Node::Add { a, b }
            | Node::Sub { a, b }
            | Node::Mul { a, b }
            | Node::Div { a, b }
            | Node::Min { a, b }
            | Node::Max { a, b } => vec![*a, *b],
            Node::Abs { a }
            | Node::Neg { a }
            | Node::Shift { a, .. }
            | Node::SmaOf { a, .. }
            | Node::EmaOf { a, .. }
            | Node::Highest { a, .. }
            | Node::Lowest { a, .. }
            | Node::Change { a, .. }
            | Node::Clamp { a, .. } => vec![*a],
        }
    }
}

/// Evaluate a (valid) definition into a per-bar series. An invalid graph (bad refs) yields all
/// `None` rather than panicking — the API rejects invalid defs at save time, this is defensive.
pub fn eval(def: &CustomIndicatorDef, b: &Bars) -> Vec<Option<f64>> {
    let n = b.close.len();
    if def.validate().is_some() {
        return vec![None; n];
    }
    let mut series: Vec<Vec<Option<f64>>> = Vec::with_capacity(def.nodes.len());
    for node in &def.nodes {
        let out = eval_node(node, &series, b, n);
        series.push(out);
    }
    series.get(def.output).cloned().unwrap_or_else(|| vec![None; n])
}

fn eval_node(node: &Node, prev: &[Vec<Option<f64>>], b: &Bars, n: usize) -> Vec<Option<f64>> {
    let get = |i: usize| prev.get(i).cloned().unwrap_or_else(|| vec![None; n]);
    match node {
        Node::Price { field } => price_series(field, b),
        Node::Const { value } => vec![Some(*value); n],
        Node::Indicator { indicator, period, fast, slow, mult, signal_period, src } => {
            match src {
                // Chained onto an earlier step's series (only single-series indicators honour it;
                // an OHLC-derived name with a `src` set falls through to all-`None`).
                Some(i) if super::indicators::is_chainable(indicator) => {
                    super::indicators::resolve_chainable(indicator, &get(*i), *period, *fast, *slow, *signal_period)
                }
                Some(_) => vec![None; n],
                None => resolve_builtin(indicator, *period, *fast, *slow, *mult, *signal_period, b),
            }
        }
        Node::Add { a, b: bb } => zip2(&get(*a), &get(*bb), |x, y| x + y),
        Node::Sub { a, b: bb } => zip2(&get(*a), &get(*bb), |x, y| x - y),
        Node::Mul { a, b: bb } => zip2(&get(*a), &get(*bb), |x, y| x * y),
        Node::Div { a, b: bb } => zip2_opt(&get(*a), &get(*bb), |x, y| if y != 0.0 { Some(x / y) } else { None }),
        Node::Min { a, b: bb } => zip2(&get(*a), &get(*bb), f64::min),
        Node::Max { a, b: bb } => zip2(&get(*a), &get(*bb), f64::max),
        Node::Abs { a } => map1(&get(*a), f64::abs),
        Node::Neg { a } => map1(&get(*a), |x| -x),
        Node::Shift { a, n: k } => shift(&get(*a), *k),
        Node::SmaOf { a, period } => rolling_mean(&get(*a), (*period).max(1)),
        Node::EmaOf { a, period } => ema_of(&get(*a), (*period).max(1)),
        Node::Highest { a, period } => rolling_ext(&get(*a), (*period).max(1), true),
        Node::Lowest { a, period } => rolling_ext(&get(*a), (*period).max(1), false),
        Node::Change { a, period } => change(&get(*a), (*period).max(1)),
        Node::Clamp { a, lo, hi } => map1(&get(*a), |x| x.clamp(*lo, *hi)),
    }
}

fn zip2(a: &[Option<f64>], b: &[Option<f64>], f: impl Fn(f64, f64) -> f64) -> Vec<Option<f64>> {
    a.iter().zip(b).map(|(x, y)| match (x, y) { (Some(x), Some(y)) => Some(f(*x, *y)), _ => None }).collect()
}
fn zip2_opt(a: &[Option<f64>], b: &[Option<f64>], f: impl Fn(f64, f64) -> Option<f64>) -> Vec<Option<f64>> {
    a.iter().zip(b).map(|(x, y)| match (x, y) { (Some(x), Some(y)) => f(*x, *y), _ => None }).collect()
}
fn map1(a: &[Option<f64>], f: impl Fn(f64) -> f64) -> Vec<Option<f64>> {
    a.iter().map(|x| x.map(&f)).collect()
}
fn shift(a: &[Option<f64>], k: usize) -> Vec<Option<f64>> {
    let n = a.len();
    (0..n).map(|i| if i >= k { a[i - k] } else { None }).collect()
}
fn change(a: &[Option<f64>], k: usize) -> Vec<Option<f64>> {
    let n = a.len();
    (0..n)
        .map(|i| if i >= k { match (a[i], a[i - k]) { (Some(x), Some(y)) => Some(x - y), _ => None } } else { None })
        .collect()
}
fn rolling_mean(a: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let n = a.len();
    (0..n)
        .map(|i| {
            if i + 1 < period {
                return None;
            }
            let win = &a[i + 1 - period..=i];
            if win.iter().any(|v| v.is_none()) {
                None
            } else {
                Some(win.iter().map(|v| v.unwrap()).sum::<f64>() / period as f64)
            }
        })
        .collect()
}
fn ema_of(a: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let n = a.len();
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut out = vec![None; n];
    let mut prev: Option<f64> = None;
    for i in 0..n {
        if let Some(v) = a[i] {
            prev = Some(match prev { Some(p) => alpha * v + (1.0 - alpha) * p, None => v });
            out[i] = prev;
        }
    }
    out
}
fn rolling_ext(a: &[Option<f64>], period: usize, hi: bool) -> Vec<Option<f64>> {
    let n = a.len();
    (0..n)
        .map(|i| {
            if i + 1 < period {
                return None;
            }
            let win = &a[i + 1 - period..=i];
            if win.iter().any(|v| v.is_none()) {
                return None;
            }
            let vals = win.iter().map(|v| v.unwrap());
            if hi {
                vals.fold(f64::NEG_INFINITY, f64::max).into()
            } else {
                vals.fold(f64::INFINITY, f64::min).into()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars<'a>(c: &'a [f64]) -> Bars<'a> {
        // Reuse close for all fields; ts unused by the DAG.
        Bars { ticker: "", ts: &[], open: c, high: c, low: c, close: c, volume: c }
    }

    #[test]
    fn add_two_sources() {
        // close + const(10) → close+10
        let def = CustomIndicatorDef {
            nodes: vec![
                Node::Price { field: "close".into() },
                Node::Const { value: 10.0 },
                Node::Add { a: 0, b: 1 },
            ],
            output: 2,
        };
        assert!(def.validate().is_none());
        let c = vec![1.0, 2.0, 3.0];
        let out = eval(&def, &bars(&c));
        assert_eq!(out, vec![Some(11.0), Some(12.0), Some(13.0)]);
    }

    #[test]
    fn sma_of_a_node() {
        // SMA(2) of close.
        let def = CustomIndicatorDef {
            nodes: vec![Node::Price { field: "close".into() }, Node::SmaOf { a: 0, period: 2 }],
            output: 1,
        };
        let c = vec![2.0, 4.0, 6.0];
        let out = eval(&def, &bars(&c));
        assert_eq!(out, vec![None, Some(3.0), Some(5.0)]);
    }

    #[test]
    fn forward_reference_rejected() {
        let def = CustomIndicatorDef {
            nodes: vec![Node::Add { a: 0, b: 1 }, Node::Const { value: 1.0 }],
            output: 0,
        };
        assert!(def.validate().is_some(), "forward/self reference must be rejected");
    }

    fn indicator(name: &str, period: usize, src: Option<usize>) -> Node {
        Node::Indicator {
            indicator: name.into(),
            period,
            fast: 0,
            slow: 0,
            mult: 0.0,
            signal_period: 0,
            src,
        }
    }

    #[test]
    fn hma_of_rsi_chains_via_src() {
        // step0 = close, step1 = RSI(close, 3), step2 = HullMA(@step1, 4). The HMA must read the
        // RSI series, not the price. Validate + eval without panicking and produce some values.
        let def = CustomIndicatorDef {
            nodes: vec![
                Node::Price { field: "close".into() },
                indicator("rsi", 3, Some(0)),
                indicator("hma", 4, Some(1)),
            ],
            output: 2,
        };
        assert!(def.validate().is_none(), "chained indicator src must validate");
        let c: Vec<f64> = (1..=40).map(|x| (x as f64 * 0.3).sin() * 5.0 + 50.0).collect();
        let out = eval(&def, &bars(&c));
        assert_eq!(out.len(), c.len());
        assert!(out.iter().any(|v| v.is_some()), "HMA-of-RSI should yield values once warmed up");
    }

    #[test]
    fn smma_of_volume_formula_chains() {
        // The @volume-normalised-then-smoothed case:
        //   0: price(volume)
        //   1: SMA(@0, 3)           (chainable indicator over the volume series)
        //   2: @volume / @sma       (formula = div node)
        //   3: EMA(@2, 2)           (chainable indicator over the formula series)
        let def = CustomIndicatorDef {
            nodes: vec![
                Node::Price { field: "volume".into() },
                indicator("sma", 3, Some(0)),
                Node::Div { a: 0, b: 1 },
                indicator("ema", 2, Some(2)),
            ],
            output: 3,
        };
        assert!(def.validate().is_none());
        // Distinct volume so SMA/EMA are well-defined; close reused by the bars helper is unused.
        let vol = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
        let b = Bars { ticker: "", ts: &[], open: &vol, high: &vol, low: &vol, close: &vol, volume: &vol };
        let out = eval(&def, &b);
        assert_eq!(out.len(), vol.len());
        assert!(out.iter().any(|v| v.is_some()), "chained volume formula should produce values");
    }

    #[test]
    fn lookback_is_cumulative_along_the_chain() {
        // HullMA(30) of RSI(9) over close: warm-up must fold both, not just take the max.
        let def = CustomIndicatorDef {
            nodes: vec![
                Node::Price { field: "close".into() },
                indicator("rsi", 9, Some(0)),
                indicator("hma", 30, Some(1)),
            ],
            output: 2,
        };
        assert_eq!(def.lookback(), 39, "9 (RSI) + 30 (HullMA) cumulative");

        // A bare price node has no warm-up; a single indicator = its own length.
        let simple = CustomIndicatorDef {
            nodes: vec![Node::Price { field: "close".into() }, indicator("sma", 20, Some(0))],
            output: 1,
        };
        assert_eq!(simple.lookback(), 20);
    }

    #[test]
    fn indicator_src_must_point_earlier() {
        // A forward src (step0 references step1) is rejected like any other forward ref.
        let def = CustomIndicatorDef {
            nodes: vec![indicator("sma", 2, Some(1)), Node::Price { field: "close".into() }],
            output: 0,
        };
        assert!(def.validate().is_some(), "forward indicator src must be rejected");
    }

    #[test]
    fn ohlc_indicator_with_src_is_none() {
        // ATR is not chainable; giving it a src yields all-None rather than nonsense.
        let def = CustomIndicatorDef {
            nodes: vec![Node::Price { field: "close".into() }, indicator("atr", 3, Some(0))],
            output: 1,
        };
        assert!(def.validate().is_none());
        let c = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(eval(&def, &bars(&c)), vec![None; 5]);
    }

    #[test]
    fn div_by_zero_is_none() {
        let def = CustomIndicatorDef {
            nodes: vec![
                Node::Const { value: 5.0 },
                Node::Const { value: 0.0 },
                Node::Div { a: 0, b: 1 },
            ],
            output: 2,
        };
        let c = vec![1.0, 1.0];
        assert_eq!(eval(&def, &bars(&c)), vec![None, None]);
    }
}
