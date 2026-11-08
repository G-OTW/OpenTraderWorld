<script>
  // `performance`: TWR and IRR per window, and the equity curve behind them.
  //
  // Both numbers are shown because neither is the other: TWR is what the investments did,
  // IRR is what the investor got. A window shorter than its own label says so instead of
  // annualizing eight months into a three-year figure.
  import { t } from '$lib/i18n';
  import { fmtSignedPct } from '$lib/format.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import TimeSeries from '$lib/ui/charts/TimeSeries.svelte';
  import Bars from '$lib/ui/charts/Bars.svelte';
  import { fmtMoney, fmtPct, na } from '../api.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import Panel from './Panel.svelte';

  let { block = null, currency = 'USD', onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const pct = (v) => fmtPct(v);
  // The equity index (1.0 at the window's start) as a return in percent, so the baseline
  // is 0 and the curve is toned by sign.
  const curve = $derived(
    (d?.series?.dates ?? []).map((ts, i) => [ts, d.series.equity[i] == null ? null : (d.series.equity[i] - 1) * 100])
  );
  // Contribution: each holding's linked part of the window's return, ranked, with the
  // cash row.
  const contrib = $derived(d?.contributions?.status === 'ok' ? d.contributions.data : null);
  const ranked = $derived(contrib ? [...contrib.rows].sort((a, b) => b.contribution_pct - a.contribution_pct) : []);
  const rowLabel = (r) => (r.label === 'cash' ? $t('portfolios.contrib.cash') : r.label);
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="pa-stack">
    <div class="pa-stats">
      <StatCard label={$t('portfolios.analytics.perf.twr')} value={pct(d.current.twr_pct)} hint={$t('portfolios.analytics.perf.twrHint')} />
      <StatCard
        label={$t('portfolios.analytics.perf.annualized')}
        value={pct(d.current.annualized_pct)}
        hint={$t('portfolios.analytics.perf.annualizedHint')}
      />
      <StatCard label={$t('portfolios.analytics.perf.irr')} value={pct(d.current.irr_pct)} hint={$t('portfolios.analytics.perf.irrHint')} />
      <StatCard
        label={$t('portfolios.analytics.perf.netFlow')}
        value={fmtMoney(d.current.net_flow, currency)}
        hint={$t('portfolios.analytics.perf.netFlowHint')}
      />
    </div>

    <Panel
      title={$t('portfolios.analytics.perf.curveTitle')}
      subtitle={$t('portfolios.analytics.perf.twrHint')}
      height={240}
      unavailable={curve.length < 2 ? { reason: $t('portfolios.analytics.missing.too_short'), icon: 'trending-up' } : null}
    >
      <TimeSeries
        series={[{ key: 'twr', label: $t('portfolios.analytics.perf.twr'), points: curve, tone: 'sign' }]}
        baseline={0}
        area
        change="none"
        height={240}
        format={(v) => fmtSignedPct(v)}
        axisFormat={(v) => fmtSignedPct(v, 0)}
        sensitive={false}
      />
    </Panel>

    <Panel
      title={$t('portfolios.contrib.title')}
      subtitle={contrib
        ? $t('portfolios.contrib.note', { total: fmtSignedPct(contrib.total_pct), days: contrib.days, skipped: contrib.skipped })
        : ''}
      height={contrib ? null : 160}
      unavailable={contrib
        ? null
        : {
            reason:
              $t('portfolios.contrib.unavailable') +
              (d?.contributions?.uncovered?.length ? ` ${$t('portfolios.contrib.uncovered', { list: d.contributions.uncovered.join(', ') })}` : ''),
            icon: 'bar-chart'
          }}
    >
      <Bars
        horizontal
        labels="ends"
        categories={ranked.map((r) => ({ key: r.label, label: rowLabel(r) }))}
        series={[{ key: 'contrib', label: $t('portfolios.contrib.title'), values: ranked.map((r) => r.contribution_pct), tone: 'sign' }]}
        format={(v) => fmtSignedPct(v)}
        sensitive={false}
        legend="none"
      />
      <details class="more">
        <summary>{$t('portfolios.contrib.details')}</summary>
        <div class="pa-scroll">
          <table class="pa-table">
            <thead>
              <tr>
                <th>{$t('portfolios.detail.colSymbol')}</th>
                <th class="r">{$t('portfolios.contrib.weight')}</th>
                <th class="r">{$t('portfolios.analytics.perf.twr')}</th>
                <th class="r">{$t('portfolios.analytics.risk.maxDrawdown')}</th>
                <th class="r">{$t('portfolios.contrib.title')}</th>
              </tr>
            </thead>
            <tbody>
              {#each ranked as r (r.label)}
                <tr>
                  <td>{rowLabel(r)}</td>
                  <td class="r pa-muted">{r.avg_weight_pct == null ? na() : `${r.avg_weight_pct.toFixed(1)}%`}</td>
                  <td class="r">{fmtPct(r.return_pct)}</td>
                  <td class="r">{fmtPct(r.drawdown_pct)}</td>
                  <td class="r" class:pa-up={r.contribution_pct > 0} class:pa-down={r.contribution_pct < 0}>{fmtPct(r.contribution_pct)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </details>
    </Panel>

    <Panel title={$t('portfolios.analytics.perf.byWindow')} flush>
      <div class="pa-scroll">
        <table class="pa-table">
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
                <td class="r" class:pa-up={w.twr_pct > 0} class:pa-down={w.twr_pct < 0}>{pct(w.twr_pct)}</td>
                <td class="r">{pct(w.annualized_pct)}</td>
                <td class="r">{pct(w.irr_pct)}</td>
                <!-- The honest half of the label: a "3y" row on a book eight months old is
                     eight months, and reporting it as three years is how a tracker prints a
                     number it has not earned. -->
                <td class="r pa-muted">{w.rows}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </Panel>

    <Sample sample={block.sample} />
  </div>
{/if}

<style>
  tr.thin td {
    opacity: 0.55;
  }
  .more {
    margin-top: var(--space-3);
  }
  .more summary {
    cursor: pointer;
    font-size: 12px;
    color: var(--dim);
  }
  .more summary:hover {
    color: var(--text);
  }
  .more table {
    margin-top: var(--space-2);
  }
</style>
