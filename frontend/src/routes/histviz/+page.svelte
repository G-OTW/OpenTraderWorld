<script>
  // Historical Data Visualization: a workspace of chart panes.
  //
  // The page owns three things and nothing else:
  //   - the **grid**: rows x columns (1x1 up to 3x4) with draggable splitters, so a vertical
  //     split, a horizontal one and a 2x2 come out of one model instead of a list of named
  //     layouts;
  //   - the **rail**: the instrument lists, from the chart's own and from the Watchlists
  //     module (see WatchRail);
  //   - the **live connection**: one SSE stream for every pane on screen. The server keys its
  //     sockets by connector, so four panes on one Alpaca key are one socket there and one
  //     connection here; each event carries the pane it belongs to.
  //
  // Everything about *a* chart (its instrument, its window, its studies, its drawings, its
  // quick session) belongs to Pane.svelte. A pane knows nothing about its neighbours.
  import { tick, untrack } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { connectorsApi } from '$lib/connectors/api.js';
  import Pane from '$lib/modules/histviz/Pane.svelte';
  import WatchRail from '$lib/modules/histviz/WatchRail.svelte';
  import QuickPanel from '$lib/modules/histviz/QuickPanel.svelte';
  import ChartSettings from '$lib/modules/histviz/ChartSettings.svelte';
  import { DEFAULT_SETTINGS, normalizeSettings } from '$lib/modules/histviz/settings.js';
  import {
    MAX_ROWS,
    MAX_COLS,
    fromRow,
    toBody,
    newWorkspace,
    newPane,
    reshape,
    resizeTrack,
    visiblePanes
  } from '$lib/modules/histviz/workspace.js';
  import { SLICE_BARS } from '$lib/modules/histviz/pane.js';

  // The workspace this browser had open, so a reload lands where you left off even before the
  // server list comes back.
  const LAST_KEY = 'otw.histviz.workspace.v1';
  /** What the single-chart page kept in this browser. Read once, so upgrading lands on the
   *  instrument you were looking at rather than on an empty pane. */
  const LEGACY_KEY = 'otw.histviz.session.v2';

  function lastSingleChart() {
    try {
      return JSON.parse(localStorage.getItem(LEGACY_KEY) || '{}')?.coords ?? null;
    } catch {
      return null;
    }
  }

  let ws = $state(newWorkspace());
  let workspaces = $state([]);
  let datasets = $state([]);
  let providers = $state([]);
  let connectors = $state([]);
  let settings = $state({ ...DEFAULT_SETTINGS });
  let settingsLoaded = $state(false);
  let settingsOpen = $state(false);
  let railOpen = $state(localStorage.getItem('otw.histviz.railOpen') !== '0');
  // The rail/charts split, dragged on the hairline between them and kept across sessions.
  const RAIL_KEY = 'otw.histviz.railW';
  const RAIL_MIN = 150;
  const RAIL_MAX = 480;
  const clampRail = (w) => Math.min(RAIL_MAX, Math.max(RAIL_MIN, Math.round(w)));
  let railW = $state(clampRail(Number(localStorage.getItem(RAIL_KEY)) || 198));
  let maximized = $state(null);
  let error = $state('');
  let gridEl = $state(null);
  let railRef = $state(null);
  /** Renaming swaps the picker for a field **in the same slot**: one control, one width,
   *  so the bar never reflows and the name is one click away instead of always in the way. */
  let renaming = $state(false);
  let nameEl = $state(null);
  /** The cell the pointer is over in the shape grid: the picker previews the layout it would
   *  apply before the click, which is what makes a 12-cell grid readable without labels. */
  let shapeHover = $state(null);
  let shapeOpen = $state(false);
  let confirmDelete = $state(false);

  // ── Quick backtest ──
  // One session for the whole workspace. Every pane replays its own fills (its bars are what
  // MAE and R are measured against) and publishes the result here, so a user can click
  // entries on three instruments and read one set of numbers.
  const QUICK_KEY = 'otw.histviz.quick';
  let quickOn = $state(localStorage.getItem(QUICK_KEY) === '1');
  let quickExpanded = $state(false);
  let quickHeight = $state(240);
  let quickTab = $state('trades');
  let quickSessions = $state({});

  /** Live handles the panes register, so the shared stream and the rail can reach them.
   *  A plain Map on purpose: it is a routing table, not state anything renders. */
  const panes = new Map();

  const shown = $derived(visiblePanes(ws));
  const activePane = $derived(ws.panes.find((p) => p.id === ws.active) ?? shown[0] ?? null);
  const activeCoords = $derived(activePane?.coords ?? null);

  // ── Catalog + workspace load ──
  let booted = false;
  $effect(() => {
    if (booted) return;
    booted = true;
    boot();
  });

  async function boot() {
    try {
      datasets = await histvizApi.datasets();
    } catch (e) {
      error = e.message;
    }
    try {
      providers = await histvizApi.providers();
    } catch {
      /* the capability matrix is only used for fallbacks */
    }
    try {
      connectors = await connectorsApi.list('histviz');
    } catch {
      /* the live account picker falls back to the server's own default */
    }
    try {
      workspaces = await histvizApi.workspaces();
    } catch (e) {
      error = e.message;
    }
    const wanted = localStorage.getItem(LAST_KEY);
    const row = workspaces.find((w) => w.id === wanted) ?? workspaces[0] ?? null;
    if (row) ws = fromRow(row);
    else ws.panes[0].coords = lastSingleChart();
    if (!ws.active) ws.active = ws.panes[0]?.id ?? null;
    // The panes render and register themselves before anything can be asked of them.
    await tick();

    // A deep link from the download module opens that dataset in the active pane.
    const id = $page.url.searchParams.get('dataset');
    const d = id ? datasets.find((x) => x.id === id) : null;
    if (d) {
      const c = connectors.find((x) => x.provider === d.provider);
      openInActive({
        connector_id: c?.id ?? null,
        provider: d.provider,
        asset_type: d.asset_type,
        ticker: d.ticker,
        timeframe: d.timeframe,
        timeframes: c?.timeframes ?? [],
        stream: (c?.stream_asset_types ?? []).includes(d.asset_type),
        stream_timeframes: c?.stream_timeframes ?? [],
        stream_note: c?.stream_note ?? ''
      });
    } else {
      loadAll();
    }
    loaded = true;
  }

  /** Open every pane that has an instrument, in one round trip. Four panes used to be four
   *  requests fired at once, which a shared connector answers by rate-limiting the lot. */
  async function loadAll() {
    const list = shown.filter((p) => p.coords?.ticker);
    if (!list.length) return;
    for (const p of list) panes.get(p.id)?.setLoading?.(true);
    try {
      const results = await histvizApi.seriesBatch(
        list.map((p) => ({ ...$state.snapshot(p.coords), bars: SLICE_BARS }))
      );
      list.forEach((p, i) => panes.get(p.id)?.apply?.(results[i]));
    } catch (e) {
      error = e.message;
      for (const p of list) panes.get(p.id)?.setLoading?.(false);
    }
    syncStream();
  }

  // ── Workspace persistence ──
  let loaded = false;
  let saveTimer = null;
  let saving = false;

  function touched() {
    if (!loaded) return;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveWorkspace, 1200);
  }

  async function saveWorkspace() {
    if (saving) return;
    saving = true;
    try {
      const body = toBody($state.snapshot(ws));
      if (ws.id) {
        await histvizApi.saveWorkspace(ws.id, body);
        workspaces = workspaces.map((w) => (w.id === ws.id ? { ...w, name: ws.name } : w));
      } else {
        const row = await histvizApi.createWorkspace(body);
        ws.id = row.id;
        workspaces = [...workspaces, row];
        localStorage.setItem(LAST_KEY, row.id);
      }
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  /** The last edit must not die with the page: the workspace is authoritative, so losing it
   *  inside the debounce window would reopen the previous grid. */
  function flushAll() {
    clearTimeout(saveTimer);
    for (const api of panes.values()) api.flushLayout?.();
    if (!loaded || !ws.id) return;
    try {
      fetch(`/api/histviz/workspaces/${ws.id}`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(toBody($state.snapshot(ws))),
        keepalive: true
      });
    } catch {
      /* nothing more we can do on the way out */
    }
  }

  async function switchWorkspace(id) {
    if (id === ws.id) return;
    flushAll();
    closeStream();
    panes.clear();
    const row = workspaces.find((w) => w.id === id);
    if (!row) return;
    ws = fromRow(row);
    localStorage.setItem(LAST_KEY, row.id ?? '');
    maximized = null;
    // The panes remount and register themselves; load once they have.
    await tick();
    loadAll();
  }

  async function addWorkspace() {
    flushAll();
    const fresh = newWorkspace($t('histviz.workspace.untitled'));
    fresh.panes[0].coords = activeCoords ? { ...$state.snapshot(activeCoords) } : null;
    try {
      const row = await histvizApi.createWorkspace(toBody(fresh));
      workspaces = [...workspaces, row];
      panes.clear();
      closeStream();
      ws = fromRow(row);
      localStorage.setItem(LAST_KEY, row.id);
      await tick();
      loadAll();
    } catch (e) {
      error = e.message;
    }
  }

  async function removeWorkspace() {
    if (!ws.id) return;
    const id = ws.id;
    try {
      await histvizApi.deleteWorkspace(id);
      workspaces = workspaces.filter((w) => w.id !== id);
      panes.clear();
      closeStream();
      const next = workspaces[0];
      ws = next ? fromRow(next) : newWorkspace();
      localStorage.setItem(LAST_KEY, next?.id ?? '');
      await tick();
      loadAll();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Grid shape ──
  function setShape(rows, cols) {
    const before = new Set(shown.map((p) => p.id));
    reshape(ws, rows, cols);
    maximized = null;
    touched();
    // Panes that just appeared inherit the active instrument, so they draw something rather
    // than asking the user to pick what they are already looking at.
    tick().then(() => {
      const fresh = visiblePanes(ws).filter((p) => !before.has(p.id) && p.coords?.ticker);
      if (fresh.length) loadPanes(fresh);
    });
  }

  async function loadPanes(list) {
    for (const p of list) panes.get(p.id)?.setLoading?.(true);
    try {
      const results = await histvizApi.seriesBatch(
        list.map((p) => ({ ...$state.snapshot(p.coords), bars: SLICE_BARS }))
      );
      list.forEach((p, i) => panes.get(p.id)?.apply?.(results[i]));
    } catch (e) {
      error = e.message;
      for (const p of list) panes.get(p.id)?.setLoading?.(false);
    }
    syncStream();
  }

  function closePane(id) {
    if (ws.panes.length <= 1) return;
    ws.panes = ws.panes.filter((p) => p.id !== id);
    panes.delete(id);
    const cells = ws.rows * ws.cols;
    if (ws.panes.length < cells) ws.panes.push(newPane(activeCoords ? { ...activeCoords } : null));
    if (ws.active === id) ws.active = ws.panes[0]?.id ?? null;
    if (maximized === id) maximized = null;
    touched();
    syncStream();
  }

  const activate = (id) => {
    if (ws.active === id) return;
    ws.active = id;
    touched();
  };

  function openInActive(coords) {
    const id = ws.active ?? shown[0]?.id;
    railRef?.remember?.(coords);
    panes.get(id)?.openCoords?.(coords);
  }

  /** A rail row that cannot be charted. The pane says so in place of its chart: the reason is
   *  a connector to fix, and the pane is where the user is already looking. */
  function failInActive(info) {
    panes.get(ws.active ?? shown[0]?.id)?.showBlocked?.(info);
  }

  // ── Splitters ──
  // A drag moves one boundary: the track before it grows by what the one after it loses, so
  // the grid never reflows as a whole and no pane can be collapsed out of reach.
  function dragTrack(e, axis, index) {
    e.preventDefault();
    const box = gridEl?.getBoundingClientRect();
    if (!box) return;
    const span = axis === 'row' ? box.height : box.width;
    const start = axis === 'row' ? e.clientY : e.clientX;
    const before = axis === 'row' ? ws.row_sizes.slice() : ws.col_sizes.slice();
    const move = (ev) => {
      const now = axis === 'row' ? ev.clientY : ev.clientX;
      const delta = (now - start) / span;
      const next = resizeTrack(before, index, delta);
      if (axis === 'row') ws.row_sizes = next;
      else ws.col_sizes = next;
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      touched();
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }

  /** Where a splitter sits, as a percentage of the axis: the fractions before it over the
   *  total. Percentages rather than pixels so a window resize needs no recomputation. */
  function offsets(list) {
    const total = list.reduce((a, b) => a + b, 0) || 1;
    const out = [];
    let cum = 0;
    for (let i = 0; i < list.length - 1; i++) {
      cum += list[i];
      out.push((cum / total) * 100);
    }
    return out;
  }

  const gridStyle = $derived(
    maximized
      ? 'grid-template-rows:1fr;grid-template-columns:1fr'
      : `grid-template-rows:${ws.row_sizes.map((x) => `${x}fr`).join(' ')};` +
        `grid-template-columns:${ws.col_sizes.map((x) => `${x}fr`).join(' ')}`
  );

  // ── The shared live stream ──
  let es = null;
  let streamKey = '';
  let streamPanes = [];
  let streamTimer = null;

  /** Rebuild the connection when the *set* of live instruments changes. Deliberately not an
   *  effect over pane state: `bars` moves on every tick, and a stream that reopened for that
   *  would spend a connection per candle. */
  function syncStream() {
    clearTimeout(streamTimer);
    streamTimer = setTimeout(openStream, 150);
  }

  function openStream() {
    const reqs = [];
    const ids = [];
    for (const p of visiblePanes(ws)) {
      const r = panes.get(p.id)?.streamRequest?.();
      if (r) {
        reqs.push(r);
        ids.push(p.id);
      }
    }
    const key = ids
      .map((id, i) => `${id}|${reqs[i].provider}|${reqs[i].ticker}|${reqs[i].timeframe}|${reqs[i].connector_id ?? ''}`)
      .join('~');
    if (key === streamKey) return;
    streamKey = key;
    closeStream();
    if (!reqs.length) return;
    streamPanes = ids;
    const src = new EventSource(histvizApi.streamsUrl(reqs));
    const onBar = (e) => {
      try {
        const d = JSON.parse(e.data);
        panes.get(streamPanes[d.i ?? 0])?.applyBar?.(d);
      } catch {
        /* ignore malformed frame */
      }
    };
    src.addEventListener('snapshot', onBar);
    src.addEventListener('bar', onBar);
    src.addEventListener('status', (e) => {
      try {
        const d = JSON.parse(e.data);
        panes.get(streamPanes[d.i ?? 0])?.applyStatus?.(d);
      } catch {
        /* ignore malformed frame */
      }
    });
    // Interactive Brokers rations live data per account, not per connection: the server
    // says how many of the account's market-data lines the open panes are holding.
    src.addEventListener('lines', (e) => {
      try {
        const d = JSON.parse(e.data);
        panes.get(streamPanes[d.i ?? 0])?.applyLines?.(d);
      } catch {
        /* ignore malformed frame */
      }
    });
    es = src;
  }

  function closeStream() {
    if (es) {
      es.close();
      es = null;
    }
    streamKey = '';
    streamPanes = [];
  }

  // ── Global chart settings ──
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

  function onKey(e) {
    if (e.key === 'Escape' && shapeOpen) {
      shapeOpen = false;
      return;
    }
    if (e.key === 'Escape' && maximized) {
      maximized = null;
      return;
    }
    if ((e.key === 'z' || e.key === 'Z') && (e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey) {
      const el = e.target;
      const tag = el?.tagName;
      if (el?.isContentEditable || tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      const api = panes.get(ws.active);
      if (!api) return;
      if (api.hasQuickFills?.()) api.quickUndo?.();
      else if (api.hasDrawings?.()) api.undoDrawing?.();
      else return;
      e.preventDefault();
    }
  }

  /** The picker's rows. The open workspace is labelled from `ws.name`, not from its stored
   *  row, so a rename reads back immediately instead of after the debounced save. */
  const wsOpts = $derived([
    ...(ws.id ? [] : [{ value: '', label: ws.name || $t('histviz.workspace.untitled') }]),
    ...workspaces.map((w) => ({ value: w.id, label: w.id === ws.id ? ws.name : w.name }))
  ]);

  async function startRename() {
    renaming = true;
    await tick();
    nameEl?.focus();
    nameEl?.select();
  }

  /** Drag the divider: the rail follows the pointer 1:1 and the width is stored on release,
   *  so a drag writes localStorage once instead of on every frame. */
  let railDrag = $state(null);
  function railDown(e) {
    if (e.button !== 0) return;
    railDrag = { x0: e.clientX, w0: railW };
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function railMove(e) {
    if (!railDrag) return;
    railW = clampRail(railDrag.w0 + (e.clientX - railDrag.x0));
  }
  function railUp(e) {
    if (!railDrag) return;
    railDrag = null;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    saveRailW();
  }
  function railKey(e) {
    const step = e.shiftKey ? 32 : 8;
    if (e.key === 'ArrowLeft') railW = clampRail(railW - step);
    else if (e.key === 'ArrowRight') railW = clampRail(railW + step);
    else return;
    e.preventDefault();
    saveRailW();
  }
  function saveRailW() {
    try {
      localStorage.setItem(RAIL_KEY, String(railW));
    } catch {
      /* non-fatal */
    }
  }

  function toggleRail() {
    railOpen = !railOpen;
    try {
      localStorage.setItem('otw.histviz.railOpen', railOpen ? '1' : '0');
    } catch {
      /* non-fatal */
    }
  }

  function toggleQuick() {
    quickOn = !quickOn;
    try {
      localStorage.setItem(QUICK_KEY, quickOn ? '1' : '0');
    } catch {
      /* non-fatal */
    }
  }

  /** A pane publishes from an effect, so the map is read outside the reactive graph: reading
   *  it inside that effect would make the effect depend on what it writes, and the flush
   *  would never settle. */
  const onQuickSession = (id, session) => {
    untrack(() => {
      if (quickSessions[id] === session) return;
      quickSessions = { ...quickSessions, [id]: session };
    });
  };

  /** Every chart's trades in one session, in the order they closed. The pane is carried on
   *  the row so deleting a trade reaches the chart that owns its fills. */
  const quickAgg = $derived.by(() => {
    const trades = [];
    const opens = [];
    for (const p of shown) {
      const q = quickSessions[p.id];
      if (!q) continue;
      const label = `${q.symbol} · ${q.timeframe}`;
      for (const tr of q.trades)
        trades.push({ ...tr, key: `${p.id}:${tr.id}`, paneId: p.id, symbol: q.symbol, timeframe: q.timeframe });
      if (q.open) opens.push({ ...q.open, paneId: p.id, symbol: q.symbol, label, last: q.last });
    }
    trades.sort((a, b) => a.closeTs - b.closeTs);
    return { trades, opens };
  });

  /** The charts a click can land on: the panes showing an instrument. */
  const quickCharts = $derived(
    shown
      .filter((p) => p.coords?.ticker)
      .map((p) => ({ value: p.id, label: `${p.coords.ticker} · ${p.coords.timeframe ?? ''}`.trim() }))
  );
  const quickTarget = $derived(quickCharts.some((c) => c.value === ws.active) ? ws.active : '');
  const quickSizing = $derived(quickSessions[quickTarget] ?? null);

  function quickReset() {
    for (const p of shown) panes.get(p.id)?.quickReset?.();
  }

  const register = (id, api) => {
    panes.set(id, api);
    // A pane that came up outside the workspace's opening batch (a restored layout, a
    // maximize) loads its own instrument: the batch it was not part of will never reach it,
    // and a chart showing its symbol next to "choose an instrument" is a dead pane.
    setTimeout(() => {
      if (panes.get(id) === api && api.needsLoad?.()) api.load?.();
    }, 0);
  };
  const unregister = (id) => panes.delete(id);

  // ── Link groups ──
  // A pane's colour is its group. A group shares the instrument and the instant it is
  // pointing at; the timeframe is deliberately never shared, because three panes on one
  // symbol at 1m, 1h and 1d is the reason to link them at all. The visible span only travels
  // between panes on the same timeframe, since the same two dates are a screenful on one and
  // two candles on the other.
  function linked(fromId, kind, payload) {
    if (kind === 'restream') {
      syncStream();
      return;
    }
    const from = ws.panes.find((p) => p.id === fromId);
    const group = from?.link;
    if (!group) return;
    for (const p of visiblePanes(ws)) {
      if (p.id === fromId || p.link !== group) continue;
      const api = panes.get(p.id);
      if (!api) continue;
      if (kind === 'symbol') api.followCoords?.(payload);
      else if (kind === 'crosshair') api.showCrosshair?.(payload);
      else if (kind === 'range') api.setTimeRange?.(payload, payload.timeframe);
    }
    if (kind === 'symbol') syncStream();
  }
</script>

<RequireModule module="histviz">
  <div class="page">
    <!-- ── Workspace bar ──
         One fixed-height row that never wraps and never changes its set of controls: the
         rail toggle, the workspace (picker or rename field, same slot), the layout, then the
         global settings. Anything that only makes sense sometimes is disabled, never removed,
         so no control ever moves under the pointer. -->
    <header class="top">
      <!-- Only this half scrolls when the window is narrow. The settings popover lives
           outside it: a scroll container clips an absolutely positioned menu. -->
      <div class="top-scroll">
      <button
        class="tbtn"
        aria-pressed={railOpen}
        title={railOpen ? $t('histviz.rail.collapse') : $t('histviz.rail.show')}
        aria-label={railOpen ? $t('histviz.rail.collapse') : $t('histviz.rail.show')}
        onclick={toggleRail}
      >
        <Icon name={railOpen ? 'chevrons-left' : 'list'} size={13} />
      </button>

      <span class="rule" aria-hidden="true"></span>

      <div class="ws-slot">
        {#if renaming}
          <input
            class="ws-name"
            bind:this={nameEl}
            bind:value={ws.name}
            oninput={touched}
            onblur={() => (renaming = false)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === 'Escape') renaming = false;
            }}
            aria-label={$t('histviz.workspace.name')}
          />
        {:else}
          <Dropdown
            value={ws.id ?? ''}
            options={wsOpts}
            ariaLabel={$t('histviz.workspace.title')}
            title={$t('histviz.workspace.switch')}
            float
            onpick={switchWorkspace}
          />
        {/if}
      </div>
      <button
        class="tbtn"
        class:on={renaming}
        title={$t('histviz.workspace.rename')}
        aria-label={$t('histviz.workspace.rename')}
        onclick={startRename}
      >
        <Icon name="pencil" size={12} />
      </button>
      <button
        class="tbtn"
        title={$t('histviz.workspace.add')}
        aria-label={$t('histviz.workspace.add')}
        onclick={addWorkspace}
      >
        <Icon name="plus" size={13} />
      </button>
      <button
        class="tbtn danger"
        title={$t('histviz.workspace.remove')}
        aria-label={$t('histviz.workspace.remove')}
        disabled={!ws.id}
        onclick={() => (confirmDelete = true)}
      >
        <Icon name="trash-2" size={13} />
      </button>

      <span class="rule" aria-hidden="true"></span>

      <!-- Quick backtest: one session over every chart on screen, so the switch belongs to
           the workspace rather than to a pane. -->
      <button
        class="tbtn wide"
        class:on={quickOn}
        aria-pressed={quickOn}
        title={$t('histviz.page.quickBacktestHint')}
        onclick={toggleQuick}
      >
        <Icon name="zap" size={13} />
        <span class="tbtn-lbl">{$t('histviz.page.quickBacktest')}</span>
      </button>

      </div>

      <!-- Layout: a dropdown whose menu *is* the grid. Pointing at a cell says the shape
           better than a list of names ever did, and the cells light up on hover so the
           layout is read before it is applied. Outside the scroller: a scroll container
           clips an absolutely positioned menu. -->
      <div class="shape-wrap" use:clickOutside={() => (shapeOpen = false)}>
        <button
          class="tbtn wide"
          class:on={shapeOpen}
          aria-expanded={shapeOpen}
          aria-haspopup="true"
          title={$t('histviz.workspace.shape')}
          onclick={() => (shapeOpen = !shapeOpen)}
        >
          <Icon name="grid" size={13} />
          <span class="shape-lbl">{ws.rows}×{ws.cols}</span>
          <Icon name={shapeOpen ? 'chevron-up' : 'chevron-down'} size={12} />
        </button>
        {#if shapeOpen}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            class="shape-pop"
            role="group"
            aria-label={$t('histviz.workspace.shape')}
            onpointerleave={() => (shapeHover = null)}
          >
            {#each Array(MAX_ROWS) as _, r}
              <div class="shape-row">
                {#each Array(MAX_COLS) as _, c}
                  <button
                    class="cellbtn"
                    class:on={r < ws.rows && c < ws.cols}
                    class:pre={shapeHover && r <= shapeHover.r && c <= shapeHover.c}
                    aria-label={`${r + 1}x${c + 1}`}
                    title={`${r + 1}x${c + 1}`}
                    onpointerenter={() => (shapeHover = { r, c })}
                    onfocus={() => (shapeHover = { r, c })}
                    onblur={() => (shapeHover = null)}
                    onclick={() => {
                      setShape(r + 1, c + 1);
                      shapeOpen = false;
                    }}
                  ></button>
                {/each}
              </div>
            {/each}
            <p class="shape-cur">
              {shapeHover ? `${shapeHover.r + 1}×${shapeHover.c + 1}` : `${ws.rows}×${ws.cols}`}
            </p>
          </div>
        {/if}
      </div>

      <div class="settings-wrap">
        <button
          class="tbtn wide"
          class:on={settingsOpen}
          aria-expanded={settingsOpen}
          title={$t('histviz.settings.title')}
          onclick={() => (settingsOpen = !settingsOpen)}
        >
          <Icon name="settings" size={13} />
          <span class="tbtn-lbl">{$t('settings.title')}</span>
        </button>
        {#if settingsOpen}
          <ChartSettings bind:settings onclose={() => (settingsOpen = false)} />
        {/if}
      </div>
    </header>

    <div class="body">
      {#if railOpen}
        <WatchRail
          bind:this={railRef}
          current={activeCoords}
          {datasets}
          {connectors}
          onpick={openInActive}
          onfail={failInActive}
          ondownload={() => goto('/histdata')}
          width={railW}
        />
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          class="rail-grip"
          class:on={!!railDrag}
          role="separator"
          aria-orientation="vertical"
          aria-label={$t('histviz.rail.resize')}
          title={$t('histviz.rail.resize')}
          aria-valuenow={railW}
          aria-valuemin={RAIL_MIN}
          aria-valuemax={RAIL_MAX}
          tabindex="0"
          onpointerdown={railDown}
          onpointermove={railMove}
          onpointerup={railUp}
          onpointercancel={railUp}
          ondblclick={() => {
            railW = 198;
            saveRailW();
          }}
          onkeydown={railKey}
        ></div>
      {/if}

      <div class="grid-wrap">
        <!-- Errors float over the grid instead of pushing it down: a chart that resizes the
             moment a request fails loses the window the user was reading. -->
        {#if error}
          <div class="err-float"><ErrorText {error} copyable compact /></div>
        {/if}
        <!-- Maximizing hides the other cells and gives the grid a single track; the pane
             itself is never unmounted. A second copy of it would come up with no bars, no
             zoom and no drawings, next to its own symbol in the bar. -->
        <div class="grid" bind:this={gridEl} style={gridStyle}>
          {#each shown as p, i (p.id)}
            <div class="cell" class:off={maximized && maximized !== p.id}>
              <Pane
                bind:pane={ws.panes[i]}
                {settings}
                {datasets}
                {providers}
                {connectors}
                quick={quickOn}
                active={ws.active === p.id}
                maximized={maximized === p.id}
                closable={ws.panes.length > 1 && !maximized}
                onactivate={activate}
                onregister={register}
                onunregister={unregister}
                onquick={onQuickSession}
                onchanged={() => {
                  touched();
                  syncStream();
                }}
                onmaximize={(id) => (maximized = maximized === id ? null : id)}
                onclose={closePane}
                onlinked={(kind, payload) => linked(p.id, kind, payload)}
                ondatasets={(rows) => (datasets = rows)}
                onsettings={(patch) => (settings = { ...settings, ...patch })}
              />
            </div>
          {/each}
        </div>
        <!-- Splitters ride over the gaps rather than in the flow: the grid stays a plain
             rows x columns, and a handle is just a percentage down the axis. -->
        {#if !maximized}
          <div class="splitters">
            {#each offsets(ws.row_sizes) as pct, i}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="split row"
                style={`top:${pct}%`}
                onpointerdown={(e) => dragTrack(e, 'row', i)}
              ></div>
            {/each}
            {#each offsets(ws.col_sizes) as pct, i}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="split col"
                style={`left:${pct}%`}
                onpointerdown={(e) => dragTrack(e, 'col', i)}
              ></div>
            {/each}
          </div>
        {/if}
      </div>
    </div>

    <!-- The quick session spans the page, not a pane: fills come from every chart on screen
         and the numbers here are their sum. -->
    {#if quickOn}
      <QuickPanel
        trades={quickAgg.trades}
        opens={quickAgg.opens}
        charts={quickCharts}
        target={quickTarget}
        size={quickSizing?.size ?? 1}
        sizeMode={quickSizing?.mode ?? 'units'}
        contractSize={quickSizing?.contract ?? 1}
        bind:expanded={quickExpanded}
        bind:height={quickHeight}
        bind:tab={quickTab}
        onpicktarget={activate}
        onsizing={(patch) => panes.get(ws.active)?.setQuickSizing?.(patch)}
        onremove={(row) => panes.get(row.paneId)?.quickRemoveTrade?.(row)}
        onreset={quickReset}
        onundo={() => panes.get(ws.active)?.quickUndo?.()}
        onclose={toggleQuick}
      />
    {/if}
  </div>

  <ConfirmModal
    bind:open={confirmDelete}
    title={$t('histviz.workspace.remove')}
    message={$t('histviz.workspace.removeConfirm', { name: ws.name })}
    confirmLabel={$t('common.delete')}
    cancelLabel={$t('common.cancel')}
    danger
    onconfirm={removeWorkspace}
  />
</RequireModule>

<svelte:window onkeydown={onKey} onbeforeunload={flushAll} onpagehide={flushAll} />

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    height: calc(100vh - 96px);
    min-height: 420px;
  }

  /* ── Workspace bar ──
     Fixed height, never wraps. Wrapping is a layout shift that costs the chart below a
     whole row of pixels, so the bar scrolls its own overflow instead. */
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    height: 34px;
    padding: 0 var(--space-1);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    flex-wrap: nowrap;
  }
  .top-scroll {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 1;
    min-width: 0;
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .top-scroll::-webkit-scrollbar {
    display: none;
  }
  /* Hairline group separator: the bar reads as three clusters, not eight buttons. */
  .rule {
    flex: none;
    width: var(--hairline);
    height: 18px;
    margin: 0 var(--space-1);
    background: var(--border);
  }

  /* One slot for the workspace, whether it is showing the picker or the rename field. */
  /* The token, not a :global override: the picker and the field are the same control at
     the same height, and nothing else in the slot has to know about it. */
  .ws-slot {
    --control-h: 26px;
    flex: none;
    width: 176px;
  }
  .ws-name {
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    font-size: var(--fs-body);
  }

  .tbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    flex: none;
    width: 26px;
    height: 26px;
    padding: 0;
    background: transparent;
    border: var(--hairline) solid transparent;
    border-radius: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .tbtn.wide {
    width: auto;
    padding: 0 var(--space-2);
  }
  .tbtn-lbl {
    font-size: var(--fs-body);
    letter-spacing: 0.02em;
  }
  .tbtn:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .tbtn.on {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
    background: var(--surface-2);
  }
  .tbtn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .tbtn.danger:hover:not(:disabled) {
    color: var(--red);
  }

  /* ── Layout picker ──
     Twelve cells in a dropdown, hover previews the shape it would apply, the trigger spells
     out the current one. One click to open, one to apply. */
  .shape-wrap {
    position: relative;
    flex: none;
  }
  .shape-pop {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: var(--z-dropdown);
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
  }
  .shape-row {
    display: flex;
    gap: 3px;
  }
  .cellbtn {
    width: 22px;
    height: 16px;
    padding: 0;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .cellbtn.on {
    background: color-mix(in srgb, var(--accent) 45%, transparent);
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border-control));
  }
  .cellbtn.pre {
    background: var(--accent);
    border-color: var(--accent);
  }
  /* Tabular width: 1x1 and 3x4 must occupy the same room or the trigger twitches. */
  .shape-lbl {
    display: inline-block;
    width: 26px;
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.04em;
    color: var(--dim);
    text-align: center;
  }
  .shape-cur {
    margin: 2px 0 0;
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.06em;
    color: var(--dim);
    text-align: center;
  }
  .settings-wrap {
    position: relative;
    flex: none;
  }

  .body {
    display: flex;
    gap: var(--space-2);
    flex: 1;
    min-height: 0;
  }
  /* The divider is a hairline until it is useful: a 7px hit area with a 1px line in it,
     lit on hover and while dragging. Double-click puts the rail back to its default. */
  .rail-grip {
    flex: none;
    width: 7px;
    /* The divider lives *in* the gap the flex row already leaves, so widening the hit area
       does not push the charts across. */
    margin: 0 calc(var(--space-2) / -2 - 3.5px);
    cursor: col-resize;
    background: transparent;
    touch-action: none;
  }
  .rail-grip::after {
    content: '';
    display: block;
    width: 1px;
    height: 100%;
    margin: 0 auto;
    background: transparent;
    transition: background 0.12s ease;
  }
  .rail-grip:hover::after,
  .rail-grip:focus-visible::after,
  .rail-grip.on::after {
    background: var(--accent);
  }
  .rail-grip:focus-visible {
    outline: none;
  }
  .grid-wrap {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .err-float {
    position: absolute;
    top: var(--space-2);
    left: var(--space-2);
    right: var(--space-2);
    z-index: var(--z-sticky);
    padding: var(--space-1) var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid color-mix(in srgb, var(--red) 45%, var(--border));
  }
  .grid {
    display: grid;
    gap: var(--space-1);
    height: 100%;
    width: 100%;
  }
  .cell {
    display: flex;
    min-width: 0;
    min-height: 0;
  }
  .cell :global(.pane) {
    flex: 1;
    min-width: 0;
  }
  /* A maximized pane takes the only track the grid has left; the rest are hidden rather
     than unmounted, so nothing they hold is thrown away. */
  .cell.off {
    display: none;
  }
  .splitters {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .split {
    position: absolute;
    pointer-events: auto;
  }
  .split.row {
    left: 0;
    right: 0;
    height: 9px;
    transform: translateY(-50%);
    cursor: row-resize;
  }
  .split.col {
    top: 0;
    bottom: 0;
    width: 9px;
    transform: translateX(-50%);
    cursor: col-resize;
  }
  .split:hover {
    background: color-mix(in srgb, var(--accent) 35%, transparent);
  }
</style>
