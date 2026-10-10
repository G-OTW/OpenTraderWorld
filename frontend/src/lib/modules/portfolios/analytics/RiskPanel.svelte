<script>
  // `risk` — volatility, drawdown, and return per unit of each.
  //
  // Everything here reads the deposit-adjusted curve. On raw market value a monthly savings
  // plan posts a heroic Sharpe and a drawdown of zero, because every contribution reads as
  // a gain.
  import { t } from '$lib/i18n';
  import { fmtPct, fmtSignedPct, fmtFixed, fmtDate, EM_DASH } from '$lib/format.js';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import LineChart from './LineChart.svelte';

  let { block = null, onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const ratio = (v) => (v == null ? EM_DASH : fmtFixed(v, 2));
  const dd = $derived(d?.drawdown_curve?.map((p) => p.dd * 100) ?? []);
  const ddLabels = $derived(d?.drawdown_curve?.map((p) => p.ts) ?? []);
  const open = $derived(d?.episodes?.find((e) => e.recovered_at == null) ?? null);
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="stats">
    <StatCard label={$t('portfolios.analytics.risk.volatility')} value={fmtPct(d.volatility_pct)} />
    <StatCard label={$t('portfolios.analytics.risk.maxDrawdown')} value={fmtPct(d.max_drawdown_pct)} />
    <StatCard
      label={$t('portfolios.analytics.risk.sharpe')}
      value={ratio(d.sharpe)}
      hint={$t('portfolios.analytics.risk.rf', { pct: fmtPct(d.risk_free_pct) })}
    />
    <StatCard label={$t('portfolios.analytics.risk.sortino')} value={ratio(d.sortino)} />
    <StatCard label={$t('portfolios.analytics.risk.calmar')} value={ratio(d.calmar)} />
    <StatCard
      label={$t('portfolios.analytics.risk.ppy')}
      value={fmtFixed(d.periods_per_year, 0)}
      hint={$t('portfolios.analytics.risk.ppyHint')}
    />
  </div>

  <div class="panel">
    <h4>{$t('portfolios.analytics.risk.underwater')}</h4>
    <LineChart values={dd} labels={ddLabels} baseline={0} tone="loss" height={140} format={(v) => fmtPct(v)} />
  </div>

  <div class="cols">
    <div class="panel">
      <h4>{$t('portfolios.analytics.risk.periods')}</h4>
      <table class="tbl">
        <tbody>
          {#each ['month', 'quarter', 'year'] as g (g)}
            {@const p = d.periods[g]}
            <tr>
              <td>{$t(`portfolios.analytics.risk.grain.${g}`)}</td>
              <!-- Coloured by the sign, not by the column: in a losing year the best
                   month is still a loss, and painting it green says the opposite. -->
              <td class="r num" class:up={p.best?.pct > 0} class:down={p.best?.pct < 0}>
                {#if p.best}{p.best.label} {fmtSignedPct(p.best.pct)}{:else}{EM_DASH}{/if}
              </td>
              <td class="r num" class:up={p.worst?.pct > 0} class:down={p.worst?.pct < 0}>
                {#if p.worst}{p.worst.label} {fmtSignedPct(p.worst.pct)}{:else}{EM_DASH}{/if}
              </td>
            </tr>
          {/each}
          <tr>
            <td>{$t('portfolios.analytics.risk.grain.day')}</td>
            <td class="r num" class:up={d.best_day_pct > 0} class:down={d.best_day_pct < 0}>
              {fmtSignedPct(d.best_day_pct)}
            </td>
            <td class="r num" class:up={d.worst_day_pct > 0} class:down={d.worst_day_pct < 0}>
              {fmtSignedPct(d.worst_day_pct)}
            </td>
          </tr>
        </tbody>
      </table>
      <p class="foot">{$t('portfolios.analytics.risk.positiveDays', { pct: fmtPct(d.positive_days_pct) })}</p>
    </div>

    <div class="panel">
      <h4>{$t('portfolios.analytics.risk.recovery')}</h4>
      {#if open}
        <!-- The one that has not finished is the one that matters today, so it is stated
             before the table rather than buried in it. -->
        <p class="open">
          {$t('portfolios.analytics.risk.stillDown', {
            pct: fmtPct(open.depth * 100),
            days: open.under_water
          })}
        </p>
      {/if}
      {#if d.episodes.length}
        <table class="tbl">
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
                <td class="r num down">{fmtPct(e.depth * 100)}</td>
                <td>{fmtDate(e.peak_at)}</td>
                <td>{fmtDate(e.trough_at)}</td>
                <td class="r num">
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
      {:else}
        <p class="foot">{$t('portfolios.analytics.risk.noEpisodes')}</p>
      {/if}
    </div>
  </div>

  <Sample sample={block.sample} />
{/if}

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: var(--space-3);
  }
  .panel {
    margin-top: var(--space-4);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: var(--space-3);
  }
  h4 {
    margin: 0 0 var(--space-2);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .r {
    text-align: right;
  }
  .foot,
  .open {
    margin: var(--space-2) 0 0;
    font-size: 12px;
    color: var(--muted);
  }
  .open {
    color: var(--amber);
    font-weight: 600;
  }
  .ongoing {
    color: var(--amber);
  }
</style>
