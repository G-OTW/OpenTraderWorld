<script>
  // One variant, read in full: the parameters that make it different, then the same statistics,
  // performance table and P&L curve a normal run gets.
  //
  // The grid keeps summary figures per variant, never trades, so this replays the variant
  // (record off, nothing enters the history) the moment a row is picked. "Keep this run" is the
  // deliberate second step that puts it in the run history, where the report, Monte Carlo and
  // pinning already live.
  import Icon from '$lib/ui/Icon.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import { backtestApi, fmtNum, toAnalysisTrades } from './api.js';
  import { patchSettings, displayValue } from './optimize.js';
  import StatsGrid from './StatsGrid.svelte';
  import PerfTable from './PerfTable.svelte';
  import OosBlock from './OosBlock.svelte';
  import TradeStats from '$lib/analysis/TradeStats.svelte';
  import TradeCurve from '$lib/analysis/TradeCurve.svelte';
  import { stats as closedTradeStats } from '$lib/analysis/trades.js';

  let {
    /** The picked ranking row: `{ i, rank, params, … }`. */
    row = null,
    axes = [],
    baseSettings = null,
    datasetIds = [],
    from = null,
    to = null
  } = $props();

  let result = $state(null);
  let loading = $state(false);
  let error = $state('');
  let kept = $state(false);
  let keeping = $state(false);

  const dayNames = $derived({
    all: $t('backtest.opt.everyDay'),
    short: ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'].map((k) => $t(`common.weekday.${k}`))
  });

  const variantSettings = $derived(
    row && baseSettings ? patchSettings(baseSettings, axes, row.params) : null
  );
  const trades = $derived(toAnalysisTrades(result?.trades ?? []));
  const closed = $derived(closedTradeStats(trades));

  // The replay keys on the trial index alone: the same row picked twice is the same simulation.
  // Deliberately not reactive: it guards the effect, it must not retrigger it.
  let loadedFor = null;
  $effect(() => {
    const key = row?.i;
    if (key == null || !variantSettings || !datasetIds.length) return;
    if (loadedFor === key) return;
    loadedFor = key;
    result = null;
    kept = false;
    error = '';
    loading = true;
    backtestApi
      .run(datasetIds, variantSettings, { record: false, from, to })
      .then((r) => (result = r))
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  /** Run it once more, recorded, so it lands in the run history under a generated name. */
  async function keep() {
    if (!variantSettings) return;
    keeping = true;
    try {
      await backtestApi.run(datasetIds, variantSettings, { record: true, from, to });
      kept = true;
    } catch (e) {
      error = e.message;
    } finally {
      keeping = false;
    }
  }
</script>

<div class="variant">
  <div class="head">
    <span class="rank">#{row?.rank ?? '–'}</span>
    <div class="params">
      {#each axes as a, k (k)}
        <span class="p">
          <span class="lbl">{a.label}</span>
          <b>{displayValue(a, row?.params?.[k], dayNames)}{a.unit ?? ''}</b>
        </span>
      {/each}
    </div>
    <button class="keep" disabled={!result || keeping || kept} onclick={keep}>
      <Icon name={kept ? 'check' : 'star'} size={13} />
      {kept ? $t('backtest.opt.kept') : $t('backtest.opt.keep')}
    </button>
  </div>

  {#if error}
    <p class="err">{error}</p>
  {:else if loading || !result}
    <div aria-busy="true"><Skeleton height="280px" /></div>
  {:else}
    <StatsGrid stats={result.stats} />
    {#if result.oos}<OosBlock oos={result.oos} />{/if}
    <PerfTable stats={result.stats} />
    <TradeStats s={closed} {trades} />
    <div class="curve"><TradeCurve {trades} /></div>
    <p class="note">{$t('backtest.opt.variantNote', { n: fmtNum(result.trades.length, 0) })}</p>
  {/if}
</div>

<style>
  .variant {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    padding-bottom: var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
  }
  .rank {
    font-size: 1.1rem;
    font-weight: var(--fw-medium);
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }
  .params {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
    flex: 1;
    min-width: 0;
  }
  .p {
    display: flex;
    flex-direction: column;
    font-size: var(--text-xs);
    color: var(--muted);
    min-width: 0;
  }
  .p .lbl {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 220px;
  }
  .p b {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .keep {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    white-space: nowrap;
  }
  .keep:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-2));
  }
  .keep:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .curve {
    height: 300px;
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
  }
  .note {
    font-size: var(--text-xs);
    color: var(--muted);
    font-style: italic;
  }
</style>
