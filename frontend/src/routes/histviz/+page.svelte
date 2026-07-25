<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Historical Data Visualization — chart any instrument a connector serves, stored or not.
  //
  // The chart is not bound to a dataset: it holds *coordinates* (connector, asset type,
  // ticker, timeframe) and asks /api/histviz/series for one window at a time. The server
  // answers with bars already in the catalog plus only the missing edges fetched from the
  // provider, so nothing is ever downloaded twice.
  //
  // Left: two tabs — Indicators (managed instances) and Data (one search box over every
  // connector granted to the chart, with a provider whitelist). Right: toolbar (timeframe
  // strip, chart type, save, live) over an ECharts pane.
  //
  // History is walked backwards *on demand*: the chart shows a "load more" button at its
  // left edge and each click asks for the previous 1500 bars. No provider request ever
  // happens without a user gesture, which is what keeps a metered API key predictable.
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { suggestBrick } from '$lib/modules/histviz/indicators.js';
  import DataTab from '$lib/modules/histviz/DataTab.svelte';
  import IndicatorPanel from '$lib/modules/histviz/IndicatorPanel.svelte';
  import IndicatorModal from '$lib/modules/histviz/IndicatorModal.svelte';
  import ChartSettings from '$lib/modules/histviz/ChartSettings.svelte';
  import Chart from '$lib/modules/histviz/Chart.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import { DEFAULT_SETTINGS, normalizeSettings } from '$lib/modules/histviz/settings.js';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  // ── Persisted session state: instrument/type/brick/indicators live per-browser in
  // localStorage; chart *settings* are a fixed global config persisted server-side. ──
  const STORE_KEY = 'otw.histviz.session.v2';
  function loadStore() {
    try {
      return JSON.parse(localStorage.getItem(STORE_KEY) || '{}');
    } catch {
      return {};
    }
  }
  const saved = loadStore();

  /** Bars per "load more" click — and per first paint. */
  const SLICE_BARS = 1500;
  /** Seconds in one canonical timeframe (mirrors the Rust `timeframe_secs`). */
  const TF_SECS = { '1m': 60, '5m': 300, '15m': 900, '1h': 3600, '4h': 14400, '1d': 86400, '1w': 604800 };
  // Timeframe strip order — by duration, the way every market tool orders it.
  const TF_ORDER = ['1m', '2m', '5m', '10m', '15m', '30m', '45m', '1h', '2h', '3h', '4h', '6h', '8h', '12h', '1d', '3d', '1w'];

  // Chart coordinates: { connector_id, provider, asset_type, ticker, timeframe,
  //                      timeframes[], stream, name? }
  let coords = $state(saved.coords ?? null);
  let datasets = $state([]);
  let providers = $state([]); // capability matrix, for timeframes of a bare provider
  let bars = $state(null);
  let meta = $state(null); // last series response: stored dataset, counts, history_start
  let notice = $state(null); // typed reason a window came back short ({code, message})
  let type = $state(saved.type ?? 'candlestick');
  let brick = $state(saved.brick ?? 0);
  let error = $state('');
  let loading = $state(false);
  let loadingMore = $state(false);
  let atHistoryStart = $state(true);
  let saving = $state(false);
  let savedFlash = $state('');
  // Aside tab: 'data' first when nothing is charted yet, else straight to indicators.
  let tab = $state(saved.coords ? 'indicators' : 'data');
  let settings = $state({ ...DEFAULT_SETTINGS });
  let settingsOpen = $state(false);
  let settingsLoaded = $state(false); // gate the save-effect until the DB load lands
  let fullscreen = $state(false);
  let chartRef = $state(null);
  // Time span to re-open after a timeframe switch, so the chart lands on the same dates.
  let restoreRange = $state(null);
  // Recently charted instruments (coordinate objects) — what the Data tab lists first.
  let recents = $state(Array.isArray(saved.recents) ? saved.recents : []);

  // ── Live market data ──
  // Live streams forming bars over SSE and mutates the last candle. The feed is addressed by
  // coordinates and writes nothing: any instrument a stream-capable provider serves goes live,
  // stored or not, and it turns itself on as soon as the newest bar is the one forming now.
  let live = $state(false);
  let liveConnected = $state(false);
  let liveLag = $state(null); // ms behind the exchange, from the last event
  let es = null; // EventSource
  let pendingBar = null; // latest unclosed bar awaiting the next coalesced flush
  let flushTimer = null;
  let autoLiveKey = null; // coordinates we already auto-started live for

  const canLive = $derived(!!coords?.stream);
  const isStored = $derived(!!meta?.stored);
  const coordKey = $derived(
    coords ? `${coords.provider}|${coords.asset_type}|${coords.ticker}|${coords.timeframe}` : ''
  );

  const tfSecs = $derived(TF_SECS[coords?.timeframe] ?? 3600);
  /** Timeframes the current connector supports, ordered by duration. */
  const timeframes = $derived.by(() => {
    if (!coords) return [];
    const fromCoords = coords.timeframes?.length ? coords.timeframes : null;
    const fromMatrix = providers.find((p) => p.provider === coords.provider)?.timeframes ?? [];
    const list = fromCoords ?? fromMatrix;
    return [...list].sort((a, b) => TF_ORDER.indexOf(a) - TF_ORDER.indexOf(b));
  });

  // Exit fullscreen on Escape.
  function onKey(e) {
    if (e.key === 'Escape' && fullscreen) fullscreen = false;
  }

  // Active indicator instances (restored from storage; ids kept so nextId stays unique).
  let instances = $state(saved.instances ?? []);
  let modalOpen = $state(false);
  let editing = $state(null);
  let nextId = (saved.instances ?? []).reduce((m, i) => Math.max(m, i.id), 0) + 1;

  // Why a window came back short, in the user's words. Unknown codes read as a plain
  // provider problem rather than leaking a raw key into the UI.
  const NOTICE_KEYS = {
    auth: 'histviz.notice.auth',
    quota: 'histviz.notice.quota',
    rate_limit: 'histviz.notice.rateLimit',
    symbol: 'histviz.notice.symbol',
    depth: 'histviz.notice.depth',
    empty: 'histviz.notice.emptyLabel',
    history_start: 'histviz.notice.historyStartLabel',
    provider: 'histviz.notice.provider'
  };
  const noticeKey = (code) => NOTICE_KEYS[code] ?? NOTICE_KEYS.provider;

  const CHART_TYPES = [
    ['candlestick', 'histviz.page.typeCandles'],
    ['ohlc', 'histviz.page.typeOhlc'],
    ['line', 'histviz.page.typeLine'],
    ['renko', 'histviz.page.typeRenko']
  ];

  // Persist session bits to localStorage (after restore, to avoid clobbering with defaults).
  let restored = $state(false);
  $effect(() => {
    const snap = { coords, type, brick, instances, recents };
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

  async function loadCatalog() {
    datasets = await histvizApi.datasets();
    if (!providers.length) {
      try {
        providers = await histvizApi.providers();
      } catch {
        /* the capability matrix is only used for fallbacks */
      }
    }
  }

  // ── Instrument selection ──

  /** Chart an instrument. Keeps the current timeframe when the connector supports it. */
  async function openCoords(next) {
    if (!next?.ticker) return;
    const list = next.timeframes?.length
      ? next.timeframes
      : (providers.find((p) => p.provider === next.provider)?.timeframes ?? []);
    const tf =
      next.timeframe && list.includes(next.timeframe)
        ? next.timeframe
        : list.includes(coords?.timeframe)
          ? coords.timeframe
          : (list.find((x) => x === '1h') ?? list.find((x) => x === '1d') ?? list[0] ?? '1h');
    coords = { ...next, timeframe: tf, timeframes: list };
    remember(coords);
    await loadSeries();
  }

  function remember(c) {
    const key = `${c.provider}|${c.asset_type}|${c.ticker}`;
    recents = [
      { ...c },
      ...recents.filter((r) => `${r.provider}|${r.asset_type}|${r.ticker}` !== key)
    ].slice(0, 8);
  }

  /** First paint of the current coordinates: the newest slice, nothing older. */
  async function loadSeries() {
    if (!coords) return;
    loading = true;
    error = '';
    notice = null;
    savedFlash = '';
    live = false;
    try {
      const r = await histvizApi.series({ ...coords, bars: SLICE_BARS });
      bars = r;
      meta = r;
      notice = r.notice ?? null;
      atHistoryStart = !!r.history_start;
      if (!r.count && !r.notice) notice = { code: 'empty', message: $t('histviz.notice.empty') };
      if (type === 'renko' && !brick) brick = suggestBrick(bars.c);
      maybeAutoLive(r);
    } catch (e) {
      error = e.message;
      bars = null;
      meta = null;
    } finally {
      loading = false;
    }
  }

  /** "Load more": the 1500 bars just before the oldest one on screen. User-triggered —
   *  stored bars are reused, only the real gap is downloaded. */
  async function loadMore() {
    if (loadingMore || atHistoryStart || !coords || !bars?.ts?.length) return;
    loadingMore = true;
    notice = null;
    try {
      const r = await histvizApi.series({ ...coords, to: bars.ts[0], bars: SLICE_BARS });
      notice = r.notice ?? null;
      const got = r.ts?.length ?? 0;
      if (!got) {
        atHistoryStart = !r.notice; // a failure is retryable; genuinely empty is the end
        if (!r.notice) notice = { code: 'history_start', message: $t('histviz.notice.historyStart') };
        return;
      }
      bars = {
        ...bars,
        count: (bars.count ?? bars.ts.length) + got,
        ts: [...r.ts, ...bars.ts],
        o: [...r.o, ...bars.o],
        h: [...r.h, ...bars.h],
        l: [...r.l, ...bars.l],
        c: [...r.c, ...bars.c],
        v: [...r.v, ...bars.v]
      };
      if (r.history_start) atHistoryStart = true;
    } catch (e) {
      notice = { code: 'provider', message: e.message };
    } finally {
      loadingMore = false;
    }
  }

  function switchTimeframe(tf) {
    if (!coords || tf === coords.timeframe) return;
    restoreRange = chartRef?.getTimeRange?.() ?? null;
    coords = { ...coords, timeframe: tf };
    remember(coords);
    loadSeries();
  }

  /** Store what is on screen: queues the normal download job over the loaded window. */
  async function saveWindow(scope = 'loaded') {
    if (!coords || !bars?.ts?.length) return;
    saving = true;
    error = '';
    savedFlash = '';
    try {
      const view = scope === 'visible' ? (chartRef?.getTimeRange?.() ?? null) : null;
      const from = view?.t0 ?? bars.ts[0];
      const lastTs = view?.t1 ?? bars.ts[bars.ts.length - 1];
      const to = new Date(Date.parse(lastTs) + tfSecs * 1000).toISOString();
      await histvizApi.savePreview({ ...coords, from, to });
      savedFlash = $t('histviz.page.savedQueued');
      // The worker writes asynchronously; refresh the catalog so the badge flips over.
      const stored = await waitForStore();
      // The live feed only records once its coordinates have a dataset: reconnect so the
      // server picks the new one up instead of waiting for the session to expire.
      if (stored && live && canLive) openStream(coords);
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  /** Watch the catalog until the queued job has written something for these coordinates. */
  async function waitForStore() {
    for (let i = 0; i < 20; i++) {
      await new Promise((r) => setTimeout(r, 600));
      datasets = await histvizApi.datasets();
      const d = datasets.find(
        (x) =>
          x.provider === coords.provider &&
          x.asset_type === coords.asset_type &&
          x.ticker === coords.ticker &&
          x.timeframe === coords.timeframe
      );
      if (d?.bar_count) {
        meta = { ...(meta ?? {}), stored: { id: d.id, bar_count: d.bar_count } };
        return d;
      }
    }
    return null;
  }

  // ── Live stream lifecycle ──
  // Whenever the toggle is on for a stream-capable instrument, an SSE connection is open; the
  // effect's cleanup closes it on toggle-off, instrument switch, or unmount.
  $effect(() => {
    void coordKey; // re-open on any coordinate change
    if (live && canLive && coords) openStream(coords);
    else closeStream();
    return closeStream;
  });
  // Live isn't meaningful for a REST-only provider; drop it when it can't stream.
  $effect(() => {
    if (live && !canLive) live = false;
  });

  /** Live is the default view of a current instrument: if the newest bar is the one forming
   *  right now, connect without being asked. Nothing is stored by watching. */
  function maybeAutoLive(r) {
    if (!r?.ts?.length || !coords?.stream) return;
    if (autoLiveKey === coordKey) return;
    const lastMs = Date.parse(r.ts[r.ts.length - 1]);
    const fresh = Date.now() - lastMs < tfSecs * 2000;
    if (!fresh) return;
    autoLiveKey = coordKey;
    live = true;
  }

  function openStream(c) {
    closeStream();
    const src = new EventSource(histvizApi.streamUrl(c));
    const onMsg = (e) => {
      try {
        onLiveBar(JSON.parse(e.data));
      } catch {
        /* ignore malformed frame */
      }
    };
    // Both the connect-time snapshot (forming bar) and every update carry the same shape.
    src.addEventListener('snapshot', onMsg);
    src.addEventListener('bar', onMsg);
    src.onopen = () => (liveConnected = true);
    src.onerror = () => (liveConnected = false); // the browser auto-reconnects
    es = src;
  }

  function closeStream() {
    if (es) {
      es.close();
      es = null;
    }
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    pendingBar = null;
    liveConnected = false;
    liveLag = null;
  }

  // Closed bars apply immediately (the boundary must be crisp); forming-bar updates are
  // coalesced to ~2.5 Hz so a busy market doesn't re-render the chart on every tick.
  function onLiveBar(d) {
    liveLag = d.lag_ms ?? null;
    if (d.closed) {
      pendingBar = null;
      applyBar(d);
      return;
    }
    pendingBar = d;
    if (!flushTimer) {
      flushTimer = setTimeout(() => {
        flushTimer = null;
        if (pendingBar) {
          applyBar(pendingBar);
          pendingBar = null;
        }
      }, 400);
    }
  }

  // Merge one bar into `bars` by its timestamp: replace the last candle, append a new one, or
  // ignore an older one. `bars` is deeply reactive, so in-place mutation redraws the chart.
  function applyBar(d) {
    if (!bars?.ts?.length) {
      bars = { ...(bars ?? {}), ts: [d.ts], o: [d.o], h: [d.h], l: [d.l], c: [d.c], v: [d.v] };
      return;
    }
    const n = bars.ts.length;
    const lastMs = Date.parse(bars.ts[n - 1]);
    const evMs = Date.parse(d.ts);
    if (evMs === lastMs) {
      bars.o[n - 1] = d.o;
      bars.h[n - 1] = d.h;
      bars.l[n - 1] = d.l;
      bars.c[n - 1] = d.c;
      bars.v[n - 1] = d.v;
    } else if (evMs > lastMs) {
      bars.ts.push(d.ts);
      bars.o.push(d.o);
      bars.h.push(d.h);
      bars.l.push(d.l);
      bars.c.push(d.c);
      bars.v.push(d.v);
    }
  }

  /** Toolbar toggle. Turning it off also stops it from turning itself back on for these
   *  coordinates, otherwise the next reload would override the choice. */
  function toggleLive() {
    live = !live;
    autoLiveKey = coordKey;
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

  // Initial load, then open from ?dataset= (deep-link from the download module) or the
  // instrument this browser was last looking at.
  let didDeepLink = false;
  $effect(() => {
    loadCatalog()
      .then(() => {
        if (didDeepLink) return;
        didDeepLink = true;
        const id = $page.url.searchParams.get('dataset');
        const d = id ? datasets.find((x) => x.id === id) : null;
        if (d) {
          openCoords({
            connector_id: null,
            provider: d.provider,
            asset_type: d.asset_type,
            ticker: d.ticker,
            timeframe: d.timeframe,
            stream: providers.find((p) => p.provider === d.provider)?.stream ?? false
          });
        } else if (coords) {
          loadSeries();
        }
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
    <div class="tabs" role="tablist">
      <button
        role="tab"
        aria-selected={tab === 'indicators'}
        class:active={tab === 'indicators'}
        onclick={() => (tab = 'indicators')}
      >
        {$t('histviz.panel.title')}
        {#if instances.length}<span class="count">{instances.length}</span>{/if}
      </button>
      <button
        role="tab"
        aria-selected={tab === 'data'}
        class:active={tab === 'data'}
        onclick={() => (tab = 'data')}
      >
        {$t('histviz.data.tab')}
      </button>
    </div>

    {#if tab === 'indicators'}
      <IndicatorPanel
        {instances}
        onadd={openAdd}
        onedit={openEdit}
        ontoggle={toggle}
        onremove={remove}
      />
    {:else}
      <DataTab
        {datasets}
        {recents}
        selected={coords}
        busy={loading}
        onselect={openCoords}
        ondownload={() => goto('/histdata')}
      />
    {/if}
  </aside>

  <main>
    <div class="toolbar">
      {#if timeframes.length > 1}
        <!-- A strip of one button per bar size ate the toolbar; a select says the same
             thing in one control and still shows the current timeframe. -->
        <select
          class="tf"
          aria-label={$t('histviz.page.timeframe')}
          title={$t('histviz.page.timeframe')}
          value={coords?.timeframe}
          onchange={(e) => switchTimeframe(e.currentTarget.value)}
        >
          {#each timeframes as tf (tf)}<option value={tf}>{tf}</option>{/each}
        </select>
      {/if}
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
      {#if coords && bars}
        {#if isStored}
          <span class="stored-tag" title={$t('histviz.page.storedHint', { bars: meta?.stored?.bar_count ?? 0 })}>
            <Icon name="database" size={12} /> {$t('histviz.page.stored')}
          </span>
        {:else}
          <span class="unsaved" title={$t('histviz.page.unsavedHint')}>
            <Icon name="database" size={12} /> {$t('histviz.page.unsaved')}
          </span>
        {/if}
        <button class="save-btn" onclick={() => saveWindow('loaded')} disabled={saving} title={$t('histviz.page.saveHint')}>
          <Icon name="save" size={13} />
          {saving ? $t('histviz.page.saving') : $t('histviz.page.save')}
        </button>
        {#if savedFlash}<span class="flash">{savedFlash}</span>{/if}
      {/if}
      {#if canLive}
        <button
          class="live-btn"
          class:on={live}
          title={live ? $t('histviz.page.liveStop') : $t('histviz.page.liveStart')}
          onclick={toggleLive}
        >
          <span class="dot" class:connected={live && liveConnected}></span>
          {live ? $t('histviz.page.live') : $t('histviz.page.goLive')}
        </button>
        {#if live && liveConnected && liveLag != null}
          <span class="lag" title={$t('histviz.page.liveLag')}>{liveLag} ms</span>
        {/if}
      {/if}
      {#if coords}
        <span class="title">{coords.ticker} · {coords.timeframe} · {coords.provider}</span>
      {/if}
      <div class="settings-wrap" class:pushed={!coords}>
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
      <button
        class="fs"
        title={fullscreen ? $t('histviz.page.exitFullscreenEsc') : $t('histviz.page.fullscreenChart')}
        onclick={() => (fullscreen = !fullscreen)}
      >
        <Icon name={fullscreen ? 'chevrons-down-up' : 'maximize'} size={14} />
      </button>
    </div>

    <ErrorText error={error} copyable />

    <!-- A provider that can't serve part of the window is a fact about the data, not a
         failure of the app: say what is missing and keep the bars that did arrive. -->
    {#if notice}
      <p class="notice" class:hard={!bars?.count}>
        <Icon name="alert-triangle" size={12} />
        <span class="code">{$t(noticeKey(notice.code))}</span>
        <span>{notice.message}</span>
      </p>
    {/if}

    <div class="chart-wrap" class:fullscreen>
      {#if fullscreen}
        <button class="fs-close" title={$t('histviz.page.exitFullscreenEsc')} onclick={() => (fullscreen = false)}
          ><Icon name="x" size={13} /></button
        >
      {/if}
      {#if loading}
        <div class="sk-chart" aria-busy="true"><Skeleton height="100%" /></div>
      {:else if bars?.count}
        <Chart
          bind:this={chartRef}
          {bars}
          {type}
          {instances}
          {brick}
          {settings}
          {fullscreen}
          {restoreRange}
          {loadingMore}
          {atHistoryStart}
          loadMoreLabel={$t('histviz.chart.loadMore', { bars: SLICE_BARS })}
          loadMoreNotice={notice?.message ?? ''}
          ontoggle={toggle}
          onfullscreen={() => (fullscreen = !fullscreen)}
          onrestored={() => (restoreRange = null)}
          onloadmore={loadMore}
        />
      {:else}
        <p class="hint">{$t('histviz.page.selectInstrument')}</p>
      {/if}
    </div>
  </main>
</div>
</RequireModule>

<svelte:window onkeydown={onKey} />

<IndicatorModal bind:open={modalOpen} edit={editing} onsave={onSave} onclose={() => (editing = null)} />

<style>
  /* Fill the chart area so the skeleton occupies the same box the chart will. */
  .sk-chart {
    flex: 1;
    min-height: 0;
    padding: var(--space-3);
  }
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
    /* The aside never scrolls itself — the active tab's list owns the leftover
       height and scrolls internally. */
    overflow: hidden;
    min-height: 0;
  }
  .aside-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex: none;
  }
  h1 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .tabs {
    display: flex;
    gap: var(--space-4);
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .tabs button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 0 var(--space-2);
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.05em;
    color: var(--muted);
    cursor: pointer;
    transition: color 0.12s ease, border-color 0.12s ease;
  }
  .tabs button:hover {
    color: var(--text);
  }
  .tabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .count {
    padding: 1px 5px;
    border-radius: var(--radius);
    background: var(--surface-2);
    border: 1px solid var(--border);
    font-size: var(--text-xs);
    letter-spacing: 0;
    color: var(--muted);
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
  /* When no instrument title pushes it, keep the gear at the right edge. */
  .settings-wrap.pushed {
    margin-left: auto;
  }
  .group {
    display: flex;
    gap: var(--space-1);
  }
  select.tf {
    font-family: var(--mono);
    font-size: var(--text-sm);
    width: 84px;
  }
  .toolbar button {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
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
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .brick input {
    width: 90px;
  }
  .title {
    margin-left: auto;
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
  }
  .live-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .live-btn.on {
    color: var(--text);
    border-color: var(--red);
  }
  .live-btn .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }
  /* Solid + pulsing once the exchange feed is actually connected. */
  .live-btn.on .dot.connected {
    background: var(--red);
    animation: live-pulse 1.6s ease-in-out infinite;
  }
  @keyframes live-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }
  /* "Not stored" / "stored" are states, not alerts: muted text, no pill. */
  .unsaved,
  .stored-tag {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--amber);
  }
  .stored-tag {
    color: var(--muted);
  }
  .save-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .flash,
  .lag {
    font-size: var(--text-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .notice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--amber);
    line-height: 1.4;
  }
  .notice.hard {
    color: var(--red);
  }
  .notice .code {
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.85;
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
  /* A fullscreen chart covers the viewport, so it belongs on the modal rung — at the
     old raw 100 it tied --z-dropdown and an open dropdown could paint over it. */
  .chart-wrap.fullscreen {
    position: fixed;
    inset: 0;
    z-index: var(--z-modal);
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
    font-size: var(--text-base);
    cursor: pointer;
  }
  .fs:hover {
    color: var(--text);
    border-color: var(--border-control);
  }
  /* Inside .chart-wrap.fullscreen, which already establishes a stacking context —
     this only has to beat its siblings, not the global ladder. */
  .fs-close {
    position: absolute;
    top: var(--space-3);
    right: var(--space-3);
    z-index: 1;
    width: 30px;
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .fs-close:hover {
    color: var(--text);
    border-color: var(--border-control);
  }
  .hint {
    color: var(--muted);
    padding: var(--space-4);
  }
</style>
