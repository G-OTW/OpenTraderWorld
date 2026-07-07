<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  // Historical Data Visualization — chart stored OHLCV. Left: dataset
  // picker + managed indicator list. Right: toolbar (chart type) over an ECharts pane.
  // Indicators are instances {id,type,params,visible} added/edited via a modal. A
  // ?dataset=<id> query param deep-links from the download module.
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { t } from '$lib/i18n';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { suggestBrick } from '$lib/modules/histviz/indicators.js';
  import DatasetPicker from '$lib/modules/histviz/DatasetPicker.svelte';
  import IndicatorPanel from '$lib/modules/histviz/IndicatorPanel.svelte';
  import IndicatorModal from '$lib/modules/histviz/IndicatorModal.svelte';
  import ChartSettings from '$lib/modules/histviz/ChartSettings.svelte';
  import Chart from '$lib/modules/histviz/Chart.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import { DEFAULT_SETTINGS, normalizeSettings } from '$lib/modules/histviz/settings.js';

  // ── Persisted session state: dataset/type/brick/indicators live per-browser in
  // localStorage; chart *settings* are a fixed global config persisted server-side. ──
  const STORE_KEY = 'otw.histviz.session.v1';
  function loadStore() {
    try {
      return JSON.parse(localStorage.getItem(STORE_KEY) || '{}');
    } catch {
      return {};
    }
  }
  const saved = loadStore();

  let datasets = $state([]);
  let selected = $state(null);
  let bars = $state(null);
  let type = $state(saved.type ?? 'candlestick');
  let brick = $state(saved.brick ?? 0);
  let error = $state('');
  let loading = $state(false);
  let settings = $state({ ...DEFAULT_SETTINGS });
  let settingsOpen = $state(false);
  let settingsLoaded = $state(false); // gate the save-effect until the DB load lands
  let fullscreen = $state(false);

  // Exit fullscreen on Escape.
  function onKey(e) {
    if (e.key === 'Escape' && fullscreen) fullscreen = false;
  }

  // Active indicator instances (restored from storage; ids kept so nextId stays unique).
  let instances = $state(saved.instances ?? []);
  let modalOpen = $state(false);
  let editing = $state(null);
  // Starting id must clear the max restored id so new instances stay unique.
  let nextId = (saved.instances ?? []).reduce((m, i) => Math.max(m, i.id), 0) + 1;

  const CHART_TYPES = [
    ['candlestick', 'histviz.page.typeCandles'],
    ['ohlc', 'histviz.page.typeOhlc'],
    ['line', 'histviz.page.typeLine'],
    ['renko', 'histviz.page.typeRenko']
  ];

  // Persist session bits to localStorage (after restore, to avoid clobbering with defaults).
  let restored = $state(false);
  $effect(() => {
    const snap = {
      datasetId: selected?.id ?? saved.datasetId ?? null,
      type,
      brick,
      instances
    };
    if (!restored) return;
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify(snap));
    } catch {
      /* non-fatal */
    }
  });

  // Load the global chart settings from the DB once, then save on every change.
  $effect(() => {
    histvizApi
      .chartSettings()
      .then((raw) => {
        settings = normalizeSettings(raw);
      })
      .catch(() => {
        /* fall back to defaults */
      })
      .finally(() => {
        settingsLoaded = true;
      });
  });
  $effect(() => {
    const snap = { ...settings };
    if (!settingsLoaded) return;
    histvizApi.saveChartSettings(snap).catch((e) => (error = e.message));
  });

  async function loadDatasets() {
    datasets = await histvizApi.datasets();
  }

  async function select(d) {
    selected = d;
    error = '';
    loading = true;
    try {
      bars = await histvizApi.bars(d.id, { limit: 10000 });
      if (type === 'renko' && !brick) brick = suggestBrick(bars.c);
    } catch (e) {
      error = e.message;
      bars = null;
    } finally {
      loading = false;
    }
  }

  // ── Indicator management ──
  function openAdd() {
    editing = null;
    modalOpen = true;
  }
  function openEdit(ind) {
    editing = ind;
    modalOpen = true;
  }
  function onSave(draft) {
    if (editing) {
      instances = instances.map((i) =>
        i.id === editing.id
          ? { ...i, type: draft.type, params: draft.params, style: draft.style }
          : i
      );
    } else {
      instances = [
        ...instances,
        { id: nextId++, type: draft.type, params: draft.params, style: draft.style, visible: true }
      ];
    }
    editing = null;
  }
  function toggle(id) {
    instances = instances.map((i) => (i.id === id ? { ...i, visible: !i.visible } : i));
  }
  function remove(id) {
    instances = instances.filter((i) => i.id !== id);
  }

  // Initial load, then select from ?dataset= (deep-link) or the last-used saved dataset.
  let didDeepLink = false;
  $effect(() => {
    loadDatasets()
      .then(() => {
        if (didDeepLink) return;
        didDeepLink = true;
        const id = $page.url.searchParams.get('dataset') ?? saved.datasetId;
        const d = id ? datasets.find((x) => x.id === id) : null;
        if (d && d.bar_count) select(d);
        restored = true;
      })
      .catch((e) => {
        error = e.message;
        restored = true;
      });
  });

  $effect(() => {
    if (type === 'renko' && bars && !brick) brick = suggestBrick(bars.c);
  });
</script>

<RequireModule module="histviz">
<div class="page">
  <aside>
    <div class="aside-head">
      <h1>{$t('histviz.page.title')}</h1>
    </div>
    <IndicatorPanel
      {instances}
      onadd={openAdd}
      onedit={openEdit}
      ontoggle={toggle}
      onremove={remove}
    />
    <hr />
    <DatasetPicker
      {datasets}
      selectedId={selected?.id}
      onselect={select}
      ondownload={() => goto('/histdata')}
    />
  </aside>

  <main>
    <div class="toolbar">
      <div class="group">
        {#each CHART_TYPES as [val, labelKey] (val)}
          <button class:active={type === val} onclick={() => (type = val)}>{$t(labelKey)}</button>
        {/each}
      </div>
      {#if type === 'renko'}
        <label class="brick">
          {$t('histviz.page.brick')}
          <input type="number" min="0" step="any" bind:value={brick} placeholder={$t('histviz.modal.auto')} />
        </label>
      {/if}
      {#if selected}
        <span class="title">{selected.ticker} · {selected.timeframe} · {selected.provider}</span>
      {/if}
      <div class="settings-wrap" class:pushed={!selected}>
        <button
          class="gear"
          class:active={settingsOpen}
          title={$t('histviz.settings.title')}
          onclick={() => (settingsOpen = !settingsOpen)}>⚙ {$t('settings.title')}</button
        >
        {#if settingsOpen}
          <ChartSettings bind:settings onclose={() => (settingsOpen = false)} />
        {/if}
      </div>
      <button class="fs" title={$t('histviz.page.fullscreenChart')} onclick={() => (fullscreen = true)}><Icon name="maximize" size={14} /></button>
    </div>

    {#if error}<p class="err" title={$t('histviz.page.clickToCopy')} use:copyLog={error}>{error}</p>{/if}

    <div class="chart-wrap" class:fullscreen>
      {#if fullscreen}
        <button class="fs-close" title={$t('histviz.page.exitFullscreenEsc')} onclick={() => (fullscreen = false)}
          ><Icon name="x" size={13} /></button
        >
      {/if}
      {#if loading}
        <p class="hint">{$t('common.loading')}</p>
      {:else if bars}
        <Chart {bars} {type} {instances} {brick} {settings} ontoggle={toggle} />
      {:else}
        <p class="hint">{$t('histviz.page.selectDataset')}</p>
      {/if}
    </div>
  </main>
</div>
</RequireModule>

<svelte:window onkeydown={onKey} />

<IndicatorModal bind:open={modalOpen} edit={editing} onsave={onSave} onclose={() => (editing = null)} />

<style>
  .page {
    height: 100%;
    display: grid;
    grid-template-columns: 280px 1fr;
    overflow: hidden;
  }
  aside {
    border-right: 1px solid var(--border);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    overflow-y: auto;
  }
  .aside-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h1 {
    font-size: 1.1rem;
    font-weight: 700;
  }
  hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 0;
  }
  main {
    display: flex;
    flex-direction: column;
    padding: var(--space-4);
    gap: var(--space-3);
    overflow: hidden;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    /* Allow the settings popover to escape the toolbar. */
    overflow: visible;
  }
  .settings-wrap {
    position: relative;
  }
  /* When no dataset title pushes it, keep the gear at the right edge. */
  .settings-wrap.pushed {
    margin-left: auto;
  }
  .group {
    display: flex;
    gap: var(--space-1);
  }
  .toolbar button {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
  }
  .toolbar button.active {
    color: var(--text);
    border-color: var(--accent);
  }
  .brick {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.8rem;
    color: var(--muted);
  }
  .brick input {
    width: 90px;
  }
  .title {
    margin-left: auto;
    font-weight: 600;
    font-size: 0.85rem;
  }
  .chart-wrap {
    position: relative;
    flex: 1;
    min-height: 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-2);
  }
  .chart-wrap.fullscreen {
    position: fixed;
    inset: 0;
    z-index: 100;
    border-radius: 0;
    border: none;
    padding: var(--space-3);
  }
  .fs {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--muted);
    font-size: 0.9rem;
    cursor: pointer;
  }
  .fs:hover {
    color: var(--text);
    border-color: var(--accent);
  }
  .fs-close {
    position: absolute;
    top: var(--space-3);
    right: var(--space-3);
    z-index: 110;
    width: 30px;
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 0.9rem;
    cursor: pointer;
  }
  .fs-close:hover {
    color: var(--text);
    border-color: var(--accent);
  }
  .hint {
    color: var(--muted);
    padding: var(--space-4);
  }
  .err {
    color: var(--red);
  }
</style>
