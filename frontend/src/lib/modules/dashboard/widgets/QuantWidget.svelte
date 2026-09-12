<script>
  // Quant widgets — the cards of the Quant mockup, each a `variant`. Everything is the
  // module's own compute endpoints; the heavy ones (correlation, seasonality) only run
  // when the widget is configured with the datasets they need, so an unconfigured card
  // costs nothing.
  //
  // The mockup's efficient-frontier scatter is not built here: it needs a two-axis plot
  // with a marked optimum, which belongs on the module page rather than in a dashboard
  // cell that can be three columns wide.
  //
  // Config: { variant, datasetId, datasetIds, metric }.
  import { quantApi } from '$lib/modules/quant/api.js';
  import { fmtPct, fmtFixed } from '$lib/format';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Buckets from './parts/Buckets.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'count');
  const datasetId = $derived(item.config?.datasetId || '');
  const datasetIds = $derived(item.config?.datasetIds ?? []);
  const metric = $derived(item.config?.metric ?? 'return');

  let datasets = $state(null);
  let runs = $state(null);
  let single = $state.raw(null);
  let basket = $state.raw(null);
  let season = $state.raw(null);
  let err = $state('');

  $effect(() => {
    if (editing || variant !== 'count') return;
    let alive = true;
    Promise.all([quantApi.datasets(), quantApi.backtestRuns()])
      .then(([d, r]) => { if (alive) { datasets = d; runs = r; } })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // The chosen dataset, else the first stored one, so the card is useful before it is
  // configured — and never guesses an instrument: it names the one it read.
  let firstId = $state('');
  $effect(() => {
    if (editing || variant === 'count' || datasetId) return;
    let alive = true;
    quantApi.datasets()
      .then((d) => { if (alive) { datasets = d; firstId = d[0]?.id ?? ''; } })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });
  const activeId = $derived(datasetId || firstId);

  $effect(() => {
    if (editing || variant !== 'risk' || !activeId) return;
    const id = activeId;
    let alive = true;
    quantApi.single(id)
      .then((r) => { if (alive) single = r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  $effect(() => {
    if (editing || variant !== 'correlation' || datasetIds.length < 2) return;
    const ids = datasetIds;
    let alive = true;
    quantApi.portfolio(ids, { samples: 1000 })
      .then((r) => { if (alive) basket = r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  $effect(() => {
    if (editing || variant !== 'seasonality' || !activeId) return;
    const id = activeId;
    const m = metric;
    let alive = true;
    quantApi.seasonality(id, { metric: m })
      .then((r) => { if (alive) season = r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const MONTHS = $derived.by(() =>
    Array.from({ length: 12 }, (_, i) =>
      new Date(2024, i, 1).toLocaleDateString(undefined, { month: 'short' })
    )
  );
  const WEEKDAYS = $derived.by(() =>
    Array.from({ length: 7 }, (_, i) =>
      new Date(2024, 0, 1 + i).toLocaleDateString(undefined, { weekday: 'short' })
    )
  );

  const matrix = $derived(basket?.correlation ?? []);
  const labels = $derived(basket?.labels ?? []);

  // Signed intensity: red for negative, green for positive, on the theme's own tokens.
  function shade(v, max = 1) {
    if (v == null || Number.isNaN(v)) return 'transparent';
    const mag = Math.min(1, Math.abs(v) / (max || 1));
    const hue = v < 0 ? 'var(--red)' : 'var(--green)';
    return `color-mix(in srgb, ${hue} ${(0.12 + 0.7 * mag) * 100}%, transparent)`;
  }
  const seasonMax = $derived(
    Math.max(
      0.0001,
      ...(season?.result?.month_weekday ?? []).flat().filter((v) => v != null).map(Math.abs)
    )
  );
</script>

<WidgetState
  {editing}
  error={err}
  loading={(variant === 'count' && datasets === null)
    || (variant === 'risk' && activeId && single === null)
    || (variant === 'correlation' && datasetIds.length >= 2 && basket === null)
    || (variant === 'seasonality' && activeId && season === null)}
  preview={$t('dashboard.widgets.quant.preview')}
  rows={4}
>
  {#if variant === 'count'}
    <Buckets
      align="left"
      cells={[
        { value: (datasets ?? []).length, label: $t('dashboard.widgets.quant.datasets') },
        { value: (runs ?? []).length, label: $t('dashboard.widgets.quant.backtestRuns') }
      ]}
    />
  {:else if variant === 'risk'}
    {#if !activeId}
      <p class="w-state">{$t('dashboard.widgets.quant.pickDataset')}</p>
    {:else if single}
      <div class="w-body">
        <span class="w-eyebrow">{single.ticker} · {single.timeframe}</span>
        <Buckets
          cells={[
            { value: fmtPct(single.result.hv_annual * 100, 1), label: $t('dashboard.widgets.quant.hv') },
            { value: fmtPct(-Math.abs(single.result.max_drawdown) * 100, 1), label: $t('dashboard.widgets.quant.maxDd'), tone: 'neg' },
            { value: fmtPct(-Math.abs(single.result.var_hist) * 100, 1), label: $t('dashboard.widgets.quant.var'), tone: 'neg' }
          ]}
        />
      </div>
    {/if}
  {:else if variant === 'correlation'}
    {#if datasetIds.length < 2}
      <p class="w-state">{$t('dashboard.widgets.quant.pickBasket')}</p>
    {:else if basket}
      <div class="grid" style:--cols={labels.length}>
        <span class="corner"></span>
        {#each labels as l (l)}<span class="head">{l}</span>{/each}
        {#each matrix as row, i (labels[i])}
          <span class="head row">{labels[i]}</span>
          {#each row as v, j (j)}
            <span class="cell" style:background={shade(v)}>{fmtFixed(v, 2)}</span>
          {/each}
        {/each}
      </div>
    {/if}
  {:else if variant === 'seasonality'}
    {#if !activeId}
      <p class="w-state">{$t('dashboard.widgets.quant.pickDataset')}</p>
    {:else if season}
      <div class="w-body">
        <span class="w-eyebrow">{season.ticker} · {season.result.metric}</span>
        <div class="grid season" style:--cols={7}>
          <span class="corner"></span>
          {#each WEEKDAYS as w (w)}<span class="head">{w}</span>{/each}
          {#each season.result.month_weekday as row, m (MONTHS[m])}
            <span class="head row">{MONTHS[m]}</span>
            {#each row as v, d (d)}
              <span class="cell" style:background={shade(v, seasonMax)}>
                {v == null ? '' : fmtFixed(v * 100, 1)}
              </span>
            {/each}
          {/each}
        </div>
      </div>
    {/if}
  {/if}
</WidgetState>

<style>
  /* A label column then one column per series; cells stay square-ish and scroll sideways
     rather than squeezing the figures out of legibility. */
  .grid {
    display: grid;
    grid-template-columns: auto repeat(var(--cols), minmax(34px, 1fr));
    gap: 2px;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: thin;
  }
  .head {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--dim);
    text-align: center;
    align-self: center;
    white-space: nowrap;
  }
  .head.row {
    text-align: right;
    padding-right: var(--space-1);
  }
  .cell {
    padding: 4px 2px;
    border-radius: 3px;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
    text-align: center;
    color: var(--text);
    white-space: nowrap;
  }
  .season .cell {
    font-size: 9.5px;
  }
</style>
