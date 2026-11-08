<script>
  // `benchmark`: the performance against the risk that was taken.
  //
  // The headline is the risk-matched return: what the index would have returned levered to
  // the portfolio's own volatility. Beating an index by taking twice its risk is not beating
  // it, and this is the only line on the page that says so.
  //
  // The block carries the two books' summary figures, not their daily curves, so the
  // comparison is drawn as paired bars (portfolio against benchmark, one legend) rather than
  // as indexed lines; the Compare view draws indexed curves between two portfolios.
  import { t } from '$lib/i18n';
  import { fmtPct as fmtPlainPct, fmtSignedPct, fmtFixed } from '$lib/format.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import Bars from '$lib/ui/charts/Bars.svelte';
  import { na, fmtPct } from '../api.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import Panel from './Panel.svelte';

  let { block = null, onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const num = (v, digits = 2) => (v == null ? na() : fmtFixed(v, digits));
  const pp = (v) => (v == null ? na() : fmtPlainPct(v));
  const edge = $derived(d?.risk_adjusted_edge_pct ?? null);
  const benchName = $derived(d?.symbol ?? $t('portfolios.analytics.bench.benchmark'));
  const ROWS = [
    { key: 'total_pct', label: 'portfolios.analytics.bench.total' },
    { key: 'annualized_pct', label: 'portfolios.analytics.bench.annualized' },
    { key: 'volatility_pct', label: 'portfolios.analytics.bench.volatility' },
    { key: 'max_drawdown_pct', label: 'portfolios.analytics.bench.maxDrawdown' }
  ];
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="pa-stack">
    <section class="verdict">
      <span class="eyebrow">{$t('portfolios.analytics.bench.verdict')}</span>
      <div class="line">
        <span class="big"><Delta value={edge} size="md" empty={na()} /></span>
        <p>
          {$t(edge != null && edge >= 0 ? 'portfolios.analytics.bench.worthIt' : 'portfolios.analytics.bench.notWorthIt', {
            symbol: d.symbol ?? '',
            matched: pp(d.risk_matched_pct),
            actual: pp(d.portfolio.annualized_pct)
          })}
        </p>
      </div>
    </section>

    <div class="pa-cols">
      <Panel title={$t('portfolios.analytics.bench.vsTitle', { symbol: benchName })}>
        <Bars
          horizontal
          stacked={false}
          labels="ends"
          categories={ROWS.map((r) => ({ key: r.key, label: $t(r.label) }))}
          series={[
            { key: 'p', label: $t('portfolios.analytics.bench.portfolio'), values: ROWS.map((r) => d.portfolio[r.key]) },
            { key: 'b', label: benchName, values: ROWS.map((r) => d.benchmark[r.key]) }
          ]}
          rowHeight={44}
          format={(v) => fmtPlainPct(v)}
          sensitive={false}
        />
      </Panel>
      <Panel title={$t('portfolios.analytics.bench.detail')} flush>
        <div class="pa-scroll">
          <table class="pa-table">
            <thead>
              <tr>
                <th></th>
                <th class="r">{$t('portfolios.analytics.bench.portfolio')}</th>
                <th class="r">{benchName}</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>{$t('portfolios.analytics.bench.total')}</td>
                <td class="r">{fmtPct(d.portfolio.total_pct)}</td>
                <td class="r">{fmtPct(d.benchmark.total_pct)}</td>
              </tr>
              <tr>
                <td>{$t('portfolios.analytics.bench.annualized')}</td>
                <td class="r">{fmtPct(d.portfolio.annualized_pct)}</td>
                <td class="r">{fmtPct(d.benchmark.annualized_pct)}</td>
              </tr>
              <tr>
                <td>{$t('portfolios.analytics.bench.volatility')}</td>
                <td class="r">{pp(d.portfolio.volatility_pct)}</td>
                <td class="r">{pp(d.benchmark.volatility_pct)}</td>
              </tr>
              <tr>
                <td>{$t('portfolios.analytics.bench.maxDrawdown')}</td>
                <td class="r">{pp(d.portfolio.max_drawdown_pct)}</td>
                <td class="r">{pp(d.benchmark.max_drawdown_pct)}</td>
              </tr>
              <tr>
                <td>{$t('portfolios.analytics.bench.sharpe')}</td>
                <td class="r">{num(d.portfolio.sharpe)}</td>
                <td class="r">{num(d.benchmark.sharpe)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </Panel>
    </div>

    <div class="pa-stats">
      <StatCard label={$t('portfolios.analytics.bench.alpha')} value={d.alpha_pct == null ? na() : fmtSignedPct(d.alpha_pct)} />
      <StatCard label={$t('portfolios.analytics.bench.beta')} value={num(d.beta)} />
      <StatCard label="R²" value={num(d.r_squared)} />
      <StatCard label={$t('portfolios.analytics.bench.tracking')} value={pp(d.tracking_error_pct)} />
      <StatCard label={$t('portfolios.analytics.bench.info')} value={num(d.information_ratio)} />
      <StatCard label={$t('portfolios.analytics.bench.upCapture')} value={pp(d.up_capture_pct)} />
      <StatCard label={$t('portfolios.analytics.bench.downCapture')} value={pp(d.down_capture_pct)} />
    </div>

    <Sample sample={block.sample} />
  </div>
{/if}

<style>
  .verdict {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
  }
  .eyebrow {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .line {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
  }
  /* The verdict is the panel's one headline figure. */
  .verdict .big :global(.delta) {
    font-size: 28px;
    font-family: var(--font);
    font-weight: 600;
  }
  .line p {
    margin: 0;
    flex: 1 1 320px;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.5;
  }
</style>
