<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Historical Data Visualization — chart any instrument a connector serves, stored or not.
  //
  // The chart is not bound to a dataset: it holds *coordinates* (connector, asset type,
  // ticker, timeframe) and asks /api/histviz/series for one window at a time. The server
  // answers with bars already in the catalog plus only the missing edges fetched from the
  // provider, so nothing is ever downloaded twice.
  //
  // The chart owns the whole page: a toolbar (symbol, timeframe, chart type, indicators,
  // backtest, save, live) over an ECharts pane, the drawing rail against its left edge and
  // the quick-backtest book under it. The instrument picker and the indicator builder are
  // modals — nothing is parked in a side panel.
  //
  // History is walked backwards *on demand*: the chart shows a "load more" button at its
  // left edge and each click asks for the previous 1500 bars. No provider request ever
  // happens without a user gesture, which is what keeps a metered API key predictable.
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { suggestBrick } from '$lib/modules/histviz/indicators.js';
  import DataModal from '$lib/modules/histviz/DataModal.svelte';
  import IndicatorModal from '$lib/modules/histviz/IndicatorModal.svelte';
  import DrawToolbar from '$lib/modules/histviz/DrawToolbar.svelte';
  import QuickPanel from '$lib/modules/histviz/QuickPanel.svelte';
  import { replay, makeFill, flipSide, resolveQty, DEFAULT_QTY } from '$lib/modules/histviz/quicktest.js';
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
  // Instrument picker, opened from the symbol button (there is no side panel any more: the
  // chart owns the whole width, indicators are managed on the chart itself).
  let dataOpen = $state(false);
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
    // Ctrl/Cmd+Z undoes the board you are working on: the quick session while it is open and
    // has fills, the drawings otherwise. Never while typing in a field.
    if ((e.key === 'z' || e.key === 'Z') && (e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey) {
      const el = e.target;
      const tag = el?.tagName;
      if (el?.isContentEditable || tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      if (quick && quickFills.length) quickUndo();
      else if (drawings.length) undoDrawing();
      else return;
      e.preventDefault();
    }
  }

  // Active indicator instances (restored from storage; ids kept so nextId stays unique).
  let instances = $state(saved.instances ?? []);
  let modalOpen = $state(false);
  let editing = $state(null);
  let nextId = (saved.instances ?? []).reduce((m, i) => Math.max(m, i.id), 0) + 1;

  // Custom instances carry a copy of their definition so the chart still draws offline, but
  // the library is the source of truth: on load, refresh every instance from its row. An edit
  // made in the backtest module therefore lands on the chart, and a deleted row leaves the
  // instance drawing its last known definition (id cleared, so it stops chasing a dead row).
  let libSynced = false;
  $effect(() => {
    if (libSynced) return;
    libSynced = true;
    const restoredList = untrack(() => instances);
    if (!restoredList.some((i) => i.type === 'custom' && i.custom?.id)) return;
    indicatorLib
      .list()
      .then((rows) => {
        instances = untrack(() => instances).map((i) => {
          if (i.type !== 'custom' || !i.custom?.id) return i;
          const row = rows.find((r) => r.id === i.custom.id);
          if (!row) return { ...i, custom: { ...i.custom, id: null } };
          return { ...i, custom: { id: row.id, name: row.name, def: row.definition } };
        });
      })
      .catch(() => {
        /* offline / library unreachable — keep the embedded definitions */
      });
  });

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
  const typeOpts = $derived(CHART_TYPES.map(([value, key]) => ({ value, label: $t(key) })));
  const tfOpts = $derived(timeframes.map((tf) => ({ value: tf, label: tf })));

  // ── Drawings ──
  // Objects are anchored in time+price by the chart; the page holds the list and the armed
  // tool so the rail can sit beside the chart box instead of floating over it.
  //
  // They are kept **per instrument, not per timeframe**: an anchor is an instant and a price,
  // so a trend line drawn on the 1h is the same line on the 15m. The store is a per-browser
  // map keyed by connector coordinates, capped so it cannot grow forever.
  const DRAW_KEY = 'otw.histviz.drawings.v1';
  const DRAW_MAX_INSTRUMENTS = 60;
  function loadDrawStore() {
    try {
      const raw = JSON.parse(localStorage.getItem(DRAW_KEY) || '{}');
      return raw && typeof raw === 'object' ? raw : {};
    } catch {
      return {};
    }
  }
  let drawStore = loadDrawStore();

  let tool = $state('cursor');
  let drawings = $state([]);
  let selectedDrawing = $state(null);
  let magnet = $state(saved.magnet ?? false);
  let drawingsHidden = $state(saved.drawingsHidden ?? false);

  const instrumentKey = $derived(
    coords ? `${coords.provider}|${coords.asset_type}|${coords.ticker}` : ''
  );

  // Switching instrument swaps the board; the previous one keeps its objects.
  let loadedKey = null;
  $effect(() => {
    const key = instrumentKey;
    if (key === loadedKey) return;
    loadedKey = key;
    drawings = key ? (drawStore[key] ?? []) : [];
    selectedDrawing = null;
  });

  // Save on every edit. Writing localStorage is not reactive state, so this cannot loop.
  $effect(() => {
    const key = untrack(() => loadedKey);
    const list = drawings;
    if (!key) return;
    if (list.length) drawStore[key] = list;
    else delete drawStore[key];
    const keys = Object.keys(drawStore);
    for (const k of keys.slice(0, Math.max(0, keys.length - DRAW_MAX_INSTRUMENTS))) delete drawStore[k];
    try {
      localStorage.setItem(DRAW_KEY, JSON.stringify(drawStore));
    } catch {
      /* quota — the board stays in memory for this session */
    }
  });

  // ── Chart layout, persisted server-side per dataset ──
  // Drawings and indicators live in this browser while you work, but a *stored* instrument
  // also carries its chart on the server: reopen it anywhere and it comes back the way you
  // left it. Only a stored dataset can carry one — an instrument you are merely looking at
  // has nothing to hang it on, which is what the "autosave data" switch below is for.
  const datasetId = $derived(meta?.stored?.id ?? null);
  let layoutLoadedFor = $state(null); // dataset whose layout has been applied
  let layoutTimer = null;
  let pendingLayout = null; // { id, snap } still inside the debounce window

  function applyLayout(l) {
    if (typeof l.type === 'string') type = l.type;
    if (Number.isFinite(l.brick)) brick = l.brick;
    if (Array.isArray(l.instances)) {
      instances = l.instances;
      nextId = instances.reduce((m, i) => Math.max(m, i.id ?? 0), 0) + 1;
    }
    if (Array.isArray(l.drawings)) {
      drawings = l.drawings;
      selectedDrawing = null;
    }
  }

  $effect(() => {
    const id = datasetId;
    if (!id || id === untrack(() => layoutLoadedFor)) return;
    histvizApi
      .layout(id)
      .then((row) => {
        if (untrack(() => datasetId) !== id) return; // the instrument changed meanwhile
        if (row && typeof row === 'object') applyLayout(row);
        layoutLoadedFor = id;
      })
      .catch(() => {
        layoutLoadedFor = id; // offline: keep working from the local copy
      });
  });

  // Autosave, debounced. Gated on the load having landed, so the first frame of a fresh
  // instrument can never overwrite the layout it is about to receive.
  $effect(() => {
    const id = datasetId;
    const snap = {
      type,
      brick,
      instances: $state.snapshot(instances),
      drawings: $state.snapshot(drawings)
    };
    if (!id || untrack(() => layoutLoadedFor) !== id) return;
    clearTimeout(layoutTimer);
    pendingLayout = { id, snap };
    layoutTimer = setTimeout(() => {
      pendingLayout = null;
      histvizApi.saveLayout(id, snap).catch(() => {
        /* the browser copy is still there; nothing to tell the user */
      });
    }, 800);
  });

  /** Leaving the page inside the debounce window would lose the last edit — and the layout
   *  is authoritative, so the *next* load would then undo it. `keepalive` lets the request
   *  outlive the document. */
  function flushLayout() {
    if (!pendingLayout) return;
    const { id, snap } = pendingLayout;
    pendingLayout = null;
    clearTimeout(layoutTimer);
    try {
      fetch(`/api/histviz/layouts/${id}`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(snap),
        keepalive: true
      });
    } catch {
      /* nothing more we can do on the way out */
    }
  }

  // "Autosave data": store the instrument the first time it is charted, so the layout above
  // has a home. Off by default — it queues a download, and a download spends an API key.
  let autoStoreKey = null;
  $effect(() => {
    if (!settings.autosaveData || !coords || !bars?.count || isStored) return;
    const key = coordKey;
    if (autoStoreKey === key) return;
    autoStoreKey = key;
    untrack(() => saveWindow('loaded'));
  });

  function deleteDrawing() {
    if (!selectedDrawing) return;
    drawings = drawings.filter((d) => d.id !== selectedDrawing);
    selectedDrawing = null;
  }
  function clearDrawings() {
    drawings = [];
    selectedDrawing = null;
  }
  /** Take back the last shape drawn. Both boards (drawings and the quick session) are
   *  append-only scratchpads, so "undo" is simply dropping what was added last. */
  function undoDrawing() {
    if (!drawings.length) return;
    const last = drawings[drawings.length - 1];
    drawings = drawings.slice(0, -1);
    if (selectedDrawing === last.id) selectedDrawing = null;
  }

  // ── Quick backtest ──
  // Clicking the chart posts *fills* (side, instant, price); the engine replays them into a
  // position, closed trades and the markers the chart paints. Like drawings, they are anchored
  // in time+price and kept per instrument in this browser — a click-through session is a
  // scratchpad, not journal data, so nothing goes to the server.
  const QUICK_KEY = 'otw.histviz.quick.v1';
  function loadQuickStore() {
    try {
      const raw = JSON.parse(localStorage.getItem(QUICK_KEY) || '{}');
      return raw && typeof raw === 'object' ? raw : {};
    } catch {
      return {};
    }
  }
  let quickStore = loadQuickStore();

  let quick = $state(saved.quick ?? false);
  let quickFills = $state([]);
  // What the size box holds, and how it is counted: units, contracts (× a point value) or
  // an amount of quote currency. Only the engine's units reach the fills.
  let quickSize = $state(saved.quickSize ?? DEFAULT_QTY);
  let quickUnit = $state(saved.quickUnit ?? 'units');
  let quickContract = $state(saved.quickContract ?? 1);
  let quickExpanded = $state(saved.quickExpanded ?? false);
  let quickHeight = $state(saved.quickHeight ?? 240);
  let quickTab = $state(saved.quickTab ?? 'trades');

  // MAE (and therefore R) is measured on the real bars, not the renko/line view.
  const quickBars = $derived.by(() => {
    if (!bars?.ts?.length) return null;
    return { ms: bars.ts.map((s) => Date.parse(s)), h: bars.h, l: bars.l };
  });
  const session = $derived(replay(quickFills, quickBars));
  const lastClose = $derived(bars?.c?.length ? bars.c[bars.c.length - 1] : null);

  let quickLoadedKey = null;
  $effect(() => {
    const key = instrumentKey;
    if (key === quickLoadedKey) return;
    quickLoadedKey = key;
    quickFills = key ? (quickStore[key] ?? []) : [];
  });

  $effect(() => {
    const key = untrack(() => quickLoadedKey);
    const list = quickFills;
    if (!key) return;
    if (list.length) quickStore[key] = list;
    else delete quickStore[key];
    const keys = Object.keys(quickStore);
    for (const k of keys.slice(0, Math.max(0, keys.length - DRAW_MAX_INSTRUMENTS))) delete quickStore[k];
    try {
      localStorage.setItem(QUICK_KEY, JSON.stringify(quickStore));
    } catch {
      /* quota — the session stays in memory */
    }
  });

  /** One click on the chart: open a position, or close the one that is open. */
  function quickClick(p) {
    const open = session.open;
    if (open) quickFills = [...quickFills, makeFill(open.dir > 0 ? 'short' : 'long', p.x, p.y, open.qty)];
    else quickFills = [...quickFills, makeFill('long', p.x, p.y, resolveQty(quickUnit, quickSize, p.y, quickContract))];
  }
  /** The long-press window: an explicit side and size — this is how you pyramid. */
  function quickFill(f) {
    quickSize = f.size;
    quickFills = [...quickFills, makeFill(f.side, f.x, f.y, resolveQty(quickUnit, f.size, f.y, quickContract))];
  }
  /** Clicking an arrow flips that fill long↔short; the whole session re-derives around it. */
  function quickToggle(id) {
    quickFills = quickFills.map((f) => (f.id === id ? { ...f, side: flipSide(f.side) } : f));
  }
  function quickRemoveTrade(trade) {
    const ids = new Set(trade.fillIds ?? []);
    quickFills = quickFills.filter((f) => !ids.has(f.id));
  }
  function quickReset() {
    quickFills = [];
  }
  /** Undo the last click on the chart — the fill, not the trade: a closing fill goes back to
   *  an open position, exactly the state before the click. */
  function quickUndo() {
    if (quickFills.length) quickFills = quickFills.slice(0, -1);
  }

  // ── Backtest handoff ──
  // Two doors out of the chart: the quick session above, and the real engine (the backtest
  // module). The engine only reads stored datasets, so the instrument is saved first when it
  // isn't in the catalog yet.
  let handoff = $state(false);

  async function toBacktest() {
    if (!coords || !bars?.count || handoff) return;
    handoff = true;
    error = '';
    try {
      let id = meta?.stored?.id ?? null;
      if (!id) {
        await saveWindow('loaded');
        id = meta?.stored?.id ?? null;
        if (!id) throw new Error($t('histviz.page.backtestSaveFailed'));
      }
      await goto(`/backtest?dataset=${id}`);
    } catch (e) {
      error = e.message;
    } finally {
      handoff = false;
    }
  }

  // Persist session bits to localStorage (after restore, to avoid clobbering with defaults).
  let restored = $state(false);
  $effect(() => {
    const snap = {
      coords,
      type,
      brick,
      instances,
      recents,
      magnet,
      drawingsHidden,
      quick,
      quickSize,
      quickUnit,
      quickContract,
      quickExpanded,
      quickHeight,
      quickTab
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
    // A custom draft also carries its definition and the pane the user picked for it.
    const body = {
      type: draft.type,
      params: draft.params,
      style: draft.style,
      ...(draft.type === 'custom' ? { custom: draft.custom, pane: draft.pane } : {})
    };
    if (editing) {
      instances = instances.map((i) => (i.id === editing.id ? { ...i, ...body } : i));
    } else {
      instances = [...instances, { id: nextId++, ...body, visible: true }];
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
  <main>
    <div class="toolbar">
      <!-- The symbol is the first control, as on every market terminal: it opens the picker
           (recents, what is stored, and a live search across the connectors). -->
      <button class="sym-btn" title={$t('histviz.data.pick')} onclick={() => (dataOpen = true)}>
        <Icon name="search" size={13} />
        <span class="sym">{coords?.ticker ?? $t('histviz.data.pickShort')}</span>
      </button>
      {#if timeframes.length > 1}
        <!-- A strip of one button per bar size ate the toolbar; a picker says the same thing
             in one control and still shows the current timeframe. -->
        <div class="pick tf">
          <Dropdown
            value={coords?.timeframe}
            options={tfOpts}
            ariaLabel={$t('histviz.page.timeframe')}
            title={$t('histviz.page.timeframe')}
            onpick={switchTimeframe}
          />
        </div>
      {/if}
      <div class="pick style">
        <Dropdown
          bind:value={type}
          options={typeOpts}
          ariaLabel={$t('histviz.page.plotStyle')}
          title={$t('histviz.page.plotStyle')}
        />
      </div>
      <button class="ind-btn" title={$t('histviz.page.addIndicator')} onclick={openAdd}>
        <Icon name="trending-up" size={13} /> {$t('histviz.panel.title')}
      </button>
      {#if type === 'renko'}
        <label class="brick">
          {$t('histviz.page.brick')}
          <input type="number" min="0" step="any" bind:value={brick} placeholder={$t('histviz.modal.auto')} />
        </label>
      {/if}
      {#if coords && bars?.count}
        <div class="bt-group">
          <button
            class="bt quick"
            class:on={quick}
            aria-pressed={quick}
            title={$t('histviz.page.quickBacktestHint')}
            onclick={() => (quick = !quick)}
          >
            <Icon name="zap" size={13} /> {$t('histviz.page.quickBacktest')}
          </button>
          <button class="bt" disabled={handoff} title={$t('histviz.page.backtestHint')} onclick={toBacktest}>
            <Icon name="flask" size={13} />
            {handoff ? $t('histviz.page.backtestOpening') : $t('histviz.page.backtest')}
          </button>
        </div>
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
      <!-- Autosave sits with the other chart-level controls rather than inside the settings
           popover: it is a mode you flip while working, not a display preference. -->
      <label class="autosave" title={$t('histviz.settings.autosaveDataHint')}>
        <input
          type="checkbox"
          role="switch"
          aria-checked={settings.autosaveData}
          bind:checked={settings.autosaveData}
        />
        <span class="slider"></span>
        <span class="lbl">{$t('histviz.settings.autosaveData')}</span>
      </label>

      <!-- The instrument is written on the chart itself (top-left, market-tool style), so the
           toolbar no longer repeats it. -->
      <div class="settings-wrap">
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
      {#if bars?.count && !loading}
        <DrawToolbar
          bind:tool
          bind:magnet
          bind:hidden={drawingsHidden}
          count={drawings.length}
          hasSelection={!!selectedDrawing}
          ondelete={deleteDrawing}
          onclear={clearDrawings}
          onundo={undoDrawing}
        />
      {/if}
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
          title={{ ticker: coords?.ticker, timeframe: coords?.timeframe, provider: coords?.provider }}
          onsymbol={() => (dataOpen = true)}
          ontoggle={toggle}
          onedit={(id) => openEdit(instances.find((i) => i.id === id))}
          onremove={remove}
          onfullscreen={() => (fullscreen = !fullscreen)}
          onrestored={() => (restoreRange = null)}
          onloadmore={loadMore}
          bind:tool
          bind:drawings
          bind:selectedDrawing
          bind:magnet
          bind:drawingsHidden
          {quick}
          marks={session.marks}
          bind:quickSize
          quickUnitLabel={$t(`histviz.quick.unit.${quickUnit}`)}
          onquickclick={quickClick}
          onquickfill={quickFill}
          onquicktoggle={quickToggle}
        />
      {:else}
        <div class="empty">
          <p class="hint">{$t('histviz.page.selectInstrument')}</p>
          <button class="pick-btn" onclick={() => (dataOpen = true)}>
            <Icon name="search" size={13} /> {$t('histviz.data.pick')}
          </button>
        </div>
      {/if}
    </div>

    <!-- The quick session's trade book, under the chart where market tools put it. It follows
         the mode alone: a session left in it survives the close, it is just out of the way. -->
    {#if !fullscreen && bars?.count && quick}
      <QuickPanel
        trades={session.trades}
        open={session.open}
        marks={session.marks}
        last={lastClose}
        bind:size={quickSize}
        bind:sizeMode={quickUnit}
        bind:contractSize={quickContract}
        bind:expanded={quickExpanded}
        bind:height={quickHeight}
        bind:tab={quickTab}
        onremove={quickRemoveTrade}
        onreset={quickReset}
        onundo={quickUndo}
        onclose={() => (quick = false)}
      />
    {/if}
  </main>
</div>
</RequireModule>

<svelte:window onkeydown={onKey} onbeforeunload={flushLayout} onpagehide={flushLayout} />

<IndicatorModal bind:open={modalOpen} edit={editing} onsave={onSave} onclose={() => (editing = null)} />

<DataModal
  bind:open={dataOpen}
  {datasets}
  {recents}
  selected={coords}
  busy={loading}
  onselect={openCoords}
  ondownload={() => goto('/histdata')}
/>

<style>
  /* Fill the chart area so the skeleton occupies the same box the chart will. */
  .sk-chart {
    flex: 1;
    min-height: 0;
    padding: var(--space-3);
  }
  .page {
    height: 100%;
    display: flex;
    overflow: hidden;
  }
  main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: var(--space-4);
    gap: var(--space-3);
    overflow: hidden;
  }
  /* The symbol is the page's identity control: bigger type, its own weight. */
  .sym-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  /* Every control on the toolbar row is one control tall — the same height the pickers
     already take — so the row reads as a single strip and not a ragged edge. */
  .toolbar > *,
  .toolbar .bt-group > *,
  .toolbar .brick input {
    height: var(--control-h);
    box-sizing: border-box;
  }
  .toolbar > * {
    display: inline-flex;
    align-items: center;
  }
  .toolbar .pick :global(.dd),
  .toolbar .settings-wrap .gear {
    flex: 1;
    min-width: 0;
    height: 100%;
  }
  .sym-btn .sym {
    font-weight: var(--fw-medium);
    color: var(--text);
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
  /* Autosave: a real switch, pushed to the right with the settings gear. */
  .autosave {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: auto;
    font-size: var(--text-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .autosave input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }
  .autosave .slider {
    position: relative;
    flex: none;
    width: 34px;
    height: 18px;
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
    transition: 0.15s;
  }
  .autosave .slider::before {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    background: var(--muted);
    transition: 0.15s;
  }
  .autosave input:checked + .slider {
    background: var(--accent);
    border-color: var(--accent);
  }
  .autosave input:checked + .slider::before {
    transform: translateX(16px);
    background: var(--bg);
  }
  .autosave input:focus-visible + .slider {
    outline: 1px solid var(--accent);
    outline-offset: 2px;
  }
  .autosave:hover .lbl {
    color: var(--text);
  }
  /* The two toolbar pickers are narrow on purpose: the chart owns the width. */
  .pick.tf {
    width: 96px;
  }
  .pick.tf :global(.trigger) {
    font-family: var(--mono);
  }
  .pick.style {
    width: 150px;
  }
  .ind-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  /* The two ways out of the chart sit together, ahead of the data/save controls. */
  .bt-group {
    display: flex;
    gap: var(--space-1);
  }
  .toolbar .bt {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .toolbar .bt.quick.on {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
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
  /* Rail + chart on one row, both inside the box that goes fullscreen. */
  .chart-wrap {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-2);
    gap: var(--space-2);
  }
  .chart-wrap :global(.chart-host) {
    flex: 1;
    min-width: 0;
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
  .empty {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-4);
  }
  .pick-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .pick-btn:hover {
    border-color: var(--accent);
  }
</style>
