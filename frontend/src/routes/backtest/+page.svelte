<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { t } from '$lib/i18n';
  // Backtest module. Flow: configure in the Settings pane → Run → the pane collapses and a
  // Results view opens (chart with entry/exit/SL/TP markers, equity curve, stats). Modify
  // reopens Settings; Save persists settings + stats to History for rerun. History lists
  // saved runs; clicking one loads its settings (migrating v1 shapes) for rerun.
  import { backtestApi, defaultSettings, migrateSettings, normalizeSettings, fmtNum } from '$lib/modules/backtest/api.js';
  import SettingsPane from '$lib/modules/backtest/SettingsPane.svelte';
  import ResultChart from '$lib/modules/backtest/ResultChart.svelte';
  import StatsGrid from '$lib/modules/backtest/StatsGrid.svelte';
  import PerfTable from '$lib/modules/backtest/PerfTable.svelte';
  import TradesTable from '$lib/modules/backtest/TradesTable.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';

  let datasets = $state([]);
  let runs = $state([]);
  let settings = $state(defaultSettings());
  let datasetId = $state(null);
  let result = $state(null); // { trades, equity, stats, ticker, timeframe }
  let bars = $state(null);
  let running = $state(false);
  let collapsed = $state(false); // settings pane retracted (auto after a run, or user-toggled)
  let tab = $state('summary'); // result detail tab: summary | trades | history
  let error = $state('');

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

  async function run() {
    if (!datasetId) return;
    error = '';
    running = true;
    try {
      const [res, b] = await Promise.all([
        backtestApi.run(datasetId, normalizeSettings(settings)),
        backtestApi.bars(datasetId)
      ]);
      result = res;
      bars = b;
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

  async function doSave() {
    const name = saveName.trim();
    if (!name) return;
    try {
      await backtestApi.save(name, datasetId, normalizeSettings(settings), result.stats);
      saveOpen = false;
      saveName = '';
      await loadRuns();
    } catch (e) {
      error = e.message;
    }
  }

  function loadRun(r) {
    settings = migrateSettings(r.settings);
    datasetId = r.dataset_id;
    collapsed = false;
    error = r.dataset_id ? '' : $t('backtest.page.datasetDeletedErr');
  }

  async function removeRun(id, e) {
    e.stopPropagation();
    await backtestApi.remove(id).catch(() => {});
    await loadRuns();
  }
</script>

<RequireModule module="backtest">
<div class="page">
  <header>
    <div>
      <h1>{$t('backtest.page.title')}</h1>
      <p class="tagline">{$t('backtest.page.tagline')}</p>
    </div>
    {#if result}
      <div class="actions">
        <button onclick={modify}><Icon name="pencil" size={13} /> {$t('backtest.page.modify')}</button>
        <button class="primary" onclick={() => (saveOpen = true)}>💾 {$t('backtest.page.saveResults')}</button>
      </div>
    {/if}
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
        <SettingsPane {datasets} bind:settings bind:datasetId {running} onrun={run} />
      {/if}
    </aside>

    <!-- Result + History -->
    <main class="pane result-pane">
      {#if result}
        <div class="result-head">
          <span class="title">{result.ticker}</span>
          <span class="chip">{result.timeframe}</span>
          <span class="sub">{$t('backtest.page.barsCount', { count: result.bars?.toLocaleString?.() ?? '' })}</span>
        </div>
        <div class="chart-box">
          <ResultChart {bars} trades={result.trades} equity={result.equity} />
        </div>
        <StatsGrid stats={result.stats} />

        <div class="tabs" role="tablist" aria-label={$t('backtest.page.resultDetail')}>
          <button role="tab" aria-selected={tab === 'summary'} class:on={tab === 'summary'}
            onclick={() => (tab = 'summary')}>{$t('backtest.page.performanceSummary')}</button>
          <button role="tab" aria-selected={tab === 'trades'} class:on={tab === 'trades'}
            onclick={() => (tab = 'trades')}>{$t('backtest.page.listOfTrades')} <span class="count">{result.trades.length}</span></button>
          <button role="tab" aria-selected={tab === 'history'} class:on={tab === 'history'}
            onclick={() => (tab = 'history')}>{$t('backtest.page.history')}{#if runs.length} <span class="count">{runs.length}</span>{/if}</button>
        </div>
        {#if tab === 'summary'}
          <PerfTable stats={result.stats} />
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
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .tagline {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .actions {
    margin-left: auto;
    display: flex;
    gap: var(--space-2);
  }
  header button,
  .ghost,
  .primary {
    border-radius: 999px;
    padding: var(--space-1) var(--space-4);
    font-size: 0.85rem;
    cursor: pointer;
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text);
  }
  header button:hover {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
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
    font-weight: 700;
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
    font-size: 0.85rem;
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
    gap: var(--space-2);
  }
  .title {
    font-weight: 700;
    font-size: 1.05rem;
  }
  .sub {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .chart-box {
    height: 440px;
    flex-shrink: 0;
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
    font-size: 0.82rem;
    font-weight: 600;
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
    font-weight: 500;
    font-size: 0.72rem;
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
    font-size: 0.85rem;
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
    font-weight: 600;
    font-size: 0.85rem;
  }
  .rmeta {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .rstat {
    margin-left: auto;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
  }
  .rx {
    color: var(--muted);
    font-size: 0.8rem;
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
    font-size: 0.85rem;
    color: var(--muted);
  }
</style>
