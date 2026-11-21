//! Long-series indicator timings (TESTING.md §4.14): one compiled script per indicator family,
//! run over 100k and 1M synthetic bars. Local only, never a CI gate:
//! `cargo bench -p otw-script --bench ta`.

use std::collections::BTreeMap;

use otw_script::input::{Bars, Limits, RunInput, SymInfo, TfUnit, Timeframe};

fn main() {
    divan::main();
}

/// Deterministic OHLCV, the integer LCG of `tests/common::synth` (seeded, no clock).
fn synth(n: usize, seed: u64) -> Bars {
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
        b.time.push(1_704_067_200_000 + i as i64 * 3_600_000);
        b.time_close.push(1_704_067_200_000 + (i as i64 + 1) * 3_600_000);
        b.open.push(open);
        b.high.push(close.max(open) * (1.0 + next() * 0.005));
        b.low.push(close.min(open) * (1.0 - next() * 0.005));
        b.close.push(close);
        b.volume.push(1000.0 + (next() * 5000.0).floor());
    }
    b
}

fn input(n: usize) -> RunInput {
    RunInput {
        bars: synth(n, 42),
        syminfo: SymInfo {
            ticker: "BENCH".into(),
            tickerid: "X:BENCH".into(),
            mintick: Some(0.01),
            pointvalue: Some(1.0),
            timezone: Some("UTC".into()),
            session: Some("24x7".into()),
            kind: Some("crypto".into()),
            currency: Some("USD".into()),
            basecurrency: Some("BNC".into()),
            description: None,
        },
        timeframe: Timeframe { multiplier: 60, unit: TfUnit::Minute },
        inputs: BTreeMap::new(),
        limits: Limits::default(),
    }
}

const SCRIPTS: [(&str, &str); 5] = [
    ("sma_ema", "plot(ta.sma(close, 50))\nplot(ta.ema(close, 200))"),
    ("rsi", "plot(ta.rsi(close, 14))"),
    ("atr_stdev", "plot(ta.atr(14))\nplot(ta.stdev(close, 20))"),
    ("macd", "[m, s, h] = ta.macd(close, 12, 26, 9)\nplot(h)"),
    ("highest_lowest", "plot(ta.highest(high, 100) - ta.lowest(low, 100))"),
];

#[divan::bench(args = SCRIPTS.map(|(name, _)| name), consts = [100_000, 1_000_000], sample_count = 10)]
fn indicator<const N: usize>(bencher: divan::Bencher, name: &str) {
    let body = SCRIPTS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b).unwrap();
    let program = otw_script::compile(&format!("//@version=6\nindicator(\"bench\")\n{body}")).expect("compiles");
    // No op budget and no wall clock: the bench measures the work, not the guard.
    let mut i = input(N);
    i.limits = Limits { max_ops: u64::MAX, timeout_ms: None, ..Limits::default() };
    bencher.bench_local(|| program.run(divan::black_box(&i)).expect("runs"));
}
