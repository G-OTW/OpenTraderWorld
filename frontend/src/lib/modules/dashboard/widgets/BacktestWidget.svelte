<script>
  // Backtest widgets — the cards of the Backtest mockup, each a `variant`. Reads the
  // module's saved runs and its strategy library; nothing is recomputed here, the stats
  // are the ones the run was recorded with.
  //
  // Config: { variant, metric, limit }.
  import { backtestApi } from '$lib/modules/backtest/api.js';
  import { fmtSignedPct, fmtPct, fmtFixed, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import RankBars from './parts/RankBars.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'best');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 5)));
  // Which number decides "best": the mockup's Return / Sharpe / Profit Factor toggle.
  const metric = $derived(item.config?.metric ?? 'return');

  const needsRuns = $derived(['best', 'leaderboard'].includes(variant));
  const needsStrategies = $derived(['library', 'favorites'].includes(variant));

  let runs = $state(null);
  let strategies = $state(null);
  let err = $state('');

  $effect(() => {
    if (editing || !needsRuns) return;
    let alive = true;
    backtestApi.runs('saved')
      .then((r) => { if (alive) runs = r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });
  $effect(() => {
    if (editing || !needsStrategies) return;
    let alive = true;
    backtestApi.strategies()
      .then((s) => { if (alive) strategies = s; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // A run's stats are stored per scope; `all` is the whole book, which is what a
  // headline figure means. Return and Sharpe are measured on the equity curve, not on the
  // trade list, so the engine reports them once at the top level and the scope carries
  // neither. Read the scope first, fall back to the run's own headline.
  const statsOf = (r) => r.stats?.all ?? r.stats ?? {};
  const num = (v) => (Number.isFinite(v) ? v : undefined);
  const statOf = (r, key) => num(statsOf(r)[key]) ?? num(r.stats?.[key]);
  const metricOf = (r) => {
    if (metric === 'sharpe') return statOf(r, 'sharpe');
    if (metric === 'pf') return statOf(r, 'profit_factor');
    return statOf(r, 'return_pct');
  };

  const ranked = $derived(
    [...(runs ?? [])]
      .filter((r) => Number.isFinite(metricOf(r)))
      .sort((a, b) => metricOf(b) - metricOf(a))
  );
  const best = $derived(ranked[0] ?? null);
  const bestStats = $derived(
    best ? { ...best.stats, ...statsOf(best), sharpe: statOf(best, 'sharpe'), return_pct: statOf(best, 'return_pct') } : {}
  );

  const leaderboard = $derived(
    [...(runs ?? [])]
      .filter((r) => Number.isFinite(statsOf(r).win_rate))
      .sort((a, b) => (statsOf(b).win_rate ?? 0) - (statsOf(a).win_rate ?? 0))
      .slice(0, limit)
  );

  // Most-used tags across the library: the mockup's tag bars.
  const tags = $derived.by(() => {
    const counts = new Map();
    for (const s of strategies ?? []) for (const tg of s.tags ?? []) counts.set(tg, (counts.get(tg) ?? 0) + 1);
    return [...counts]
      .map(([label, value]) => ({ label, value, display: String(value) }))
      .sort((a, b) => b.value - a.value)
      .slice(0, limit);
  });
</script>

<WidgetState
  {editing}
  error={err}
  loading={(needsRuns && runs === null) || (needsStrategies && strategies === null)}
  empty={(needsRuns && (runs ?? []).length === 0) || (needsStrategies && (strategies ?? []).length === 0)}
  preview={$t('dashboard.widgets.backtest.preview')}
  emptyText={$t('dashboard.widgets.backtest.empty')}
  rows={4}
>
  {#if variant === 'best'}
    {#if best}
      <div class="w-body">
        <div class="besthead">
          <Stat
            value={metric === 'return'
              ? fmtSignedPct(bestStats.return_pct, 1)
              : metric === 'sharpe'
                ? fmtFixed(bestStats.sharpe, 2)
                : fmtFixed(bestStats.profit_factor, 2)}
            tone={(metricOf(best) ?? 0) >= 0 ? 'pos' : 'neg'}
            note={metric === 'return'
              ? $t('dashboard.widgets.backtest.totalReturn')
              : metric === 'sharpe'
                ? $t('dashboard.widgets.backtest.sharpe')
                : $t('dashboard.widgets.backtest.profitFactor')}
          />
          <div class="ident">
            <span class="w-name runname">{best.name}</span>
            <span class="w-sub">{best.ticker} · {best.timeframe}</span>
          </div>
        </div>
        <ul class="trio">
          <li><span class="fig">{fmtFixed(bestStats.sharpe, 2)}</span><span class="w-sub">{$t('dashboard.widgets.backtest.sharpe')}</span></li>
          <li><span class="fig">{fmtFixed(bestStats.profit_factor, 2)}</span><span class="w-sub">{$t('dashboard.widgets.backtest.profitFactor')}</span></li>
          <li><span class="fig">{fmtPct(bestStats.win_rate, 0)}</span><span class="w-sub">{$t('dashboard.widgets.backtest.winRate')}</span></li>
        </ul>
      </div>
    {/if}
  {:else if variant === 'leaderboard'}
    <table class="w-tbl">
      <thead>
        <tr>
          <th>{$t('dashboard.widgets.backtest.strategy')}</th>
          <th class="num">{$t('dashboard.widgets.backtest.winRate')}</th>
          <th class="num">{$t('dashboard.widgets.backtest.profitFactor')}</th>
        </tr>
      </thead>
      <tbody>
        {#each leaderboard as r (r.id)}
          <tr>
            <td class="w-name">{r.name}</td>
            <td class="num w-pos">{fmtPct(statsOf(r).win_rate, 0)}</td>
            <td class="num">{fmtFixed(statsOf(r).profit_factor, 2)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else if variant === 'library'}
    <div class="w-body">
      <Stat value={String((strategies ?? []).length)} note={$t('dashboard.widgets.backtest.strategies')} />
      {#if tags.length}
        <span class="w-eyebrow">{$t('dashboard.widgets.backtest.mostUsedTags')}</span>
        <RankBars rows={tags} />
      {/if}
    </div>
  {:else if variant === 'favorites'}
    <ul class="w-list">
      {#each (strategies ?? []).slice(0, limit) as s (s.id)}
        <li>
          <a class="w-row stack" href="/backtest">
            <span class="w-name runname">{s.name}</span>
            <span class="w-sub">
              {#if s.tags?.length}{s.tags.join(' · ')} · {/if}{$ago(s.updated_at)}
            </span>
          </a>
        </li>
      {/each}
    </ul>
  {/if}
</WidgetState>

<style>
  .besthead {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    min-width: 0;
  }
  .ident {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 1px;
    min-width: 0;
    text-align: right;
  }
  .runname {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  /* Three supporting figures under the headline, split by hairlines. */
  .trio {
    display: flex;
    list-style: none;
    margin: 0;
    padding: var(--space-3) 0 0;
    border-top: var(--hairline) solid var(--border);
  }
  .trio li {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .trio li + li {
    padding-left: var(--space-3);
    border-left: var(--hairline) solid var(--border);
  }
  .fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 17px;
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  a:hover .runname {
    color: var(--accent);
  }
</style>
