//! Shared test helpers: compile + run a script over simple bars.
#![allow(dead_code)]

use std::collections::BTreeMap;

use otw_script::input::{Bars, InputValue, Limits, RunInput, SymInfo, TfUnit, Timeframe};
use otw_script::output::RunOutput;

pub fn syminfo() -> SymInfo {
    SymInfo {
        ticker: "TEST".into(),
        tickerid: "X:TEST".into(),
        mintick: Some(0.01),
        pointvalue: Some(1.0),
        timezone: Some("UTC".into()),
        session: Some("24x7".into()),
        kind: Some("crypto".into()),
        currency: Some("USD".into()),
        basecurrency: Some("TST".into()),
        description: Some("Test".into()),
    }
}

pub fn input(bars: Bars) -> RunInput {
    RunInput {
        bars,
        syminfo: syminfo(),
        timeframe: Timeframe { multiplier: 60, unit: TfUnit::Minute },
        inputs: BTreeMap::new(),
        limits: Limits::default(),
    }
}

/// o = h = l = c, volume 1, one bar per hour from epoch 0.
pub fn bars_close(c: &[f64]) -> Bars {
    Bars {
        time: (0..c.len() as i64).map(|i| i * 3_600_000).collect(),
        time_close: (0..c.len() as i64).map(|i| (i + 1) * 3_600_000).collect(),
        open: c.to_vec(),
        high: c.to_vec(),
        low: c.to_vec(),
        close: c.to_vec(),
        volume: vec![1.0; c.len()],
    }
}

pub fn compile(src: &str) -> otw_script::Program {
    otw_script::compile(src).unwrap_or_else(|e| panic!("compile: {e:?}"))
}

/// Compile and run; panics with the diagnostics or the runtime error.
pub fn run_src(src: &str, bars: Bars) -> RunOutput {
    run_with(src, input(bars))
}

pub fn run_with(src: &str, input: RunInput) -> RunOutput {
    compile(src).run(&input).unwrap_or_else(|e| panic!("run: {e:?}"))
}

pub fn run_inputs(src: &str, bars: Bars, inputs: &[(&str, InputValue)]) -> Result<RunOutput, otw_script::diag::RuntimeError> {
    let mut i = input(bars);
    i.inputs = inputs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    compile(src).run(&i)
}

pub fn plot(out: &RunOutput, i: usize) -> Vec<Option<f64>> {
    out.plots[i].values.clone()
}

/// `//@version=6` + `indicator("t")` + body.
pub fn v6(body: &str) -> String {
    format!("//@version=6\nindicator(\"t\")\n{body}")
}

pub fn v5(body: &str) -> String {
    format!("//@version=5\nindicator(\"t\")\n{body}")
}

pub fn some(v: &[f64]) -> Vec<Option<f64>> {
    v.iter().map(|x| Some(*x)).collect()
}

/// Deterministic OHLCV (the integer LCG of `backtest/custom.rs` parity::synth), 1h bars.
pub fn synth(n: usize, seed: u64) -> Bars {
    let mut x = seed;
    let mut next = || {
        x = (x * 1103515245 + 12345) % 2147483648;
        x as f64 / 2147483648.0
    };
    let mut b = Bars::default();
    let mut close = 100.0f64;
    for i in 0..n {
        let open = close;
        close *= 1.0 + (next() - 0.5) * 0.02;
        let high = close.max(open) * (1.0 + next() * 0.005);
        let low = close.min(open) * (1.0 - next() * 0.005);
        let vol = 1000.0 + (next() * 5000.0).floor();
        b.time.push(1_704_067_200_000 + i as i64 * 3_600_000);
        b.time_close.push(1_704_067_200_000 + (i as i64 + 1) * 3_600_000);
        b.open.push(open);
        b.high.push(high);
        b.low.push(low);
        b.close.push(close);
        b.volume.push(vol);
    }
    b
}

/// The Phase 1 perf script: ~20 statements, 5 `ta.*` calls.
pub const PERF_BODY: &str = "len = input.int(14)\nfast = ta.sma(close, len)\nslow = ta.ema(close, len * 2)\nr = ta.rsi(close, len)\na = ta.atr(len)\nh = ta.highest(high, 20)\nspread = fast - slow\nnorm = a > 0 ? spread / a : 0.0\nvar float acc = 0.0\nacc := acc * 0.9 + norm\nup = r > 70\ndown = r < 30\nscore = up ? 1 : down ? -1 : 0\nband = h - a * 2\nmid = (fast + slow) / 2\ndist = close - mid\nz = dist / math.max(a, 1e-9)\nsig = z > 1 and score >= 0\nplot(acc)\nplot(sig ? band : na)";

/// Bars `from..to` of `b`.
pub fn sub(b: &Bars, from: usize, to: usize) -> Bars {
    Bars {
        time: b.time[from..to].to_vec(),
        time_close: b.time_close[from..to].to_vec(),
        open: b.open[from..to].to_vec(),
        high: b.high[from..to].to_vec(),
        low: b.low[from..to].to_vec(),
        close: b.close[from..to].to_vec(),
        volume: b.volume[from..to].to_vec(),
    }
}
