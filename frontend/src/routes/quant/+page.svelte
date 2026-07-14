<script>
  // Quant Tools module. Three tabs:
  //  • Single Asset — pick one dataset → HV, Max DD, VaR, CVaR with drawdown + distribution charts.
  //  • Portfolio    — pick 2+ datasets → correlation heatmap, efficient frontier, risk parity.
  //  • Kelly        — manual win-rate/payoff calculator.
  // Datasets come from the Historical Data catalog; the module needs it installed with data.
  import { quantApi, fmtRatioPct, dsLabel, CONFIDENCE_LEVELS } from '$lib/modules/quant/api.js';
  import DatasetPicker from '$lib/modules/quant/DatasetPicker.svelte';
  import MetricCards from '$lib/modules/quant/MetricCards.svelte';
  import SingleCharts from '$lib/modules/quant/SingleCharts.svelte';
  import CorrelationHeatmap from '$lib/modules/quant/CorrelationHeatmap.svelte';
  import FrontierChart from '$lib/modules/quant/FrontierChart.svelte';
  import WeightsBars from '$lib/modules/quant/WeightsBars.svelte';
  import KellyPanel from '$lib/modules/quant/KellyPanel.svelte';
  import PositionSizePanel from '$lib/modules/quant/PositionSizePanel.svelte';
  import SeasonalityPanel from '$lib/modules/quant/SeasonalityPanel.svelte';
  import MonteCarloPanel from '$lib/modules/quant/MonteCarloPanel.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { fmtNum } from '$lib/format.js';

  // Remember the active tab across refreshes.
  const TAB_KEY = 'quant.tab';
  const initialTab = (() => {
    try {
      const t = localStorage.getItem(TAB_KEY);
      return ['single', 'portfolio', 'seasonality', 'montecarlo', 'size', 'kelly'].includes(t) ? t : 'single';
    } catch {
      return 'single';
    }
  })();
  let tab = $state(initialTab);
  $effect(() => {
    try {
      localStorage.setItem(TAB_KEY, tab);
    } catch {
      /* non-fatal */
    }
  });
  let datasets = $state([]);
  let error = $state('');

  // Single
  let singleId = $state(null);
  let confidence = $state(0.95); // fraction (0.95 = 95%)
  // Custom confidence as a whole percent (1–99); empty until the user types one.
  let confPct = $state('');
  let single = $state(null); // { ticker, timeframe, result }
  let singleBusy = $state(false);

  // Time-range window (inclusive) over the selected dataset's available span. `range` holds
  // [startDay, endDay] as day offsets from the dataset's first day; the slider drives them.
  const DAY = 86400000;
  const singleDs = $derived(datasets.find((d) => d.id === singleId) ?? null);
  const dsBounds = $derived.by(() => {
    if (!singleDs?.range_from || !singleDs?.range_to) return null;
    const startOfDay = (s) => {
      const d = new Date(s);
      d.setHours(0, 0, 0, 0);
      return d;
    };
    const from = startOfDay(singleDs.range_from);
    const to = startOfDay(singleDs.range_to);
    const days = Math.max(0, Math.round((to.getTime() - from.getTime()) / DAY));
    return { from, days };
  });
  let range = $state(null); // [startDay, endDay] or null when full/unset

  // Reset the window to full whenever the selected dataset changes.
  $effect(() => {
    void singleId;
    range = null;
  });

  // Deliberately UTC, not dateKey(): these are dataset bar dates, anchored to the exchange
  // calendar rather than to the viewer's timezone.
  const dayToISO = (d) => {
    if (!dsBounds) return null;
    const dt = new Date(dsBounds.from.getTime() + d * DAY);
    return dt.toISOString().slice(0, 10);
  };
  const rangeStart = $derived(range ? range[0] : 0);
  const rangeEnd = $derived(range ? range[1] : dsBounds?.days ?? 0);
  const isFull = $derived(!range || (rangeStart === 0 && rangeEnd === (dsBounds?.days ?? 0)));

  function setStart(v) {
    const s = Math.min(Number(v), rangeEnd);
    range = [s, rangeEnd];
  }
  function setEnd(v) {
    const e = Math.max(Number(v), rangeStart);
    range = [rangeStart, e];
  }
  function resetRange() {
    range = null;
  }

  // Portfolio
  let selected = $state([]); // dataset ids
  let portSearch = $state('');
  let port = $state(null);
  let portBusy = $state(false);
  let pickedWeights = $state(null); // { label, weights } from clicking a frontier point

  // Seasonality
  let seasonId = $state(null);
  let seasonMetric = $state('return'); // 'return' | 'volatility'
  let season = $state(null); // { ticker, timeframe, result }
  let seasonBusy = $state(false);

  async function runSeasonality() {
    if (!seasonId) return;
    error = '';
    seasonBusy = true;
    try {
      season = await quantApi.seasonality(seasonId, { metric: seasonMetric });
    } catch (e) {
      error = e.message;
      season = null;
    } finally {
      seasonBusy = false;
    }
  }

  const portFiltered = $derived.by(() => {
    const q = portSearch.trim().toLowerCase();
    if (!q) return datasets;
    return datasets.filter((d) =>
      `${d.ticker} ${d.timeframe} ${d.provider ?? ''}`.toLowerCase().includes(q)
    );
  });

  $effect(() => {
    quantApi
      .datasets()
      .then((d) => (datasets = d))
      .catch((e) => (error = e.message));
  });

  // Confidence presets + custom whole-percent input.
  function setConfPreset(frac) {
    confidence = frac;
    confPct = '';
  }
  function onConfInput(e) {
    confPct = e.target.value;
    const n = Math.round(Number(confPct));
    if (Number.isFinite(n) && n >= 1 && n <= 99) confidence = n / 100;
  }
  const confActive = (frac) => !confPct && Math.abs(confidence - frac) < 1e-9;

  async function runSingle() {
    if (!singleId) return;
    error = '';
    singleBusy = true;
    // Translate the day window into an inclusive [from, until] on RFC3339 boundaries; a full
    // window sends nulls so the backend reads the whole dataset.
    let win = {};
    if (!isFull && dsBounds) {
      const fromISO = dayToISO(rangeStart);
      const untilISO = dayToISO(rangeEnd);
      win = { from: `${fromISO}T00:00:00Z`, until: `${untilISO}T23:59:59Z` };
    }
    try {
      single = await quantApi.single(singleId, confidence, win);
    } catch (e) {
      error = e.message;
      single = null;
    } finally {
      singleBusy = false;
    }
  }

  function toggle(id) {
    selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
  }

  async function runPortfolio() {
    if (selected.length < 2) return;
    error = '';
    portBusy = true;
    pickedWeights = null;
    try {
      port = await quantApi.portfolio(selected);
    } catch (e) {
      error = e.message;
      port = null;
    } finally {
      portBusy = false;
    }
  }

  function pickFrontier(which) {
    if (!port) return;
    const p = port.frontier[which];
    pickedWeights = {
      label: which === 'max_sharpe' ? $t('quant.page.maxSharpePortfolio') : $t('quant.page.minVolatilityPortfolio'),
      weights: p.weights,
      ret: p.ret,
      vol: p.vol,
      sharpe: p.sharpe
    };
  }
</script>

<RequireModule module="quant">
<div class="page">
  <header>
    <h1>{$t('quant.page.title')}</h1>
    <nav class="tabs">
      <button class:active={tab === 'single'} onclick={() => (tab = 'single')}>{$t('quant.page.tabSingleAsset')}</button>
      <button class:active={tab === 'portfolio'} onclick={() => (tab = 'portfolio')}>{$t('quant.page.tabPortfolio')}</button>
      <button class:active={tab === 'seasonality'} onclick={() => (tab = 'seasonality')}>{$t('quant.page.tabSeasonality')}</button>
      <button class:active={tab === 'montecarlo'} onclick={() => (tab = 'montecarlo')}>{$t('quant.page.tabMonteCarlo')}</button>
      <button class:active={tab === 'size'} onclick={() => (tab = 'size')}>{$t('quant.page.tabPositionSize')}</button>
      <button class:active={tab === 'kelly'} onclick={() => (tab = 'kelly')}>{$t('quant.page.tabKelly')}</button>
    </nav>
  </header>

  <ErrorText error={error} copyable />

  {#if !datasets.length && tab !== 'kelly' && tab !== 'size' && tab !== 'montecarlo'}
    <p class="hint">
      {@html $t('quant.page.noDatasetsHint')}
    </p>
  {/if}

  <!-- ── Single asset ─────────────────────────────────────────── -->
  {#if tab === 'single'}
    <div class="controls">
      <div class="ctrl">
        <span class="ctrl-label">{$t('quant.page.dataset')}</span>
        <DatasetPicker bind:value={singleId} {datasets} />
      </div>

      <div class="ctrl grow">
        <div class="ctrl-label-row">
          <span class="ctrl-label">{$t('quant.page.timeRange')}</span>
          <button class="full-btn" onclick={resetRange} disabled={isFull} title={$t('quant.page.useFullDatasetTitle')}>
            {$t('quant.page.full')}
          </button>
        </div>
        {#if singleDs && dsBounds && dsBounds.days > 0}
          <div class="slider">
            <input
              type="range"
              min="0"
              max={dsBounds.days}
              value={rangeStart}
              oninput={(e) => setStart(e.target.value)}
            />
            <input
              type="range"
              min="0"
              max={dsBounds.days}
              value={rangeEnd}
              oninput={(e) => setEnd(e.target.value)}
            />
          </div>
          <div class="range-labels">
            <span>{dayToISO(rangeStart)}</span>
            <span class="muted">→</span>
            <span>{dayToISO(rangeEnd)}</span>
          </div>
        {:else}
          <span class="muted small">{$t('quant.page.pickDatasetForRange')}</span>
        {/if}
      </div>

      <div class="ctrl">
        <span class="ctrl-label">{$t('quant.page.confidence')}</span>
        <div class="conf-row">
          {#each CONFIDENCE_LEVELS as c (c)}
            <button class="preset" class:active={confActive(c)} onclick={() => setConfPreset(c)}>
              {fmtRatioPct(c, 0)}
            </button>
          {/each}
          <label class="conf-custom" class:filled={confPct != null && confPct !== ''}>
            <input
              type="number"
              min="1"
              max="99"
              step="1"
              placeholder={$t('quant.page.customPlaceholder')}
              value={confPct}
              oninput={onConfInput}
            />
            <span class="pct-sign">%</span>
          </label>
        </div>
      </div>

      <button class="primary" onclick={runSingle} disabled={!singleId || singleBusy}>
        {singleBusy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
      </button>
    </div>

    {#if single}
      <div class="head">
        <span class="title">{single.ticker} · {single.timeframe}</span>
        <span class="sub">{$t('quant.page.periodsCount', { count: fmtNum(single.result.periods, 0) })}</span>
      </div>
      <MetricCards result={single.result} />
      <div class="card-box">
        <SingleCharts result={single.result} />
      </div>
    {:else if !singleBusy}
      <p class="hint">{$t('quant.page.pickAndAnalyzeHint')}</p>
    {/if}
  {/if}

  <!-- ── Portfolio ────────────────────────────────────────────── -->
  {#if tab === 'portfolio'}
    <div class="port-layout">
      <aside class="picker">
        <h3>{$t('quant.page.datasetsHeading')} <span class="muted">{$t('quant.page.selectedCount', { count: selected.length })}</span></h3>
        <p class="muted small">{$t('quant.page.pickTwoPlusHint')}</p>
        <input class="picker-search" placeholder={$t('quant.page.searchDatasetsPlaceholder')} bind:value={portSearch} />
        <div class="list">
          {#each portFiltered as d (d.id)}
            <label class="chk">
              <input type="checkbox" checked={selected.includes(d.id)} onchange={() => toggle(d.id)} />
              <span>{dsLabel(d)}</span>
            </label>
          {/each}
          {#if portFiltered.length === 0}
            <p class="muted small">{$t('quant.page.noMatches')}</p>
          {/if}
        </div>
        <button class="primary" onclick={runPortfolio} disabled={selected.length < 2 || portBusy}>
          {portBusy ? $t('quant.page.computing') : $t('quant.page.analyzePortfolio')}
        </button>
      </aside>

      <main class="port-main">
        {#if port}
          <div class="head">
            <span class="title">{port.labels.join(' · ')}</span>
            <span class="sub">{$t('quant.page.alignedPeriodsCount', { count: fmtNum(port.periods, 0) })}</span>
          </div>

          <section class="block">
            <h3>{$t('quant.page.correlationMatrix')}</h3>
            <p class="muted small">{$t('quant.page.correlationHint')}</p>
            <CorrelationHeatmap corr={port.correlation} />
          </section>

          <section class="block">
            <h3>{$t('quant.page.efficientFrontier')}</h3>
            <p class="muted small">{$t('quant.page.frontierHint')}</p>
            <FrontierChart frontier={port.frontier} onpick={pickFrontier} />
            {#if pickedWeights}
              <div class="picked">
                <strong>{pickedWeights.label}</strong>
                <span class="muted small">
                  {$t('quant.page.pickedWeightsSummary', {
                    ret: fmtRatioPct(pickedWeights.ret, 1),
                    vol: fmtRatioPct(pickedWeights.vol, 1),
                    sharpe: pickedWeights.sharpe.toFixed(2)
                  })}
                </span>
                <WeightsBars labels={port.labels} weights={pickedWeights.weights} title={$t('quant.page.allocation')} />
              </div>
            {/if}
          </section>

          <section class="block">
            <h3>{$t('quant.page.riskParity')}</h3>
            <p class="muted small">{$t('quant.page.riskParityHint')}</p>
            <WeightsBars
              labels={port.risk_parity.labels}
              weights={port.risk_parity.weights}
              risk={port.risk_parity.risk_contribution}
              title={$t('quant.page.riskParityWeightsTitle')}
            />
          </section>
        {:else if !portBusy}
          <p class="hint">{$t('quant.page.selectDatasetsHint')}</p>
        {/if}
      </main>
    </div>
  {/if}

  <!-- ── Seasonality ──────────────────────────────────────────── -->
  {#if tab === 'seasonality'}
    <div class="controls">
      <div class="ctrl">
        <span class="ctrl-label">{$t('quant.page.dataset')}</span>
        <DatasetPicker bind:value={seasonId} {datasets} />
      </div>
      <div class="ctrl">
        <span class="ctrl-label">{$t('quant.seasonality.metric')}</span>
        <div class="conf-row">
          <button class="preset" class:active={seasonMetric === 'return'} onclick={() => (seasonMetric = 'return')}>
            {$t('quant.seasonality.metricReturn')}
          </button>
          <button class="preset" class:active={seasonMetric === 'volatility'} onclick={() => (seasonMetric = 'volatility')}>
            {$t('quant.seasonality.metricVolatility')}
          </button>
        </div>
      </div>
      <button class="primary" onclick={runSeasonality} disabled={!seasonId || seasonBusy}>
        {seasonBusy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
      </button>
    </div>

    {#if season}
      <div class="head">
        <span class="title">{season.ticker} · {season.timeframe}</span>
        <span class="sub">{$t('quant.page.periodsCount', { count: fmtNum(season.result.periods, 0) })}</span>
      </div>
      <div class="card-box">
        <SeasonalityPanel result={season.result} />
      </div>
    {:else if !seasonBusy}
      <p class="hint">{$t('quant.seasonality.pickHint')}</p>
    {/if}
  {/if}

  <!-- ── Monte Carlo ──────────────────────────────────────────── -->
  {#if tab === 'montecarlo'}
    <MonteCarloPanel />
  {/if}

  <!-- ── Position size ────────────────────────────────────────── -->
  {#if tab === 'size'}
    <div class="size-wrap">
      <PositionSizePanel {datasets} />
    </div>
  {/if}

  <!-- ── Kelly ────────────────────────────────────────────────── -->
  {#if tab === 'kelly'}
    <div class="kelly-wrap">
      <KellyPanel />
    </div>
  {/if}
</div>
</RequireModule>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-4);
    gap: var(--space-3);
    overflow-y: auto;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }
  h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-semibold);
  }
  .tabs {
    display: flex;
    gap: var(--space-1);
    margin-left: auto;
  }
  .tabs button {
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--text);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .tabs button.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }
  /* Single-asset control bar: dataset picker · time range · confidence · analyze. */
  .controls {
    display: flex;
    align-items: flex-end;
    gap: var(--space-4);
    flex-wrap: wrap;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-3) var(--space-4);
  }
  .ctrl {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .ctrl.grow {
    flex: 1 1 260px;
    min-width: 220px;
  }
  .ctrl-label,
  .ctrl-label-row .ctrl-label {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .ctrl-label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .full-btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: 999px;
    padding: 2px var(--space-3);
    font-size: var(--text-xs);
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    cursor: pointer;
    transition:
      color 0.12s,
      border-color 0.12s;
  }
  .full-btn:not(:disabled):hover {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .full-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  /* Two range inputs stacked so both thumbs sit on one track. */
  .slider {
    position: relative;
    height: 26px;
  }
  .slider input[type='range'] {
    width: 100%;
    margin: 0;
  }
  .slider input[type='range']::-webkit-slider-thumb {
    width: 14px;
    margin-top: -5px;
  }
  .slider input[type='range']::-moz-range-thumb {
    width: 14px;
  }
  /* Range endpoints shown as read-only date pills. */
  .range-labels {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .range-labels span:not(.muted) {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px var(--space-2);
    font-variant-numeric: tabular-nums;
    font-weight: var(--fw-medium);
  }
  /* Confidence: segmented preset buttons + a proper custom field. */
  .conf-row {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .preset {
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 0 var(--space-3);
    height: 38px;
    min-width: 52px;
    font-size: var(--text-base);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
    transition:
      background 0.12s,
      border-color 0.12s,
      color 0.12s;
  }
  .preset:hover:not(.active) {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .preset.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }
  /* Custom % entry: no border at all, just the placeholder/value. */
  .conf-custom {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 38px;
    margin-left: var(--space-1);
  }
  .conf-custom input {
    width: 93px;
    border: none;
    background: transparent;
    padding: 0;
    font-size: var(--text-md);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .conf-custom input::placeholder {
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .conf-custom input:focus {
    outline: none;
  }
  /* Hide native number spinners for a cleaner field. */
  .conf-custom input::-webkit-outer-spin-button,
  .conf-custom input::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .conf-custom input[type='number'] {
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .pct-sign {
    color: var(--muted);
    font-size: var(--text-base);
    font-weight: var(--fw-semibold);
  }
  .picker-search {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2);
    font-size: var(--text-sm);
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .title {
    font-weight: var(--fw-semibold);
  }
  .sub {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .card-box {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-3);
  }
  .hint {
    color: var(--muted);
    padding: var(--space-4);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
  .port-layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 300px 1fr;
    gap: var(--space-4);
  }
  .picker {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    height: fit-content;
  }
  .picker h3 {
    font-size: var(--text-base);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-height: 360px;
    overflow-y: auto;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
    padding: var(--space-1);
    cursor: pointer;
  }
  .port-main {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .block {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .block h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-semibold);
  }
  .picked {
    margin-top: var(--space-3);
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .kelly-wrap {
    max-width: 640px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
  }
  .size-wrap {
    max-width: 920px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
  }
</style>
