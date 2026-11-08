<script>
  // `risk`: volatility, drawdown, and return per unit of each.
  //
  // Everything here reads the deposit-adjusted curve. On raw market value a monthly savings
  // plan posts a heroic Sharpe and a drawdown of zero, because every contribution reads as
  // a gain.
  import { t } from '$lib/i18n';
  import { fmtPct as fmtPlainPct, fmtSignedPct, fmtFixed } from '$lib/format.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import TimeSeries from '$lib/ui/charts/TimeSeries.svelte';
  import { na, fmtDate } from '../api.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import Panel from './Panel.svelte';

  let { block = null, onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const c = $derived(chartColors());
  const ratio = (v) => (v == null ? na() : fmtFixed(v, 2));
  const pp = (v) => (v == null ? na() : fmtPlainPct(v));
  // Drawdown from the running peak, in percent: 0 at a high, negative below it.
  const dd = $derived((d?.drawdown_curve ?? []).map((p) => [p.ts, p.dd * 100]));
  const open = $derived(d?.episodes?.find((e) => e.recovered_at == null) ?? null);
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="pa-stack">
    <div class="pa-stats">
      <StatCard label={$t('portfolios.analytics.risk.volatility')} value={pp(d.volatility_pct)} />
      <StatCard label={$t('portfolios.analytics.risk.maxDrawdown')} value={pp(d.max_drawdown_pct)} />
      <StatCard
        label={$t('portfolios.analytics.risk.sharpe')}
        value={ratio(d.sharpe)}
        hint={$t('portfolios.analytics.risk.rf', { pct: pp(d.risk_free_pct) })}
      />
      <StatCard label={$t('portfolios.analytics.risk.sortino')} value={ratio(d.sortino)} />
      <StatCard label={$t('portfolios.analytics.risk.calmar')} value={ratio(d.calmar)} />
      <StatCard
        label={$t('portfolios.analytics.risk.ppy')}
        value={d.periods_per_year == null ? na() : fmtFixed(d.periods_per_year, 0)}
        hint={$t('portfolios.analytics.risk.ppyHint')}
      />
    </div>

    <!-- A drawdown is never good news: drawn in red from a 0 baseline, whatever it ends at. -->
    <Panel
      title={$t('portfolios.analytics.risk.underwater')}
      subtitle={$t('portfolios.analytics.risk.underwaterSub')}
      height={200}
      unavailable={dd.length < 2 ? { reason: $t('portfolios.analytics.missing.too_short'), icon: 'trending-up' } : null}
    >
      <TimeSeries
        series={[{ key: 'dd', label: $t('portfolios.analytics.risk.maxDrawdown'), points: dd, color: c.red }]}
        baseline={0}
        area
        change="none"
        height={200}
        format={(v) => fmtPlainPct(v)}
        axisFormat={(v) => fmtPlainPct(v, 0)}
        sensitive={false}
      />
    </Panel>

    <div class="pa-cols">
      <Panel title={$t('portfolios.analytics.risk.periods')} subtitle={$t('portfolios.analytics.risk.positiveDays', { pct: pp(d.positive_days_pct) })} flush>
        <div class="pa-scroll">
          <table class="pa-table">
            <thead>
              <tr>
                <th></th>
                <th class="r">{$t('portfolios.detail.best')}</th>
                <th class="r">{$t('portfolios.detail.worst')}</th>
              </tr>
            </thead>
            <tbody>
              {#each ['month', 'quarter', 'year'] as g (g)}
                {@const p = d.periods[g]}
                <tr>
                  <td>{$t(`portfolios.analytics.risk.grain.${g}`)}</td>
                  <!-- Coloured by the sign, not by the column: in a losing year the best
                       month is still a loss, and painting it green says the opposite. -->
                  <td class="r" class:pa-up={p.best?.pct > 0} class:pa-down={p.best?.pct < 0}>
                    {#if p.best}<span class="pa-muted when">{p.best.label}</span> {fmtSignedPct(p.best.pct)}{:else}<span class="pa-muted">{na()}</span>{/if}
                  </td>
                  <td class="r" class:pa-up={p.worst?.pct > 0} class:pa-down={p.worst?.pct < 0}>
                    {#if p.worst}<span class="pa-muted when">{p.worst.label}</span> {fmtSignedPct(p.worst.pct)}{:else}<span class="pa-muted">{na()}</span>{/if}
                  </td>
                </tr>
              {/each}
              <tr>
                <td>{$t('portfolios.analytics.risk.grain.day')}</td>
                <td class="r" class:pa-up={d.best_day_pct > 0} class:pa-down={d.best_day_pct < 0}>
                  {d.best_day_pct == null ? na() : fmtSignedPct(d.best_day_pct)}
                </td>
                <td class="r" class:pa-up={d.worst_day_pct > 0} class:pa-down={d.worst_day_pct < 0}>
                  {d.worst_day_pct == null ? na() : fmtSignedPct(d.worst_day_pct)}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </Panel>
      <Panel
        title={$t('portfolios.analytics.risk.recovery')}
        subtitle={open ? $t('portfolios.analytics.risk.stillDown', { pct: pp(open.depth * 100), days: open.under_water }) : ''}
        flush
      >
        {#if d.episodes.length}
          <div class="pa-scroll">
            <table class="pa-table">
              <thead>
                <tr>
                  <th class="r">{$t('portfolios.analytics.risk.depth')}</th>
                  <th>{$t('portfolios.analytics.risk.peak')}</th>
                  <th>{$t('portfolios.analytics.risk.trough')}</th>
                  <th class="r">{$t('portfolios.analytics.risk.toRecover')}</th>
                </tr>
              </thead>
              <tbody>
                {#each d.episodes.slice(0, 8) as e (e.peak_at + e.trough_at)}
                  <tr>
                    <td class="r pa-down">{fmtSignedPct(-Math.abs(e.depth * 100))}</td>
                    <td>{fmtDate(e.peak_at)}</td>
                    <td>{fmtDate(e.trough_at)}</td>
                    <td class="r">
                      {#if e.to_recover == null}
                        <span class="ongoing">{$t('portfolios.analytics.risk.ongoing')}</span>
                      {:else}
                        {$t('portfolios.analytics.risk.days', { n: e.to_recover })}
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <p class="pa-note pad">{$t('portfolios.analytics.risk.noEpisodes')}</p>
        {/if}
      </Panel>
    </div>

    <Sample sample={block.sample} />
  </div>
{/if}

<style>
  .when {
    margin-right: var(--space-2);
    font-family: var(--font);
  }
  .ongoing {
    color: var(--amber-ink);
  }
  .pad {
    padding: 0 var(--space-4) var(--space-3);
  }
</style>
