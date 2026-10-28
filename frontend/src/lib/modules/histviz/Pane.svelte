<script>
  // One chart pane: an instrument, its window of bars, its studies, its drawings and its
  // quick-backtest session. A workspace holds up to twelve of these, so everything here is
  // instance state: nothing reaches for the page.
  //
  // What the pane does NOT own is the live connection. Panes share one multiplexed SSE
  // stream (a socket per connector on the server, one HTTP connection in the browser), so the
  // page opens it and hands each pane its own bars through `applyBar`. The pane still owns
  // the *decision*: `pane.live` is its toggle and its status banner is its own.
  import { tick, untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { t } from '$lib/i18n';
  import { tz, makeTsFmt } from '$lib/tz.svelte.js';
  import { histvizApi } from './api.js';
  import { isTemp } from './tempSeries.js';
  import { indicatorLib, onIndicatorRemoved } from '$lib/indicators/api.js';
  import { instrumentMeta, scriptServer } from '$lib/script/api.js';
  import { createScriptRuns } from '$lib/script/runs.svelte.js';
  import { fetchWarm } from '$lib/script/warm.js';
  import ScriptWarnings from '$lib/script/ScriptWarnings.svelte';
  import InstrumentMetaModal from '$lib/script/InstrumentMetaModal.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import {
    suggestBrick,
    instanceLabel,
    isCustom,
    isScript,
    refreshScriptInstances,
    withoutIndicator,
    CHART_TYPE_GROUPS,
    SIZED_TYPES,
    sourceInstance
  } from './indicators.js';
  import Chart from './Chart.svelte';
  import { mergeLiveBar, fillBars } from './bars.js';
  import DataModal from './DataModal.svelte';
  import IndicatorModal from './IndicatorModal.svelte';
  import DrawToolbar from './DrawToolbar.svelte';
  import AlertsModal from './AlertsModal.svelte';
  import ObjectTree from './ObjectTree.svelte';
  import CustomTimeframeModal from './CustomTimeframeModal.svelte';
  import ChartSettings from './ChartSettings.svelte';
  import ChartSettingsModal from './ChartSettingsModal.svelte';
  import { STYLE_KEYS } from './settings.js';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { portal } from '$lib/ui/portal.js';
  import TemplateMenu from './TemplateMenu.svelte';
  import { applyTemplate } from './templates.js';
  import { replay, makeFill, flipSide, resolveQty, lotFor, DEFAULT_QTY } from './quicktest.js';
  import { bookFor, bookLines, MODULE } from './book.js';
  import { brokersApi } from '$lib/brokers/api.js';
  import QuickOrder from './QuickOrder.svelte';
  import CompareStyleModal from './CompareStyleModal.svelte';
  import { RANGES, pickTimeframe, rangeStart, isoSecond } from './ranges.js';
  import { buildOrders, orderLines, tifsFor, sizeFrom, tradeUnitsFor, SHARES } from './orders.js';
  import { priceDigitsOf, priceTickOf, makePriceFmt } from './format.js';
  import {
    SLICE_BARS,
    tfSeconds,
    consolidationBase,
    byDuration,
    loadQuick,
    saveQuick,
    loadTradeSize,
    saveTradeSize,
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
    /** Shared bars the page lays out once for every chart, TradingView style: the active
     *  pane renders its controls in `barDock` and its drawing rail in `railDock`. Without
     *  docks, both stay inside the pane. */
    barDock = null,
    railDock = null,
    /** Slots at the right of the page bar: the backtest hand-off (next to the quick
     *  backtest) and the chart's own controls (after the book and the ticket). */
    actDock = null,
    ctlDock = null,
    /** The page's bottom bar, where the active chart puts its range buttons. */
    rangeDock = null,
    maximized = false,
    closable = true,
    /** The workspace's quick-backtest mode. Global, not per pane: one session takes fills
     *  from every chart on screen and the bar at the bottom of the page adds them up. */
    quick = false,
    /** The workspace's synced broker book, or null while nothing is synced. One account for
     *  every chart on screen; each pane keeps only the levels that belong to its own
     *  instrument. */
    book = null,
    /** Chart trading. `orders` are the workspace's draft orders (each pane draws its own
     *  instrument's); `tradeAccount` is the broker account they would go to, null while none
     *  is connected, which turns the `+` and the quick buttons off. */
    orders = [],
    tradeAccount = null,
    /** The size step `tradeAccount` takes on this instrument (Binance's `LOT_SIZE`), or null. */
    tradeStep = null,
    /** Why `tradeAccount` is null, as the page words it (see book.js `tradeLock`). */
    tradeLock = '',
    /** (orders, error): drafts made on this chart, or the reason none could be. */
    ontrade,
    onordermove,
    onordercancel,
    /** (localOrderId) / (brokerOrderId): an order double-clicked on this chart, to change it. */
    onorderedit,
    onbookorderedit,
    /** (position): its ✕ on this chart, to close it at the market. */
    onpositionclose,
    /** ({ side, type, price }): open the ticket at a price picked on this chart. */
    onticket,
    /** (paneId, { last, digits, bid, ask, tick }): what the ticket needs to price this chart. */
    onmarket,
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
    onsettings,
    /** Copy this chart's style to every chart of the workspace (and make it the default). */
    onapplysettings,
    /** The workspace's drawings, symbol -> list, shared by every chart showing the symbol. */
    boards = $bindable({}),
    /** Saved chart templates (global); saving and deleting go through the page. */
    templates = [],
    onsavetemplate,
    ondeletetemplate,
    /** Open the "apply to charts" picker for a template. */
    onapplyto
  } = $props();

  // ── This chart's settings ──
  // Each chart owns its style (`pane.settings`, saved with the workspace); a chart that was
  // never set follows the global settings, which are also the default a new chart starts
  // from. Shared lists (custom timeframes, formulas, drawing templates) and the data
  // behaviour stay global whatever the chart.
  const chartSettings = $derived.by(() => {
    const own = pane.settings;
    if (!own) return settings;
    const out = { ...settings };
    for (const k of STYLE_KEYS) if (own[k] !== undefined) out[k] = own[k];
    return out;
  });
  const styleOf = (src) => Object.fromEntries(STYLE_KEYS.map((k) => [k, src[k]]));
  /** Give the chart its own copy before the first edit, so it stops following the default. */
  function ownSettings() {
    if (!pane.settings) pane.settings = styleOf($state.snapshot(chartSettings));
  }
  /** The chart's own writes (the L switch, a drawing template): style to this chart, the
   *  rest to the global settings. */
  function chartPatch(patch) {
    const mine = {};
    const shared = {};
    for (const [k, v] of Object.entries(patch)) (STYLE_KEYS.includes(k) ? mine : shared)[k] = v;
    if (Object.keys(mine).length) {
      ownSettings();
      Object.assign(pane.settings, mine);
    }
    if (Object.keys(shared).length) onsettings?.(shared);
  }
  // A style edit is a workspace edit: save it like any other pane change (not on mount).
  let styleSeen = false;
  $effect(() => {
    JSON.stringify(pane.settings ?? null);
    if (!styleSeen) {
      styleSeen = true;
      return;
    }
    untrack(() => onchanged?.());
  });
  let cogOpen = $state(false);
  /** Where the chart's scales sit (from the chart): the quick order bar docks past them. */
  let plotInset = $state(null);
  let cogAt = $state(null);
  let cogBtn = $state(null);
  let settingsModalOpen = $state(false);
  // Opening the menu gives the chart its own copy of the style; closing it untouched hands
  // the chart back to the global settings, so a look at the menu is not an unsaved change.
  let cogBorrowed = false;
  $effect(() => {
    if (cogOpen) return;
    untrack(() => {
      if (!cogBorrowed) return;
      cogBorrowed = false;
      if (JSON.stringify(pane.settings) === JSON.stringify(styleOf($state.snapshot(settings)))) pane.settings = null;
    });
  });
  function toggleCog() {
    if (cogOpen) {
      cogOpen = false;
      return;
    }
    cogBorrowed = !pane.settings;
    ownSettings();
    const r = cogBtn?.getBoundingClientRect();
    cogAt = r ? { bottom: window.innerHeight - r.top + 6, right: Math.max(8, window.innerWidth - r.right) } : null;
    cogOpen = true;
  }

  // ── Series state (this pane's own) ──
  // Raw, not a deep proxy: a series is thousands of numbers read on every render, and a live
  // tick edits the forming bar in place. A new series (a load, older history, a new bar) is
  // a new object; a tick that only moves a bar bumps `barsTick` instead, which the readers of
  // the forming bar (the chart, the scripts, the last price) follow.
  let bars = $state.raw(null);
  let barsTick = $state(0);
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
  /** The level the price scale's `+` asked for ({ price, last }); null = the toolbar bell. */
  let alertAt = $state(null);
  $effect(() => {
    if (!alertsOpen) alertAt = null;
  });
  let alerts = $state([]);
  // Comparison instruments: their raw series, keyed by instrument+timeframe (see `cmpKey`)
  // so the data outlives the row that asked for it.
  let compareBars = $state({});
  let compareOpen = $state(false);
  /** The comparison whose line style is being edited, by id. */
  let compareEditId = $state(null);
  let treeOpen = $state(false);

  const coords = $derived(pane.coords);
  const coordKey = $derived(coordKeyOf(coords));
  const instrumentKey = $derived(instrumentKeyOf(coords));
  const tfSecs = $derived(tfSeconds(coords?.timeframe) ?? 3600);
  const isStored = $derived(!!meta?.stored);

  /** This pane's share of the synced book: the levels whose instrument is the one on screen.
   *  A trailing stop is drawn where it trails now, from the bars as they move. */
  const trailSeen = new Map();
  const bookLevels = $derived.by(() => {
    void barsTick;
    const slice = bookFor(book, coords?.ticker);
    const trails = slice.orders.some((o) => Number(o.trail_rate) > 0);
    const series = trails && bars?.ts?.length ? { ms: bars.ts.map((s) => Date.parse(s)), h: bars.h, l: bars.l, c: bars.c } : null;
    return bookLines(slice, series, trailSeen);
  });

  // ── Live (state here, connection on the page) ──
  let liveStatus = $state(null); // {state, code, message, grain} from the server's `status` event
  let liveLag = $state(null);
  // Interactive Brokers only: market-data lines this account is holding, and the ceiling a
  // plain account has. IB refuses silently once it is reached, so the count is shown before
  // the pane that crosses it comes back empty.
  let lineUse = $state(null); // {connector, used, cap}
  // Best bid and ask, when this chart asked for them (its bid/ask setting) and the venue
  // streams them. Hidden once nothing has come for a minute: a frozen quote drawn as current
  // is worse than none.
  let liveQuote = $state(null); // {bid, ask}
  let quoteStatus = $state(null); // {state, code, message, source} from `quote_status`
  let quoteStale = null;
  const QUOTE_STALE_MS = 60_000;
  let tickRaf = 0; // the frame that will publish the ticks received since the last one
  // ── Seams ──
  // A live chart is drawn by index, so a stretch the feed never delivered (a sleeping tab, a
  // dropped gateway, a server restart) does not look missing: the bars either side of it sit
  // side by side as if one followed the other. Every such seam is read back from the series
  // endpoint (store first, then the provider), and one that cannot be read is marked on the
  // chart and named in the strip, never left to pass for continuous data.
  let filling = $state(false);
  /** Stretches the chart could not fill: {before, after, code, message}. `before` and `after`
   *  are the bars either side (ISO); `after` is null for the stretch up to now. */
  let holes = $state([]);
  let owed = null; // [fromMs, toMs], exclusive both ends: what the next fill asks for
  let seamTimer = null;
  let tailChecked = null; // {lastMs, at}: the tail last read back with nothing missing
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
      since: bars?.ts?.length ? bars.ts[bars.ts.length - 1] : null,
      quotes: !!chartSettings.bidAsk
    };
  }

  function clearQuote() {
    clearTimeout(quoteStale);
    quoteStale = null;
    liveQuote = null;
  }
  function onLiveQuote(d) {
    if (!Number.isFinite(d?.bid) || !Number.isFinite(d?.ask)) return;
    liveQuote = { bid: d.bid, ask: d.ask };
    clearTimeout(quoteStale);
    quoteStale = setTimeout(clearQuote, QUOTE_STALE_MS);
  }
  function onQuoteStatus(s) {
    quoteStatus = s;
    if (s?.state !== 'live') clearQuote();
  }
  // Another instrument, account or feed: the quote on screen belongs to the old one.
  $effect(() => {
    void coordKey;
    void pane.live;
    void pane.connector_id;
    untrack(() => {
      clearQuote();
      quoteStatus = null;
    });
  });
  $effect(() => () => clearTimeout(quoteStale));
  // Turning bid/ask on or off changes what this pane asks of the shared stream.
  let bidAskSeen;
  $effect(() => {
    const on = !!chartSettings.bidAsk;
    untrack(() => {
      if (bidAskSeen !== undefined && on !== bidAskSeen) {
        clearQuote();
        quoteStatus = null;
        if (pane.live) onlinked?.('restream');
      }
      bidAskSeen = on;
    });
  });
  /** Where the bid/ask lines come from, or why there are none, for the live tooltip. */
  const quoteHint = $derived.by(() => {
    if (!pane.live || !chartSettings.bidAsk || !quoteStatus) return '';
    if (quoteStatus.state === 'none') return $t('histviz.page.bidAskNone', { provider: coords?.provider ?? '' });
    if (quoteStatus.message) return $t('histviz.page.bidAskFault', { message: quoteStatus.message });
    if (quoteStatus.source === 'candle_sides') return $t('histviz.page.bidAskCandles');
    // Alpaca's free stock feed is IEX: its book, not the national best bid and offer.
    if (coords?.provider === 'alpaca' && ['equity', 'etf'].includes(coords?.asset_type)) return $t('histviz.page.bidAskIex');
    return $t('histviz.page.bidAskNative');
  });

  // Register with the page so the shared stream can reach this pane, and so the rail and the
  // link groups can drive it.
  $effect(() => {
    onregister?.(pane.id, {
      applyBar: onLiveBar,
      applyStatus: onLiveStatus,
      applyLines: (d) => (lineUse = d),
      applyQuote: onLiveQuote,
      applyQuoteStatus: onQuoteStatus,
      streamRequest,
      openCoords,
      switchTimeframe,
      apply,
      load,
      needsLoad,
      showBlocked,
      setLoading,
      useTemplate,
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
        width: c.width ?? null,
        lineType: c.lineType ?? null,
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

  function patchCompare(id, patch) {
    pane.compare = (pane.compare ?? []).map((c) => (c.id === id ? { ...c, ...patch } : c));
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
    void barsTick;
    return bars?.c?.length ? bars.c[bars.c.length - 1] : null;
  });
  const alertFromLine = $derived(
    !!drawings.find((d) => d.id === selectedDrawing && d.tool === 'hline')
  );

  // ── Indicators ──
  let instances = $state(pane.instances ?? []);
  let nextId = (pane.instances ?? []).reduce((m, i) => Math.max(m, i.id ?? 0), 0) + 1;

  // Custom instances carry a copy of their definition so the chart still draws offline, but
  // the library is the source of truth: on load, refresh every instance from its row, and
  // drop the ones whose row was deleted.
  let libSynced = false;
  $effect(() => {
    if (libSynced) return;
    libSynced = true;
    const restoredList = untrack(() => instances);
    if (!restoredList.some((i) => (i.type === 'custom' && i.custom?.id) || (isScript(i) && i.script?.id))) return;
    indicatorLib
      .list()
      .then((rows) => {
        const dagRows = rows.filter((r) => r.kind !== 'script');
        instances = refreshScriptInstances(
          untrack(() => instances).flatMap((i) => {
            if (i.type !== 'custom' || !i.custom?.id) return [i];
            const row = dagRows.find((r) => r.id === i.custom.id);
            if (!row) return [];
            return [{ ...i, custom: { id: row.id, name: row.name, def: row.definition } }];
          }),
          rows.filter((r) => r.kind === 'script')
        );
      })
      .catch(() => {
        /* offline or library unreachable: keep the embedded definitions */
      });
  });

  // ── Script indicators: instrument facts + worker runs ──
  // `syminfo.*` facts of the instrument on screen (null = none stored; crypto gets its
  // rule-based ones in `syminfoFor`). Reloaded on instrument change and after an edit.
  let instrumentFacts = $state(null);
  let factsTick = $state(0);
  $effect(() => {
    const c = coords;
    void factsTick;
    instrumentFacts = null;
    // The chart snaps prices to the tick size and the ticket measures in ticks, so every
    // charted instrument loads them, script or not.
    if (!c?.ticker) return;
    instrumentMeta
      .get(c)
      .then((m) => {
        if (coords === c) instrumentFacts = m;
      })
      .catch(() => {
        /* unreachable: unknown facts, a script reading one is refused with the field named */
      });
  });
  // The legend's issue button opens what a script run reported; instrument facts are edited
  // from there (an unknown `syminfo` field) or from the cog menu, and every script reruns.
  let scriptInfoId = $state(null);
  let instrumentOpen = $state(false);
  const scriptInfo = $derived(scriptInfoId == null ? null : (scriptRuns.results.get(scriptInfoId) ?? {}));
  const factsSaved = () => factsTick++;
  const scriptRuns = createScriptRuns({
    instances: () => instances,
    bars: () => bars,
    tick: () => barsTick,
    type: () => pane.type,
    brick: () => pane.brick,
    coords: () => coords,
    meta: () => instrumentFacts,
    tfSecs: () => tfSecs,
    fetchWarm,
    serverRun: scriptServer.run
  });



  // ── Drawings (per symbol, in the workspace) ──
  // `boards` is the workspace's map symbol -> drawings: every chart showing the symbol reads
  // and writes the same list, compared as JSON so an echo from another chart settles.
  let tool = $state('cursor');
  let drawings = $state([]);
  let selectedDrawing = $state(null);
  let magnet = $state(false);
  let drawingsHidden = $state(false);

  let boardKey = null;
  let boardJson = '[]';
  $effect(() => {
    const key = instrumentKey;
    const json = JSON.stringify(boards?.[key] ?? []);
    untrack(() => {
      if (key === boardKey && json === boardJson) return;
      if (key !== boardKey) selectedDrawing = null;
      boardKey = key;
      boardJson = json;
      drawings = JSON.parse(json);
    });
  });
  $effect(() => {
    const json = JSON.stringify(drawings);
    untrack(() => {
      if (!boardKey || json === boardJson) return;
      boardJson = json;
      const next = { ...boards };
      if (drawings.length) next[boardKey] = JSON.parse(json);
      else delete next[boardKey];
      boards = next;
    });
  });

  // ── Quick backtest (per instrument, this browser) ──
  let quickFills = $state([]);
  let quickSize = $state(DEFAULT_QTY);
  let quickUnit = $state('units');
  let quickContract = $state(1);

  const quickBars = $derived.by(() => {
    void barsTick;
    if (!bars?.ts?.length) return null;
    return { ms: bars.ts.map((s) => Date.parse(s)), h: bars.h, l: bars.l };
  });
  const session = $derived(replay(quickFills, quickBars));

  // ── Broker fills (the `fills` setting) ──
  // The account's own fills on this instrument, drawn with the quick backtest's markers at
  // their price and instant: replayed in order, so an entry is solid and an exit hollow. A
  // position opened before the window reads its first exit as an entry.
  let brokerFills = $state([]);
  const fillsFrom = $derived(chartSettings.fills && pane.broker && coords?.ticker ? (bars?.ts?.[0] ?? null) : null);
  // A fill moves a position: read again when the book's positions change.
  const bookSig = $derived(JSON.stringify((book?.positions ?? []).map((p) => [p.symbol, p.side, p.qty])));
  $effect(() => {
    const from = fillsFrom;
    void bookSig;
    if (!from) {
      brokerFills = [];
      return;
    }
    const account = pane.broker;
    const { ticker, asset_type } = coords;
    // To the next whole minute, so a second read within it is the server's cached pull.
    const to = new Date(Math.ceil(Date.now() / 60_000) * 60_000).toISOString();
    let gone = false;
    const timer = setTimeout(async () => {
      try {
        const rows = await brokersApi.fills(account, ticker, asset_type, from, to, MODULE);
        if (!gone) brokerFills = rows;
      } catch {
        if (!gone) brokerFills = [];
      }
    }, 300);
    return () => {
      gone = true;
      clearTimeout(timer);
    };
  });
  const fillMarks = $derived(
    replay(
      brokerFills.map((f, i) => ({
        id: `fill:${f.id || i}`,
        side: f.side === 'sell' ? 'short' : 'long',
        x: Date.parse(f.at),
        y: Number(f.price),
        qty: Number(f.qty) || 0,
        lot: 1
      })).sort((a, b) => a.x - b.x)
    ).marks
  );
  const chartMarks = $derived(fillMarks.length ? [...session.marks, ...fillMarks] : session.marks);
  const lastClose = $derived.by(() => {
    void barsTick;
    return bars?.c?.length ? bars.c[bars.c.length - 1] : null;
  });
  /** When the chart last saw its price, epoch ms: the last live update's arrival (kept to the
   *  nearest few seconds), else the last bar's start. A price collar refuses a price too old. */
  const PRICE_AT_STEP_MS = 5000;
  let priceAt = $state(null); // { key: instrumentKey, at }
  const lastAt = $derived(
    priceAt?.key === instrumentKey ? priceAt.at : bars?.ts?.length ? Date.parse(bars.ts[bars.ts.length - 1]) : null
  );

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

  // ── Chart trading ──
  // The `+` and the quick bar need a broker to send to, an instrument and a market to price
  // against; the bar itself is this pane's to show (`pane.trade`, saved with the layout).
  const tradable = $derived(!!tradeAccount && !!coords?.ticker && lastClose != null);
  const myOrderLines = $derived(orderLines(orders, instrumentKey));
  const priceDigits = $derived(priceDigitsOf(bars));
  /** The increment prices snap to: the stored `mintick` when the user gave one, else the
   *  one the loaded bars trade on. */
  const priceTick = $derived(Number(instrumentFacts?.mintick) > 0 ? Number(instrumentFacts.mintick) : priceTickOf(bars));
  const fmtPrice = $derived(makePriceFmt(priceDigits));
  // The quick bar's size is typed in the instrument's own unit or, where it allows one, as an
  // amount of money turned into a size at the price of the order (see `tradeUnitsFor`). The
  // pair is remembered per instrument, in this browser.
  const tradeUnits = $derived(
    tradeUnitsFor({
      kind: instrumentFacts?.kind,
      assetType: coords?.asset_type,
      broker: tradeAccount?.broker,
      ticker: coords?.ticker,
      base: instrumentFacts?.basecurrency,
      quote: instrumentFacts?.currency
    })
  );
  let tradeUnitPick = $state('units');
  const tradeUnit = $derived(tradeUnits.some((u) => u.value === tradeUnitPick) ? tradeUnitPick : 'units');
  const tradePointValue = $derived(Number(instrumentFacts?.pointvalue) > 0 ? Number(instrumentFacts.pointvalue) : 1);
  /** Contracts and shares trade whole; the rest in fractions. */
  const tradeWhole = $derived(
    [...SHARES, 'future', 'option'].includes(String(instrumentFacts?.kind ?? coords?.asset_type ?? '').toLowerCase())
  );
  /** The size an amount of money buys at `price`, down to what the broker takes; null below one step. */
  const sizeAt = (amount, price) => sizeFrom('notional', amount, { entry: price, pointValue: tradePointValue, whole: tradeWhole, step: tradeStep });
  const tradeQty = $derived.by(() => {
    const q = Number(pane.trade?.qty) > 0 ? Number(pane.trade.qty) : 1;
    return tradeUnit === 'notional' ? sizeAt(q, lastClose) : q;
  });

  let tradeSizeKey = null;
  $effect(() => {
    const key = instrumentKey;
    if (key === tradeSizeKey) return;
    tradeSizeKey = key;
    const saved = loadTradeSize(key);
    if (!saved) return;
    tradeUnitPick = saved.unit;
    untrack(() => {
      if (pane.trade) pane.trade.qty = saved.qty;
    });
  });
  $effect(() => {
    const key = untrack(() => tradeSizeKey);
    const unit = tradeUnitPick;
    const qty = pane.trade?.qty;
    if (key && pane.trade?.on) saveTradeSize(key, { unit, qty });
  });
  // A unit the instrument does not take (an amount carried to a listed future) is no size in
  // the one it falls back to: start again from one.
  $effect(() => {
    if (tradeUnitPick === tradeUnit) return;
    tradeUnitPick = tradeUnit;
    untrack(() => {
      if (pane.trade) pane.trade.qty = 1;
    });
  });

  /** Another unit picked on the bar: the size carries over, converted at the market. */
  function setTradeUnit(next) {
    const px = liveQuote?.ask != null && liveQuote?.bid != null ? (liveQuote.ask + liveQuote.bid) / 2 : lastClose;
    const q = Number(pane.trade?.qty);
    let n = null;
    if (q > 0 && px > 0) n = next === 'notional' ? Math.round(q * px * tradePointValue * 100) / 100 : sizeAt(q, px);
    tradeUnitPick = next;
    if (pane.trade) pane.trade = { ...pane.trade, qty: n > 0 ? n : 1 };
  }

  /** The quick bar's buttons: a market order, its size worked out at the side it crosses. */
  function quickOrder(side) {
    const q = pane.trade.qty;
    if (tradeUnit !== 'notional') return draft({ side, type: 'market', qty: q });
    const px = side === 'buy' ? (liveQuote?.ask ?? lastClose) : (liveQuote?.bid ?? lastClose);
    const qty = sizeAt(q, px);
    if (!qty) return ontrade?.(null, 'amountSmall');
    draft({ side, type: 'market', qty });
  }
  const tradeOff = $derived(
    !tradeAccount ? tradeLock || $t('histviz.trade.lock.noAccount') : lastClose == null ? $t('histviz.trade.noMarket') : ''
  );

  $effect(() => {
    onmarket?.(pane.id, {
      last: lastClose,
      lastAt,
      digits: priceDigits,
      bid: liveQuote?.bid ?? null,
      ask: liveQuote?.ask ?? null,
      tick: priceTick,
      pointValue: Number(instrumentFacts?.pointvalue) > 0 ? Number(instrumentFacts.pointvalue) : null,
      kind: instrumentFacts?.kind ?? pane.coords?.asset_type ?? null,
      // order id -> where each trailing stop on this chart trails now, for the Orders tab.
      trails: Object.fromEntries(bookLevels.filter((l) => l.kind === 'order' && l.level === 'stop' && l.trailing).map((l) => [l.id, l.price]))
    });
  });

  function toggleTradeBar() {
    pane.trade = pane.trade?.on ? { ...pane.trade, on: false } : { edge: 'top', at: 0, qty: 1, ...pane.trade, on: true };
  }

  function draft(req) {
    const res = buildOrders(req, { instrument: instrumentKey, symbol: coords?.ticker, account: tradeAccount?.id, connector: activeConnector?.id ?? null, tifs: tifsFor(tradeAccount) });
    ontrade?.(res.orders ?? null, res.error ?? null);
  }

  /** The `+` menu: a resting order at the price the crosshair was on. */
  function tradeAt({ side, type, price }) {
    if (!tradeQty) return ontrade?.(null, 'amountSmall');
    draft({ side, type, qty: tradeQty, limit: type === 'limit' ? price : null, stop: type === 'stop' ? price : null });
  }

  /** The bottom bar edits the sizing of the chart it is pointing at. */
  export function setQuickSizing(p) {
    if (p.size != null) quickSize = p.size;
    if (p.mode != null) quickUnit = p.mode;
    if (p.contract != null) quickContract = p.contract;
  }

  // ── Chart templates ──
  let tplOpen = $state(false);
  let tplAt = $state(null);
  let tplBtn = $state(null);
  function toggleTemplates() {
    if (tplOpen) {
      tplOpen = false;
      return;
    }
    const r = tplBtn?.getBoundingClientRect();
    tplAt = r ? { top: r.bottom + 6, right: Math.max(8, window.innerWidth - r.right) } : null;
    tplOpen = true;
  }
  /** Give this chart a template's studies and layout (see templates.js). */
  function useTemplate(tpl) {
    applyTemplate(pane, tpl);
    instances = pane.instances;
    nextId = instances.reduce((m, i) => Math.max(m, i.id ?? 0), 0) + 1;
    onchanged?.();
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
    // A custom timeframe stays as long as the new provider serves something that divides it.
    const usable = (tf) => !!tf && (list.includes(tf) || !!consolidationBase(list, tf));
    const tf =
      usable(next.timeframe)
        ? next.timeframe
        : usable(coords?.timeframe)
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
    // Holes inside the stored bars the server could not check this time (provider down,
    // request budget spent): marked, and asked again below until they are filled.
    holes = (r.holes ?? []).map((h) => ({
      before: h.before,
      after: h.after,
      code: r.notice?.code ?? 'provider',
      message: r.notice?.message ?? $t('histviz.notice.provider')
    }));
    owed = null;
    notice = r.notice ?? null;
    atHistoryStart = !!r.history_start;
    if (!r.count && !r.notice) notice = { code: 'empty', message: $t('histviz.notice.empty') };
    if (SIZED_TYPES.has(pane.type) && !pane.brick) pane.brick = suggestBrick(bars.c);
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
    range = null;
    restoreRange = chartRef?.getTimeRange?.() ?? null;
    pane.coords = { ...coords, timeframe: tf };
    recents = rememberRecent(pane.coords);
    onchanged?.();
    load();
  }

  // ── Range bar ──
  // A period framed at the timeframe that reads it (see ranges.js). The key stays lit until
  // the timeframe is changed another way.
  let range = $state(null);
  let ranging = $state(false);

  // Clock at the right end of the bar, in the zone the chart's time axis is labelled in
  // (Settings → Defaults), with its UTC offset: the time the axis is read against.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });
  const clockFmt = $derived.by(() => {
    try {
      return new Intl.DateTimeFormat('en-GB', {
        timeZone: tz.value,
        hourCycle: 'h23',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        timeZoneName: 'shortOffset'
      });
    } catch {
      return null;
    }
  });
  const clock = $derived.by(() => {
    if (!clockFmt) return '';
    const parts = clockFmt.formatToParts(now);
    const time = parts
      .filter((p) => p.type !== 'timeZoneName')
      .map((p) => p.value)
      .join('')
      .trim();
    const off = parts.find((p) => p.type === 'timeZoneName')?.value.replace('GMT', 'UTC') ?? '';
    return off ? `${time} (${off})` : time;
  });

  async function frameRange(r) {
    if (!coords || ranging) return;
    const tf = pickTimeframe(timeframes, r.tf);
    if (!tf) return;
    ranging = true;
    range = r.key;
    try {
      if (tf !== coords.timeframe) {
        restoreRange = null;
        pane.coords = { ...coords, timeframe: tf };
        recents = rememberRecent(pane.coords);
        onchanged?.();
        await load();
      }
      if (!bars?.ts?.length) return;
      const lastMs = Date.parse(bars.ts[bars.ts.length - 1]);
      const from = rangeStart(r.back, lastMs);
      // Older bars until the period is covered, a few slices at most: "5Y" on a minute-only
      // feed must not download a decade.
      for (let i = 0; i < 4 && !atHistoryStart && (from == null || Date.parse(bars.ts[0]) > from); i++) {
        const before = bars.ts.length;
        await loadMore();
        if (bars.ts.length === before) break;
      }
      await tick();
      const first = Date.parse(bars.ts[0]);
      chartRef?.setTimeRange?.(
        from == null || from <= first ? bars.ts[0] : isoSecond(from),
        bars.ts[bars.ts.length - 1]
      );
    } finally {
      ranging = false;
    }
  }

  /** What the provider serves as is. */
  const served = $derived.by(() => {
    if (!coords) return [];
    const fromCoords = coords.timeframes?.length ? coords.timeframes : null;
    const fromMatrix = providers.find((p) => p.provider === coords.provider)?.timeframes ?? [];
    return fromCoords ?? fromMatrix;
  });
  const customTimeframes = $derived(settings.customTimeframes ?? []);
  /** Served timeframes plus the user's custom ones this provider can consolidate. */
  const timeframes = $derived(
    [
      ...served,
      ...customTimeframes.filter((tf) => !served.includes(tf) && consolidationBase(served, tf))
    ].sort(byDuration)
  );
  /** The served timeframe the current one is built from, when it is a custom one. */
  const consolidatedFrom = $derived(
    meta?.consolidated_from ?? consolidationBase(served, coords?.timeframe)
  );
  let customTfOpen = $state(false);

  /** Store what is on screen: queues the normal download job over the loaded window. */
  async function saveWindow(scope = 'loaded') {
    if (!coords || !bars?.ts?.length) return;
    if (isTemp(coords)) return keepFile();
    saving = true;
    error = '';
    flash('');
    try {
      const view = scope === 'visible' ? (chartRef?.getTimeRange?.() ?? null) : null;
      const from = view?.t0 ?? bars.ts[0];
      const lastTs = view?.t1 ?? bars.ts[bars.ts.length - 1];
      const to = new Date(Date.parse(lastTs) + tfSecs * 1000).toISOString();
      // A custom timeframe has nothing of its own to store: its base bars are what it is
      // built from, and what makes it load from the catalog next time.
      const timeframe = consolidatedFrom ?? coords.timeframe;
      await histvizApi.savePreview({ ...$state.snapshot(coords), timeframe, from, to });
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

  /** A file opened without being kept has no provider to download from: storing it is the
   *  import it skipped, and the stored dataset then serves the same coordinates. */
  async function keepFile() {
    saving = true;
    error = '';
    try {
      pane.coords = { ...(await histvizApi.keepTemp($state.snapshot(coords))) };
      onchanged?.();
      histvizApi.datasets().then((rows) => ondatasets?.(rows)).catch(() => {});
      await load();
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
          x.timeframe === (consolidatedFrom ?? coords.timeframe)
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
    const back = s?.state === 'live' && liveStatus?.state !== 'live';
    liveStatus = s;
    // The feed is back: whatever closed while it was away is not coming from it.
    if (back) checkTail();
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

  // Every update is applied as it arrives. Merged by its timestamp, *ordered*, not appended:
  // the catch-up the server sends when the stream opens is a run of bars newer than the
  // chart's history but older than the bar forming now, and the axis is drawn by index, so a
  // bar dropped for arriving out of order shifts every later candle one slot left.
  //
  // An update to a bar already there (the forming one) is edited in place and published once
  // per frame, which is as fast as a screen shows it: the chart redraws only what it changed.
  // A new bar is a new series.
  function onLiveBar(d) {
    liveLag = d.lag_ms ?? null;
    const seen = Date.now();
    if (priceAt?.key !== instrumentKey || seen - priceAt.at > PRICE_AT_STEP_MS) priceAt = { key: instrumentKey, at: seen };
    const n0 = bars?.ts?.length ?? 0;
    const lastMs = n0 ? Date.parse(bars.ts[n0 - 1]) : null;
    const evMs = Date.parse(d.ts);
    // Left a few seconds first: the server backfills a hole it sees in its own feed and
    // sends those bars down this same stream, and a seam it closed costs nothing more.
    if (lastMs != null && evMs - lastMs > tfSecs * 1000 * SEAM_SLACK) oweSeam(lastMs, evMs, 5000);
    const merged = mergeLiveBar(bars, d);
    if (merged !== bars || merged.ts.length !== n0) {
      bars = { ...merged };
      return;
    }
    if (!tickRaf)
      tickRaf = requestAnimationFrame(() => {
        tickRaf = 0;
        barsTick++;
      });
  }
  $effect(() => () => cancelAnimationFrame(tickRaf));

  /** More than this many periods between two bars is a seam worth reading back. Not one:
   *  a month or a session-stamped day is not a fixed number of seconds. */
  const SEAM_SLACK = 1.5;
  /** The most one fill reads (the series endpoint's slice ceiling). Past it the chart takes
   *  the newest slice whole instead of walking forward. */
  const SEAM_MAX_BARS = 5000;

  /** Add `(fromMs, toMs)` to what the next fill reads, and schedule it. */
  function oweSeam(fromMs, toMs, wait = 300) {
    if (!(toMs > fromMs)) return;
    owed = owed ? [Math.min(owed[0], fromMs), Math.max(owed[1], toMs)] : [fromMs, toMs];
    clearTimeout(seamTimer);
    seamTimer = setTimeout(fillSeams, wait);
  }

  /** Whether `(fromMs, toMs)` still has a stretch longer than a period with no bar in it. */
  function seamOpen(fromMs, toMs) {
    const lim = tfSecs * 1000 * SEAM_SLACK;
    let next = toMs;
    for (let i = bars.ts.length - 1; i >= 0; i--) {
      const ms = Date.parse(bars.ts[i]);
      if (ms >= toMs) continue;
      if (ms <= fromMs) break;
      if (next - ms > lim) return true;
      next = ms;
    }
    return next - fromMs > lim;
  }
  $effect(() => () => clearTimeout(seamTimer));

  /** The stretch from the newest bar to now, when the chart has been away from the feed. */
  function checkTail() {
    if (!pane.live || !bars?.ts?.length) return;
    const lastMs = Date.parse(bars.ts[bars.ts.length - 1]);
    const now = Date.now();
    if (now - lastMs <= tfSecs * 1000 * SEAM_SLACK) return;
    // A closed market answers the same "nothing" on every return to the tab: asked once a
    // minute at most for the same newest bar.
    if (tailChecked?.lastMs === lastMs && now - tailChecked.at < 60_000) return;
    oweSeam(lastMs, now);
  }

  $effect(() => {
    const onBack = () => {
      if (document.visibilityState === 'visible') checkTail();
    };
    document.addEventListener('visibilitychange', onBack);
    window.addEventListener('online', onBack);
    return () => {
      document.removeEventListener('visibilitychange', onBack);
      window.removeEventListener('online', onBack);
    };
  });

  const isoOf = (ms) => new Date(ms).toISOString();

  /** Read the owed stretch back and put what comes into the chart. An answer with no bars
   *  and no notice is the market saying nothing traded there (a night, a weekend): nothing to
   *  draw and nothing to report. A notice or a failure is a stretch nobody can vouch for, and
   *  stays marked until a later read fills it. */
  async function fillSeams() {
    if (filling || !owed || !coords || !bars?.ts?.length) return;
    const [fromMs, toMs] = owed;
    owed = null;
    // The marks this read answers for, whatever it finds.
    const unmark = (list) =>
      list.filter((h) => Date.parse(h.before) < fromMs || (h.after ? Date.parse(h.after) : Infinity) > toMs);
    if (!seamOpen(fromMs, toMs)) {
      holes = unmark(holes);
      return;
    }
    const key = coordKey;
    const step = tfSecs * 1000;
    const want = Math.ceil((toMs - fromMs) / step) + 1;
    const tail = toMs >= Date.now() - step;
    filling = true;
    try {
      if (want > SEAM_MAX_BARS) {
        // Too far behind to read forward: the newest slice whole, and the bars the feed
        // delivered while it loaded kept on top of it.
        const r = await histvizApi.series({ ...$state.snapshot(coords), bars: SLICE_BARS });
        if (key !== coordKey || !r?.ts?.length) return;
        const lastMs = Date.parse(r.ts[r.ts.length - 1]);
        // Only the feed's bars newer than the slice: the older history ends where the hole
        // starts, and keeping it would glue the two together again.
        bars = fillBars(r, bars, lastMs, Infinity);
        meta = r;
        notice = r.notice ?? null;
        atHistoryStart = !!r.history_start;
        holes = r.notice
          ? [{ before: r.ts[r.ts.length - 1], after: null, code: r.notice.code, message: r.notice.message }]
          : [];
        if (!r.notice) tailChecked = { lastMs, at: Date.now() };
        return;
      }
      const r = await histvizApi.series({
        ...$state.snapshot(coords),
        to: isoOf(toMs),
        bars: Math.min(want, SEAM_MAX_BARS)
      });
      if (key !== coordKey) return;
      bars = fillBars(bars, r, fromMs, toMs);
      const before = isoOf(fromMs);
      const after = tail ? null : isoOf(toMs);
      holes = r.notice
        ? [...unmark(holes), { before, after, code: r.notice.code, message: r.notice.message }]
        : unmark(holes);
      if (tail && !r.notice) tailChecked = { lastMs: fromMs, at: Date.now() };
    } catch (e) {
      if (key !== coordKey) return;
      holes = [
        ...unmark(holes),
        { before: isoOf(fromMs), after: tail ? null : isoOf(toMs), code: 'provider', message: e.message }
      ];
    } finally {
      filling = false;
      if (owed) fillSeams();
    }
  }

  /** Read every marked stretch again (the strip's retry). */
  function retryHoles() {
    for (const h of holes) oweSeam(Date.parse(h.before), h.after ? Date.parse(h.after) : Date.now());
  }

  // A marked stretch is asked again on its own, further apart each time it stays missing:
  // a gateway back after a restart fills it without anyone pressing retry.
  const HOLE_RETRY_S = [30, 60, 120, 300];
  let holeTry = 0;
  $effect(() => {
    if (!holes.length) {
      holeTry = 0;
      return;
    }
    const wait = HOLE_RETRY_S[Math.min(holeTry, HOLE_RETRY_S.length - 1)] * 1000;
    const timer = setTimeout(() => {
      holeTry++;
      retryHoles();
    }, wait);
    return () => clearTimeout(timer);
  });

  /** The first stretch still missing, in the strip's words. */
  const holeText = $derived.by(() => {
    const h = holes[0];
    if (!h) return '';
    // In the chart's own zone and spelling, so the strip names the stamps the axis shows.
    const label = makeTsFmt(tz.value, tfSecs < 86400);
    const at = (iso) => label(Date.parse(iso));
    const span = h.after
      ? $t('histviz.gap.between', { from: at(h.before), to: at(h.after) })
      : $t('histviz.gap.since', { from: at(h.before) });
    return `${span}: ${h.message}`;
  });

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
  // Studies another one may read: never itself, nor one already reading it (a loop).
  let modalSources = $state([]);
  function studySources(exclude) {
    const all = chartRef?.studyOutputs?.() ?? [];
    const readsFrom = (inst, id) => sourceInstance(inst?.params?.source) === String(id);
    return all.filter(
      (s) => s.outputs.length && (exclude == null || (s.id !== exclude && !readsFrom(instances.find((i) => i.id === s.id), exclude)))
    );
  }
  function openAdd() {
    editing = null;
    modalSources = studySources(null);
    modalOpen = true;
  }
  function openEdit(ind) {
    editing = ind;
    modalSources = studySources(ind.id);
    modalOpen = true;
  }
  function onSave(draft) {
    const body = {
      type: draft.type,
      params: draft.params,
      style: draft.style,
      // Custom studies always name their pane; a catalog one only when fed by a derived
      // source (undefined clears a pane left over from an earlier source).
      pane: draft.pane,
      ...(draft.type === 'custom' ? { custom: draft.custom } : {}),
      ...(draft.type === 'script' ? { script: draft.script } : {})
    };
    if (editing) instances = instances.map((i) => (i.id === editing.id ? { ...i, ...body } : i));
    else instances = [...instances, { id: nextId++, ...body, visible: true }];
    editing = null;
  }
  const toggleInstance = (id) =>
    (instances = instances.map((i) => (i.id === id ? { ...i, visible: !i.visible } : i)));
  const removeInstance = (id) => (instances = instances.filter((i) => i.id !== id));
  // A row deleted from the library leaves every chart on screen at once.
  $effect(() => onIndicatorRemoved((id) => (instances = withoutIndicator(instances, id))));

  // Keep the pane's saved copy of its studies in step with what is on screen.
  $effect(() => {
    pane.instances = $state.snapshot(instances);
  });

  $effect(() => {
    if (SIZED_TYPES.has(pane.type) && bars && !pane.brick) pane.brick = suggestBrick(bars.c);
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

  const typeOpts = $derived(
    CHART_TYPE_GROUPS.flatMap(([group, types]) => [
      { value: `group:${group}`, label: $t(`histviz.page.typeGroup.${group}`), header: true },
      ...types.map((value) => ({ value, label: $t(`histviz.page.type.${value}`) }))
    ])
  );
  /** The picker always carries the loaded timeframe, even when the provider matrix has not
   *  come back yet: a disabled picker showing the wrong thing is worse than a slow one. */
  const tfOpts = $derived([
    ...(timeframes.length ? timeframes : coords?.timeframe ? [coords.timeframe] : []).map((tf) => ({
      value: tf,
      label: served.includes(tf) ? tf : `${tf} *`
    })),
    { value: CUSTOM_TF, label: $t('histviz.customTf.option') }
  ]);

  const CUSTOM_TF = '__custom';
  let tfValue = $state('');
  $effect(() => {
    tfValue = coords?.timeframe ?? '';
  });

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
      .map((i) => (isCustom(i) ? (i.custom?.name ?? '') : isScript(i) ? (i.script?.name ?? '') : instanceLabel(i.type, i.params)))
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
  class:docked={!!barDock}
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
  {#if !barDock || active}
  <div class="bar" class:docked={!!barDock} use:portal={barDock}>
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
          bind:value={tfValue}
          options={tfOpts}
          disabled={!coords}
          placeholder={$t('histviz.page.timeframeNone')}
          ariaLabel={$t('histviz.page.timeframe')}
          title={$t('histviz.page.timeframe')}
          float
          onpick={(tf) => {
            if (tf === CUSTOM_TF) {
              // An action, not a timeframe: the picker keeps showing the loaded one.
              tfValue = coords?.timeframe ?? '';
              customTfOpen = true;
              return;
            }
            switchTimeframe(tf);
            onlinked?.('timeframe', tf);
          }}
        />
      </div>
      {#if consolidatedFrom}
        <span
          class="tf-info"
          role="img"
          aria-label={$t('histviz.customTf.info', { tf: coords?.timeframe ?? '', base: consolidatedFrom })}
          title={$t('histviz.customTf.info', { tf: coords?.timeframe ?? '', base: consolidatedFrom })}
        >
          <Icon name="info" size={12} />
        </span>
      {/if}
      <div class="pick style">
        <Dropdown
          bind:value={pane.type}
          options={typeOpts}
          ariaLabel={$t('histviz.page.plotStyle')}
          title={$t('histviz.page.plotStyle')}
          float
        />
      </div>
      {#if SIZED_TYPES.has(pane.type)}
        <label class="brick">
          <span>{$t(`histviz.page.size.${pane.type}`)}</span>
          <input
            type="number"
            min="0"
            step="any"
            bind:value={pane.brick}
            placeholder={$t('histviz.modal.auto')}
          />
        </label>
      {/if}

      <!-- Live: a dot and, while streaming, the lag. Hollow = off (click to go live), green =
           streaming, amber = reconnecting; the tooltip spells it out. -->
      {#if canLive}
        <div class="live-group" class:on={pane.live}>
          <button
            class="live-btn"
            aria-pressed={pane.live}
            aria-label={pane.live ? $t('histviz.page.liveStop') : $t('histviz.page.liveStart')}
            title={[
              pane.live ? $t('histviz.page.liveStop') : $t('histviz.page.liveStart'),
              pane.live && liveLag != null ? `${$t('histviz.page.liveLag')}: ${liveLag} ms` : '',
              liveGrainHint,
              quoteHint
            ]
              .filter(Boolean)
              .join('\n')}
            onclick={toggleLive}
          >
            <span
              class="dot"
              class:off={!pane.live}
              class:connected={pane.live && liveStatus?.state === 'live'}
              class:warn={pane.live && liveStatus?.state === 'retrying'}
            ></span>
            {#if pane.live}
              <span class="lag">{liveLag != null && !liveStatus?.message ? `${liveLag} ms` : ''}</span>
            {/if}
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
      {:else if liveOffTimeframe}
        <span
          class="live-off"
          title={$t('histviz.page.liveTimeframeHint', { list: liveTimeframes.join(', ') })}
        >
          <span class="dot"></span>
        </span>
      {/if}

      <span class="sep" aria-hidden="true"></span>

      <!-- Studies: compare, its mode, the objects, the indicators, the alerts. -->
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

      <!-- The rest stands at the right edge (in the page bar: next to the quick backtest,
           the book and the ticket). -->
      <span class="grow" aria-hidden="true"></span>
    </div>

    <span class="dock-part" use:portal={actDock}>
      <button
        class="icon-btn"
        disabled={!coords || !bars?.count || handoff}
        title={$t('histviz.page.backtestHint')}
        aria-label={$t('histviz.page.backtest')}
        onclick={toBacktest}
      >
        <Icon name="flask" size={12} />
      </button>
    </span>
    <span class="dock-part" use:portal={ctlDock}>
      <button
        class="icon-btn"
        class:on={!!pane.trade?.on}
        disabled={!coords}
        aria-pressed={!!pane.trade?.on}
        title={$t('histviz.trade.barToggle')}
        aria-label={$t('histviz.trade.barToggle')}
        onclick={toggleTradeBar}
      >
        <Icon name="arrow-up-down" size={12} />
      </button>
      <span class="sep" aria-hidden="true"></span>
      <!-- The pane's own controls. Outside the scroller, always reachable. -->
      <span class="cog-wrap" use:clickOutside={() => (tplOpen = false)}>
        <button
          bind:this={tplBtn}
          class="icon-btn"
          class:on={tplOpen}
          aria-expanded={tplOpen}
          aria-haspopup="menu"
          title={$t('histviz.templates.title')}
          aria-label={$t('histviz.templates.title')}
          onclick={toggleTemplates}
        >
          <Icon name="layout-template" size={12} />
        </button>
        {#if tplOpen}
          <TemplateMenu
            {templates}
            at={tplAt}
            onapply={(tpl) => {
              tplOpen = false;
              useTemplate(tpl);
            }}
            onapplyto={(tpl) => {
              tplOpen = false;
              onapplyto?.(tpl);
            }}
            onsave={(name) => onsavetemplate?.(name)}
            ondelete={(tpl) => ondeletetemplate?.(tpl.id)}
          />
        {/if}
      </span>
    </span>
  </div>
  {/if}

  <div class="chart-wrap">
    <!-- Settings and link group, bottom right of every pane: the axis corner is the one spot
         the chart never paints on, and the buttons stay reachable with the bar docked. -->
    <span class="corner">
      <span class="cog-wrap" use:clickOutside={() => (cogOpen = false)}>
        <button
          bind:this={cogBtn}
          class="icon-btn"
          class:on={cogOpen}
          aria-expanded={cogOpen}
          title={$t('histviz.settings.chartTitle')}
          aria-label={$t('histviz.settings.chartTitle')}
          onclick={toggleCog}
        >
          <Icon name="settings" size={12} />
        </button>
        {#if cogOpen && pane.settings}
          <ChartSettings
            bind:settings={pane.settings}
            at={cogAt}
            onapplyall={() => {
              cogOpen = false;
              onapplysettings?.(styleOf($state.snapshot(chartSettings)));
            }}
            onmore={() => {
              cogOpen = false;
              settingsModalOpen = true;
            }}
            oninstrument={coords?.ticker
              ? () => {
                  cogOpen = false;
                  instrumentOpen = true;
                }
              : null}
          />
        {/if}
      </span>
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
    </span>
    <!-- ── Status strip ──
         Errors and provider notices float over the chart rather than sitting above it: a
         banner in the flow resizes the chart, and the window the user was reading moves.
         Bottom-left is the only corner the chart itself does not paint on. -->
    <div class="strip">
      {#if error && !issue}
        <div class="msg err">
          <ErrorText {error} copyable compact />
          <button
            class="dismiss"
            title={$t('common.close')}
            aria-label={$t('common.close')}
            onclick={() => (error = '')}
          >
            <Icon name="x" size={10} />
          </button>
        </div>
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
      {#if holes.length}
        <p class="msg hard">
          <Icon name="alert-triangle" size={11} />
          <span class="code">{$t('histviz.gap.label')}</span>
          <span class="txt" title={holeText}>{holeText}</span>
          <button class="retry" onclick={retryHoles} disabled={filling}>{$t('histviz.live.retry')}</button>
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
    {#if !railDock || active}
    <div
      class="rail-slot"
      class:idle={!bars?.count || loading}
      inert={!bars?.count || loading}
      use:portal={railDock}
    >
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
    {/if}
    {#if !bars?.count && (loading || (coords?.ticker && !bars && !issue))}
      <!-- An empty pane on its way to a chart: a loader in the middle, naming what it fetches.
           A skeleton alone read as a blank pane, with nothing to say anything was happening.
           Also shown before the request leaves (a pane mounted ahead of the workspace's
           batch), which otherwise flashed "select an instrument" next to its own symbol. -->
      <div class="sk-chart" aria-busy="true">
        <Skeleton height="100%" />
        <div class="pane-loader" role="status">
          <span class="spin big" aria-hidden="true"></span>
          <span class="what">{coords?.ticker ?? ''}{coords?.timeframe ? ` · ${coords.timeframe}` : ''}</span>
          <span class="hint">{$t('common.loading')}</span>
        </div>
      </div>
    {:else if bars?.count}
      <!-- A reload keeps the chart on screen under a progress line. Swapping it for a
           skeleton would throw away the window being read and rebuild the whole plot. -->
      <div class="chart-slot" class:busy={loading || filling} aria-busy={loading || filling}>
        {#if loading || filling}
          <span class="progress" aria-hidden="true"></span>
          <div class="sync-badge" role="status">
            <span class="spin" aria-hidden="true"></span>
            {$t('histviz.chart.syncing')}
          </div>
        {/if}
      <Chart
        bind:this={chartRef}
        {bars}
        tick={barsTick}
        tickSize={priceTick}
        type={pane.type}
        {instances}
        scriptRuns={scriptRuns.results}
        onscriptinfo={(id) => (scriptInfoId = id)}
        brick={pane.brick}
        settings={chartSettings}
        quote={chartSettings.bidAsk && pane.live && liveStatus?.state === 'live' ? liveQuote : null}
        {holes}
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
        {maximized}
        onmaximize={() => onmaximize?.(pane.id)}
        {closable}
        onclose={() => onclose?.(pane.id)}
        bind:plotInset
        onrestored={() => (restoreRange = null)}
        onloadmore={loadMore}
        compare={compareSeries}
        oncomparetoggle={toggleCompare}
        oncompareremove={removeCompare}
        oncompareedit={(id) => (compareEditId = id)}
        onsettings={chartPatch}
        onhover={(ms) => onlinked?.('crosshair', ms)}
        onwindow={(r) => onlinked?.('range', { ...r, timeframe: coords?.timeframe })}
        bind:tool
        bind:drawings
        bind:selectedDrawing
        bind:magnet
        bind:drawingsHidden
        bind:paneHeights={pane.paneHeights}
        {quick}
        book={bookLevels}
        marks={chartMarks}
        bind:quickSize
        quickUnitLabel={$t(`histviz.quick.unit.${quickUnit}`)}
        onquickclick={quickClick}
        onquickfill={quickFill}
        onquicktoggle={quickToggle}
        trading={tradable}
        {tradeQty}
        orderLines={myOrderLines}
        ontradeat={tradeAt}
        onticketat={(p) => onticket?.(p)}
        onalertat={coords
          ? (p) => {
              alertAt = p;
              alertsOpen = true;
            }
          : null}
        onordermove={(p) => onordermove?.(p)}
        onordercancel={(id) => onordercancel?.(id)}
        onorderedit={(id) => onorderedit?.(id)}
        onbookorderedit={(id) => onbookorderedit?.(id)}
        onpositionclose={onpositionclose ? (pos) => onpositionclose(pos) : null}
      />
      {#if pane.trade?.on && coords}
        <QuickOrder
          edge={pane.trade.edge}
          at={pane.trade.at}
          inset={plotInset}
          bind:qty={pane.trade.qty}
          unit={tradeUnit}
          units={tradeUnits}
          onunit={setTradeUnit}
          bid={liveQuote?.bid ?? lastClose}
          ask={liveQuote?.ask ?? lastClose}
          {fmtPrice}
          disabled={!tradable}
          reason={tradeOff}
          onbuy={() => quickOrder('buy')}
          onsell={() => quickOrder('sell')}
          onmove={(p) => (pane.trade = { ...pane.trade, ...p })}
        />
      {/if}
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

  <!-- Range bar: in place of a slider, and a fraction of its height. Docked, only the active
       chart fills the page's bottom bar, like the bar above. -->
  {#if coords && (!rangeDock || active)}
    <div class="ranges" class:docked={!!rangeDock} role="toolbar" aria-label={$t('histviz.range.label')} use:portal={rangeDock}>
      {#each RANGES as r (r.key)}
        <button
          type="button"
          class:on={range === r.key}
          disabled={ranging || !bars?.count}
          title={$t('histviz.range.hint', { range: r.key, tf: pickTimeframe(timeframes, r.tf) ?? '' })}
          onclick={() => frameRange(r)}>{r.key}</button
        >
      {/each}
      {#if clock}
        <span class="clock" title={tz.value}>{clock}</span>
      {/if}
    </div>
  {/if}
</section>

<ObjectTree bind:open={treeOpen} bind:drawings bind:selected={selectedDrawing} />

{#if pane.settings}
  <ChartSettingsModal
    bind:open={settingsModalOpen}
    bind:settings={pane.settings}
    global={settings}
    onglobal={(patch) => onsettings?.(patch)}
  />
{/if}

<CustomTimeframeModal
  bind:open={customTfOpen}
  {served}
  custom={customTimeframes}
  onchange={(list) => onsettings?.({ customTimeframes: list })}
  onpick={(tf) => {
    switchTimeframe(tf);
    onlinked?.('timeframe', tf);
  }}
/>

<AlertsModal
  bind:open={alertsOpen}
  coords={coords ? $state.snapshot(coords) : null}
  suggested={alertLevel}
  level={alertAt}
  connectorId={activeConnector?.id ?? coords?.connector_id ?? null}
  {canLive}
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

{#if compareEditId != null}
  {@const editing = (pane.compare ?? []).find((c) => c.id === compareEditId)}
  <CompareStyleModal
    open={!!editing}
    series={editing}
    color={compareSeries.find((c) => c.id === compareEditId)?.color}
    onpatch={(p) => patchCompare(compareEditId, p)}
    onclose={() => (compareEditId = null)}
  />
{/if}

{#if scriptInfoId != null}
  <Modal open={true} size="md" title={$t('histviz.script.issues')} onclose={() => (scriptInfoId = null)}>
    <div class="script-info">
      <ScriptWarnings
        warnings={scriptInfo.warnings ?? []}
        error={scriptInfo.error ?? null}
        diagnostics={scriptInfo.diagnostics ?? []}
        inputErrors={scriptInfo.inputErrors ?? []}
        onBricks={!!scriptInfo.onBricks}
        coords={coords ? $state.snapshot(coords) : null}
        onmeta={factsSaved}
      />
    </div>
  </Modal>
{/if}

{#if coords?.ticker}
  <InstrumentMetaModal bind:open={instrumentOpen} coords={$state.snapshot(coords)} onsaved={factsSaved} />
{/if}

<IndicatorModal
  bind:open={modalOpen}
  edit={editing}
  sources={modalSources}
  formulas={settings.formulas ?? []}
  onformulas={(list) => onsettings?.({ formulas: list })}
  onsave={onSave}
  onclose={() => (editing = null)}
/>

<DataModal
  bind:open={dataOpen}
  {datasets}
  {recents}
  selected={coords}
  busy={loading}
  onselect={openCoords}
  ondownload={() => goto('/histdata')}
  importable
  onimported={() => histvizApi.datasets().then((rows) => ondatasets?.(rows)).catch(() => {})}
/>

<style>
  .script-info {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
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
  /* Docked: the shared bar above acts on this pane, so the frame is the only sign of it. */
  .pane.docked.active {
    border-color: var(--accent);
  }
  .bar.docked {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border-bottom: none;
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
  .grow {
    flex: 1;
    min-width: var(--space-2);
  }
  .dock-part {
    display: inline-flex;
    align-items: center;
    flex: none;
    gap: 1px;
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
    border: var(--hairline) solid transparent;
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    font-size: var(--fs-body);
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  .sym-btn:hover {
    background: var(--surface-2);
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

  /* Pickers carry the bar's height through the token, so they line up with the buttons
     instead of each control choosing its own. */
  .pick {
    --control-h: 22px;
    flex: none;
  }
  /* Bar pickers are ghosts like the buttons around them: no frame at rest, the hover fill
     says they are controls, the accent hairline says which one is open. */
  .pick :global(.trigger) {
    gap: var(--space-1);
    padding: 0 var(--space-2);
    background: transparent;
    border-color: transparent;
    border-radius: var(--radius);
    transition: background-color var(--dur-fast) var(--ease);
  }
  .pick :global(.trigger:hover:not(:disabled)) {
    background: var(--surface-2);
  }
  .pick :global(.trigger[aria-expanded='true']) {
    background: var(--surface-2);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
  }
  .pick :global(.trigger svg) {
    flex: none;
    color: var(--dim);
  }
  .pick.tf {
    width: 68px;
  }
  .tf-info {
    display: inline-flex;
    align-items: center;
    flex: none;
    color: var(--amber);
    cursor: help;
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

  .cog-wrap {
    display: inline-flex;
    flex: none;
  }
  .corner {
    position: absolute;
    right: 2px;
    bottom: 2px;
    z-index: 6;
    display: inline-flex;
    align-items: center;
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
    gap: var(--space-1);
    flex: none;
    height: 22px;
  }
  .live-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 100%;
    min-width: 22px;
    justify-content: center;
    padding: 0 5px;
    background: none;
    border: 0;
    border-radius: var(--radius);
    cursor: pointer;
    white-space: nowrap;
  }
  .live-btn:hover {
    background: var(--surface-2);
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
  /* Off: a hollow ring, the button to go live. */
  .dot.off {
    background: none;
    box-shadow: inset 0 0 0 1.5px var(--muted);
  }
  .dot.connected {
    background: var(--green);
    animation: live-pulse 2s ease-out infinite;
  }
  .dot.warn {
    background: var(--amber);
  }
  @keyframes live-pulse {
    0% {
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--green) 55%, transparent);
    }
    70%,
    100% {
      box-shadow: 0 0 0 4px transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dot.connected {
      animation: none;
    }
  }
  /* Reserved while live: the lag reads out without pushing the bar around as it changes. */
  .lag {
    flex: none;
    min-width: 40px;
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    color: var(--dim);
    text-align: left;
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
  .ranges {
    display: flex;
    align-items: center;
    flex: none;
    gap: 2px;
    height: 24px;
    padding: 0 var(--space-2);
    border-top: var(--hairline) solid var(--border);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .ranges.docked {
    flex: 1;
    height: 100%;
    border-top: none;
  }
  .ranges button {
    flex: none;
    height: 20px;
    padding: 0 var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .ranges button:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .ranges button.on {
    color: var(--accent);
  }
  .ranges button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .ranges .clock {
    flex: none;
    margin-left: auto;
    padding: 0 var(--space-2);
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
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
  /* Overlay only: pointer-events none, so the chart stays usable underneath. */
  .sync-badge {
    position: absolute;
    top: var(--space-3);
    left: 50%;
    transform: translateX(-50%);
    z-index: 7;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    color: var(--text);
    font-size: 0.8rem;
    pointer-events: none;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.35);
  }
  .spin {
    width: 12px;
    height: 12px;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
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
    .spin {
      animation: none;
    }
    .progress::after {
      animation: none;
      width: 100%;
      opacity: 0.5;
    }
  }
  .sk-chart {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .pane-loader {
    position: absolute;
    inset: 0;
    z-index: 7;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    pointer-events: none;
    color: var(--text);
  }
  .pane-loader .what {
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 0.9rem;
  }
  .pane-loader .hint {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .spin.big {
    width: 28px;
    height: 28px;
    border-width: 3px;
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
  /* Tablet: the shared bar has its own row there (see the page), room for the whole label. */
  @media (min-width: 768px) and (max-width: 1099px) {
    .bar.docked .pick.tf { width: 84px; }
  }
  @media (max-width: 767px) {
    /* Two rows on a phone: the instrument controls take the full width (and scroll there if
       they still do not fit), the pane's own buttons sit on the row below. */
    .bar, .bar.docked { height: auto; min-height: 44px; flex-wrap: wrap; justify-content: flex-end; }
    .bar-main { flex: 1 1 100%; min-height: 44px; scrollbar-width: thin; }
    .pick.tf { width: 84px; }
    .sym-btn { height: 40px; }
    .icon-btn { width: 40px; height: 40px; }
    .pick { --control-h: 40px; }
    .brick input { height: 40px; }
  }
</style>
