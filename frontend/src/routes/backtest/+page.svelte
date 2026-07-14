<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { t } from '$lib/i18n';
  // Backtest module. Flow: configure in the Settings pane → Run → the pane collapses and a
  // Results view opens (chart with entry/exit/SL/TP markers, equity curve, stats). Modify
  // reopens Settings; Save persists settings + stats to History for rerun. History lists
  // saved runs; clicking one loads its settings (migrating v1 shapes) for rerun.
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { backtestApi, defaultSettings, migrateSettings, normalizeSettings, fmtNum, buildReportMd, downloadText } from '$lib/modules/backtest/api.js';
  import SettingsPane from '$lib/modules/backtest/SettingsPane.svelte';
  import ResultChart from '$lib/modules/backtest/ResultChart.svelte';
  import StatsGrid from '$lib/modules/backtest/StatsGrid.svelte';
  import PerfTable from '$lib/modules/backtest/PerfTable.svelte';
  import TradesTable from '$lib/modules/backtest/TradesTable.svelte';
  import AssetBreakdown from '$lib/modules/backtest/AssetBreakdown.svelte';
  import OosBlock from '$lib/modules/backtest/OosBlock.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';

  let datasets = $state([]);
  let runs = $state([]);
  let settings = $state(defaultSettings());
  let datasetIds = $state([]); // selected dataset ids (1 = single-asset, ≥2 = portfolio)
  let result = $state(null); // { trades, equity, stats, per_asset, warmup_bars, alignment, … }
  let bars = $state(null);
  let chartAssetId = $state(null); // dataset id the chart is showing (multi-asset picker)
  let running = $state(false);

  // Assets available in the current run, for the chart's dataset picker (multi-asset only).
  const chartAssets = $derived(
    datasetIds
      .map((id) => datasets.find((d) => d.id === id))
      .filter(Boolean)
      .map((d) => ({ id: d.id, ticker: d.ticker }))
  );

  // Switch the charted asset: refetch that dataset's bars (trades filter client-side).
  async function switchChartAsset(id) {
    const nid = chartAssets.find((a) => String(a.id) === String(id))?.id ?? id;
    chartAssetId = nid;
    try {
      bars = await backtestApi.bars(nid);
    } catch (e) {
      error = e.message;
    }
  }
  let alignment = $state(null); // pre-run alignment preview (multi-asset)
  let aligning = $state(false);
  let collapsed = $state(false); // settings pane retracted (auto after a run, or user-toggled)
  let chartHidden = $state(false); // hide the chart to give the tables full pane height
  let tab = $state('summary'); // result detail tab: summary | trades | history
  let error = $state('');

  const multi = $derived(datasetIds.length > 1);

  // The pane shows as a thin strip when the user retracts it or a run auto-collapses it.
  const paneMini = $derived(collapsed);

  let saveOpen = $state(false);
  let saveName = $state('');

  async function loadDatasets() {
    datasets = await backtestApi.datasets();
  }
  async function loadRuns() {
    runs = await backtestApi.runs();
  }
  $effect(() => {
    loadDatasets().catch((e) => (error = e.message));
    loadRuns().catch(() => {});
  });

  // Fetch the alignment preview when the multi-asset *selection* changes. The only dependency is
  // `datasetKey` (a primitive join of the ids) — settings (period/indicator edits) are read
  // untracked so tweaking the strategy doesn't refetch alignment and make the banner jump.
  // Warm-up shifts are minor and update on run.
  const datasetKey = $derived(datasetIds.join(','));
  $effect(() => {
    datasetKey; // sole dependency
    const ids = untrack(() => datasetIds.slice());
    if (ids.length < 2) {
      alignment = null;
      return;
    }
    const snap = untrack(() => normalizeSettings(settings));
    aligning = true;
    backtestApi
      .align(ids, snap)
      .then((a) => (alignment = a))
      .catch(() => (alignment = null))
      .finally(() => (aligning = false));
  });

  async function run() {
    if (!datasetIds.length) return;
    error = '';
    running = true;
    try {
      // The result chart overlays trades on the first (primary) dataset's bars.
      const [res, b] = await Promise.all([
        backtestApi.run(datasetIds, normalizeSettings(settings)),
        backtestApi.bars(datasetIds[0])
      ]);
      result = res;
      bars = b;
      chartAssetId = datasetIds[0];
      collapsed = true;
    } catch (e) {
      error = e.message;
    } finally {
      running = false;
    }
  }

  function modify() {
    collapsed = false;
  }
  function togglePane() {
    collapsed = !collapsed;
  }

  // Report name for the current result (dataset tickers).
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
      settings: normalizeSettings(settings),
      meta: {
        bars: result.bars,
        period: result.trading_start_ts ? `from ${result.trading_start_ts}` : undefined,
        warmupBars: result.warmup_bars
      }
    });
    downloadText(`${reportName}.md`, md);
  }

  // Open the print-clean report (browser Print → PDF). Needs a saved run, so save silently
  // under an auto name if the result isn't persisted yet, then navigate to the report route.
  async function openLiveReport() {
    if (!result) return;
    error = '';
    try {
      const { id } = await backtestApi.save(
        `${reportName} — ${new Date().toISOString().slice(0, 16).replace('T', ' ')}`,
        datasetIds,
        normalizeSettings(settings),
        result.stats
      );
      await loadRuns();
      goto(`/backtest/report/${id}?from=normal`);
    } catch (e) {
      error = e.message;
    }
  }

  async function doSave() {
    const name = saveName.trim();
    if (!name) return;
    try {
      await backtestApi.save(name, datasetIds, normalizeSettings(settings), result.stats);
      saveOpen = false;
      saveName = '';
      await loadRuns();
    } catch (e) {
      error = e.message;
    }
  }

  function loadRun(r) {
    settings = migrateSettings(r.settings);
    // Prefer the full dataset set (portfolio runs); fall back to the legacy scalar.
    datasetIds = r.dataset_ids?.length ? [...r.dataset_ids] : r.dataset_id ? [r.dataset_id] : [];
    collapsed = false;
    error = datasetIds.length ? '' : $t('backtest.page.datasetDeletedErr');
  }

  // Restore a run and show its result (used when returning from the PDF report via ?run=<id>).
  async function restoreRun(id) {
    const list = runs.length ? runs : await backtestApi.runs().catch(() => []);
    const r = list.find((x) => x.id === id);
    if (!r) return;
    loadRun(r);
    if (datasetIds.length) await run();
  }
  // Honor ?run=<id> once (from the report Back link), then strip it from the URL.
  let restored = $state(false);
  $effect(() => {
    const rid = $page.url.searchParams.get('run');
    if (rid && !restored) {
      restored = true;
      restoreRun(rid).finally(() => goto('/backtest', { replaceState: true, keepFocus: true, noScroll: true }));
    }
  });

  async function removeRun(id, e) {
    e.stopPropagation();
    await backtestApi.remove(id).catch(() => {});
    await loadRuns();
  }

  function openReport(id, e) {
    e.stopPropagation();
    goto(`/backtest/report/${id}?from=normal`);
  }
</script>

<RequireModule module="backtest">
<div class="page">
  <header>
    <div>
      <h1>{$t('backtest.page.title')}</h1>
      <p class="tagline">{$t('backtest.page.tagline')}</p>
    </div>
    <div class="actions">
      {#if result}
        <button onclick={modify}><Icon name="pencil" size={13} /> {$t('backtest.page.modify')}</button>
        <button onclick={downloadReport}><Icon name="download" size={13} /> {$t('backtest.report.downloadMd')}</button>
        <button onclick={openLiveReport}><Icon name="file-text" size={13} /> {$t('backtest.report.printPdf')}</button>
        <button class="primary" onclick={() => (saveOpen = true)}>💾 {$t('backtest.page.saveResults')}</button>
      {/if}
      <button class="expert" onclick={() => goto('/backtest/expert')} title={$t('backtest.page.expertHint')}>
        <Icon name="settings" size={13} /> {$t('backtest.page.expertMode')}
      </button>
    </div>
  </header>

  {#if error}<p class="err" title={$t('backtest.page.clickToCopy')} use:copyLog={error}>{error}</p>{/if}

  <div class="layout" class:has-result={paneMini}>
    <!-- Settings pane (retracts to a thin strip; auto after a run, or via the toggle) -->
    <aside class="pane settings-pane" class:mini={paneMini}>
      {#if paneMini}
        <button class="reopen" onclick={togglePane} title={$t('backtest.page.expandSettings')}>⚙ {$t('backtest.page.settings')}</button>
      {:else}
        <div class="pane-head">
          <span class="pane-title">{$t('backtest.page.settings')}</span>
          <button class="retract" onclick={togglePane} title={$t('backtest.page.collapseSettings')}>
            <Icon name="chevron-left" size={16} />
          </button>
        </div>
        <SettingsPane {datasets} bind:settings bind:datasetIds {alignment} {aligning} {running} onrun={run} />
      {/if}
    </aside>

    <!-- Result + History -->
    <main class="pane result-pane">
      {#if result}
        <div class="result-head">
          {#if chartAssets.length > 1}
            <!-- Multi-asset: the tickers double as the chart's asset switcher. -->
            <div class="asset-seg" aria-label={$t('backtest.chart.asset')}>
              {#each chartAssets as a (a.id)}
                <button class:on={a.id === chartAssetId} onclick={() => switchChartAsset(a.id)}>{a.ticker}</button>
              {/each}
            </div>
          {:else}
            <span class="title">
              {#if result.per_asset?.length > 1}
                {result.per_asset.map((a) => a.ticker).join(' · ')}
              {:else}
                {result.ticker}
              {/if}
            </span>
          {/if}
          <span class="chip">{result.timeframe}</span>
          <span class="sub">{$t('backtest.page.barsCount', { count: result.bars?.toLocaleString?.() ?? '' })}</span>
          {#if result.warmup_bars > 0}
            <span class="sub warmup" title={result.trading_start_ts ?? ''}>
              {$t('backtest.page.warmup', { n: result.warmup_bars })}
            </span>
          {/if}
          <button class="chart-toggle" onclick={() => (chartHidden = !chartHidden)}
            title={chartHidden ? $t('backtest.page.showChart') : $t('backtest.page.hideChart')}>
            <Icon name={chartHidden ? 'chevron-down' : 'chevron-up'} size={13} />
            {chartHidden ? $t('backtest.page.showChart') : $t('backtest.page.hideChart')}
          </button>
        </div>
        {#if !chartHidden}
          <div class="chart-box">
            <ResultChart {bars} trades={result.trades} equity={result.equity} benchmark={result.benchmark ?? []}
              assets={chartAssets} activeAssetId={chartAssetId} />
          </div>
        {/if}
        <StatsGrid stats={result.stats} />

        {#if result.skipped_min_size || result.skipped_margin || result.halted_bars}
          <div class="badges">
            {#if result.skipped_min_size}
              <span class="badge">{$t('backtest.page.skippedMin', { n: result.skipped_min_size })}</span>
            {/if}
            {#if result.skipped_margin}
              <span class="badge">{$t('backtest.page.skippedMargin', { n: result.skipped_margin })}</span>
            {/if}
            {#if result.halted_bars}
              <span class="badge warn">{$t('backtest.page.halted', { n: result.halted_bars })}</span>
            {/if}
          </div>
        {/if}

        <div class="tabs" role="tablist" aria-label={$t('backtest.page.resultDetail')}>
          <button role="tab" aria-selected={tab === 'summary'} class:on={tab === 'summary'}
            onclick={() => (tab = 'summary')}>{$t('backtest.page.performanceSummary')}</button>
          {#if result.per_asset?.length > 1}
            <button role="tab" aria-selected={tab === 'assets'} class:on={tab === 'assets'}
              onclick={() => (tab = 'assets')}>{$t('backtest.asset.title')} <span class="count">{result.per_asset.length}</span></button>
          {/if}
          <button role="tab" aria-selected={tab === 'trades'} class:on={tab === 'trades'}
            onclick={() => (tab = 'trades')}>{$t('backtest.page.listOfTrades')} <span class="count">{result.trades.length}</span></button>
          <button role="tab" aria-selected={tab === 'history'} class:on={tab === 'history'}
            onclick={() => (tab = 'history')}>{$t('backtest.page.history')}{#if runs.length} <span class="count">{runs.length}</span>{/if}</button>
        </div>
        {#if tab === 'summary'}
          {#if result.grid}
            <div class="grid-stats">
              <span class="cap">{$t('backtest.grid.statsTitle')}</span>
              <span>{$t('backtest.grid.fills')}: <b>{result.grid.fills}</b></span>
              <span>{$t('backtest.grid.roundTrips')}: <b>{result.grid.round_trips}</b></span>
              <span>{$t('backtest.grid.endInventory')}: <b>{fmtNum(result.grid.end_inventory, 4)}</b></span>
            </div>
          {/if}
          {#if result.oos}<OosBlock oos={result.oos} />{/if}
          <PerfTable stats={result.stats} />
        {:else if tab === 'assets'}
          <AssetBreakdown perAsset={result.per_asset ?? []} />
        {:else if tab === 'trades'}
          <TradesTable trades={result.trades} exitReasons={result.stats.exit_reasons} />
        {:else}
          {@render historyList()}
        {/if}
      {:else}
        <div class="empty">
          <span class="glyph">🧪</span>
          <p>{$t('backtest.page.emptyHint')}</p>
        </div>
        <div class="tabs" role="tablist" aria-label={$t('backtest.page.savedRuns')}>
          <button role="tab" aria-selected="true" class="on">{$t('backtest.page.history')}{#if runs.length} <span class="count">{runs.length}</span>{/if}</button>
        </div>
        {@render historyList()}
      {/if}
    </main>
  </div>
</div>

{#snippet historyList()}
  <section class="history">
    {#if !runs.length}
      <p class="muted">{$t('backtest.page.noSavedRuns')}</p>
    {/if}
    {#each runs as r (r.id)}
      <button class="run-row" onclick={() => loadRun(r)}>
        <span class="rn">{r.name}</span>
        <span class="rmeta">{r.ticker} · {r.timeframe}</span>
        <span class="rstat" class:pos={r.stats?.return_pct > 0} class:neg={r.stats?.return_pct < 0}>
          {$t('backtest.page.runStat', { returnPct: fmtNum(r.stats?.return_pct), trades: fmtNum(r.stats?.trades, 0) })}
        </span>
        <span class="rreport" role="button" tabindex="0" title={$t('backtest.report.open')}
          onclick={(e) => openReport(r.id, e)} onkeydown={(e) => e.key === 'Enter' && openReport(r.id, e)}><Icon name="file-text" size={12} /></span>
        <span class="rx" role="button" tabindex="0" onclick={(e) => removeRun(r.id, e)}
          onkeydown={(e) => e.key === 'Enter' && removeRun(r.id, e)}><Icon name="x" size={11} /></span>
      </button>
    {/each}
  </section>
{/snippet}

<Modal bind:open={saveOpen} title={$t('backtest.page.saveResults')}>
  <label class="save-field">
    <span>{$t('backtest.page.name')}</span>
    <input bind:value={saveName} placeholder={$t('backtest.page.namePlaceholder')} onkeydown={(e) => e.key === 'Enter' && doSave()} />
  </label>
  {#snippet footer()}
    <button class="ghost" onclick={() => (saveOpen = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={doSave}>{$t('common.save')}</button>
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
    gap: var(--space-4);
  }
  h1 {
    font-size: 1.4rem;
    font-weight: var(--fw-semibold);
    letter-spacing: -0.01em;
  }
  .tagline {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .actions {
    margin-left: auto;
    display: flex;
    gap: var(--space-2);
  }
  header button,
  .ghost,
  .primary {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    border-radius: 999px;
    padding: var(--space-1) var(--space-4);
    font-size: var(--text-base);
    cursor: pointer;
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text);
  }
  header button:hover {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }
  /* Expert mode is the gateway "one button" — give it a subtle accent tint. */
  .expert {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-2)) !important;
    border-color: color-mix(in srgb, var(--accent) 35%, var(--border)) !important;
  }
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 430px minmax(0, 1fr);
    gap: var(--space-4);
    overflow: hidden;
  }
  .layout.has-result {
    grid-template-columns: 52px minmax(0, 1fr);
  }
  .pane {
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    background: var(--surface);
    box-shadow: var(--shadow-1);
  }
  .settings-pane {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .pane-head {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4) 0;
  }
  .pane-title {
    font-size: 0.7rem;
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: 0.09em;
    color: var(--muted);
  }
  .retract {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: 3px;
    cursor: pointer;
  }
  .retract:hover {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }
  .settings-pane.mini {
    padding: var(--space-2);
    display: flex;
    align-items: flex-start;
    justify-content: center;
  }
  .reopen {
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-base);
  }
  .reopen:hover {
    color: var(--accent);
  }
  .result-pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .result-head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  /* Multi-asset ticker switcher: segmented pills, active = the charted asset. */
  .asset-seg {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .asset-seg button {
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 2px var(--space-3);
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    cursor: pointer;
    transition:
      background 0.12s,
      border-color 0.12s,
      color 0.12s;
  }
  .asset-seg button:hover:not(.on) {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .asset-seg button.on {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }
  .title {
    font-weight: var(--fw-semibold);
    font-size: 1.05rem;
  }
  .sub {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .warmup {
    color: var(--amber);
  }
  .grid-stats {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-base);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface-2) 40%, transparent);
  }
  .grid-stats .cap {
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .grid-stats b {
    font-variant-numeric: tabular-nums;
  }
  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .badge {
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 10px;
  }
  .badge.warn {
    color: var(--amber);
    border-color: color-mix(in srgb, var(--amber) 40%, var(--border));
  }
  .chart-box {
    height: 440px;
    flex-shrink: 0;
  }
  .chart-toggle {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--muted);
    font-size: var(--text-xs);
    padding: 2px 10px;
    cursor: pointer;
  }
  .chart-toggle:hover {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }
  .tabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--muted);
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    padding: var(--space-1) var(--space-3) var(--space-2);
    cursor: pointer;
    border-radius: 0;
  }
  .tabs button:hover {
    color: var(--text);
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .count {
    color: var(--muted);
    font-weight: var(--fw-medium);
    font-size: var(--text-xs);
    background: var(--surface-2);
    border-radius: 999px;
    padding: 0 8px;
    margin-left: 2px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
    padding: var(--space-8);
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
  }
  .glyph {
    font-size: 1.8rem;
    opacity: 0.7;
  }
  .history {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-base);
  }
  .run-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    text-align: left;
    color: var(--text);
    transition: border-color 0.12s ease, transform 0.12s ease;
  }
  .run-row:hover {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
    transform: translateY(-1px);
  }
  .rn {
    font-weight: var(--fw-semibold);
    font-size: var(--text-base);
  }
  .rmeta {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .rstat {
    margin-left: auto;
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }
  .rreport {
    color: var(--muted);
    padding: 0 4px;
    display: inline-flex;
  }
  .rreport:hover {
    color: var(--accent);
  }
  .rx {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: 0 4px;
  }
  .rx:hover {
    color: var(--red);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .err {
    color: var(--red);
  }
  .save-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-base);
    color: var(--muted);
  }
</style>
