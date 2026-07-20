<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { t } from '$lib/i18n';
  // Expert mode: a full-screen builder with a Library rail (Strategies / Indicators), the
  // strategy builder in the center (reuses SettingsPane), and results that replace it after a
  // run. Custom indicators are edited in a modal (the formula-stack builder) and can then be
  // referenced as operands. Normal mode (/backtest) is untouched — this is a separate route.
  import {
    backtestApi,
    defaultSettings,
    migrateSettings,
    normalizeSettings,
    embedIndicators,
    defaultIndicatorDef,
    fmtNum,
    buildReportMd,
    downloadText
  } from '$lib/modules/backtest/api.js';
  import SettingsPane from '$lib/modules/backtest/SettingsPane.svelte';
  import ResultChart from '$lib/modules/backtest/ResultChart.svelte';
  import StatsGrid from '$lib/modules/backtest/StatsGrid.svelte';
  import PerfTable from '$lib/modules/backtest/PerfTable.svelte';
  import TradesTable from '$lib/modules/backtest/TradesTable.svelte';
  import AssetBreakdown from '$lib/modules/backtest/AssetBreakdown.svelte';
  import OosBlock from '$lib/modules/backtest/OosBlock.svelte';
  import IndicatorBuilder from '$lib/modules/backtest/IndicatorBuilder.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';

  let datasets = $state([]);
  let strategies = $state([]);
  let indicators = $state([]); // [{ id, name, definition, ... }]

  let settings = $state(defaultSettings());
  let datasetIds = $state([]);
  let strategyId = $state(null); // provenance of the loaded strategy (null = unsaved draft)
  let strategyName = $state('');
  let dirty = $state(false); // unsaved edits to the strategy draft

  let result = $state(null);
  let bars = $state(null);
  let chartAssetId = $state(null);
  let running = $state(false);
  let alignment = $state(null);
  let aligning = $state(false);

  const chartAssets = $derived(
    datasetIds
      .map((id) => datasets.find((d) => d.id === id))
      .filter(Boolean)
      .map((d) => ({ id: d.id, ticker: d.ticker }))
  );
  async function switchChartAsset(id) {
    const nid = chartAssets.find((a) => String(a.id) === String(id))?.id ?? id;
    chartAssetId = nid;
    try {
      bars = await backtestApi.bars(nid);
    } catch (e) {
      error = e.message;
    }
  }
  let showResult = $state(false);
  let chartHidden = $state(false); // hide the chart to give the tables full pane height
  let tab = $state('summary');
  let libTab = $state('strategies');
  let libQuery = $state('');
  let error = $state('');

  // Custom indicators available to the operand picker.
  const customIndicators = $derived(indicators.map((i) => ({ id: i.id, name: i.name })));

  async function loadAll() {
    [datasets, strategies, indicators] = await Promise.all([
      backtestApi.datasets(),
      backtestApi.strategies(),
      backtestApi.indicators()
    ]);
  }
  $effect(() => {
    loadAll().catch((e) => (error = e.message));
  });

  // Any change to the draft marks it dirty (once loaded). A shallow JSON watch is enough.
  $effect(() => {
    JSON.stringify(settings);
    datasetIds.length;
    if (loadedOnce) dirty = true;
  });
  let loadedOnce = $state(false);

  // Alignment preview for multi-asset selections. Sole dependency is `datasetKey` (a primitive
  // join of the ids); settings + indicators are read untracked so strategy edits don't refetch
  // alignment (no banner jump).
  const datasetKey = $derived(datasetIds.join(','));
  $effect(() => {
    datasetKey; // sole dependency
    const ids = untrack(() => datasetIds.slice());
    if (ids.length < 2) {
      alignment = null;
      return;
    }
    const snap = untrack(() => embedIndicators(normalizeSettings(settings), indicators));
    aligning = true;
    backtestApi
      .align(ids, snap)
      .then((a) => (alignment = a))
      .catch(() => (alignment = null))
      .finally(() => (aligning = false));
  });

  const filteredStrategies = $derived(
    strategies.filter((s) => s.name.toLowerCase().includes(libQuery.trim().toLowerCase()))
  );
  const filteredIndicators = $derived(
    indicators.filter((i) => i.name.toLowerCase().includes(libQuery.trim().toLowerCase()))
  );

  function newStrategy() {
    settings = defaultSettings();
    datasetIds = [];
    strategyId = null;
    strategyName = '';
    result = null;
    showResult = false;
    loadedOnce = true;
    dirty = false;
  }

  async function openStrategy(s) {
    const full = await backtestApi.getStrategy(s.id).catch(() => s);
    settings = migrateSettings(full.settings);
    strategyId = full.id;
    strategyName = full.name;
    result = null;
    showResult = false;
    loadedOnce = true;
    // defer clearing dirty until after the settings-watch effect runs
    queueMicrotask(() => (dirty = false));
  }

  async function run() {
    if (!datasetIds.length) return;
    error = '';
    running = true;
    try {
      const payload = embedIndicators(normalizeSettings(settings), indicators);
      const [res, b] = await Promise.all([
        backtestApi.run(datasetIds, payload),
        backtestApi.bars(datasetIds[0])
      ]);
      result = res;
      bars = b;
      chartAssetId = datasetIds[0];
      showResult = true;
    } catch (e) {
      error = e.message;
    } finally {
      running = false;
    }
  }

  // Restore a run and show its result (used when returning from the PDF report via ?run=<id>).
  async function restoreRun(id) {
    const list = await backtestApi.runs().catch(() => []);
    const r = list.find((x) => x.id === id);
    if (!r) return;
    settings = migrateSettings(r.settings);
    datasetIds = r.dataset_ids?.length ? [...r.dataset_ids] : r.dataset_id ? [r.dataset_id] : [];
    strategyId = null;
    strategyName = r.name ?? '';
    loadedOnce = true;
    if (datasetIds.length) await run();
    queueMicrotask(() => (dirty = false));
  }
  // Honor ?run=<id> once (from the report Back link), then strip it from the URL.
  let restored = $state(false);
  $effect(() => {
    const rid = $page.url.searchParams.get('run');
    if (rid && !restored) {
      restored = true;
      restoreRun(rid).finally(() => goto('/backtest/expert', { replaceState: true, keepFocus: true, noScroll: true }));
    }
  });

  // Report name for the current result (dataset tickers + timeframe).
  const reportName = $derived(
    result
      ? (result.per_asset?.length > 1 ? result.per_asset.map((a) => a.ticker).join('-') : result.ticker) +
          ` ${result.timeframe}`
      : 'backtest'
  );

  // Download .md — generated client-side, so it works for an unsaved result too.
  function downloadReport() {
    if (!result) return;
    const md = buildReportMd({
      name: reportName,
      ticker: result.ticker,
      timeframe: result.timeframe,
      stats: result.stats,
      perAsset: result.per_asset,
      settings: embedIndicators(normalizeSettings(settings), indicators),
      meta: {
        bars: result.bars,
        period: result.trading_start_ts ? `from ${result.trading_start_ts}` : undefined,
        warmupBars: result.warmup_bars
      }
    });
    downloadText(`${reportName}.md`, md);
  }

  // Open the print-clean report (browser Print → PDF). The run is already in the history, so
  // the report route reads it straight from its run_id — no extra save needed.
  async function openLiveReport() {
    if (!result?.run_id) return;
    goto(`/backtest/report/${result.run_id}?from=expert`);
  }

  async function saveStrategy() {
    const name = (strategyName || '').trim();
    if (!name) {
      error = $t('backtest.expert.nameRequired');
      return;
    }
    error = '';
    // Persist the raw editor state (reverse_side toggle + both sides) so a strategy round-trips
    // losslessly. normalizeSettings is a run-time transform (derives/drops sides) — applying it
    // here would silently strip settings the user configured. Indicators are still embedded so
    // the saved strategy is self-contained.
    const payload = embedIndicators(migrateSettings(settings), indicators);
    try {
      if (strategyId) {
        await backtestApi.updateStrategy(strategyId, name, '', payload);
      } else {
        const { id } = await backtestApi.createStrategy(name, '', payload);
        strategyId = id;
      }
      dirty = false;
      strategies = await backtestApi.strategies();
    } catch (e) {
      error = e.message;
    }
  }

  async function duplicateStrategy(s, e) {
    e.stopPropagation();
    const full = await backtestApi.getStrategy(s.id).catch(() => s);
    try {
      await backtestApi.createStrategy(`${full.name} copy`, full.description ?? '', full.settings);
      strategies = await backtestApi.strategies();
    } catch (err) {
      error = err.message;
    }
  }
  async function deleteStrategy(s, e) {
    e.stopPropagation();
    await backtestApi.deleteStrategy(s.id).catch(() => {});
    if (strategyId === s.id) newStrategy();
    strategies = await backtestApi.strategies();
  }

  // ── Indicator editing (modal) ──
  let indOpen = $state(false);
  let indId = $state(null);
  let indName = $state('');
  let indDef = $state(defaultIndicatorDef());
  let indError = $state('');

  function newIndicator() {
    indId = null;
    indName = '';
    indDef = defaultIndicatorDef();
    indError = '';
    indOpen = true;
  }
  async function editIndicator(i, e) {
    e?.stopPropagation();
    const full = await backtestApi.getIndicator(i.id).catch(() => i);
    indId = full.id;
    indName = full.name;
    indDef = full.definition;
    indError = '';
    indOpen = true;
  }
  async function saveIndicator() {
    const name = (indName || '').trim();
    if (!name) {
      indError = $t('backtest.expert.nameRequired');
      return;
    }
    try {
      if (indId) await backtestApi.updateIndicator(indId, name, '', indDef);
      else await backtestApi.createIndicator(name, '', indDef);
      indOpen = false;
      indicators = await backtestApi.indicators();
    } catch (e) {
      indError = e.message;
    }
  }
  async function deleteIndicator(i, e) {
    e.stopPropagation();
    await backtestApi.deleteIndicator(i.id).catch(() => {});
    indicators = await backtestApi.indicators();
  }
</script>

<RequireModule module="backtest">
<div class="page">
  <header>
    <div class="lhs">
      <button class="exit" onclick={() => goto('/backtest')}><Icon name="chevron-left" size={14} /> {$t('backtest.expert.exit')}</button>
      <h1>{$t('backtest.expert.title')}</h1>
    </div>
    <div class="rhs">
      <input class="strat-name" bind:value={strategyName} placeholder={$t('backtest.expert.strategyName')} />
      <button class="save" class:dirty onclick={saveStrategy}>
        <Icon name="save" size={13} /> {$t('common.save')}{#if dirty}<span class="dot"></span>{/if}
      </button>
    </div>
  </header>

  {#if error}<p class="err">{error}</p>{/if}

  <div class="layout">
    <!-- Library rail -->
    <aside class="rail">
      <div class="lib-tabs">
        <button class:on={libTab === 'strategies'} onclick={() => (libTab = 'strategies')}>{$t('backtest.expert.strategies')}</button>
        <button class:on={libTab === 'indicators'} onclick={() => (libTab = 'indicators')}>{$t('backtest.expert.indicators')}</button>
      </div>
      <input class="lib-search" bind:value={libQuery} placeholder={$t('backtest.expert.search')} />

      {#if libTab === 'strategies'}
        <button class="new-item" onclick={newStrategy}><Icon name="plus" size={12} /> {$t('backtest.expert.newStrategy')}</button>
        <div class="items">
          {#each filteredStrategies as s (s.id)}
            <button class="item" class:sel={strategyId === s.id} onclick={() => openStrategy(s)}>
              <span class="iname">{s.name}</span>
              <span class="iacts">
                <span role="button" tabindex="0" title={$t('backtest.expert.duplicate')} onclick={(e) => duplicateStrategy(s, e)} onkeydown={(e) => e.key === 'Enter' && duplicateStrategy(s, e)}><Icon name="copy" size={12} /></span>
                <span role="button" tabindex="0" title={$t('common.remove')} onclick={(e) => deleteStrategy(s, e)} onkeydown={(e) => e.key === 'Enter' && deleteStrategy(s, e)}><Icon name="x" size={12} /></span>
              </span>
            </button>
          {/each}
          {#if !filteredStrategies.length}<p class="empty">{$t('backtest.expert.noStrategies')}</p>{/if}
        </div>
      {:else}
        <button class="new-item" onclick={newIndicator}><Icon name="plus" size={12} /> {$t('backtest.expert.newIndicator')}</button>
        <div class="items">
          {#each filteredIndicators as i (i.id)}
            <button class="item" onclick={(e) => editIndicator(i, e)}>
              <span class="iname">{i.name}</span>
              <span class="iacts">
                <span role="button" tabindex="0" title={$t('common.remove')} onclick={(e) => deleteIndicator(i, e)} onkeydown={(e) => e.key === 'Enter' && deleteIndicator(i, e)}><Icon name="x" size={12} /></span>
              </span>
            </button>
          {/each}
          {#if !filteredIndicators.length}<p class="empty">{$t('backtest.expert.noIndicators')}</p>{/if}
        </div>
      {/if}
    </aside>

    <!-- Builder / results -->
    <main class="center">
      {#if showResult && result}
        <div class="result-head">
          <button class="back" onclick={() => (showResult = false)}><Icon name="pencil" size={13} /> {$t('backtest.expert.editStrategy')}</button>
          <span class="title">
            {#if result.per_asset?.length > 1}{result.per_asset.map((a) => a.ticker).join(' · ')}{:else}{result.ticker}{/if}
          </span>
          <span class="chip">{result.timeframe}</span>
          {#if result.warmup_bars > 0}<span class="sub warmup">{$t('backtest.page.warmup', { n: result.warmup_bars })}</span>{/if}
          <div class="head-actions">
            <button class="chart-toggle" onclick={() => (chartHidden = !chartHidden)}
              title={chartHidden ? $t('backtest.page.showChart') : $t('backtest.page.hideChart')}>
              <Icon name={chartHidden ? 'chevron-down' : 'chevron-up'} size={13} />
              {chartHidden ? $t('backtest.page.showChart') : $t('backtest.page.hideChart')}
            </button>
            <button class="chart-toggle" onclick={downloadReport}><Icon name="download" size={13} /> {$t('backtest.report.downloadMd')}</button>
            <button class="chart-toggle" onclick={openLiveReport}><Icon name="file-text" size={13} /> {$t('backtest.report.printPdf')}</button>
          </div>
        </div>
        {#if !chartHidden}
          <div class="chart-box"><ResultChart {bars} trades={result.trades} equity={result.equity} benchmark={result.benchmark ?? []}
            assets={chartAssets} bind:activeAssetId={chartAssetId} onassetchange={switchChartAsset} /></div>
        {/if}
        <StatsGrid stats={result.stats} />
        <div class="tabs" role="tablist">
          <button role="tab" aria-selected={tab === 'summary'} class:on={tab === 'summary'} onclick={() => (tab = 'summary')}>{$t('backtest.page.performanceSummary')}</button>
          {#if result.per_asset?.length > 1}
            <button role="tab" aria-selected={tab === 'assets'} class:on={tab === 'assets'} onclick={() => (tab = 'assets')}>{$t('backtest.asset.title')} <span class="count">{result.per_asset.length}</span></button>
          {/if}
          <button role="tab" aria-selected={tab === 'trades'} class:on={tab === 'trades'} onclick={() => (tab = 'trades')}>{$t('backtest.page.listOfTrades')} <span class="count">{result.trades.length}</span></button>
        </div>
        {#if tab === 'summary'}
          {#if result.oos}<OosBlock oos={result.oos} />{/if}
          <PerfTable stats={result.stats} />
        {:else if tab === 'assets'}
          <AssetBreakdown perAsset={result.per_asset ?? []} />
        {:else}
          <TradesTable trades={result.trades} exitReasons={result.stats.exit_reasons} />
        {/if}
      {:else}
        <div class="builder-shell">
          <SettingsPane {datasets} bind:settings bind:datasetIds {alignment} {aligning} {running} {customIndicators} onrun={run} />
        </div>
      {/if}
    </main>
  </div>
</div>

<Modal bind:open={indOpen} title={indId ? $t('backtest.expert.editIndicator') : $t('backtest.expert.newIndicator')} size="lg">
  <label class="ind-field">
    <span>{$t('backtest.expert.indicatorName')}</span>
    <input bind:value={indName} placeholder={$t('backtest.expert.indicatorNamePlaceholder')} />
  </label>
  <IndicatorBuilder bind:def={indDef} />
  {#if indError}<p class="err">{indError}</p>{/if}
  {#snippet footer()}
    <button class="ghost" onclick={() => (indOpen = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={saveIndicator}>{$t('common.save')}</button>
  {/snippet}
</Modal>
</RequireModule>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-4);
    gap: var(--space-3);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .lhs,
  .rhs {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  h1 {
    font-size: 1.2rem;
    font-weight: var(--fw-medium);
  }
  .exit,
  .save,
  .ghost,
  .primary {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    border-radius: 0;
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-base);
    cursor: pointer;
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text);
  }
  .exit:hover,
  .save:hover {
    border-color: var(--border-control);
  }
  .save.dirty {
    border-color: var(--accent);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    margin-left: 4px;
  }
  .strat-name {
    min-width: 220px;
  }
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    gap: var(--space-4);
    overflow: hidden;
  }
  .rail {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    overflow: hidden;
  }
  .lib-tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 0;
    padding: 3px;
    gap: 3px;
  }
  .lib-tabs button {
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    padding: var(--space-1) 0;
    border-radius: 0;
    cursor: pointer;
  }
  .lib-tabs button.on {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    color: var(--text);
  }
  .lib-search {
    width: 100%;
  }
  .new-item {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
  }
  .new-item:hover {
    color: var(--text);
    border-color: var(--border-control);
  }
  .items {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    cursor: pointer;
    color: var(--text);
    text-align: left;
  }
  .item:hover {
    border-color: var(--border-control);
  }
  .item.sel {
    border-color: var(--accent);
  }
  .iname {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .iacts {
    display: inline-flex;
    gap: var(--space-1);
    color: var(--muted);
    flex-shrink: 0;
  }
  .iacts span:hover {
    color: var(--text);
  }
  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-2);
  }
  .center {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .builder-shell {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }
  .result-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-3) 0;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
  }
  .title {
    font-weight: var(--fw-medium);
    font-size: 1.05rem;
  }
  .sub {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .warmup {
    color: var(--amber);
  }
  .head-actions {
    margin-left: auto;
    display: inline-flex;
    gap: var(--space-2);
  }
  .chart-toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    padding: 2px 10px;
    cursor: pointer;
  }
  .chart-toggle:hover {
    color: var(--text);
    border-color: var(--border-control);
  }
  .chart-box {
    height: 420px;
    flex-shrink: 0;
    padding: 0 var(--space-3);
  }
  :global(.center > :not(.result-head):not(.chart-box):not(.builder-shell)) {
    margin: 0 var(--space-3);
  }
  .tabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: 1px solid var(--border);
    margin: 0 var(--space-3);
  }
  .tabs button {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--muted);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    padding: var(--space-1) var(--space-3) var(--space-2);
    cursor: pointer;
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .count {
    color: var(--muted);
    font-size: var(--text-xs);
    background: var(--surface-2);
    border-radius: 0;
    padding: 0 8px;
  }
  .ind-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-base);
    color: var(--muted);
    margin-bottom: var(--space-3);
  }
  .err {
    color: var(--red);
    font-size: var(--text-base);
  }
</style>
