# Market fixtures

Price series the backtest snapshots, the indicator tests and the benchmarks read. Columns:
`ts,open,high,low,close,volume`, `ts` in RFC 3339 UTC, prices with 2 decimals.

| File | Provenance |
|---|---|
| `synth_daily_a_seed42.csv` | Generated, 160 daily bars from 2024-01-01, start 100.00, seed 42. |
| `synth_daily_b_seed7.csv` | Generated, 160 daily bars from 2024-01-01, start 50.00, seed 7. |

Both are synthetic: no real instrument, no vendor data, no licence attached. The generator is
the integer LCG `x = (x * 1103515245 + 12345) mod 2^31`, `u = x / 2^31`, the same one as
`core/otw-script/tests/common::synth`. Per bar, from the previous close `c`:

```
open   = c
close  = open * (1 + (u1 - 0.5) * 0.04)
high   = max(open, close) * (1 + u2 * 0.01)
low    = min(open, close) * (1 - u3 * 0.01)
volume = 1000 + floor(u4 * 5000)
```

Each price is rounded to 2 decimals, then `high`/`low` are widened to hold `open` and `close`,
and the rounded close seeds the next bar. A test that needs a different shape (a gap, a flat
stretch, a crash) derives it from these files in the test itself, never by editing them.

Snapshots built on these files (`core/otw-core/src/backtest/snapshots/`) change only when the
engine's results change; a changed snapshot is justified in the change that moves it.
