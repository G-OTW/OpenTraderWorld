<script>
  // One chart pane: an instrument, its window of bars, its studies, its drawings and its
  // quick-backtest session. A workspace holds up to twelve of these, so everything here is
  // instance state: nothing reaches for the page.
  //
  // What the pane does NOT own is the live connection. Panes share one multiplexed SSE
  // stream (a socket per connector on the server, one HTTP connection in the browser), so the
  // page opens it and hands each pane its own bars through `applyBar`. The pane still owns
  // the *decision*: `pane.live` is its toggle and its status banner is its own.
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { t } from '$lib/i18n';
  import { histvizApi } from './api.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { suggestBrick, instanceLabel, isCustom } from './indicators.js';
  import Chart from './Chart.svelte';
  import DataModal from './DataModal.svelte';
  import IndicatorModal from './IndicatorModal.svelte';
  import DrawToolbar from './DrawToolbar.svelte';
  import AlertsModal from './AlertsModal.svelte';
  import ObjectTree from './ObjectTree.svelte';
  import { replay, makeFill, flipSide, resolveQty, lotFor, DEFAULT_QTY } from './quicktest.js';
  import {
    SLICE_BARS,
    TF_SECS,
    TF_ORDER,
    loadDrawings,
    saveDrawings,
    loadQuick,
    saveQuick,
    instrumentKeyOf,
    coordKeyOf,
    loadRecents,
    rememberRecent
  } from './pane.js';

  let {
    pane = $bindable(),
    settings = {},
    datasets = [],
    providers = [],
    connectors = [],
    active = false,
    maximized = false,
    closable = true,
    /** The workspace's quick-backtest mode. Global, not per pane: one session takes fills
     *  from every chart on screen and the bar at the bottom of the page adds them up. */
    quick = false,
    onactivate,
    onregister,
    onunregister,
    onchanged,
    onmaximize,
    onclose,
    onlinked,
    ondatasets,
    /** This pane's slice of the quick session, republished whenever it moves. */
    onquick,
    // The chart's own writes to the global settings (drawing templates and the style new
    // objects start with). The page owns the blob and persists it.
    onsettings
  } = $props();

  // ── Series state (this pane's own) ──
  let bars = $state(null);
  let meta = $state(null); // last series response: stored dataset, counts, history_start
  let notice = $state(null); // typed reason a window came back short ({code, message})
  let error = $state('');
  /** An instrument the rail could not turn into chart coordinates: {ticker, code, connector,
   *  message}. It takes the chart's place until something else is charted. */
  let blocked = $state(null);
  let loading = $state(false);
  let loadingMore = $state(false);
  let atHistoryStart = $state(true);
  let saving = $state(false);
  let savedFlash = $state('');
  let flashTimeout = null;

  /** A confirmation, not a state: it says the job was queued and then gets out of the way.
   *  The lasting answer is the stored badge in the bar, which the catalog poll turns on. */
  function flash(msg) {
    clearTimeout(flashTimeout);
    savedFlash = msg;
    if (msg) flashTimeout = setTimeout(() => (savedFlash = ''), 4000);
  }
  let handoff = $state(false);
  let dataOpen = $state(false);
  let modalOpen = $state(false);
  let editing = $state(null);
  let chartRef = $state(null);
  let restoreRange = $state(null);
  let recents = $state(loadRecents());
  let alertsOpen = $state(false);
  let alerts = $state([]);
  // Comparison instruments: their raw series, keyed by instrument+timeframe (see `cmpKey`)
  // so the data outlives the row that asked for it.
  let compareBars = $state({});
  let compareOpen = $state(false);
  let treeOpen = $state(false);

  const coords = $derived(pane.coords);
  const coordKey = $derived(coordKeyOf(coords));
  const instrumentKey = $derived(instrumentKeyOf(coords));
  const tfSecs = $derived(TF_SECS[coords?.timeframe] ?? 3600);
  const isStored = $derived(!!meta?.stored);

  // ── Live (state here, connection on the page) ──
  let liveStatus = $state(null); // {state, code, message, grain} from the server's `status` event
  let liveLag = $state(null);
  // Interactive Brokers only: market-data lines this account is holding, and the ceiling a
  // plain account has. IB refuses silently once it is reached, so the count is shown before
  // the pane that crosses it comes back empty.
  let lineUse = $state(null); // {connector, used, cap}
  let pendingBar = null; // latest unclosed bar awaiting the next coalesced flush
  let flushTimer = null;
  let autoLiveKey = null;
  /** True while this pane is following its group, so it does not answer back. */
  let linking = false;

  // The exact answer for the loaded window; `coords.stream` is the coarser flag a search hit
  // carries, used before the first series lands.
  const canLive = $derived(meta?.stream ?? !!coords?.stream);
  const liveTimeframes = $derived(meta?.stream_timeframes ?? coords?.stream_timeframes ?? []);
  const liveOffTimeframe = $derived(
    !canLive &&
      liveTimeframes.length > 0 &&
      !!coords?.timeframe &&
      !liveTimeframes.includes(coords.timeframe)
  );
  // What the provider needs before it will stream at all. It used to live only on the
  // connector admin screen, which is not where anyone is standing when a feed is refused.
  const streamNote = $derived(meta?.stream_note ?? coords?.stream_note ?? '');
  /** What the socket publishes underneath, in words. The dot only says connected: on a daily
   *  chart fed by minute bars the candle moves once a minute, and on a weekly one it can look
   *  frozen for days, which reads as a stall unless the pane says what it is waiting for. */
  const liveGrainHint = $derived.by(() => {
    const g = liveStatus?.grain;
    if (!pane.live || !g) return '';
    const tf = coords?.timeframe ?? '';
    if (g === 'native') return $t('histviz.page.liveGrainNative', { tf });
    if (g === 'trades') return $t('histviz.page.liveGrainTrades', { tf });
    return $t('histviz.page.liveGrainBars', { grain: g, tf });
  });
  const paneConnectors = $derived(
    coords ? connectors.filter((c) => c.provider === coords.provider) : []
  );
  const activeConnector = $derived(
    paneConnectors.find((c) => c.id === pane.connector_id) ?? paneConnectors[0] ?? null
  );

  /** What this pane contributes to the workspace stream: the instrument, the account it
   *  spends, and where its chart currently ends so the server can close the seam. */
  export function streamRequest() {
    if (!pane.live || !canLive || !coords) return null;
    return {
      provider: coords.provider,
      asset_type: coords.asset_type,
      ticker: coords.ticker,
      timeframe: coords.timeframe,
      connector_id: activeConnector?.id ?? coords.connector_id ?? null,
      since: bars?.ts?.length ? bars.ts[bars.ts.length - 1] : null
    };
  }

  // Register with the page so the shared stream can reach this pane, and so the rail and the
  // link groups can drive it.
  $effect(() => {
    onregister?.(pane.id, {
      applyBar: onLiveBar,
      applyStatus: onLiveStatus,
      applyLines: (d) => (lineUse = d),
      streamRequest,
      openCoords,
      switchTimeframe,
      apply,
      load,
      needsLoad,
      showBlocked,
      setLoading,
      flushLayout,
      setQuickSizing,
      quickReset,
      quickRemoveTrade,
      quickUndo,
      undoDrawing,
      hasQuickFills,
      hasDrawings,
      followCoords,
      setTimeRange,
      showCrosshair
    });
    return () => onunregister?.(pane.id);
  });

  // ── Compare ──
  // Another instrument on the same chart, rebased so the two are comparable: prices in
  // dollars and prices in yen have nothing to say to each other, percentages do. The series
  // is mapped onto *this* pane's clock (last value at or before each of its bars), because
  // two markets do not close at the same instants and a naive index match would slide one
  // series against the other.
  // Canvas comparison lines need resolved colors. The helper is reactive, so the palette
  // follows a light/dark switch rather than retaining a legacy fixed-blue ramp.
  const COMPARE_COLORS = $derived(chartColors().series.slice(0, 4));

  /** Cache key of a comparison: the instrument and the period, never the row id. Taking one
   *  off and putting it back, or charting it again after a detour, must not pay twice for a
   *  window the pane still holds. */
  const cmpKey = (c) => `${c.provider}|${c.asset_type}|${c.ticker}|${coords?.timeframe}`;
  /** The current comparisons plus a few recent ones, so an untick is undoable for free. */
  const CMP_CACHE_MAX = 8;

  const compareSeries = $derived.by(() => {
    const list = pane.compare ?? [];
    if (!list.length || !bars?.ts?.length) return [];
    const base = { ts: bars.ts, ms: bars.ts.map((t) => Date.parse(t)) };
    return list.map((c, i) => {
      const raw = compareBars[cmpKey(c)];
      const values = raw ? rebase(base, raw, pane.compareMode ?? 'pct') : [];
      return {
        id: c.id,
        label: c.ticker + (pane.compareMode === 'ratio' ? ` (${coords?.ticker}/${c.ticker})` : ''),
        color: c.color ?? COMPARE_COLORS[i % COMPARE_COLORS.length],
        hidden: !!c.hidden,
        mode: pane.compareMode ?? 'pct',
        values
      };
    });
  });

  /** Map a comparison series onto this pane's bars and rebase it.
   *
   *  `pct` is the comparison every terminal draws: both instruments start at 0 on the left
   *  edge of the window and the distance between the lines is the relative performance.
   *  `ratio` is the pair trade's view: this instrument divided by the other, rebased to 100.
   *
   *  The mapping is the server's wherever it bucketed this bar (`raw.map`, one bucket set for
   *  both instruments) and this pane's own walk everywhere else: an intraday comparison across
   *  two providers has no honest bucket, so the server refuses to invent one, and a bar that
   *  arrived after the last batch (a live candle, a "load more") is drawn from the last close
   *  at or before it rather than left blank until the refresh lands.
   *
   *  A period the other instrument did not trade (a holiday, a listing gap) carries the last
   *  close forward, so the line has no hole where the market was simply shut.
   */
  function rebase(base, raw, mode) {
    const otherC = raw.c ?? [];
    const otherMs = raw.ms ?? [];
    const n = base.ts.length;
    const out = new Array(n).fill(null);
    let last = null;
    let w = -1; // walk cursor: both series are sorted, so one pass is enough
    for (let i = 0; i < n; i++) {
      while (w + 1 < otherMs.length && otherMs[w + 1] <= base.ms[i]) w++;
      const bucketed = raw.map?.has(base.ts[i]);
      const j = bucketed ? raw.map.get(base.ts[i]) : w >= 0 ? w : null;
      if (j != null && otherC[j] != null) last = otherC[j];
      out[i] = last;
    }
    const first = out.findIndex((x) => x != null);
    if (first < 0) return out;
    if (mode === 'ratio') {
      const base0 = bars.c[first];
      const other0 = out[first];
      if (!other0 || !base0) return out.map(() => null);
      const k = 100 / (base0 / other0);
      return out.map((x, i) => (x == null || !x ? null : (bars.c[i] / x) * k));
    }
    const v0 = out[first];
    if (!v0) return out.map(() => null);
    return out.map((x) => (x == null ? null : (x / v0 - 1) * 100));
  }

  let compareTimer = null;
  let compareToken = 0;
  let compareAbort = null;
  let compareInflight = null; // signature of the request on the wire, never fired twice

  /** Load the comparisons in `list` over this pane's own window, in one round trip.
   *
   *  Only what the cache cannot answer is asked for: the rest is already drawn. An older
   *  request is dropped rather than awaited, because its answer describes a window that has
   *  moved on. */
  async function loadCompare(list) {
    if (!list.length || !coords || !bars?.ts?.length) return;
    const clock = bars.ts.slice();
    const sig = `${clock[0]}|${clock[clock.length - 1]}|${clock.length}|${list.map(cmpKey).join(',')}`;
    if (sig === compareInflight) return;
    const to = new Date(Date.parse(clock[clock.length - 1]) + tfSecs * 1000).toISOString();
    const token = ++compareToken;
    compareInflight = sig;
    compareAbort?.abort();
    const ctrl = new AbortController();
    compareAbort = ctrl;
    try {
      // On this pane's clock: the overlay is drawn on these candles, so the server buckets
      // each comparison onto them rather than answering with a set of rows of its own.
      const { results } = await histvizApi.seriesBatchOn(
        list.map((c) => ({
          connector_id: c.connector_id ?? null,
          provider: c.provider,
          asset_type: c.asset_type,
          ticker: c.ticker,
          timeframe: coords.timeframe,
          to,
          bars: Math.max(clock.length, 100)
        })),
        { provider: coords.provider, ts: clock },
        { signal: ctrl.signal }
      );
      if (token !== compareToken) return;
      const next = { ...compareBars };
      list.forEach((c, i) => {
        const r = results[i];
        if (!r || r.error) return;
        next[cmpKey(c)] = {
          c: r.c ?? [],
          ms: (r.ts ?? []).map((t) => Date.parse(t)),
          // The server's bucketing kept **by timestamp**, not by row: the pane's bars grow at
          // both edges (a live candle, a "load more"), and a row-indexed map would slide one
          // series against the other the moment they do.
          map: Array.isArray(r.map) ? new Map(clock.map((ts, k) => [ts, r.map[k]])) : null
        };
      });
      compareBars = prune(next, new Set((pane.compare ?? []).map(cmpKey)));
    } catch {
      /* a comparison that cannot load leaves the price chart alone */
    } finally {
      if (compareInflight === sig) compareInflight = null;
    }
  }

  /** Does the cached entry already answer for the whole window on screen? */
  function covered(e, first, last) {
    if (!e) return false;
    // The server bucketed this window: a row it aligned is exact, and one it never saw is not.
    if (e.map) return e.map.has(first) && e.map.has(last);
    // No honest bucket to be had (intraday across providers): the walk *is* the answer, so
    // what matters is that the other instrument's own bars span the window.
    const n = e.ms.length;
    return n > 0 && e.ms[0] <= Date.parse(first) && e.ms[n - 1] >= Date.parse(last);
  }

  function prune(cache, keep) {
    const spare = Object.keys(cache).filter((k) => !keep.has(k));
    const over = Object.keys(cache).length - CMP_CACHE_MAX;
    if (over <= 0) return cache;
    for (const k of spare.slice(0, over)) delete cache[k];
    return cache;
  }

  // What to fetch, and how urgently. A comparison the pane has no data for at all is fetched
  // now (there is nothing on screen to look at); one whose window merely grew is redrawn from
  // the cache immediately and refreshed once the panning stops, so scrolling the chart never
  // waits on the network.
  $effect(() => {
    const list = pane.compare ?? [];
    const n = bars?.ts?.length ?? 0;
    const first = n ? bars.ts[0] : null;
    const last = n ? bars.ts[n - 1] : null;
    void coordKey;
    untrack(() => {
      clearTimeout(compareTimer);
      if (!list.length || !n) {
        compareToken++;
        compareAbort?.abort();
        return;
      }
      const missing = list.filter((c) => !compareBars[cmpKey(c)]);
      // Bars the last batch did not bucket: drawn from the walk until the refresh lands.
      const stale = list.filter((c) => {
        const e = compareBars[cmpKey(c)];
        return e && !covered(e, first, last);
      });
      if (missing.length) loadCompare([...missing, ...stale]);
      else if (stale.length) compareTimer = setTimeout(() => loadCompare(stale), 300);
    });
    return () => clearTimeout(compareTimer);
  });

  function addCompare(next) {
    const list = pane.compare ?? [];
    const id = `${next.provider}|${next.asset_type}|${next.ticker}`;
    if (list.some((c) => c.id === id) || list.length >= 4) return;
    pane.compare = [
      ...list,
      {
        id,
        provider: next.provider,
        asset_type: next.asset_type,
        ticker: next.ticker,
        connector_id: next.connector_id ?? null,
        color: COMPARE_COLORS[list.length % COMPARE_COLORS.length],
        hidden: false
      }
    ];
    onchanged?.();
  }

  function toggleCompare(id) {
    pane.compare = (pane.compare ?? []).map((c) =>
      c.id === id ? { ...c, hidden: !c.hidden } : c
    );
    onchanged?.();
  }

  function removeCompare(id) {
    pane.compare = (pane.compare ?? []).filter((c) => c.id !== id);
    onchanged?.();
  }

  // ── Alerts ──
  // The levels the *server* is watching for this instrument. The pane only shows how many
  // are armed and opens the list: an alert works with the browser closed, which is exactly
  // why none of its logic lives here.
  const armedAlerts = $derived(alerts.filter((a) => a.enabled).length);

  $effect(() => {
    const key = coordKey;
    if (!key) {
      alerts = [];
      return;
    }
    const c = untrack(() => $state.snapshot(coords));
    histvizApi
      .alerts(c)
      .then((rows) => {
        if (untrack(() => coordKey) === key) alerts = rows;
      })
      .catch(() => {
        /* the badge is a convenience: a failure here must not break the pane */
      });
  });

  function refreshAlerts() {
    if (!coords) return;
    histvizApi
      .alerts($state.snapshot(coords))
      .then((rows) => (alerts = rows))
      .catch(() => {});
  }

  /** The price an alert would be created at: the selected horizontal line, else the last
   *  close. Drawing the level first and arming it second is the gesture every chart uses. */
  const alertLevel = $derived.by(() => {
    const sel = drawings.find((d) => d.id === selectedDrawing && d.tool === 'hline');
    if (sel) return sel.a.y;
    return bars?.c?.length ? bars.c[bars.c.length - 1] : null;
  });
  const alertFromLine = $derived(
    !!drawings.find((d) => d.id === selectedDrawing && d.tool === 'hline')
  );

  // ── Indicators ──
  let instances = $state(pane.instances ?? []);
  let nextId = (pane.instances ?? []).reduce((m, i) => Math.max(m, i.id ?? 0), 0) + 1;

  // Custom instances carry a copy of their definition so the chart still draws offline, but
  // the library is the source of truth: on load, refresh every instance from its row.
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
        /* offline or library unreachable: keep the embedded definitions */
      });
  });

  // ── Drawings (per instrument, this browser) ──
  let tool = $state('cursor');
  let drawings = $state([]);
  let selectedDrawing = $state(null);
  let magnet = $state(false);
  let drawingsHidden = $state(false);

  let loadedKey = null;
  $effect(() => {
    const key = instrumentKey;
    if (key === loadedKey) return;
    loadedKey = key;
    drawings = key ? loadDrawings(key) : [];
    selectedDrawing = null;
  });
  $effect(() => {
    const key = untrack(() => loadedKey);
    const list = drawings;
    if (key) saveDrawings(key, list);
  });

  // ── Quick backtest (per instrument, this browser) ──
  let quickFills = $state([]);
  let quickSize = $state(DEFAULT_QTY);
  let quickUnit = $state('units');
  let quickContract = $state(1);

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
    quickFills = key ? loadQuick(key) : [];
  });

  $effect(() => {
    const key = untrack(() => quickLoadedKey);
    const list = quickFills;
    if (key) saveQuick(key, list);
  });

  /** What this pane contributes to the workspace's session. The replay happens here because
   *  MAE and R are measured against *these* bars; the page only adds the results up. */
  $effect(() => {
    onquick?.(
      pane.id,
      quick && coords?.ticker
        ? {
            symbol: coords.ticker,
            timeframe: coords.timeframe ?? '',
            trades: session.trades,
            open: session.open,
            last: lastClose,
            size: quickSize,
            mode: quickUnit,
            contract: quickContract
          }
        : null
    );
  });

  /** The bottom bar edits the sizing of the chart it is pointing at. */
  export function setQuickSizing(p) {
    if (p.size != null) quickSize = p.size;
    if (p.mode != null) quickUnit = p.mode;
    if (p.contract != null) quickContract = p.contract;
  }

  // ── Layout, persisted server-side per instrument ──
  // Coordinates, not a dataset id: a symbol you are only looking at keeps its studies too.
  let layoutLoadedFor = $state(null);
  let layoutTimer = null;
  let pendingLayout = null;

  function applyLayout(l) {
    if (typeof l.type === 'string') pane.type = l.type;
    if (Number.isFinite(l.brick)) pane.brick = l.brick;
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
    const key = coordKey;
    if (!key || key === untrack(() => layoutLoadedFor)) return;
    const c = untrack(() => coords);
    histvizApi
      .layoutOf(c)
      .then((row) => {
        if (untrack(() => coordKey) !== key) return; // the instrument changed meanwhile
        if (row && typeof row === 'object') applyLayout(row);
        layoutLoadedFor = key;
      })
      .catch(() => {
        layoutLoadedFor = key; // offline: keep working from the local copy
      });
  });

  // Autosave, debounced. Gated on the load having landed, so the first frame of a fresh
  // instrument can never overwrite the layout it is about to receive.
  $effect(() => {
    const key = coordKey;
    const snap = {
      type: pane.type,
      brick: pane.brick,
      instances: $state.snapshot(instances),
      drawings: $state.snapshot(drawings)
    };
    if (!key || untrack(() => layoutLoadedFor) !== key) return;
    const c = untrack(() => $state.snapshot(coords));
    clearTimeout(layoutTimer);
    pendingLayout = { c, snap };
    layoutTimer = setTimeout(() => {
      pendingLayout = null;
      histvizApi.saveLayoutOf(c, snap).catch(() => {
        /* the browser copy is still there; nothing to tell the user */
      });
    }, 800);
  });

  /** Leaving the page inside the debounce window would lose the last edit, and the layout is
   *  authoritative, so the *next* load would then undo it. `keepalive` lets the request
   *  outlive the document. */
  export function flushLayout() {
    if (!pendingLayout) return;
    const { c, snap } = pendingLayout;
    pendingLayout = null;
    clearTimeout(layoutTimer);
    try {
      fetch('/api/histviz/layout', {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ ...c, layout: snap }),
        keepalive: true
      });
    } catch {
      /* nothing more we can do on the way out */
    }
  }

  // "Autosave data": store the instrument the first time it is charted, so a download exists
  // to chart offline later. Off by default, it spends an API key.
  let autoStoreKey = null;
  $effect(() => {
    if (!settings.autosaveData || !coords || !bars?.count || isStored) return;
    const key = coordKey;
    if (autoStoreKey === key) return;
    autoStoreKey = key;
    untrack(() => saveWindow('loaded'));
  });

  // ── Loading ──

  /** Chart an instrument in this pane. Keeps the current timeframe when the connector
   *  supports it. */
  export async function openCoords(next) {
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
    pane.coords = { ...next, timeframe: tf, timeframes: list };
    recents = rememberRecent(pane.coords);
    onchanged?.();
    if (!linking) onlinked?.('symbol', $state.snapshot(pane.coords));
    await load();
  }

  /** First paint of the current coordinates: the newest slice, nothing older. */
  export async function load() {
    if (!coords) return;
    loading = true;
    error = '';
    notice = null;
    blocked = null;
    flash('');
    if (pane.live) {
      pane.live = false;
      onlinked?.('restream');
    }
    try {
      const r = await histvizApi.series({ ...$state.snapshot(coords), bars: SLICE_BARS });
      apply(r);
    } catch (e) {
      error = e.message;
      bars = null;
      meta = null;
    } finally {
      loading = false;
    }
  }

  /** Take a series answer, whether it came from this pane's own call or the workspace's
   *  batch (one round trip for every pane on open). */
  export function apply(r) {
    if (r?.error) {
      error = r.error;
      bars = null;
      meta = null;
      loading = false;
      return;
    }
    bars = r;
    meta = r;
    blocked = null;
    notice = r.notice ?? null;
    atHistoryStart = !!r.history_start;
    if (!r.count && !r.notice) notice = { code: 'empty', message: $t('histviz.notice.empty') };
    if (pane.type === 'renko' && !pane.brick) pane.brick = suggestBrick(bars.c);
    loading = false;
    maybeAutoLive(r);
  }

  export function setLoading(v) {
    loading = v;
  }

  /** Say, in place of the chart, that an instrument cannot be served and what to do about it.
   *  A watchlist row bound to a broker that is off is not an empty chart: it is a connector to
   *  fix, and the pane is where the user is looking. */
  export function showBlocked(info) {
    blocked = info ?? null;
    bars = null;
    meta = null;
    notice = null;
    error = '';
    loading = false;
  }

  /** True when this pane is showing an instrument it has no bars for and nothing is on its
   *  way: a pane that mounted outside the workspace's opening batch (a maximize, a restored
   *  layout) would otherwise sit on the empty state next to its own symbol. */
  export function needsLoad() {
    return !!coords?.ticker && !bars && !loading && !error;
  }

  /** "Load more": the 1500 bars just before the oldest one on screen. User-triggered, stored
   *  bars are reused and only the real gap is downloaded. */
  async function loadMore() {
    if (loadingMore || atHistoryStart || !coords || !bars?.ts?.length) return;
    loadingMore = true;
    notice = null;
    try {
      const r = await histvizApi.series({
        ...$state.snapshot(coords),
        to: bars.ts[0],
        bars: SLICE_BARS
      });
      notice = r.notice ?? null;
      const got = r.ts?.length ?? 0;
      if (!got) {
        atHistoryStart = !r.notice; // a failure is retryable; genuinely empty is the end
        if (!r.notice)
          notice = { code: 'history_start', message: $t('histviz.notice.historyStart') };
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

  export function switchTimeframe(tf) {
    if (!coords || tf === coords.timeframe) return;
    restoreRange = chartRef?.getTimeRange?.() ?? null;
    pane.coords = { ...coords, timeframe: tf };
    recents = rememberRecent(pane.coords);
    onchanged?.();
    load();
  }

  const timeframes = $derived.by(() => {
    if (!coords) return [];
    const fromCoords = coords.timeframes?.length ? coords.timeframes : null;
    const fromMatrix = providers.find((p) => p.provider === coords.provider)?.timeframes ?? [];
    const list = fromCoords ?? fromMatrix;
    return [...list].sort((a, b) => TF_ORDER.indexOf(a) - TF_ORDER.indexOf(b));
  });

  /** Store what is on screen: queues the normal download job over the loaded window. */
  async function saveWindow(scope = 'loaded') {
    if (!coords || !bars?.ts?.length) return;
    saving = true;
    error = '';
    flash('');
    try {
      const view = scope === 'visible' ? (chartRef?.getTimeRange?.() ?? null) : null;
      const from = view?.t0 ?? bars.ts[0];
      const lastTs = view?.t1 ?? bars.ts[bars.ts.length - 1];
      const to = new Date(Date.parse(lastTs) + tfSecs * 1000).toISOString();
      await histvizApi.savePreview({ ...$state.snapshot(coords), from, to });
      flash($t('histviz.page.savedQueued'));
      const stored = await waitForStore();
      // The badge in the bar now says it: the message has nothing left to add.
      if (stored) flash('');
      // The live feed only records once its coordinates have a dataset: ask the page to
      // reopen the stream so the server picks the new one up.
      if (stored && pane.live) onlinked?.('restream');
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
      const rows = await histvizApi.datasets();
      ondatasets?.(rows);
      const d = rows.find(
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

  // ── Backtest handoff ──
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

  // ── Live ──

  /** Live is the default view of a current instrument: if the newest bar is the one forming
   *  right now, connect without being asked. Nothing is stored by watching. */
  function maybeAutoLive(r) {
    if (!r?.ts?.length || !r?.stream) return;
    if (autoLiveKey === coordKey) return;
    const lastMs = Date.parse(r.ts[r.ts.length - 1]);
    if (Date.now() - lastMs >= tfSecs * 2000) return;
    autoLiveKey = coordKey;
    pane.live = true;
    onlinked?.('restream');
  }

  export function toggleLive() {
    pane.live = !pane.live;
    autoLiveKey = coordKey;
    if (!pane.live) liveStatus = null;
    onlinked?.('restream');
  }

  function pickConnector(id) {
    if (id === pane.connector_id) return;
    pane.connector_id = id;
    liveStatus = null;
    onchanged?.();
    if (pane.live) onlinked?.('restream');
  }

  /** The server's own verdict on the feed. `stopped` means it has given up on purpose (a key
   *  to fix, a plan to upgrade, a subscription to buy), so the toggle goes off with it: the
   *  banner keeps the reason on screen. */
  function onLiveStatus(s) {
    liveStatus = s;
    if (s?.state === 'stopped') {
      pane.live = false;
      autoLiveKey = coordKey; // don't let auto-live restart what the server refused
      onlinked?.('restream');
    }
  }

  function retryLive() {
    liveStatus = null;
    autoLiveKey = coordKey;
    pane.live = true;
    onlinked?.('restream');
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

  // Merge one bar into `bars` by its timestamp. Keyed on the timestamp and *ordered*, not
  // appended: the catch-up the server sends when the stream opens is a run of bars newer than
  // the chart's history but older than the bar forming now, and the axis is drawn by index,
  // so a bar dropped for arriving out of order shifts every later candle one slot left.
  function applyBar(d) {
    if (!bars?.ts?.length) {
      bars = { ...(bars ?? {}), ts: [d.ts], o: [d.o], h: [d.h], l: [d.l], c: [d.c], v: [d.v] };
      return;
    }
    const evMs = Date.parse(d.ts);
    const n = bars.ts.length;
    let i = n - 1;
    while (i >= 0 && Date.parse(bars.ts[i]) > evMs) i--;
    if (i >= 0 && Date.parse(bars.ts[i]) === evMs) {
      bars.o[i] = d.o;
      bars.h[i] = d.h;
      bars.l[i] = d.l;
      bars.c[i] = d.c;
      bars.v[i] = d.v;
      return;
    }
    const at = i + 1;
    bars.ts.splice(at, 0, d.ts);
    bars.o.splice(at, 0, d.o);
    bars.h.splice(at, 0, d.h);
    bars.l.splice(at, 0, d.l);
    bars.c.splice(at, 0, d.c);
    bars.v.splice(at, 0, d.v);
  }

  // ── Quick backtest actions ──
  // What one clicked unit is worth, at the price of that click: a notional fill reads in
  // currency, so it needs its own price, not the position's.
  const quickLot = (price) => lotFor(quickUnit, price, quickContract);

  function quickClick(p) {
    const open = session.open;
    if (open)
      quickFills = [
        ...quickFills,
        makeFill(open.dir > 0 ? 'short' : 'long', p.x, p.y, open.qty, quickLot(p.y))
      ];
    else
      quickFills = [
        ...quickFills,
        makeFill('long', p.x, p.y, resolveQty(quickUnit, quickSize, p.y, quickContract), quickLot(p.y))
      ];
  }
  function quickFill(f) {
    quickSize = f.size;
    quickFills = [
      ...quickFills,
      makeFill(f.side, f.x, f.y, resolveQty(quickUnit, f.size, f.y, quickContract), quickLot(f.y))
    ];
  }
  function quickToggle(id) {
    quickFills = quickFills.map((f) => (f.id === id ? { ...f, side: flipSide(f.side) } : f));
  }
  export function quickRemoveTrade(trade) {
    const ids = new Set(trade.fillIds ?? []);
    quickFills = quickFills.filter((f) => !ids.has(f.id));
  }
  export function quickReset() {
    quickFills = [];
  }
  export function quickUndo() {
    if (quickFills.length) quickFills = quickFills.slice(0, -1);
  }
  export function undoDrawing() {
    if (!drawings.length) return;
    const last = drawings[drawings.length - 1];
    drawings = drawings.slice(0, -1);
    if (selectedDrawing === last.id) selectedDrawing = null;
  }
  export function hasQuickFills() {
    return quickFills.length > 0;
  }
  export function hasDrawings() {
    return drawings.length > 0;
  }
  function deleteDrawing() {
    if (!selectedDrawing) return;
    drawings = drawings.filter((d) => d.id !== selectedDrawing);
    selectedDrawing = null;
  }
  function clearDrawings() {
    drawings = [];
    selectedDrawing = null;
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
    const body = {
      type: draft.type,
      params: draft.params,
      style: draft.style,
      ...(draft.type === 'custom' ? { custom: draft.custom, pane: draft.pane } : {})
    };
    if (editing) instances = instances.map((i) => (i.id === editing.id ? { ...i, ...body } : i));
    else instances = [...instances, { id: nextId++, ...body, visible: true }];
    editing = null;
  }
  const toggleInstance = (id) =>
    (instances = instances.map((i) => (i.id === id ? { ...i, visible: !i.visible } : i)));
  const removeInstance = (id) => (instances = instances.filter((i) => i.id !== id));

  // Keep the pane's saved copy of its studies in step with what is on screen.
  $effect(() => {
    pane.instances = $state.snapshot(instances);
  });

  $effect(() => {
    if (pane.type === 'renko' && bars && !pane.brick) pane.brick = suggestBrick(bars.c);
  });

  // ── Presentation ──
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

  /** What stands in for the chart when there is nothing to draw: a rail row that could not be
   *  resolved, or a request that came back with nothing. One card: what failed, one short
   *  reason, the server's own line, and the buttons that fix it. The buttons say what to do,
   *  so the card does not also spell it out in a sentence. */
  const ISSUE_WHY = {
    grant: 'histviz.issue.grant',
    symbol: 'histviz.issue.symbol',
    none: 'histviz.issue.none',
    auth: 'histviz.issue.auth',
    unsupported: 'histviz.issue.unsupported',
    transport: 'histviz.issue.transport'
  };
  const issue = $derived.by(() => {
    if (loading || bars?.count) return null;
    if (blocked) {
      const name = blocked.connector || coords?.provider || '';
      return {
        ticker: blocked.ticker || '',
        why: $t(ISSUE_WHY[blocked.code] ?? 'histviz.issue.transport', { name }),
        detail: blocked.message ?? '',
        retry: false
      };
    }
    if (error && coords)
      return {
        ticker: coords.ticker ?? '',
        why: $t('histviz.issue.transport', { name: activeConnector?.name ?? coords.provider ?? '' }),
        detail: error,
        retry: true
      };
    return null;
  });

  const LIVE_FAULT_KEYS = {
    auth: 'histviz.live.auth',
    plan: 'histviz.live.plan',
    entitlement: 'histviz.live.entitlement',
    symbol: 'histviz.live.symbol',
    unsupported: 'histviz.live.unsupported',
    conflict: 'histviz.live.conflict',
    transport: 'histviz.live.transport'
  };
  const liveFaultKey = (code) => LIVE_FAULT_KEYS[code] ?? LIVE_FAULT_KEYS.transport;
  const liveIssue = $derived(
    liveStatus?.message && (liveStatus.state === 'stopped' || liveStatus.state === 'retrying')
      ? liveStatus
      : null
  );
  /** A refused feed is nearly always an entitlement, and the provider's own line on what live
   *  costs is the answer: show it here, next to the refusal, not only on the admin screen. */
  const liveIssueNote = $derived(
    liveIssue && (liveIssue.code === 'plan' || liveIssue.code === 'entitlement') ? streamNote : ''
  );
  const liveIssueText = $derived(
    [liveIssue?.message, liveIssueNote].filter(Boolean).join('. ').replace('.. ', '. ')
  );

  /** Within ten lines of the cap. Below that the number is trivia; at the cap IB stops
   *  answering, and a chart that just goes quiet is the worst way to learn about a limit. */
  const lineWarn = $derived(
    lineUse && pane.live && lineUse.used >= lineUse.cap - 10 ? lineUse : null
  );

  const CHART_TYPES = [
    ['candlestick', 'histviz.page.typeCandles'],
    ['ohlc', 'histviz.page.typeOhlc'],
    ['line', 'histviz.page.typeLine'],
    ['renko', 'histviz.page.typeRenko']
  ];
  const typeOpts = $derived(CHART_TYPES.map(([value, label]) => ({ value, label: $t(label) })));
  /** The picker always carries the loaded timeframe, even when the provider matrix has not
   *  come back yet: a disabled picker showing the wrong thing is worse than a slow one. */
  const tfOpts = $derived(
    (timeframes.length ? timeframes : coords?.timeframe ? [coords.timeframe] : []).map((tf) => ({
      value: tf,
      label: tf
    }))
  );

  // ── Link group ──
  // A group shares the *instrument* and the *instant*, never the timeframe: three panes on
  // one symbol at 1m, 1h and 1d is the reason to link them in the first place. The visible
  // span only follows between panes on the same timeframe, since the same two dates are a
  // screenful on one and two candles on the other.

  /** The group told this pane to chart something else. Same load as a click, minus the
   *  broadcast: a pane that re-announced what it was just told would loop the group. */
  export function followCoords(next) {
    linking = true;
    openCoords(next).finally(() => (linking = false));
  }

  export function setTimeRange(range, timeframe) {
    if (timeframe && timeframe !== coords?.timeframe) return;
    chartRef?.setTimeRange?.(range.t0, range.t1);
  }

  export function showCrosshair(ms) {
    chartRef?.showCrosshairAt?.(ms);
  }

  /** Cycle the pane through the link groups. The colour *is* the group, as on every market
   *  terminal: no names to invent, and which panes move together is visible at a glance. */
  const LINK_COLORS = ['', 'blue', 'green', 'orange', 'purple'];
  const LINK_HUES = {
    blue: 'var(--chart-1)',
    green: 'var(--chart-2)',
    orange: 'var(--chart-5)',
    purple: 'var(--chart-6)'
  };
  function cycleLink() {
    const i = LINK_COLORS.indexOf(pane.link ?? '');
    pane.link = LINK_COLORS[(i + 1) % LINK_COLORS.length];
    onchanged?.();
  }

  /** Save the chart as it looks right now. The browser does the saving: the image is the
   *  instance's own render, overlays included, so what is filed is what was on screen. */
  async function exportPng() {
    const url = await chartRef?.snapshot?.();
    if (!url) return;
    const a = document.createElement('a');
    a.href = url;
    a.download = `${exportName()}.png`;
    a.click();
  }

  /** Save the chart as a page: the picture, what it is a picture *of*, and the bars it was
   *  drawn from, in one file that opens offline in any browser.
   *
   *  The image is the chart's own render, so a PNG and this carry the same pixels; what the
   *  HTML adds is provenance (instrument, window, studies, who served the bars) and the data
   *  itself, embedded as JSON. A screenshot pasted into a journal entry or a doc is a claim
   *  nobody can check afterwards; this one can be re-read. Nothing is uploaded: the file is
   *  built in the browser and saved from it. */
  async function exportHtml() {
    const png = await chartRef?.snapshot?.();
    if (!png || !bars?.ts?.length) return;
    const esc = (v) =>
      String(v ?? '').replace(
        /[&<>"']/g,
        (ch) =>
          ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[ch]
      );
    const rows = bars.ts.length;
    const studies = instances
      .filter((i) => i.visible !== false)
      .map((i) => (isCustom(i) ? (i.custom?.name ?? '') : instanceLabel(i.type, i.params)))
      .join(', ');
    const compared = (pane.compare ?? []).map((c) => c.ticker).join(', ');
    const heading = `${coords?.ticker ?? ''} ${coords?.timeframe ?? ''}`.trim();
    // A row is written only when it says something: an empty "Compared with" line is noise
    // in a document meant to be read later.
    const facts = [
      [$t('histviz.export.range'), `${bars.ts[0]} → ${bars.ts[rows - 1]}`],
      [$t('histviz.export.bars'), `${rows} (${coords?.timeframe ?? ''})`],
      [$t('histviz.notice.provider'), coords?.provider ?? ''],
      ...(studies ? [[$t('histviz.export.studies'), studies]] : []),
      ...(compared ? [[$t('histviz.export.compare'), compared]] : []),
      ...(drawings.length ? [[$t('histviz.export.objects'), String(drawings.length)]] : []),
      [$t('histviz.export.generated'), new Date().toLocaleString()]
    ];
    const tail = Math.max(0, rows - 200);
    const body = bars.ts
      .slice(tail)
      .map(
        (ts, k) =>
          `<tr><td>${esc(ts)}</td><td>${bars.o[tail + k]}</td><td>${bars.h[tail + k]}</td>` +
          `<td>${bars.l[tail + k]}</td><td>${bars.c[tail + k]}</td><td>${bars.v[tail + k]}</td></tr>`
      )
      .join('');
    // `</` inside a script element would close it early, whatever the JSON says.
    const data = JSON.stringify({
      instrument: $state.snapshot(coords),
      bars: $state.snapshot(bars)
    }).replace(/<\//g, '<\\/');
    const doc = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(heading)}</title>
<style>
  :root { color-scheme: light dark; --bg:#fff; --fg:#111; --mut:#666; --line:#e3e3e3; }
  @media (prefers-color-scheme: dark) {
    :root { --bg:#14161a; --fg:#e8e8e8; --mut:#9aa0a6; --line:#2a2e35; }
  }
  body { margin:0; padding:24px; background:var(--bg); color:var(--fg);
         font:14px/1.5 system-ui, -apple-system, Segoe UI, Roboto, sans-serif; }
  main { max-width: 1100px; margin: 0 auto; }
  h1 { font-size:1.25rem; margin:0 0 2px; }
  p.note { color:var(--mut); margin:0 0 16px; font-size:.85rem; }
  img { width:100%; height:auto; border:1px solid var(--line); border-radius:6px; }
  dl { display:grid; grid-template-columns:max-content 1fr; gap:2px 16px; margin:16px 0; font-size:.85rem; }
  dt { color:var(--mut); }
  dd { margin:0; }
  details { margin-top:16px; }
  .wrap { overflow-x:auto; }
  table { border-collapse:collapse; font-size:.78rem; width:100%; }
  th, td { border-bottom:1px solid var(--line); padding:2px 8px; text-align:right; white-space:nowrap; }
  th:first-child, td:first-child { text-align:left; }
</style>
</head>
<body>
<main>
  <h1>${esc(heading)}</h1>
  <p class="note">${esc($t('histviz.export.note'))}</p>
  <img src="${png}" alt="${esc(heading)}">
  <dl>${facts.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join('')}</dl>
  <details>
    <summary>${esc($t('histviz.export.data'))}</summary>
    <div class="wrap"><table>
      <thead><tr><th>time</th><th>open</th><th>high</th><th>low</th><th>close</th><th>volume</th></tr></thead>
      <tbody>${body}</tbody>
    </table></div>
  </details>
  <script type="application/json" id="otw-chart">${data}<\/script>
</main>
</body>
</html>`;
    const url = URL.createObjectURL(new Blob([doc], { type: 'text/html' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = `${exportName()}.html`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  /** Instrument, timeframe and the minute it was taken: two exports of one chart never
   *  overwrite each other. */
  const exportName = () =>
    `${coords?.ticker ?? 'chart'}-${coords?.timeframe ?? ''}-` +
    new Date().toISOString().slice(0, 16).replace(/[:T]/g, '-');

  /** An instrument dropped from the rail (or another pane) lands here. */
  function onDrop(e) {
    const raw = e.dataTransfer?.getData('application/x-otw-instrument');
    if (!raw) return;
    e.preventDefault();
    try {
      openCoords(JSON.parse(raw));
      onactivate?.(pane.id);
    } catch {
      /* not ours */
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<section
  class="pane"
  class:active
  class:maximized
  onpointerdowncapture={() => onactivate?.(pane.id)}
  ondragover={(e) => e.preventDefault()}
  ondrop={onDrop}
>
  <!-- ── Pane bar ──
       Three clusters at a fixed height: the instrument, the tools, then the pane's own
       controls. The set of buttons never changes: one that does not apply is disabled, not
       removed, so nothing ever moves under the pointer while a chart loads or a study is
       added. Only the instrument cluster scrolls when a pane is too narrow to hold it; the
       pane's own buttons never do, because a control scrolled out of sight is a control the
       user cannot find. -->
  <div class="bar">
    <div class="bar-main">
      <!-- Instrument -->
      <!-- While a row cannot be charted the bar names *it*, not the instrument that happened
           to be there before: the bar and the card have to be talking about the same thing. -->
      <button class="sym-btn" title={$t('histviz.data.pick')} onclick={() => (dataOpen = true)}>
        <Icon name="search" size={12} />
        <span class="sym" class:dead={!!blocked}
          >{blocked?.ticker ?? coords?.ticker ?? $t('histviz.data.pickShort')}</span
        >
      </button>
      <div class="pick tf">
        <Dropdown
          value={coords?.timeframe ?? ''}
          options={tfOpts}
          disabled={timeframes.length < 2}
          placeholder={$t('histviz.page.timeframeNone')}
          ariaLabel={$t('histviz.page.timeframe')}
          title={$t('histviz.page.timeframe')}
          float
          onpick={(tf) => {
            switchTimeframe(tf);
            onlinked?.('timeframe', tf);
          }}
        />
      </div>
      <div class="pick style">
        <Dropdown
          bind:value={pane.type}
          options={typeOpts}
          ariaLabel={$t('histviz.page.plotStyle')}
          title={$t('histviz.page.plotStyle')}
          float
        />
      </div>
      {#if pane.type === 'renko'}
        <label class="brick">
          <span>{$t('histviz.page.brick')}</span>
          <input
            type="number"
            min="0"
            step="any"
            bind:value={pane.brick}
            placeholder={$t('histviz.modal.auto')}
          />
        </label>
      {/if}

      <span class="sep" aria-hidden="true"></span>

      <!-- Studies -->
      <button
        class="icon-btn"
        disabled={!coords}
        title={$t('histviz.page.addIndicator')}
        aria-label={$t('histviz.page.addIndicator')}
        onclick={openAdd}
      >
        <Icon name="trending-up" size={12} />
      </button>
      <button
        class="icon-btn"
        class:on={(pane.compare ?? []).length > 0}
        disabled={!coords}
        title={$t('histviz.compare.add')}
        aria-label={$t('histviz.compare.add')}
        onclick={() => (compareOpen = true)}
      >
        <Icon name="git-compare" size={12} />
      </button>
      <button
        class="icon-btn"
        disabled={!(pane.compare ?? []).length}
        title={$t(
          pane.compareMode === 'ratio' ? 'histviz.compare.modeRatio' : 'histviz.compare.modePct'
        )}
        onclick={() => {
          pane.compareMode = pane.compareMode === 'ratio' ? 'pct' : 'ratio';
          onchanged?.();
        }}
      >
        <span class="mode">{pane.compareMode === 'ratio' ? 'A/B' : '%'}</span>
      </button>
      <button
        class="icon-btn"
        disabled={!drawings.length}
        title={$t('histviz.objects.open')}
        aria-label={$t('histviz.objects.open')}
        onclick={() => (treeOpen = true)}
      >
        <Icon name="layers" size={12} />
        <span class="badge" class:show={drawings.length > 0}>{drawings.length}</span>
      </button>

      <span class="sep" aria-hidden="true"></span>

      <!-- Analysis -->
      <button
        class="icon-btn"
        disabled={!coords || !bars?.count || handoff}
        title={$t('histviz.page.backtestHint')}
        aria-label={$t('histviz.page.backtest')}
        onclick={toBacktest}
      >
        <Icon name="flask" size={12} />
      </button>
      <button
        class="icon-btn"
        class:on={armedAlerts > 0}
        disabled={!coords}
        title={alertFromLine
          ? $t('histviz.alerts.fromLine', { price: alertLevel })
          : $t('histviz.alerts.open')}
        aria-label={$t('histviz.alerts.open')}
        onclick={() => (alertsOpen = true)}
      >
        <Icon name="bell" size={12} />
        <span class="badge" class:show={armedAlerts > 0}>{armedAlerts}</span>
      </button>

      <span class="sep" aria-hidden="true"></span>

      <!-- Data: one slot, whether the window is already stored or still has to be saved. -->
      <span class="store-slot">
        {#if isStored}
          <span
            class="stored-tag"
            title={$t('histviz.page.storedHint', { bars: meta?.stored?.bar_count ?? 0 })}
          >
            <Icon name="database" size={11} />
          </span>
        {:else}
          <button
            class="icon-btn"
            onclick={() => saveWindow('loaded')}
            disabled={saving || !coords || !bars}
            title={$t('histviz.page.saveHint')}
            aria-label={$t('histviz.page.save')}
          >
            <Icon name="save" size={12} />
          </button>
        {/if}
      </span>
      <button
        class="icon-btn export"
        disabled={!bars?.count}
        title={$t('histviz.chart.exportPng')}
        aria-label={$t('histviz.chart.exportPng')}
        onclick={exportPng}
      >
        <Icon name="image" size={12} />
      </button>
      <button
        class="icon-btn export"
        disabled={!bars?.count}
        title={$t('histviz.chart.exportHtml')}
        aria-label={$t('histviz.chart.exportHtml')}
        onclick={exportHtml}
      >
        <Icon name="file-text" size={12} />
      </button>

      {#if canLive || liveOffTimeframe}
        <span class="sep" aria-hidden="true"></span>
      {/if}

      <!-- Live. The label and the lag both sit in fixed-width slots: a connection that comes
           up must not push every button beside it sideways. -->
      {#if canLive}
        <div class="live-group" class:on={pane.live}>
          <button
            class="live-btn"
            class:split={paneConnectors.length > 1}
            title={[
              pane.live ? $t('histviz.page.liveStop') : $t('histviz.page.liveStart'),
              liveGrainHint
            ]
              .filter(Boolean)
              .join('\n')}
            onclick={toggleLive}
          >
            <span
              class="dot"
              class:connected={pane.live && liveStatus?.state === 'live'}
              class:warn={pane.live && liveStatus?.state === 'retrying'}
            ></span>
            <span class="live-lbl">{pane.live ? $t('histviz.page.live') : $t('histviz.page.goLive')}</span>
          </button>
          {#if paneConnectors.length > 1}
            <div class="live-src">
              <Dropdown
                value={activeConnector?.id ?? ''}
                options={paneConnectors.map((c) => ({ value: c.id, label: c.name }))}
                ariaLabel={$t('histviz.page.liveSource')}
                title={$t('histviz.page.liveSourceHint', { name: activeConnector?.name ?? '' })}
                float
                onpick={pickConnector}
              />
            </div>
          {/if}
        </div>
        <span class="lag" title={$t('histviz.page.liveLag')}
          >{pane.live && liveLag != null && !liveStatus?.message ? `${liveLag} ms` : ''}</span
        >
      {:else if liveOffTimeframe}
        <span
          class="live-off"
          title={$t('histviz.page.liveTimeframeHint', { list: liveTimeframes.join(', ') })}
        >
          <span class="dot"></span>
        </span>
      {/if}
    </div>

    <!-- The pane's own controls. Outside the scroller, always reachable. -->
    <span class="sep" aria-hidden="true"></span>
    <button
      class="link-btn"
      class:on={!!pane.link}
      style={pane.link ? `--link:${LINK_HUES[pane.link]}` : ''}
      title={pane.link
        ? $t('histviz.workspace.linkOn', { group: pane.link })
        : $t('histviz.workspace.linkOff')}
      aria-label={$t('histviz.workspace.linkOff')}
      onclick={cycleLink}
    >
      <span class="link-dot"></span>
    </button>
    <button
      class="icon-btn"
      title={maximized ? $t('histviz.workspace.restorePane') : $t('histviz.workspace.maximizePane')}
      aria-label={maximized
        ? $t('histviz.workspace.restorePane')
        : $t('histviz.workspace.maximizePane')}
      onclick={() => onmaximize?.(pane.id)}
    >
      <Icon name={maximized ? 'chevrons-down-up' : 'maximize'} size={12} />
    </button>
    <button
      class="icon-btn close"
      disabled={!closable}
      title={$t('histviz.workspace.closePane')}
      aria-label={$t('histviz.workspace.closePane')}
      onclick={() => onclose?.(pane.id)}
    >
      <Icon name="x" size={12} />
    </button>
  </div>

  <div class="chart-wrap">
    <!-- ── Status strip ──
         Errors and provider notices float over the chart rather than sitting above it: a
         banner in the flow resizes the chart, and the window the user was reading moves.
         Bottom-left is the only corner the chart itself does not paint on. -->
    <div class="strip">
      {#if error && !issue}
        <div class="msg err"><ErrorText {error} copyable compact /></div>
      {/if}
      {#if notice}
        <p class="msg" class:hard={!bars?.count}>
          <Icon name="alert-triangle" size={11} />
          <span class="code">{$t(noticeKey(notice.code))}</span>
          <span class="txt">{notice.message}</span>
        </p>
      {/if}
      {#if liveIssue}
        <p class="msg" class:hard={liveIssue.state === 'stopped'}>
          <Icon name="alert-triangle" size={11} />
          <span class="code">{$t(liveFaultKey(liveIssue.code))}</span>
          <!-- The row is one line, so the provider's note rides in the same span and the
               title carries the whole thing for a reader who needs all of it. -->
          <span class="txt" title={liveIssueText}>{liveIssueText}</span>
          {#if liveIssue.state === 'stopped'}
            <button class="retry" onclick={retryLive}>{$t('histviz.live.retry')}</button>
          {/if}
          <button
            class="dismiss"
            title={$t('common.close')}
            aria-label={$t('common.close')}
            onclick={() => (liveStatus = null)}
          >
            <Icon name="x" size={10} />
          </button>
        </p>
      {/if}
      {#if lineWarn}
        <p class="msg">
          <Icon name="alert-triangle" size={11} />
          <span class="code">{$t('histviz.live.linesLabel')}</span>
          <span class="txt"
            >{$t('histviz.live.lines', {
              used: lineWarn.used,
              cap: lineWarn.cap,
              connector: lineWarn.connector
            })}</span
          >
          <button
            class="dismiss"
            title={$t('common.close')}
            aria-label={$t('common.close')}
            onclick={() => (lineUse = null)}
          >
            <Icon name="x" size={10} />
          </button>
        </p>
      {/if}
      {#if savedFlash}
        <p class="msg ok"><Icon name="check" size={11} /> <span class="txt">{savedFlash}</span></p>
      {/if}
    </div>

    <!-- The rail is always mounted, dimmed when there is nothing to draw on: mounting it
         with the data would move the chart sideways by its own width on every load. -->
    <div class="rail-slot" class:idle={!bars?.count || loading} inert={!bars?.count || loading}>
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
    </div>
    {#if loading && !bars?.count}
      <div class="sk-chart" aria-busy="true"><Skeleton height="100%" /></div>
    {:else if bars?.count}
      <!-- A reload keeps the chart on screen under a progress line. Swapping it for a
           skeleton would throw away the window being read and rebuild the whole plot. -->
      <div class="chart-slot" class:busy={loading} aria-busy={loading}>
        {#if loading}<span class="progress" aria-hidden="true"></span>{/if}
      <Chart
        bind:this={chartRef}
        {bars}
        type={pane.type}
        {instances}
        brick={pane.brick}
        {settings}
        {restoreRange}
        {loadingMore}
        {atHistoryStart}
        loadMoreLabel={$t('histviz.chart.loadMore', { bars: SLICE_BARS })}
        loadMoreNotice={notice?.message ?? ''}
        title={{
          ticker: coords?.ticker,
          timeframe: coords?.timeframe,
          provider: coords?.provider
        }}
        onsymbol={() => (dataOpen = true)}
        ontoggle={toggleInstance}
        onedit={(id) => openEdit(instances.find((i) => i.id === id))}
        onremove={removeInstance}
        onfullscreen={() => onmaximize?.(pane.id)}
        onrestored={() => (restoreRange = null)}
        onloadmore={loadMore}
        compare={compareSeries}
        oncomparetoggle={toggleCompare}
        oncompareremove={removeCompare}
        {onsettings}
        onhover={(ms) => onlinked?.('crosshair', ms)}
        onwindow={(r) => onlinked?.('range', { ...r, timeframe: coords?.timeframe })}
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
      </div>
    {:else if issue}
      <div class="issue" role="alert">
        <span class="glyph"><Icon name="plug" size={22} /></span>
        <h3>{$t('histviz.issue.title', { ticker: issue.ticker })}</h3>
        <p class="why">{issue.why}</p>
        {#if issue.detail}<p class="detail">{issue.detail}</p>{/if}
        <div class="acts">
          {#if issue.retry}
            <button class="primary" onclick={load}>
              <Icon name="refresh-cw" size={12} />
              {$t('histviz.issue.retry')}
            </button>
          {/if}
          <a class="ghost" href="/connectors">
            <Icon name="plug" size={12} />
            {$t('histviz.issue.openConnectors')}
          </a>
          <button class="ghost" onclick={() => (dataOpen = true)}>
            <Icon name="search" size={12} />
            {$t('histviz.issue.findManually')}
          </button>
        </div>
      </div>
    {:else}
      <div class="empty">
        <Icon name="candlestick" size={26} />
        <p class="hint">{$t('histviz.page.selectInstrument')}</p>
        <button class="pick-btn" onclick={() => (dataOpen = true)}>
          <Icon name="search" size={13} />
          {$t('histviz.data.pick')}
        </button>
      </div>
    {/if}
  </div>

</section>

<ObjectTree bind:open={treeOpen} bind:drawings bind:selected={selectedDrawing} />

<AlertsModal
  bind:open={alertsOpen}
  coords={coords ? $state.snapshot(coords) : null}
  suggested={alertLevel}
  connectorId={activeConnector?.id ?? coords?.connector_id ?? null}
  onchanged={refreshAlerts}
/>

<DataModal
  bind:open={compareOpen}
  {datasets}
  {recents}
  selected={null}
  busy={false}
  onselect={addCompare}
  ondownload={() => goto('/histdata')}
/>

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
  .pane {
    display: flex;
    flex-direction: column;
    container-type: inline-size;
    min-width: 0;
    min-height: 0;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  /* The active pane is what the rail, the keyboard and the link groups act on, so it says
     so with a hairline rather than a badge. The rule is drawn inside the bar so the border
     width never changes and no pane can shift by a pixel when focus moves. */
  .pane.active {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
  }
  .pane.active .bar {
    box-shadow: inset 0 var(--active-rule) 0 0 var(--accent);
  }

  /* ── Bar ── fixed height, never wraps. */
  .bar {
    display: flex;
    align-items: center;
    gap: 1px;
    flex: none;
    height: 30px;
    padding: 0 var(--space-1);
    border-bottom: var(--hairline) solid var(--border);
    flex-wrap: nowrap;
  }
  /* The instrument's own controls: they scroll, the pane's do not. */
  .bar-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 1px;
    flex-wrap: nowrap;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .bar-main::-webkit-scrollbar {
    display: none;
  }
  .sep {
    flex: none;
    width: var(--hairline);
    height: 16px;
    margin: 0 var(--space-1);
    background: var(--border);
  }

  /* The ticker is the pane's title: mono, so two panes stacked line up character for
     character, and medium weight, which is as loud as this design system goes. */
  .sym-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: none;
    height: 22px;
    padding: 0 var(--space-2);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    font-size: var(--fs-body);
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  .sym-btn:hover {
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .sym-btn :global(svg) {
    color: var(--dim);
    flex: none;
  }
  .sym-btn .sym {
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .icon-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 24px;
    height: 22px;
    padding: 0;
    background: none;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    transition: color var(--dur-fast) var(--ease), background-color var(--dur-fast) var(--ease);
  }
  .icon-btn:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .icon-btn.on {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
    background: var(--surface-2);
  }
  .icon-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
  .icon-btn.close:hover:not(:disabled) {
    color: var(--red);
  }

  /* Pickers carry the bar's height through the token, so they line up with the buttons
     instead of each control choosing its own. */
  .pick {
    --control-h: 22px;
    flex: none;
  }
  .pick.tf {
    width: 68px;
  }
  .pick.style {
    width: 100px;
  }
  .brick {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  .brick input {
    width: 58px;
    height: 22px;
    padding: 0 var(--space-2);
    font-size: var(--fs-body);
  }

  /* A count sits in the button's corner and keeps its box whether or not it shows: the
     bar must not twitch when an alert is armed or an object drawn. */
  /* Inside the button box, not hanging off it: the bar is a scroller and one overflowing
     pixel is one pixel of scrollable width. */
  .badge {
    position: absolute;
    top: 1px;
    right: 1px;
    min-width: 11px;
    padding: 0 1px;
    background: var(--accent);
    color: var(--accent-contrast);
    font-family: var(--mono);
    font-size: 8px;
    line-height: 10px;
    text-align: center;
    visibility: hidden;
  }
  .badge.show {
    visibility: visible;
  }
  .mode {
    font-family: var(--mono);
    font-size: 9px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
  }

  .link-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 24px;
    height: 22px;
    background: none;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
  }
  .link-btn:hover {
    background: var(--surface-2);
  }
  .link-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 1px solid var(--faint);
  }
  .link-btn.on .link-dot {
    background: var(--link);
    border-color: var(--link);
  }

  /* Stored or saveable, one slot: the icon changes, the width does not. */
  .store-slot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 24px;
    height: 22px;
  }
  .stored-tag {
    display: inline-flex;
    align-items: center;
    color: var(--green);
  }

  .live-group {
    display: inline-flex;
    align-items: center;
    flex: none;
    height: 22px;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .live-group.on {
    border-color: color-mix(in srgb, var(--green) 60%, var(--border));
  }
  .live-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 100%;
    padding: 0 var(--space-2);
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--fs-desc);
    white-space: nowrap;
  }
  .live-btn:hover {
    background: var(--surface-2);
  }
  .live-group.on .live-btn {
    color: var(--green);
  }
  .live-btn.split {
    border-right: var(--hairline) solid var(--border-control);
  }
  /* "Go live" and "Live" are different lengths; the slot is not. */
  .live-lbl {
    display: inline-block;
    min-width: 40px;
    text-align: left;
  }
  .live-src {
    --control-h: 22px;
    width: 84px;
  }
  .dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--faint);
  }
  .dot.connected {
    background: var(--green);
  }
  .dot.warn {
    background: var(--amber);
  }
  /* Reserved: the lag reads out without pushing the bar around when it appears. */
  .lag {
    flex: none;
    width: 46px;
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    color: var(--dim);
    text-align: right;
    white-space: nowrap;
  }
  .live-off {
    display: inline-flex;
    align-items: center;
    flex: none;
    padding: 0 var(--space-1);
    opacity: 0.6;
  }

  /* ── Chart area ── */
  .chart-wrap {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .rail-slot {
    display: flex;
    flex: none;
    transition: opacity var(--dur-base) var(--ease);
  }
  .rail-slot.idle {
    opacity: 0.35;
    pointer-events: none;
  }
  .chart-slot {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .chart-slot > :global(.chart-host) {
    flex: 1;
  }
  .chart-slot.busy {
    opacity: 0.55;
  }
  /* An indeterminate hairline, not a spinner: it says "still fetching" without covering a
     single candle. */
  .progress {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 6;
    height: 2px;
    overflow: hidden;
    background: transparent;
  }
  .progress::after {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    width: 30%;
    height: 100%;
    background: var(--accent);
    animation: slide 1.1s ease-in-out infinite;
  }
  @keyframes slide {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(400%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .progress::after {
      animation: none;
      width: 100%;
      opacity: 0.5;
    }
  }
  .sk-chart {
    flex: 1;
    min-height: 0;
  }

  /* ── Status strip ── floats over the chart, bottom left, never in the flow. */
  .strip {
    position: absolute;
    left: 40px;
    bottom: 26px;
    z-index: 7;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    max-width: calc(100% - 80px);
    pointer-events: none;
  }
  .strip > :global(*) {
    pointer-events: auto;
  }
  .msg {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    max-width: 100%;
    margin: 0;
    padding: 2px var(--space-2);
    background: color-mix(in srgb, var(--surface) 92%, transparent);
    border: var(--hairline) solid var(--border);
    backdrop-filter: blur(2px);
    font-size: var(--fs-desc);
    color: var(--muted);
  }
  .msg.hard {
    color: var(--amber-ink);
    border-color: color-mix(in srgb, var(--amber) 45%, var(--border));
  }
  .msg.err {
    border-color: color-mix(in srgb, var(--red) 45%, var(--border));
  }
  .msg.ok {
    color: var(--green-ink);
    border-color: color-mix(in srgb, var(--green) 45%, var(--border));
  }
  .msg .code {
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }
  .msg .txt {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .msg .retry,
  .msg .dismiss {
    flex: none;
    background: none;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: inherit;
    cursor: pointer;
    font-size: var(--fs-desc);
    padding: 0 5px;
  }
  .msg .retry:hover,
  .msg .dismiss:hover {
    background: var(--surface-2);
  }
  .msg .dismiss {
    display: inline-flex;
    align-items: center;
    border-color: transparent;
  }

  .sym.dead {
    color: var(--amber);
    text-decoration: line-through;
  }

  .empty {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    color: var(--faint);
  }

  /* ── Nothing to draw, and a reason ──
     Centred like the empty state, but it is an answer rather than an invitation: what failed,
     why, and the one place that fixes it. Amber, not red: nothing is broken, something is
     not connected. */
  .issue {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-4);
    text-align: center;
  }
  .issue .glyph {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    margin-bottom: var(--space-1);
    border: var(--hairline) solid color-mix(in srgb, var(--amber) 45%, var(--border));
    background: color-mix(in srgb, var(--amber) 8%, transparent);
    color: var(--amber);
  }
  .issue h3 {
    margin: 0;
    font-family: var(--mono);
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
  }
  .issue .why {
    margin: 0;
    max-width: 44ch;
    font-size: var(--fs-body);
    color: var(--dim);
    line-height: 1.45;
  }
  /* The server's own words, kept verbatim and set apart: it is the line to paste in a ticket. */
  .issue .detail {
    margin: 0;
    max-width: 52ch;
    padding: var(--space-1) var(--space-2);
    background: var(--surface-2);
    border-left: var(--active-rule) solid color-mix(in srgb, var(--amber) 55%, var(--border));
    font-family: var(--mono);
    font-size: var(--fs-desc);
    color: var(--dim);
    text-align: left;
    overflow-wrap: anywhere;
  }
  .issue .acts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .issue .acts button,
  .issue .acts a {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: var(--control-h);
    padding: 0 var(--space-3);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-body);
    letter-spacing: 0.02em;
    text-decoration: none;
    cursor: pointer;
  }
  .issue .acts .primary {
    border-color: var(--accent);
    color: var(--accent);
  }
  .issue .acts button:hover,
  .issue .acts a:hover {
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .empty .hint {
    margin: 0;
    font-size: var(--fs-body);
    color: var(--dim);
  }
  .pick-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: var(--control-h);
    padding: 0 var(--space-4);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--fs-body);
    letter-spacing: 0.02em;
  }
  .pick-btn:hover {
    background: var(--surface-2);
    border-color: var(--accent);
  }

  /* A pane in a 3x4 grid is barely wider than its own toolbar. The bar scrolls, but a
     control scrolled out of sight is a control that is gone, so the ones with another way in
     leave first: the chart type is in the layout the pane reopens with, and an export is one
     click away once the pane is maximized. The symbol, the timeframe, live and the pane's own
     buttons never leave. */
  @container (max-width: 460px) {
    .pick.style {
      width: 78px;
    }
    .lag {
      display: none;
    }
  }
  @container (max-width: 380px) {
    .pick.style {
      display: none;
    }
  }
  @container (max-width: 320px) {
    .icon-btn.export {
      display: none;
    }
  }
</style>
