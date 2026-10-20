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
  import { goto, beforeNavigate } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import { chrome } from '$lib/ui/chrome.svelte.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ChoiceModal from '$lib/ui/ChoiceModal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { connectorsApi } from '$lib/connectors/api.js';
  import Pane from '$lib/modules/histviz/Pane.svelte';
  import WatchRail from '$lib/modules/histviz/WatchRail.svelte';
  import QuickPanel from '$lib/modules/histviz/QuickPanel.svelte';
  import BookPanel from '$lib/modules/histviz/BookPanel.svelte';
  import { brokersApi } from '$lib/brokers/api.js';
  import {
    MODULE as BOOK_MODULE,
    loadBookAccount,
    saveBookAccount
  } from '$lib/modules/histviz/book.js';
  import { DEFAULT_SETTINGS, normalizeSettings } from '$lib/modules/histviz/settings.js';
  import { liveStream } from '$lib/live/stream.js';
  import {
    MAX_ROWS,
    MAX_COLS,
    fromRow,
    toBody,
    savedKey,
    migrateLegacy,
    newWorkspace,
    newPane,
    reshape,
    resizeTrack,
    visiblePanes
  } from '$lib/modules/histviz/workspace.js';
  import { SLICE_BARS, loadDrawings } from '$lib/modules/histviz/pane.js';
  import { templateOf, templatesIn } from '$lib/modules/histviz/templates.js';

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
  let barDock = $state(null);
  let railDock = $state(null);
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
  let revertOpen = $state(false);
  /** The workspace the delete confirmation is about: { id, name }. */
  let deleting = $state(null);
  // The workspace menu: opened under its button, drawn fixed (the bar is a scroller).
  let wsMenuOpen = $state(false);
  let wsMenuAt = $state(null);
  let wsBtn = $state(null);
  function toggleWsMenu() {
    if (wsMenuOpen) {
      wsMenuOpen = false;
      return;
    }
    const r = wsBtn?.getBoundingClientRect();
    wsMenuAt = r ? { top: r.bottom + 4, left: r.left, width: Math.max(r.width, 240) } : null;
    wsMenuOpen = true;
  }
  function renameFromMenu(id) {
    wsMenuOpen = false;
    if (!id || id === ws.id) return startRename();
    guard(async () => {
      await switchWorkspace(id);
      startRename();
    });
  }
  function askDelete(id, name) {
    wsMenuOpen = false;
    deleting = { id, name };
    confirmDelete = true;
  }

  // ── Broker book ──
  // One account for the whole workspace, like the live stream: an account is an account, not
  // a property of a chart. The pull is explicit and its result is a snapshot, never a
  // subscription, and no call from here can place, change or cancel anything.
  let bookOpen = $state(false);
  let bookAccounts = $state([]);
  let bookAccountId = $state(loadBookAccount());
  let book = $state(null);
  let bookAt = $state(0);
  let bookBusy = $state(false);
  let bookError = $state('');

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
  /** Instruments on screen, so the book panel can say which of its symbols is drawn. */
  const openTickers = $derived(shown.map((p) => p.coords?.ticker).filter(Boolean));
  const bookLevelCount = $derived(
    (book?.positions?.length ?? 0) + (book?.orders?.length ?? 0)
  );

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
    if (row) await openRow(row);
    else {
      ws.panes[0].coords = lastSingleChart();
      baseline = savedKey($state.snapshot(ws));
    }
    if (!ws.active) ws.active = ws.panes[0]?.id ?? null;
    // The panes render and register themselves before anything can be asked of them.
    await tick();

    // A deep link from the download module opens that dataset in the active pane; one from an
    // alert (its notification, the global alert list) opens the alert's instrument.
    const id = $page.url.searchParams.get('dataset');
    const d = id ? datasets.find((x) => x.id === id) : null;
    const alertId = $page.url.searchParams.get('alert');
    if (alertId) {
      openedAlert = alertId;
      loadAll(ws.active);
      await openAlert(alertId);
    } else if (d) {
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
  async function loadAll(skip = null) {
    const list = shown.filter((p) => p.coords?.ticker && p.id !== skip);
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
  // Saved on request only (the Save button, Ctrl/Cmd+S), like a trading desk's layouts: an
  // edit changes the workspace on screen, and `dirty` says it differs from the last save.
  let loaded = $state(false);
  let saving = $state(false);
  /** `savedKey` of the workspace as last saved (or opened). */
  let baseline = $state('');
  const dirty = $derived(loaded && savedKey(ws) !== baseline);

  /** The stored row a body becomes, so the picker reopens a workspace as it was saved. */
  const rowOf = (id, body) => ({ id, name: body.name, grid_rows: body.rows, grid_cols: body.cols, panes: body.panes, settings: body.settings });

  /** Put a stored workspace on screen. One saved before workspaces held their charts takes
   *  its studies and drawings from the instrument layouts first. */
  async function openRow(row) {
    const next = row ? fromRow(row) : newWorkspace();
    if (row && next.legacy) await migrateLegacy(next, histvizApi.layoutOf, loadDrawings);
    if (!next.active) next.active = next.panes[0]?.id ?? null;
    ws = next;
    baseline = savedKey(next);
  }

  async function saveWorkspace() {
    if (saving) return false;
    saving = true;
    try {
      const snap = $state.snapshot(ws);
      const body = toBody(snap);
      if (ws.id) {
        await histvizApi.saveWorkspace(ws.id, body);
        workspaces = workspaces.map((w) => (w.id === ws.id ? { ...w, ...rowOf(ws.id, body) } : w));
      } else {
        const row = await histvizApi.createWorkspace(body);
        ws.id = row.id;
        workspaces = [...workspaces, { ...row, ...rowOf(row.id, body) }];
        localStorage.setItem(LAST_KEY, row.id);
      }
      baseline = savedKey(snap);
      return true;
    } catch (e) {
      error = e.message;
      return false;
    } finally {
      saving = false;
    }
  }

  /** Save what is on screen as a new workspace, which becomes the open one; the workspace it
   *  came from keeps its last save. */
  let saveAsOpen = $state(false);
  async function saveAs(name) {
    const snap = $state.snapshot(ws);
    snap.name = name;
    try {
      const body = toBody(snap);
      const row = await histvizApi.createWorkspace(body);
      workspaces = [...workspaces, { ...row, ...rowOf(row.id, body) }];
      ws.id = row.id;
      ws.name = name;
      localStorage.setItem(LAST_KEY, row.id);
      baseline = savedKey($state.snapshot(ws));
    } catch (e) {
      error = e.message;
    }
  }

  /** Back to the last save. */
  async function revert() {
    const row = workspaces.find((w) => w.id === ws.id);
    panes.clear();
    closeStream();
    maximized = null;
    await openRow(row ?? null);
    await tick();
    loadAll();
  }

  // ── Unsaved changes ──
  // Leaving the workspace (another one, a new one, another page) asks first when it has
  // unsaved changes: save, discard, or stay.
  let unsavedOpen = $state(false);
  let unsavedGo = null;
  function guard(go) {
    if (!dirty) return go();
    unsavedGo = go;
    unsavedOpen = true;
  }
  async function unsavedPick(choice) {
    const go = unsavedGo;
    unsavedGo = null;
    if (choice === 'save' && !(await saveWorkspace())) return;
    if (choice === 'save' || choice === 'discard') {
      baseline = savedKey($state.snapshot(ws)); // nothing left to ask about on the way out
      go?.();
    }
  }
  beforeNavigate((nav) => {
    if (!dirty || nav.type === 'leave' || !nav.to) return;
    nav.cancel();
    const url = nav.to.url;
    guard(() => goto(url));
  });
  function onBeforeUnload(e) {
    if (!dirty) return;
    e.preventDefault();
    e.returnValue = '';
  }

  async function switchWorkspace(id) {
    if (id === ws.id) return;
    const row = workspaces.find((w) => w.id === id);
    if (!row) return;
    closeStream();
    panes.clear();
    maximized = null;
    await openRow(row);
    localStorage.setItem(LAST_KEY, row.id ?? '');
    // The panes remount and register themselves; load once they have.
    await tick();
    loadAll();
  }

  async function addWorkspace() {
    const fresh = newWorkspace($t('histviz.workspace.untitled'));
    fresh.panes[0].coords = activeCoords ? { ...$state.snapshot(activeCoords) } : null;
    try {
      const row = await histvizApi.createWorkspace(toBody(fresh));
      workspaces = [...workspaces, row];
      panes.clear();
      closeStream();
      await openRow(row);
      localStorage.setItem(LAST_KEY, row.id);
      await tick();
      loadAll();
    } catch (e) {
      error = e.message;
    }
  }

  /** Delete a workspace. Another one than the open one leaves the screen alone; the open one
   *  hands the screen to the first that remains. */
  async function removeWorkspace(id = ws.id) {
    if (!id) return;
    if (id !== ws.id) {
      try {
        await histvizApi.deleteWorkspace(id);
        workspaces = workspaces.filter((w) => w.id !== id);
      } catch (e) {
        error = e.message;
      }
      return;
    }
    try {
      await histvizApi.deleteWorkspace(id);
      workspaces = workspaces.filter((w) => w.id !== id);
      panes.clear();
      closeStream();
      const next = workspaces[0];
      await openRow(next ?? null);
      localStorage.setItem(LAST_KEY, next?.id ?? '');
      await tick();
      loadAll();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Chart templates ──
  // Kept in the global chart settings, so every workspace offers the same list; applying one
  // changes the charts, which the workspace saves like any other edit.
  const templates = $derived(templatesIn(settings));
  function saveTemplate(pane, name) {
    const tpl = templateOf($state.snapshot(pane), name);
    const old = templates.find((x) => x.name === tpl.name);
    if (old) tpl.id = old.id;
    settings = { ...settings, chartTemplates: [...templates.filter((x) => x.name !== tpl.name), tpl] };
  }
  const deleteTemplate = (id) =>
    (settings = { ...settings, chartTemplates: templates.filter((x) => x.id !== id) });

  /** "Apply to charts": the template and the charts ticked (every chart on screen to start). */
  let applying = $state(null);
  let applyIds = $state([]);
  function openApplyTo(tpl) {
    applying = tpl;
    applyIds = shown.map((p) => p.id);
  }
  function applyTemplateTo() {
    for (const id of applyIds) panes.get(id)?.useTemplate?.(applying);
    applying = null;
  }
  const paneLabel = (p) =>
    p.coords?.ticker ? `${p.coords.ticker} · ${p.coords.timeframe ?? ''}` : $t('histviz.templates.emptyChart');

  // ── Grid shape ──
  function setShape(rows, cols) {
    const before = new Set(shown.map((p) => p.id));
    reshape(ws, rows, cols);
    maximized = null;
    // A pane the grid no longer shows stops streaming (its instrument leaves the shared
    // connection); the panes still on screen carry on through the swap, see openStream.
    syncStream();
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
    syncStream();
  }

  const activate = (id) => {
    if (ws.active === id) return;
    ws.active = id;
  };

  /** The instrument an alert watches, in the active pane, read through the alert's own
   *  connector when it still has one. */
  let openedAlert = null;
  async function openAlert(id) {
    try {
      const a = await histvizApi.alert(id);
      const c = connectors.find((x) => x.id === a.connector_id) ?? connectors.find((x) => x.provider === a.provider);
      openInActive({
        connector_id: c?.id ?? null,
        provider: a.provider,
        asset_type: a.asset_type,
        ticker: a.ticker,
        timeframe: a.timeframe,
        timeframes: c?.timeframes ?? [],
        stream: (c?.stream_asset_types ?? []).includes(a.asset_type),
        stream_timeframes: c?.stream_timeframes ?? [],
        stream_note: c?.stream_note ?? ''
      });
    } catch (e) {
      error = e.message;
    }
  }
  // Already on the page (the global alert list links here): follow the link in place.
  $effect(() => {
    const id = $page.url.searchParams.get('alert');
    if (!loaded || !id || id === openedAlert) return;
    openedAlert = id;
    untrack(() => openAlert(id));
  });

  function openInActive(coords) {
    const id = ws.active ?? shown[0]?.id;
    railRef?.remember?.(coords);
    panes.get(id)?.openCoords?.(coords);
    if (railOpen && matchMedia('(max-width: 767px)').matches) toggleRail();
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
      .map(
        (id, i) =>
          `${id}|${reqs[i].provider}|${reqs[i].ticker}|${reqs[i].timeframe}|${reqs[i].connector_id ?? ''}|${reqs[i].quotes ? 'q' : ''}`
      )
      .join('~');
    if (key === streamKey) return;
    streamKey = key;
    // Make before break: the new connection opens first and the old one closes only once it
    // is up, so a pane that stays live never goes dark. Each connection routes with its own
    // pane list (the indices differ between the two), and a bar both deliver is merged by
    // its timestamp. Whatever closes during the swap comes back as the new stream's catch-up
    // (`since` is each pane's newest bar), and the server keeps a dropped subscription
    // through a grace period, so the upstream feed of a remaining instrument is never torn down.
    const prev = es;
    es = null;
    if (!reqs.length) {
      prev?.close();
      return;
    }
    let retired = false;
    const retire = () => {
      if (retired) return;
      retired = true;
      prev?.close();
    };
    // A connection that never opens must not keep the old one forever.
    setTimeout(retire, 5000);
    const route = (fn) => (d) => panes.get(ids[d?.i ?? 0])?.[fn]?.(d);
    // Supervised (lib/live/stream.js): a dropped, refused or silent connection is reopened
    // with backoff, and each reopen re-reads every pane's newest bar so the server's catch-up
    // starts from what the chart holds.
    es = liveStream({
      url: () =>
        histvizApi.streamsUrl(ids.map((id, i) => panes.get(id)?.streamRequest?.() ?? reqs[i])),
      events: {
        snapshot: route('applyBar'),
        bar: route('applyBar'),
        status: route('applyStatus'),
        // Interactive Brokers rations live data per account, not per connection: the server
        // says how many of the account's market-data lines the open panes are holding.
        lines: route('applyLines'),
        // Best bid and ask, for the panes that asked (the chart's bid/ask setting).
        quote: route('applyQuote'),
        quote_status: route('applyQuoteStatus')
      },
      onstate: (state, info) => {
        if (state === 'open') retire();
        // The server's own status events take over again once the connection is back.
        if (state === 'reconnecting') {
          const message = $t('histviz.live.serverLost', {
            seconds: Math.round((info.retryInMs ?? 0) / 1000)
          });
          for (const id of ids) {
            panes.get(id)?.applyStatus?.({ state: 'retrying', code: 'transport', message });
          }
        }
      }
    });
  }

  function closeStream() {
    if (es) {
      es.close();
      es = null;
    }
    streamKey = '';
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

  // Fullscreen folds the app top bar away; leaving the page always brings it back.
  $effect(() => () => {
    chrome.hideTopbar = false;
  });

  function onKey(e) {
    if ((e.key === 's' || e.key === 'S') && (e.ctrlKey || e.metaKey) && !e.altKey) {
      e.preventDefault();
      if (e.shiftKey) saveAsOpen = true;
      else if (dirty || !ws.id) saveWorkspace();
      return;
    }
    if (e.key === 'Escape' && shapeOpen) {
      shapeOpen = false;
      return;
    }
    if (e.key === 'Escape' && maximized) {
      maximized = null;
      return;
    }
    if (e.key === 'Escape' && chrome.hideTopbar) {
      chrome.hideTopbar = false;
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

  /** Accounts this module may read, loaded the first time the panel is opened: a chart page
   *  should not spend a request on a broker list nobody asked for. */
  async function loadBookAccounts() {
    bookError = '';
    try {
      bookAccounts = await brokersApi.list(BOOK_MODULE);
      if (!bookAccounts.some((a) => a.id === bookAccountId)) {
        bookAccountId = bookAccounts[0]?.id ?? '';
      }
    } catch (e) {
      bookError = e.message;
    }
  }

  function toggleBook() {
    bookOpen = !bookOpen;
    if (bookOpen && !bookAccounts.length) loadBookAccounts();
  }

  async function syncBook() {
    if (!bookAccountId) return;
    bookBusy = true;
    bookError = '';
    try {
      book = await brokersApi.book(bookAccountId, BOOK_MODULE);
      bookAt = Date.now();
      saveBookAccount(bookAccountId);
    } catch (e) {
      bookError = e.message;
    } finally {
      bookBusy = false;
    }
  }

  /** Another account means another book: the levels on screen are dropped until it is
   *  synced, rather than left showing the previous account's. */
  function pickBookAccount(id) {
    bookAccountId = id;
    clearBook();
  }

  function clearBook() {
    book = null;
    bookAt = 0;
    bookError = '';
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
  <div class="page" class:full={chrome.hideTopbar}>
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
            onblur={() => (renaming = false)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === 'Escape') renaming = false;
            }}
            aria-label={$t('histviz.workspace.name')}
          />
        {:else}
          <div class="ws-pick" use:clickOutside={() => (wsMenuOpen = false)}>
            <button
              bind:this={wsBtn}
              class="ws-btn"
              class:open={wsMenuOpen}
              aria-haspopup="menu"
              aria-expanded={wsMenuOpen}
              title={$t('histviz.workspace.switch')}
              onclick={toggleWsMenu}
            >
              <span class="ws-label">{ws.name || $t('histviz.workspace.untitled')}</span>
              <Icon name={wsMenuOpen ? 'chevron-up' : 'chevron-down'} size={12} />
            </button>
            {#if wsMenuOpen}
              <div
                class="ws-menu"
                role="menu"
                style={wsMenuAt
                  ? `top:${wsMenuAt.top}px;left:${wsMenuAt.left}px;width:${wsMenuAt.width}px`
                  : ''}
              >
                <button
                  class="ws-add"
                  role="menuitem"
                  onclick={() => {
                    wsMenuOpen = false;
                    guard(addWorkspace);
                  }}
                >
                  <Icon name="plus" size={13} />
                  <span>{$t('histviz.workspace.add')}</span>
                </button>
                <button
                  class="ws-add"
                  role="menuitem"
                  onclick={() => {
                    wsMenuOpen = false;
                    saveAsOpen = true;
                  }}
                >
                  <Icon name="copy" size={13} />
                  <span>{$t('histviz.workspace.saveAs')}</span>
                </button>
                <button
                  class="ws-add"
                  role="menuitem"
                  disabled={!dirty || !ws.id}
                  onclick={() => {
                    wsMenuOpen = false;
                    revertOpen = true;
                  }}
                >
                  <Icon name="rotate-ccw" size={13} />
                  <span>{$t('histviz.workspace.revert')}</span>
                </button>
                {#each wsOpts as o (o.value)}
                  <div class="ws-row" class:current={(ws.id ?? '') === o.value}>
                    <button
                      class="ws-go"
                      role="menuitem"
                      onclick={() => {
                        wsMenuOpen = false;
                        guard(() => switchWorkspace(o.value));
                      }}>{o.label}</button
                    >
                    <button
                      class="ws-act"
                      title={$t('histviz.workspace.rename')}
                      aria-label={$t('histviz.workspace.rename')}
                      onclick={() => renameFromMenu(o.value)}
                    >
                      <Icon name="pencil" size={12} />
                    </button>
                    <button
                      class="ws-act danger"
                      title={$t('histviz.workspace.remove')}
                      aria-label={$t('histviz.workspace.remove')}
                      disabled={!o.value}
                      onclick={() => askDelete(o.value, o.label)}
                    >
                      <Icon name="trash-2" size={12} />
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <!-- Save: the workspace changes only on screen until this is pressed (or Ctrl/Cmd+S).
           The dot says there is something to save. -->
      <button
        class="tbtn wide save"
        class:dirty
        disabled={saving || (!dirty && !!ws.id)}
        title={dirty ? $t('histviz.workspace.saveHint') : $t('histviz.workspace.saved')}
        aria-label={$t('histviz.workspace.save')}
        onclick={saveWorkspace}
      >
        <Icon name={dirty || !ws.id ? 'save' : 'check'} size={13} />
        <span class="tbtn-lbl">{dirty || !ws.id ? $t('histviz.workspace.save') : $t('histviz.workspace.saved')}</span>
        {#if dirty}<span class="unsaved" aria-hidden="true"></span>{/if}
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

      <span class="rule" aria-hidden="true"></span>

      <!-- The active chart's own controls (symbol, timeframe, studies, alerts, live...). One
           bar for every chart, TradingView style: the pane with the focus renders into it. -->
      <div class="pane-dock" bind:this={barDock}></div>

      <!-- Broker book: one synced account for every chart on screen. Outside the scroller
           for the same reason as the settings sheet: a scroll container clips a popover. -->
      <div class="book-wrap" use:clickOutside={() => (bookOpen = false)}>
        <button
          class="tbtn wide"
          class:on={bookOpen || !!book}
          aria-expanded={bookOpen}
          aria-haspopup="true"
          title={$t('histviz.book.hint')}
          onclick={toggleBook}
        >
          <Icon name="briefcase" size={13} />
          <span class="tbtn-lbl">{$t('histviz.book.title')}</span>
          {#if bookLevelCount}<span class="badge">{bookLevelCount}</span>{/if}
        </button>
        {#if bookOpen}
          <BookPanel
            accounts={bookAccounts}
            accountId={bookAccountId}
            onpickaccount={pickBookAccount}
            {book}
            syncedAt={bookAt}
            busy={bookBusy}
            error={bookError}
            tickers={openTickers}
            onsync={syncBook}
            onclear={clearBook}
            onaccounts={loadBookAccounts}
            onclose={() => (bookOpen = false)}
          />
        {/if}
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

      <!-- Fullscreen: folds the app top bar (search, agent, account) away, keeps the rest. -->
      <button
        class="tbtn"
        class:on={chrome.hideTopbar}
        aria-pressed={chrome.hideTopbar}
        title={chrome.hideTopbar ? $t('histviz.page.exitFullscreenEsc') : $t('histviz.page.fullscreen')}
        aria-label={chrome.hideTopbar ? $t('histviz.page.exitFullscreenEsc') : $t('histviz.page.fullscreen')}
        onclick={() => (chrome.hideTopbar = !chrome.hideTopbar)}
      >
        <Icon name={chrome.hideTopbar ? 'minimize' : 'maximize'} size={13} />
      </button>

    </header>

    <nav class="mobile-pane-nav" aria-label={$t('histviz.page.title')}>
      <button class:current={railOpen} onclick={toggleRail}>{railOpen ? $t('histviz.rail.collapse') : $t('histviz.rail.show')}</button>
      {#each shown as p, i (p.id)}
        <button class:current={!railOpen && ws.active === p.id} onclick={() => { activate(p.id); if (railOpen) toggleRail(); }}>
          {p.coords?.ticker ?? `${$t('histviz.page.title')} ${i + 1}`}
        </button>
      {/each}
    </nav>

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

      <!-- The active chart's drawing tools, shared like the bar above. -->
      <div class="draw-dock" class:mobile-rail={railOpen} bind:this={railDock}></div>

      <div class="grid-wrap" class:mobile-rail={railOpen}>
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
            <div class="cell" class:off={maximized && maximized !== p.id} class:mobile-inactive={ws.active !== p.id}>
              <Pane
                bind:pane={ws.panes[i]}
                {settings}
                {datasets}
                {providers}
                {connectors}
                quick={quickOn}
                {book}
                active={ws.active === p.id}
                {barDock}
                {railDock}
                maximized={maximized === p.id}
                closable={ws.panes.length > 1 && !maximized}
                onactivate={activate}
                onregister={register}
                onunregister={unregister}
                onquick={onQuickSession}
                onchanged={syncStream}
                bind:boards={ws.drawings}
                {templates}
                onsavetemplate={(name) => saveTemplate(p, name)}
                ondeletetemplate={deleteTemplate}
                onapplyto={openApplyTo}
                onmaximize={(id) => (maximized = maximized === id ? null : id)}
                onclose={closePane}
                onlinked={(kind, payload) => linked(p.id, kind, payload)}
                ondatasets={(rows) => (datasets = rows)}
                onsettings={(patch) => (settings = { ...settings, ...patch })}
                onapplysettings={(style) => {
                  // Every chart takes this style, and so does the next one opened.
                  for (const p of ws.panes) p.settings = { ...style };
                  settings = { ...settings, ...style };
                }}
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
    message={$t('histviz.workspace.removeConfirm', { name: deleting?.name ?? ws.name })}
    confirmLabel={$t('common.delete')}
    cancelLabel={$t('common.cancel')}
    danger
    onconfirm={() => removeWorkspace(deleting?.id ?? ws.id)}
  />
</RequireModule>

<ChoiceModal
  bind:open={unsavedOpen}
  title={$t('histviz.workspace.unsavedTitle')}
  message={$t('histviz.workspace.unsavedMessage', { name: ws.name })}
  cancelLabel={$t('common.cancel')}
  choices={[
    { value: 'discard', label: $t('histviz.workspace.discard'), variant: 'ghost' },
    { value: 'save', label: $t('histviz.workspace.save'), variant: 'primary' }
  ]}
  onpick={unsavedPick}
  oncancel={() => (unsavedGo = null)}
/>

<ConfirmModal
  bind:open={revertOpen}
  title={$t('histviz.workspace.revert')}
  message={$t('histviz.workspace.revertConfirm', { name: ws.name })}
  confirmLabel={$t('histviz.workspace.discard')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={revert}
/>

<PromptModal
  bind:open={saveAsOpen}
  title={$t('histviz.workspace.saveAs')}
  fields={[{ key: 'name', label: $t('histviz.workspace.name'), value: `${ws.name} (2)`, required: true }]}
  confirmLabel={$t('histviz.workspace.save')}
  onconfirm={(v) => saveAs(String(v.name ?? '').trim() || ws.name)}
/>

<Modal open={!!applying} size="sm" title={$t('histviz.templates.applyTitle', { name: applying?.name ?? '' })} onclose={() => (applying = null)}>
  <div class="apply">
    <label class="apply-all">
      <input
        type="checkbox"
        checked={applyIds.length === shown.length}
        indeterminate={applyIds.length > 0 && applyIds.length < shown.length}
        onchange={(e) => (applyIds = e.currentTarget.checked ? shown.map((p) => p.id) : [])}
      />
      <span>{$t('histviz.templates.allCharts')}</span>
    </label>
    {#each shown as p (p.id)}
      <label class="apply-row">
        <input
          type="checkbox"
          checked={applyIds.includes(p.id)}
          onchange={(e) =>
            (applyIds = e.currentTarget.checked ? [...applyIds, p.id] : applyIds.filter((x) => x !== p.id))}
        />
        <span class="apply-name">{paneLabel(p)}</span>
        {#if p.id === ws.active}<span class="apply-tag">{$t('histviz.templates.activeChart')}</span>{/if}
      </label>
    {/each}
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (applying = null)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={!applyIds.length} onclick={applyTemplateTo}>
      {$t('histviz.templates.applyN', { n: applyIds.length })}
    </button>
  {/snippet}
</Modal>

<svelte:window onkeydown={onKey} onbeforeunload={onBeforeUnload} />

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    height: calc(100vh - 96px);
    min-height: 420px;
  }
  /* The app top bar is folded away: its height goes back to the charts. */
  .page.full {
    height: calc(100vh - 96px + var(--topbar-height));
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
    flex: 0 1 auto;
    min-width: 0;
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .top-scroll::-webkit-scrollbar {
    display: none;
  }
  .pane-dock {
    display: flex;
    align-items: center;
    flex: 1 1 0;
    min-width: 0;
    height: 100%;
  }
  .draw-dock {
    display: flex;
    flex: none;
    min-height: 0;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .draw-dock:empty {
    display: none;
  }
  .draw-dock :global(.rail) {
    border-right: none;
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
  .ws-pick {
    width: 100%;
  }
  .ws-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-body);
    cursor: pointer;
  }
  .ws-btn.open,
  .ws-btn:hover {
    border-color: var(--accent);
  }
  .ws-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ws-menu {
    position: fixed;
    z-index: var(--z-dropdown);
    max-height: min(60vh, 420px);
    overflow-y: auto;
    padding: var(--space-1) 0;
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
  }
  .ws-add {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    color: var(--accent);
    font-size: var(--fs-body);
    text-align: left;
    cursor: pointer;
  }
  .ws-add:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .ws-add:disabled {
    color: var(--muted);
    opacity: 0.6;
    cursor: not-allowed;
  }
  .ws-row {
    display: flex;
    align-items: center;
    padding-right: var(--space-1);
  }
  .ws-row:hover {
    background: var(--surface-2);
  }
  .ws-go {
    flex: 1;
    min-width: 0;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: var(--fs-body);
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .ws-row.current .ws-go {
    color: var(--accent);
  }
  .ws-row:hover .ws-go {
    color: var(--text);
  }
  .ws-act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 26px;
    height: 26px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .ws-act:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface);
  }
  .ws-act.danger:hover:not(:disabled) {
    color: var(--red);
  }
  .ws-act:disabled {
    opacity: 0.35;
    cursor: not-allowed;
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
  /* Save: quiet once saved, accent with a dot while there are unsaved changes. */
  .tbtn.save {
    position: relative;
  }
  .tbtn.save:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .tbtn.save.dirty {
    color: var(--accent);
  }
  .unsaved {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber);
  }

  /* ── Apply a template to charts ── */
  .apply {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .apply-all,
  .apply-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .apply-all {
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    font-weight: 600;
  }
  .apply-row:hover {
    background: var(--surface-2);
  }
  .apply-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .apply-tag {
    color: var(--muted);
    font-size: var(--fs-small, 11px);
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

  /* ── Broker book ── */
  .book-wrap {
    position: relative;
    flex: none;
  }
  /* How many levels the synced account has in total, so the button says the book is there
     without the panel having to be open. */
  .badge {
    min-width: 15px;
    padding: 0 3px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: 9px;
    font-weight: var(--fw-medium);
    line-height: 15px;
    text-align: center;
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
  .mobile-pane-nav { display: none; }
  @media (max-width: 767px) {
    .page { height: max(650px, calc(100dvh - 96px)); min-height: 0; }
    .page.full { height: max(650px, calc(100dvh - 96px + var(--topbar-height))); }
    .top { height: auto; min-height: 44px; flex-wrap: wrap; padding: var(--space-1); }
    .top-scroll { flex: 1 1 100%; height: auto; min-height: 44px; flex-wrap: wrap; overflow: visible; }
    .top .rule { display: none; }
    .ws-slot { flex: 1 1 140px; width: auto; min-width: 0; }
    .top .tbtn { min-width: 44px; min-height: 44px; }
    .top .tbtn.wide { flex: 1 1 auto; }
    .book-wrap, .shape-wrap { flex: 1 1 auto; }
    .book-wrap .tbtn, .shape-wrap .tbtn { width: 100%; }
    .mobile-pane-nav { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-1); flex: none; }
    .mobile-pane-nav button { min-width: 0; min-height: 44px; padding: var(--space-2); overflow-wrap: anywhere; background: var(--surface); color: var(--text); border: 1px solid var(--border); }
    .mobile-pane-nav button.current { border-color: var(--accent); color: var(--accent); }
    .body { min-width: 0; }
    .body > :global(.rail) { width: 100% !important; flex: 1; min-width: 0; }
    .pane-dock { flex: 1 1 100%; height: 44px; }
    .draw-dock.mobile-rail { display: none; }
    .rail-grip, .splitters { display: none; }
    .grid-wrap.mobile-rail { display: none; }
    .grid { grid-template-columns: minmax(0, 1fr) !important; grid-template-rows: minmax(0, 1fr) !important; }
    .cell.mobile-inactive { display: none; }
  }
</style>
