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

  /** Multi-asset: correlation, efficient frontier, risk parity. */
  portfolio: (dataset_ids, { samples = 5000, risk_free = 0 } = {}) =>
    req('/quant/portfolio', {
      method: 'POST',
      body: JSON.stringify({ dataset_ids, samples, risk_free })
    }),

  /** Seasonality heatmaps (month/weekday/hour) of one dataset's period returns.
   *  metric: 'return' | 'volatility'. Returns { ticker, timeframe, result }. */
  seasonality: (dataset_id, { from = null, until = null, metric = 'return' } = {}) =>
    req('/quant/seasonality', {
      method: 'POST',
      body: JSON.stringify({ dataset_id, from, until, metric })
    }),

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
