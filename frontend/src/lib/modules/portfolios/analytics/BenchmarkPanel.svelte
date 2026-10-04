<script>
  // `benchmark` — the performance against the risk that was taken.
  //
  // The headline is the risk-matched return: what the index would have returned levered to
  // the portfolio's own volatility. Beating an index by taking twice its risk is not beating
  // it, and this is the only line on the page that says so.
  import { t } from '$lib/i18n';
  import { fmtPct, fmtSignedPct, fmtFixed, EM_DASH } from '$lib/format.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';

  let { block = null, onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const num = (v, digits = 2) => (v == null ? EM_DASH : fmtFixed(v, digits));
  const edge = $derived(d?.risk_adjusted_edge_pct ?? null);
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="verdict" class:good={edge > 0} class:bad={edge < 0}>
    <span class="lbl">{$t('portfolios.analytics.bench.verdict')}</span>
    <strong class="num">{edge == null ? EM_DASH : fmtSignedPct(edge)}</strong>
    <p>
      {$t(edge != null && edge >= 0 ? 'portfolios.analytics.bench.worthIt' : 'portfolios.analytics.bench.notWorthIt', {
        symbol: d.symbol ?? '',
        matched: d.risk_matched_pct == null ? EM_DASH : fmtPct(d.risk_matched_pct),
        actual: fmtPct(d.portfolio.annualized_pct)
      })}
    </p>
  </div>

  <table class="tbl vs">
    <thead>
      <tr>
        <th></th>
        <th class="r">{$t('portfolios.analytics.bench.portfolio')}</th>
        <th class="r">{d.symbol ?? $t('portfolios.analytics.bench.benchmark')}</th>
      </tr>
    </thead>
    <tbody>
      <tr>
        <td>{$t('portfolios.analytics.bench.total')}</td>
        <td class="r num">{fmtSignedPct(d.portfolio.total_pct)}</td>
        <td class="r num">{fmtSignedPct(d.benchmark.total_pct)}</td>
      </tr>
      <tr>
        <td>{$t('portfolios.analytics.bench.annualized')}</td>
        <td class="r num">{fmtSignedPct(d.portfolio.annualized_pct)}</td>
        <td class="r num">{fmtSignedPct(d.benchmark.annualized_pct)}</td>
      </tr>
      <tr>
        <td>{$t('portfolios.analytics.bench.volatility')}</td>
        <td class="r num">{fmtPct(d.portfolio.volatility_pct)}</td>
        <td class="r num">{fmtPct(d.benchmark.volatility_pct)}</td>
      </tr>
      <tr>
        <td>{$t('portfolios.analytics.bench.maxDrawdown')}</td>
        <td class="r num">{fmtPct(d.portfolio.max_drawdown_pct)}</td>
        <td class="r num">{fmtPct(d.benchmark.max_drawdown_pct)}</td>
      </tr>
      <tr>
        <td>{$t('portfolios.analytics.bench.sharpe')}</td>
        <td class="r num">{num(d.portfolio.sharpe)}</td>
        <td class="r num">{num(d.benchmark.sharpe)}</td>
      </tr>
    </tbody>
  </table>

  <div class="chips">
    <span><b>{$t('portfolios.analytics.bench.alpha')}</b> {d.alpha_pct == null ? EM_DASH : fmtSignedPct(d.alpha_pct)}</span>
    <span><b>{$t('portfolios.analytics.bench.beta')}</b> {num(d.beta)}</span>
    <span><b>R²</b> {num(d.r_squared)}</span>
    <span><b>{$t('portfolios.analytics.bench.tracking')}</b> {fmtPct(d.tracking_error_pct)}</span>
    <span><b>{$t('portfolios.analytics.bench.info')}</b> {num(d.information_ratio)}</span>
    <span><b>{$t('portfolios.analytics.bench.upCapture')}</b> {d.up_capture_pct == null ? EM_DASH : fmtPct(d.up_capture_pct)}</span>
    <span><b>{$t('portfolios.analytics.bench.downCapture')}</b> {d.down_capture_pct == null ? EM_DASH : fmtPct(d.down_capture_pct)}</span>
  </div>

  <Sample sample={block.sample} />
{/if}

<style>
  .verdict {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-1) var(--space-3);
    align-items: baseline;
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-left: 3px solid var(--muted);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .verdict.good {
    border-left-color: var(--green);
  }
  .verdict.bad {
    border-left-color: var(--red);
  }
  .verdict .lbl {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .verdict strong {
    grid-row: 1 / 3;
    grid-column: 1;
    font-size: 30px;
  }
  .verdict.good strong {
    color: var(--green);
  }
  .verdict.bad strong {
    color: var(--red);
  }
  .verdict p {
    grid-column: 2;
    margin: 0;
    color: var(--muted);
    font-size: 13px;
  }
  .vs {
    margin-top: var(--space-4);
  }
  .r {
    text-align: right;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-top: var(--space-3);
    font-size: 12px;
  }
  .chips span {
    padding: 3px 9px;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--muted);
  }
  .chips b {
    color: var(--text);
    font-weight: 600;
  }
</style>
