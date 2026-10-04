/** Quant Tools API client.
 *
 * Stateless analysis over the Historical Data catalog (same datasets the visualization and
 * backtest modules use). Single-asset endpoint returns risk metrics + curves for one dataset;
 * portfolio endpoint takes several and returns correlation / efficient frontier / risk parity.
 * Kelly is a pure calculator from manually-entered win-rate and payoff. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

/** Start a provider task, report its progress, resolve with its result. A curve or a chain is
 * dozens of paced provider requests, so the server answers with a task id at once. */
async function runTask(path, body, onProgress = () => {}) {
  const { task } = await req(path, { method: 'POST', body: JSON.stringify(body) });
  for (;;) {
    await new Promise((r) => setTimeout(r, 1500));
    const s = await req(`/quant/market/tasks/${task}`);
    if (s.status === 'done') return s.result;
    if (s.status === 'error') throw new Error(s.error);
    onProgress({ done: s.done, total: s.total, step: s.step });
  }
}

export const quantApi = {
  /** Stored datasets (same catalog as Historical Data). */
  datasets: () => req('/histdata/datasets').then((r) => r.datasets),

  /** Single-asset risk metrics over an optional [from, until] window (RFC3339 strings).
   *  Returns { ticker, timeframe, result }. */
  single: (dataset_id, confidence = 0.95, { from = null, until = null } = {}) =>
    req('/quant/single', {
      method: 'POST',
      body: JSON.stringify({ dataset_id, confidence, from, until })
    }),

  /** Kelly fractions from manual win-rate / avg win / avg loss. */
  kelly: (win_rate, avg_win, avg_loss) =>
    req('/quant/kelly', { method: 'POST', body: JSON.stringify({ win_rate, avg_win, avg_loss }) }),

  /** Risk-based position size. Pass either risk_pct (fraction) or risk_amount (currency). */
  size: (body) => req('/quant/size', { method: 'POST', body: JSON.stringify(body) }),

  /** Asset-derived stop suggestions (HV/ATR/swing) over an optional [from, until] window.
   *  Returns { ticker, timeframe, signals }. */
  assetSignals: (dataset_id, side, entry, { from = null, until = null } = {}) =>
    req('/quant/asset-signals', {
      method: 'POST',
      body: JSON.stringify({ dataset_id, side, entry, from, until })
    }),

  /** Multi-asset: correlation, efficient frontier, risk parity.
   *  `measure` picks the granularity the statistics are computed at: null/'auto' drops a
   *  basket mixing a 24/7 market with an exchange one to weekly, 'stored' forces the
   *  datasets' own timeframe, or name one ('1w'). */
  portfolio: (dataset_ids, { samples = 5000, risk_free = 0, measure = null } = {}) =>
    req('/quant/portfolio', {
      method: 'POST',
      body: JSON.stringify({ dataset_ids, samples, risk_free, ...(measure ? { measure } : {}) })
    }),

  /** Seasonality heatmaps (month/weekday/hour) of one dataset's period returns.
   *  metric: 'return' | 'volatility'. Returns { ticker, timeframe, result }. */
  seasonality: (dataset_id, { from = null, until = null, metric = 'return' } = {}) =>
    req('/quant/seasonality', {
      method: 'POST',
      body: JSON.stringify({ dataset_id, from, until, metric })
    }),

  /** Distribution, serial dependence, Hurst, variance ratio, unit-root tests and Sharpe
   *  significance of one dataset. */
  stats: (dataset_id, { from = null, until = null, lags = 20, risk_free = 0, benchmark_sharpe = 0 } = {}) =>
    req('/quant/stats', {
      method: 'POST',
      body: JSON.stringify({ dataset_id, from, until, lags, risk_free, benchmark_sharpe })
    }),

  /** Range-based volatility estimators, cones and a GARCH(1,1) forecast. */
  volatility: (dataset_id, { from = null, until = null, window = 21, horizon = 21 } = {}) =>
    req('/quant/volatility', {
      method: 'POST',
      body: JSON.stringify({ dataset_id, from, until, window, horizon })
    }),

  /** Gaussian hidden Markov regimes (2 to 4 states). */
  regimes: (dataset_id, { from = null, until = null, states = 2 } = {}) =>
    req('/quant/regimes', { method: 'POST', body: JSON.stringify({ dataset_id, from, until, states }) }),

  /** Forward returns after a condition, against the unconditional baseline. */
  events: (dataset_id, condition, { from = null, until = null, horizons = null, pre = 10, post = 20, min_gap = 1 } = {}) =>
    req('/quant/events', {
      method: 'POST',
      body: JSON.stringify({ dataset_id, from, until, condition, horizons, pre, post, min_gap })
    }),

  /** Pair analysis: cointegration, spread, rolling co-movement, lead-lag, Granger. */
  pairs: (dataset_ids, { measure = null, window = 60, max_lag = 5, log = true } = {}) =>
    req('/quant/pairs', {
      method: 'POST',
      body: JSON.stringify({ dataset_ids, window, max_lag, log, ...(measure ? { measure } : {}) })
    }),

  /** Basket analysis: PCA, clustering, HRP, relative strength, stress windows. */
  basket: (dataset_ids, { measure = null, linkage = 'single', weights = null } = {}) =>
    req('/quant/basket', {
      method: 'POST',
      body: JSON.stringify({ dataset_ids, linkage, weights, ...(measure ? { measure } : {}) })
    }),

  /** Factor regression of one dataset on factor datasets. */
  regression: (dataset_id, factor_ids, { measure = null, risk_free = 0, window = 60 } = {}) =>
    req('/quant/regression', {
      method: 'POST',
      body: JSON.stringify({ dataset_id, factor_ids, risk_free, window, ...(measure ? { measure } : {}) })
    }),

  /** Trade-list analytics of a saved run: expectancy, R-multiples, SQN, MAE/MFE. */
  trades: (run_id) => req('/quant/trades', { method: 'POST', body: JSON.stringify({ run_id }) }),

  /** Several saved runs side by side: curves, correlation, deflated Sharpe, PBO. */
  compare: (run_ids, partitions = 16) =>
    req('/quant/compare', { method: 'POST', body: JSON.stringify({ run_ids, partitions }) }),

  /** Calculators: the body is passed through as the endpoint documents it. */
  ruin: (body) => req('/quant/ruin', { method: 'POST', body: JSON.stringify(body) }),
  options: (body) => req('/quant/options', { method: 'POST', body: JSON.stringify(body) }),
  iv: (body) => req('/quant/iv', { method: 'POST', body: JSON.stringify(body) }),
  basis: (body) => req('/quant/basis', { method: 'POST', body: JSON.stringify(body) }),
  compound: (body) => req('/quant/compound', { method: 'POST', body: JSON.stringify(body) }),
  sharpe: (body) => req('/quant/sharpe', { method: 'POST', body: JSON.stringify(body) }),
  voltarget: (body) => req('/quant/voltarget', { method: 'POST', body: JSON.stringify(body) }),

  /** Derivatives read from a provider (IB or Massive): each starts a task that is polled. */
  futuresCurve: (body, onProgress) => runTask('/quant/market/futures', body, onProgress),
  optionSurface: (body, onProgress) => runTask('/quant/market/options', body, onProgress),
  ivHistory: (body, onProgress) => runTask('/quant/market/ivhistory', body, onProgress),

  /** Saved backtest runs (history) — the source for Monte-Carlo resampling. */
  backtestRuns: () => req('/backtest/runs').then((r) => r.runs),

  /** Monte-Carlo resample a saved run's realized trade sequence → drawdown/ruin bands.
   *  The run is replayed server-side to regenerate its exact trades (no schema change).
   *  Returns { name, ticker, timeframe, result }. */
  monteCarlo: (run_id, { iterations = 5000, horizon = null, block = 1, ruin_pct = 0.5 } = {}) =>
    req(`/backtest/runs/${run_id}/montecarlo`, {
      method: 'POST',
      body: JSON.stringify({ iterations, horizon, block, ruin_pct })
    })
};

// Formatting lives in $lib/format.js. Quant's metrics are fractions (0.05 = 5%), so its
// percent formatter is fmtRatioPct — the old local `fmtPct` took a fraction too, which
// made it silently incompatible with the identically-named fmtPct in journal/portfolios
// (those take a percent). Importers here must use fmtRatioPct.
export { fmtNum, fmtRatioPct } from '$lib/format.js';

export const CONFIDENCE_LEVELS = [0.9, 0.95, 0.99];

/** A short label for a dataset in pickers/lists. */
export const dsLabel = (d) => `${d.ticker} · ${d.timeframe}`;
