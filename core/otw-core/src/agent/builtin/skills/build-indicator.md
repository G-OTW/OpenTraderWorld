# Building a custom indicator

## When to use

When a signal needs a series the built-in indicator list does not provide — a ratio, a
normalisation, a spread between two indicators, a smoothed version of another indicator.

## The format

A custom indicator is a **node graph**, not code: an ordered list of steps where every reference
points to an *earlier* index, plus which index is the output. `POST /api/backtest/indicators`
with `{name, description, definition}`.

This one is ATR-normalised momentum — the 10-bar price change divided by ATR(14) — and it
validates as-is:

```json
{
  "nodes": [
    {"op": "price",     "field": "close"},
    {"op": "change",    "a": 0, "period": 10},
    {"op": "indicator", "indicator": "atr", "period": 14},
    {"op": "div",       "a": 1, "b": 2}
  ],
  "output": 3
}
```

**Sources:** `price` (`field`: open/high/low/close/volume), `const` (`value`), `indicator`
(`indicator`, plus `period`/`fast`/`slow`/`mult`/`signal_period` as that indicator needs, and an
optional `src` index to chain it onto an earlier step instead of the price).

**Binary** (`a`, `b` are node indices): `add`, `sub`, `mul`, `div`, `min`, `max`.
**Unary:** `abs`, `neg`, `shift` (`n` bars), `sma_of`/`ema_of` (`period`), `highest`/`lowest`
(`period`), `change` (`period`), `clamp` (`lo`, `hi`).

## Rules the validator enforces

- **No forward or self references.** A node may only reference a *lower* index. Violating it
  returns `step N references step M which is not before it`. Build bottom-up: sources first,
  combinations after.
- **Bounded size** (64 nodes).
- `output` must be a valid index — it is the series the strategy actually sees.

## Pitfalls

- **Lookahead.** `shift` with a negative intent is impossible by construction, which is
  deliberate: an indicator that peeks at the future backtests beautifully and loses money live.
  If a step feels like it needs tomorrow's bar, the idea is wrong, not the format.
- **Warm-up compounds.** SMA(20) of RSI(14) is undefined for ~34 bars, not 20. The engine
  computes this for you, but a short dataset can leave you with very few usable bars — check the
  effective trading start.
- **Division by a series that touches zero** produces undefined values that propagate. Prefer a
  denominator that cannot vanish (ATR, a clamped value) or clamp it.
- **Chaining only works on single-series indicators.** `src` is honoured by RSI, HullMA, MACD
  and similar; on others it is ignored.

## Verification

After saving, **use it in a real backtest** and check the trade count and the effective start
bar are sane. An indicator that produces zero trades usually means the output is always on one
side of its comparison — plot the logic in your head against a few bars before blaming the data.

## Report shape

What the indicator computes in one sentence, the node list, the warm-up cost, and the result of
the sanity run that proves it produces signals.
