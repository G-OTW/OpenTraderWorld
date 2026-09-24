<script>
  // `performance` — TWR and IRR per window, and the equity curve behind them.
  //
  // Both numbers are shown because neither is the other: TWR is what the investments did,
  // IRR is what the investor got. A window shorter than its own label says so instead of
  // annualizing eight months into a three-year figure.
  import { t } from '$lib/i18n';
  import { fmtMoney, fmtSignedPct, EM_DASH } from '$lib/format.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import LineChart from './LineChart.svelte';

  let { block = null, currency = 'USD', onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const pct = (v) => (v == null ? EM_DASH : fmtSignedPct(v));
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="stats">
    <StatCard
      label={$t('portfolios.analytics.perf.twr')}
      value={pct(d.current.twr_pct)}
      hint={$t('portfolios.analytics.perf.twrHint')}
    />
    <StatCard
      label={$t('portfolios.analytics.perf.annualized')}
      value={pct(d.current.annualized_pct)}
      hint={$t('portfolios.analytics.perf.annualizedHint')}
    />
    <StatCard
      label={$t('portfolios.analytics.perf.irr')}
      value={pct(d.current.irr_pct)}
      hint={$t('portfolios.analytics.perf.irrHint')}
    />
    <StatCard
      label={$t('portfolios.analytics.perf.netFlow')}
      value={fmtMoney(d.current.net_flow, currency)}
      hint={$t('portfolios.analytics.perf.netFlowHint')}
    />
  </div>

  <div class="panel">
    <LineChart
      values={d.series.equity}
      labels={d.series.dates}
      baseline={1}
      height={200}
      format={(v) => fmtSignedPct((v - 1) * 100)}
    />
  </div>

  <table class="tbl grid">
    <thead>
      <tr>
        <th>{$t('portfolios.analytics.perf.window')}</th>
        <th class="r">{$t('portfolios.analytics.perf.twr')}</th>
        <th class="r">{$t('portfolios.analytics.perf.annualized')}</th>
        <th class="r">{$t('portfolios.analytics.perf.irr')}</th>
        <th class="r">{$t('portfolios.analytics.perf.days')}</th>
      </tr>
    </thead>
    <tbody>
      {#each d.windows as w (w.window)}
        <tr class:thin={!w.covered}>
          <td>{$t(`portfolios.analytics.window.${w.window}`)}</td>
          <td class="r num" class:up={w.twr_pct > 0} class:down={w.twr_pct < 0}>{pct(w.twr_pct)}</td>
          <td class="r num">{pct(w.annualized_pct)}</td>
          <td class="r num">{pct(w.irr_pct)}</td>
          <!-- The honest half of the label: a "3y" row on a book eight months old is eight
               months, and reporting it as three years is how a tracker prints a number it
               has not earned. -->
          <td class="r num muted">{w.rows}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  <Sample sample={block.sample} />
{/if}

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--space-3);
  }
  .panel {
    margin-top: var(--space-4);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .grid {
    margin-top: var(--space-4);
  }
  .r {
    text-align: right;
  }
  .muted {
    color: var(--muted);
  }
  tr.thin td {
    opacity: 0.55;
  }
</style>
