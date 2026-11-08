//! Synthetic hourly bars shared by the examples: the LCG of `backtest/custom.rs`
//! parity::synth, so every example measures the same series for a given seed.

use otw_script::input::Bars;

pub fn synth(n: usize, seed: u64) -> Bars {
    let mut x = seed;
    let mut next = || {
        x = (x * 1103515245 + 12345) % 2147483648;
        x as f64 / 2147483648.0
    };
    let mut b = Bars::default();
    let mut close = 100.0f64;
    for i in 0..n as i64 {
        let open = close;
        close *= 1.0 + (next() - 0.5) * 0.02;
        b.high.push(close.max(open) * (1.0 + next() * 0.005));
        b.low.push(close.min(open) * (1.0 - next() * 0.005));
        b.volume.push(1000.0 + (next() * 5000.0).floor());
        b.open.push(open);
        b.close.push(close);
        b.time.push(1_704_067_200_000 + i * 3_600_000);
        b.time_close.push(1_704_067_200_000 + (i + 1) * 3_600_000);
    }
    b
}
