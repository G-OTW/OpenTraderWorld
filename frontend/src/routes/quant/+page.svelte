<script>
  // Quant Tools module, tabs in four groups:
  //  • Asset: one dataset, one time window shared by every tab of the group. Risk (HV, drawdown,
  //    VaR, tail, drawdown table), statistics, volatility, regimes, events, seasonality.
  //  • Multi-asset: portfolio (correlation, frontier, risk parity), pairs (cointegration,
  //    spread, lead-lag), basket (PCA, clusters, HRP, relative strength, stress), regression.
  //  • Strategy: saved backtests. Monte Carlo over a run's trades, trade-list analytics (R,
  //    SQN, MAE/MFE), several runs compared (correlation, deflated Sharpe, PBO).
  //  • Sizing: position size, Kelly, volatility targeting, risk of ruin.
  //  • Calculators: options and implied vol, futures basis, compounding, Sharpe significance.
  //  • Derivatives: read from an IB or Massive connector. Futures curve (term structure, roll,
  //    basis), implied-volatility surface of an option chain, implied against realized.
  // Datasets come from the Historical Data catalog; the module needs it installed with data.
  import { quantApi, fmtRatioPct, dsLabel, CONFIDENCE_LEVELS } from '$lib/modules/quant/api.js';
  import { page } from '$app/stores';
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
  import StatsPanel from '$lib/modules/quant/StatsPanel.svelte';
  import VolatilityPanel from '$lib/modules/quant/VolatilityPanel.svelte';
  import RegimesPanel from '$lib/modules/quant/RegimesPanel.svelte';
  import EventsPanel from '$lib/modules/quant/EventsPanel.svelte';
  import SingleExtras from '$lib/modules/quant/SingleExtras.svelte';
  import PairsPanel from '$lib/modules/quant/PairsPanel.svelte';
  import BasketPanel from '$lib/modules/quant/BasketPanel.svelte';
  import RegressionPanel from '$lib/modules/quant/RegressionPanel.svelte';
  import TradesPanel from '$lib/modules/quant/TradesPanel.svelte';
  import ComparePanel from '$lib/modules/quant/ComparePanel.svelte';
  import RuinPanel from '$lib/modules/quant/RuinPanel.svelte';
  import VolTargetPanel from '$lib/modules/quant/VolTargetPanel.svelte';
  import OptionsPanel from '$lib/modules/quant/OptionsPanel.svelte';
  import BasisPanel from '$lib/modules/quant/BasisPanel.svelte';
  import CompoundPanel from '$lib/modules/quant/CompoundPanel.svelte';
  import SharpeCalcPanel from '$lib/modules/quant/SharpeCalcPanel.svelte';
  import FuturesCurvePanel from '$lib/modules/quant/FuturesCurvePanel.svelte';
  import OptionSurfacePanel from '$lib/modules/quant/OptionSurfacePanel.svelte';
  import IvHistoryPanel from '$lib/modules/quant/IvHistoryPanel.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { fmtNum } from '$lib/format.js';

  // Tabs by group; the label key of a tab is quant.page.tab.<id>, of a group quant.page.group.<id>.
  const GROUPS = [
    { id: 'asset', tabs: ['single', 'stats', 'volatility', 'regimes', 'events', 'seasonality'] },
    { id: 'multi', tabs: ['portfolio', 'pairs', 'basket', 'regression'] },
    { id: 'strategy', tabs: ['montecarlo', 'trades', 'compare'] },
    { id: 'sizing', tabs: ['size', 'kelly', 'voltarget', 'ruin'] },
    { id: 'calc', tabs: ['options', 'basis', 'compound', 'sharpe'] },
    { id: 'derivs', tabs: ['futcurve', 'surface', 'ivhistory'] }
  ];
  const ALL_TABS = GROUPS.flatMap((g) => g.tabs);
  // Tabs that read saved runs, typed numbers or a provider, not the dataset catalog.
  const NO_DATA_TABS = [
    'kelly', 'size', 'montecarlo', 'trades', 'compare', 'ruin', 'options', 'basis', 'compound', 'sharpe',
    'futcurve', 'surface', 'ivhistory'
  ];
  const groupOf = (id) => GROUPS.find((g) => g.tabs.includes(id)) ?? GROUPS[0];

  // Remember the active tab across refreshes.
  const TAB_KEY = 'quant.tab';
  const initialTab = (() => {
    const linked = $page.url.searchParams.get('tab');
    if (ALL_TABS.includes(linked)) return linked;
    try {
      const t = localStorage.getItem(TAB_KEY);
      return ALL_TABS.includes(t) ? t : 'single';
    } catch {
      return 'single';
    }
  })();
  let tab = $state(initialTab);
  const group = $derived(groupOf(tab));
  const isAsset = $derived(group.id === 'asset');
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

  // The analysis window every asset tab receives: nulls for the full dataset.
  const win = $derived.by(() => {
    if (isFull || !dsBounds) return { from: null, until: null };
    return { from: `${dayToISO(rangeStart)}T00:00:00Z`, until: `${dayToISO(rangeEnd)}T23:59:59Z` };
  });

  // Portfolio
  let selected = $state([]); // dataset ids
  let portSearch = $state('');
  let port = $state(null);
  let portBusy = $state(false);
  let pickedWeights = $state(null); // { label, weights } from clicking a frontier point

  // Seasonality (uses the shared asset dataset and window)
  let seasonMetric = $state('return'); // 'return' | 'volatility' | 'volume' | 'range'
  let season = $state(null); // { ticker, timeframe, result }
  let seasonBusy = $state(false);

  async function runSeasonality() {
    if (!singleId) return;
    error = '';
    seasonBusy = true;
    try {
      season = await quantApi.seasonality(singleId, { ...win, metric: seasonMetric });
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
      .then((d) => {
        datasets = d;
        const id = $page.url.searchParams.get('dataset');
        if (d.some((ds) => ds.id === id)) singleId = id;
        const ids = ($page.url.searchParams.get('datasets') ?? '').split(',');
        selected = ids.filter((id) => d.some((ds) => ds.id === id));
      })
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
  <h1>{$t('quant.page.title')}</h1>
  <header>
    <nav class="tabs">
      {#each GROUPS as g (g.id)}
        <button class:active={group.id === g.id} onclick={() => (tab = g.tabs[0])}>{$t(`quant.page.group.${g.id}`)}</button>
      {/each}
    </nav>
    {#if group.tabs.length > 1}
      <nav class="subtabs">
        {#each group.tabs as id (id)}
          <button class:active={tab === id} onclick={() => (tab = id)}>{$t(`quant.page.tab.${id}`)}</button>
        {/each}
      </nav>
    {/if}
  </header>

  <ErrorText error={error} copyable />

  {#if !datasets.length && !NO_DATA_TABS.includes(tab)}
    <p class="hint">
      {@html $t('quant.page.noDatasetsHint')}
    </p>
  {/if}

  <!-- ── Asset group: shared dataset + window ─────────────────── -->
  {#if isAsset}
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

      {#if tab === 'single'}
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
      {/if}
    </div>
  {/if}

  <!-- ── Single asset ─────────────────────────────────────────── -->
  {#if tab === 'single'}
    {#if single}
      <div class="head">
        <span class="title">{single.ticker} · {single.timeframe}</span>
        <span class="sub">{$t('quant.page.periodsCount', { count: fmtNum(single.result.periods, 0) })}</span>
      </div>
      <MetricCards result={single.result} />
      <div class="card-box">
        <SingleCharts result={single.result} />
      </div>
      <SingleExtras tail={single.tail} episodes={single.episodes} />
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
            {#if port.measured_at}
              <span class="sub">{$t('quant.page.measuredAt', { tf: port.measured_at })}</span>
            {/if}
          </div>
          {#if port.resampled}
            <p class="muted small">
              {$t('quant.page.resampled', { from: port.resampled.from, to: port.resampled.to })}
            </p>
          {/if}

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

  <!-- ── Statistics / volatility / regimes / events ─────────── -->
  {#if tab === 'stats'}
    <StatsPanel datasetId={singleId} {win} />
  {/if}
  {#if tab === 'volatility'}
    <VolatilityPanel datasetId={singleId} {win} />
  {/if}
  {#if tab === 'regimes'}
    <RegimesPanel datasetId={singleId} {win} />
  {/if}
  {#if tab === 'events'}
    <EventsPanel datasetId={singleId} {win} />
  {/if}

  <!-- ── Pairs / basket / regression ─────────────────────────── -->
  {#if tab === 'pairs'}
    <PairsPanel {datasets} />
  {/if}
  {#if tab === 'basket'}
    <BasketPanel {datasets} />
  {/if}
  {#if tab === 'regression'}
    <RegressionPanel {datasets} />
  {/if}

  <!-- ── Seasonality ──────────────────────────────────────────── -->
  {#if tab === 'seasonality'}
    <div class="controls">
      <div class="ctrl">
        <span class="ctrl-label">{$t('quant.seasonality.metric')}</span>
        <div class="conf-row">
          <button class="preset" class:active={seasonMetric === 'return'} onclick={() => (seasonMetric = 'return')}>
            {$t('quant.seasonality.metricReturn')}
          </button>
          <button class="preset" class:active={seasonMetric === 'volatility'} onclick={() => (seasonMetric = 'volatility')}>
            {$t('quant.seasonality.metricVolatility')}
          </button>
          <button class="preset" class:active={seasonMetric === 'range'} onclick={() => (seasonMetric = 'range')}>
            {$t('quant.seasonality.metricRange')}
          </button>
          <button class="preset" class:active={seasonMetric === 'volume'} onclick={() => (seasonMetric = 'volume')}>
            {$t('quant.seasonality.metricVolume')}
          </button>
        </div>
      </div>
      <button class="primary" onclick={runSeasonality} disabled={!singleId || seasonBusy}>
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

  {#if tab === 'trades'}
    <TradesPanel />
  {/if}
  {#if tab === 'compare'}
    <ComparePanel />
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
  {#if tab === 'voltarget'}
    <VolTargetPanel {datasets} />
  {/if}
  {#if tab === 'ruin'}
    <RuinPanel />
  {/if}

  <!-- ── Calculators ──────────────────────────────────────────── -->
  {#if tab === 'options'}
    <OptionsPanel />
  {/if}
  {#if tab === 'basis'}
    <BasisPanel />
  {/if}
  {#if tab === 'compound'}
    <CompoundPanel />
  {/if}
  {#if tab === 'sharpe'}
    <SharpeCalcPanel />
  {/if}

  <!-- ── Derivatives (provider data) ────────────────────────────── -->
  {#if tab === 'futcurve'}
    <FuturesCurvePanel />
  {/if}
  {#if tab === 'surface'}
    <OptionSurfacePanel />
  {/if}
  {#if tab === 'ivhistory'}
    <IvHistoryPanel />
  {/if}
</div>
</RequireModule>

<style>

  .page {
    height: 100%;
    width: 100%;
    max-width: 100vw;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: var(--space-4);
    gap: var(--space-4);
    overflow-y: auto;
    container: quant-page / inline-size;
  }
  .page > :global(*) {
    flex-shrink: 0;
  }
  h1 {
    font-size: var(--fs-page-title);
    font-weight: var(--fw-medium);
    letter-spacing: -0.02em;
    line-height: var(--lh-tight);
  }
  header {
    min-width: 0;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    padding: var(--space-2);
    border-bottom: none;
  }
  .tabs button {
    flex: 1 1 auto;
    margin-bottom: 0;
    min-height: var(--control-h);
    border: var(--hairline) solid transparent;
    background: transparent;
    color: var(--muted);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-4);
    font-size: var(--fs-body);
    font-weight: var(--fw-medium);
    cursor: pointer;
  }
  .tabs button:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .tabs button.active {
    background: var(--surface-2);
    border-color: var(--border-control);
    color: var(--text);
  }
  .subtabs {
    display: flex;
    gap: var(--space-4);
    padding: 0 var(--space-4);
    border-top: var(--hairline) solid var(--border);
    overflow-x: auto;
    scrollbar-width: thin;
  }
  .subtabs button {
    flex-shrink: 0;
    min-height: 42px;
    background: transparent;
    border: none;
    border-bottom: var(--active-rule) solid transparent;
    color: var(--muted);
    padding: var(--space-2) var(--space-1);
    font-size: var(--fs-body);
    cursor: pointer;
    white-space: nowrap;
  }
  .subtabs button:hover {
    color: var(--text);
  }
  .subtabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  /* Shared control height; the date window keeps a flexible column. */
  .controls {
    display: flex;
    align-items: flex-end;
    gap: var(--space-4);
    flex-wrap: wrap;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-4);
  }
  .ctrl {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .ctrl:first-child {
    flex: 1 1 240px;
  }
  .ctrl.grow {
    flex: 1.5 1 260px;
    min-width: 0;
  }
  .ctrl-label {
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    line-height: var(--lh-base);
  }
  .ctrl-label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .full-btn {
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    color: var(--text);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-size: var(--fs-metric-label);
    font-weight: var(--fw-medium);
    cursor: pointer;
  }
  .full-btn:not(:disabled):hover {
    background: var(--surface-hover);
  }
  .full-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .slider {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .slider input[type='range'] {
    width: 100%;
    margin: 0;
    accent-color: var(--accent);
  }
  .range-labels {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--fs-desc);
    color: var(--text);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .conf-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
  }
  .preset {
    background: var(--surface-2);
    border: var(--hairline) solid transparent;
    color: var(--muted);
    border-radius: var(--radius-sm);
    padding: 0 var(--space-3);
    height: var(--control-h);
    font-size: var(--fs-body);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .preset:hover {
    color: var(--text);
    border-color: var(--border-control);
  }
  .preset.active {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
    border-color: var(--accent);
    color: var(--text);
  }
  .conf-custom {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: var(--control-h);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    padding: 0 var(--space-2);
    background: var(--surface);
  }
  .conf-custom.filled {
    border-color: var(--accent);
  }
  .conf-custom:focus-within {
    border-color: var(--accent);
    box-shadow: var(--ring);
  }
  .conf-custom input {
    width: 62px;
    min-width: 0;
    height: 100%;
    border: none;
    background: transparent;
    padding: 0;
    font-size: var(--fs-body);
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .conf-custom input:focus {
    outline: none;
    box-shadow: none;
  }
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
    font-size: var(--fs-body);
  }
  .picker-search {
    width: 100%;
    min-width: 0;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2) var(--space-3);
  }
  .title {
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    overflow-wrap: anywhere;
  }
  .sub {
    color: var(--muted);
    font-size: var(--fs-desc);
  }
  .card-box, .block, .picker, .kelly-wrap, .size-wrap {
    min-width: 0;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-4);
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-body);
    line-height: 1.6;
    padding: var(--space-6);
    background: var(--surface);
    border: var(--hairline) dashed var(--border-control);
    border-radius: var(--radius);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--fs-desc);
    line-height: var(--lh-base);
  }
  .port-layout {
    display: grid;
    grid-template-columns: 280px minmax(0, 1fr);
    gap: var(--space-4);
    align-items: start;
  }
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .picker h3 {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
  }
  .picker h3 .muted {
    font-size: var(--fs-desc);
    font-weight: var(--fw-normal);
  }
  .list {
    display: flex;
    flex-direction: column;
    max-height: 360px;
    overflow-y: auto;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--fs-body);
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .chk:hover {
    background: var(--surface-hover);
  }
  .chk:has(input:checked) {
    background: var(--surface-2);
  }
  .chk input {
    flex-shrink: 0;
  }
  .chk span {
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .port-main {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .block h3 {
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
  }
  .picked {
    margin-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
    padding-top: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .kelly-wrap {
    width: 100%;
    max-width: 720px;
  }
  .size-wrap {
    width: 100%;
    max-width: 1120px;
  }
  @media (max-width: 767px) {
    .tabs button { min-width: min(100%, 125px); min-height: 44px; }
    .subtabs {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 0 var(--space-2);
      padding: 0 var(--space-2);
      overflow: visible;
    }
    .subtabs button {
      min-width: 0;
      min-height: 44px;
      white-space: normal;
      overflow-wrap: anywhere;
      text-align: center;
    }
  }
  @container quant-page (max-width: 760px) {
    .port-layout {
      grid-template-columns: minmax(0, 1fr);
    }
    .tabs button {
      flex: 1 1 calc(33.333% - var(--space-2));
      padding-inline: var(--space-2);
    }
    .controls > .primary {
      margin-left: auto;
    }
  }
  @container quant-page (max-width: 440px) {
    .controls {
      gap: var(--space-3);
    }
    .ctrl, .ctrl:first-child, .ctrl.grow {
      flex: 1 1 100%;
    }
    .controls > .primary {
      width: 100%;
    }
  }
</style>
