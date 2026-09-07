<script>
  import Icon from '$lib/ui/Icon.svelte';
  // ECharts price chart. Main pane: candlestick / OHLC bar / line / Renko. Below: volume,
  // then one sub-pane per distinct oscillator type in use (RSI, MACD, …) plus one per custom
  // indicator the user sent off the price pane. Overlay indicators (SMA/EMA/Bollinger/…) draw
  // on the price pane. Indicators come from the `instances` prop ({id, type, params, visible});
  // `computeInstance` produces the series generically, so adding a catalog entry needs no
  // change here, and `paneOf`/`paneKey` own every pane decision.
  //
  // ── Navigation is hand-rolled, NOT ECharts' built-in roam ──
  // ECharts' inside dataZoom animates every pan step over 100ms (roams.js, not overridable),
  // so dragging only ever nudged the window and felt stuck. Here the inside dataZoom is inert
  // (all its gestures off) and the visible window lives in `win` as *bar indices* — pointer
  // drag pans 1:1 with the cursor, the wheel zooms anchored under it, the axis gutters
  // drag-scale, and the keyboard mirrors all of it. Index-based (startValue/endValue) instead
  // of percent so every gesture is exact arithmetic instead of a percentage round-trip.
  import { onMount, onDestroy, untrack } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t } from '$lib/i18n';
  import {
    renko,
    catalogDef,
    instanceLabel,
    isCustom,
    paneOf,
    paneKey,
    computeInstance
  } from './indicators.js';
  import {
    drawTool,
    makeDrawing,
    translate,
    moveAnchor,
    copyDrawing,
    pasteDrawing,
    canPaste,
    nearestAnchor,
    DEFAULT_DRAW_STYLE,
    FIB_LEVELS
  } from './drawings.js';
  import { priceDigitsOf, makePriceFmt, makeAxisFmt } from './format.js';
  import { tz, makeTsFmt } from '$lib/tz.svelte.js';
  import { QUICK_COLORS, formatSize } from './quicktest.js';

  let {
    bars = null,
    type = 'candlestick',
    instances = [],
    brick = 0,
    settings = {},
    fullscreen = false,
    // Set by the parent to re-open a specific time span on the next dataset (timeframe
    // switch keeps you looking at the same dates). Consumed once, then `onrestored` fires.
    restoreRange = null,
    loadingMore = false,
    atHistoryStart = true,
    // Label and (optional) reason for the left-edge history button. History is never
    // fetched by scrolling: the user asks for the previous slice explicitly.
    loadMoreLabel = '',
    loadMoreNotice = '',
    // Drawing rail state, owned by the page so the rail can live outside the chart box.
    tool = $bindable('cursor'),
    drawings = $bindable([]),
    selectedDrawing = $bindable(null),
    magnet = $bindable(false),
    drawingsHidden = $bindable(false),
    // ── Quick backtest ──
    // The chart is only the surface: it turns gestures into (instant, price) and paints the
    // markers the engine replayed. The position itself lives with the page.
    quick = false,
    marks = [],
    // Print the size next to each marker. A click-through session sizes by hand, so the number
    // means something; a simulated fill's size is arithmetic, and only clutters the chart.
    markSizes = true,
    quickSize = $bindable(1),
    quickUnitLabel = '', // what the size box counts: units, contracts, dollars…
    // Instrument identity, printed on the chart itself the way market tools do it.
    title = null, // { ticker, timeframe, provider }
    ontoggle,
    onedit,
    onremove,
    onsymbol,
    onfullscreen,
    onrestored,
    onloadmore,
    // Linked panes: what this chart is pointing at, and the span it is showing. Both fire
    // only on the user's own gestures, never when the link itself moved this chart.
    onhover,
    onwindow,
    // Comparison series: other instruments rebased onto this chart's own clock. Values are
    // percentages (or a ratio rebased to 100), so they ride the hidden overlay scale and
    // cannot flatten the price axis.
    compare = [],
    oncomparetoggle,
    oncompareremove,
    // A change to the *global* chart settings made from inside the chart (the drawing
    // templates live there, next to the colours and the scale).
    onsettings,
    onquickclick,
    onquickfill,
    onquicktoggle
  } = $props();

  const GRID_AXIS = 50; // px reserved for the price axis labels (the y drag gutter)
  const GRID_PAD = 16; // breathing room on the side without the axis
  const SLIDER_BAND = 24; // px at the very bottom owned by the ECharts range slider
  const MIN_BARS = 8; // tightest zoom
  const DEFAULT_BARS = 200; // bars shown on first load / Fit
  const ZOOM_STEP = 0.032; // ≈3 % per mouse notch — smooth, never a jump
  const ZOOM_MIN_UNIT = 0.25; // floor: ≈4 % per trackpad tick, which fires far more often

  let el;
  let host;
  let chart = $state(null); // reactive: the render effect must re-run once init lands

  onDestroy(() => {
    if (quickTimer) clearTimeout(quickTimer);
    chart?.dispose();
  });

  // ── Theme palette, with per-setting color overrides (blank setting = keep theme) ──
  // chartColors() is what makes this derived depend on the theme: a bare getComputedStyle
  // read is not reactive, so the canvas would keep the old palette until the next data change.
  const palette = $derived.by(() => {
    const c = chartColors();
    const up = settings.upColor || c.green;
    const down = settings.downColor || c.red;
    const accent = settings.lineColor || c.accent;
    return {
      up,
      down,
      green: up,
      red: down,
      text: c.text,
      muted: c.muted,
      dim: c.dim,
      faint: c.faint,
      border: c.border,
      gridLine: c.gridLine,
      accent,
      amber: c.amber,
      surface: c.surface,
      bg: c.bg,
      mono: c.mono
    };
  });

  // Renko replaces source bars with a synthetic series; everything else uses bars as-is.
  const view = $derived.by(() => {
    if (!bars) return null;
    if (type === 'renko') {
      const r = renko(bars.ts, bars.c, brick);
      return { ts: r.ts, o: r.o, h: r.h, l: r.l, c: r.c, v: r.ts.map(() => 0) };
    }
    return bars;
  });

  const n = $derived(view?.ts?.length ?? 0);

  // Which side the price scale (and everything pinned to it — value tags, the last-price
  // tag, the drag gutter) lives on. Default left, as this chart has always drawn it.
  const axisRight = $derived(settings.yAxisSide === 'right');
  const gridLeft = $derived(axisRight ? GRID_PAD : GRID_AXIS);
  const gridRight = $derived(axisRight ? GRID_AXIS : GRID_PAD);

  // ── Right margin ──
  // Market tools never pin the last bar to the axis: they keep empty space after it so the
  // crosshair tags clear the edge and you can pan past the end. We do it by padding the
  // category axis with extrapolated future timestamps — no data is drawn there.
  const pad = $derived(n ? Math.max(3, Math.round(Math.min(n, DEFAULT_BARS) * 0.1)) : 0);
  const total = $derived(n + pad);

  // Bar spacing in ms, from the last two bars — drives both the pad labels and the date format.
  const stepMs = $derived.by(() => {
    if (!view || n < 2) return 86400000;
    const a = Date.parse(view.ts[n - 2]);
    const b = Date.parse(view.ts[n - 1]);
    return Number.isFinite(a) && Number.isFinite(b) && b > a ? b - a : 86400000;
  });
  const intraday = $derived(stepMs < 86400000);

  const cats = $derived.by(() => {
    if (!view) return [];
    const last = Date.parse(view.ts[n - 1]);
    const future = Number.isFinite(last)
      ? Array.from({ length: pad }, (_, k) => new Date(last + stepMs * (k + 1)).toISOString())
      : Array.from({ length: pad }, (_, k) => `+${k + 1}`);
    return [...view.ts, ...future];
  });

  // RFC3339 in, compact label out, in the timezone picked in Settings → Defaults.
  // Slicing the string is what it used to do, and it printed two clocks in one axis: a
  // provider stamps its bars in the exchange's offset, the live hub and the future-pad
  // stamp theirs in UTC, so the same wall clock jumped two hours mid-scale. Parsing to an
  // instant and formatting it is the only way the labels are comparable at all.
  // Daily and coarser stamps stay a plain slice: they are a *date*, not an instant, and
  // shifting midnight UTC into a western zone would re-label the bar as the day before.
  const tsLabel = $derived(makeTsFmt(tz.value, intraday));

  function fmtTs(s) {
    if (typeof s !== 'string' || s.length < 10) return s ?? '';
    if (!intraday) return s.slice(0, 10);
    const ms = Date.parse(s);
    return Number.isFinite(ms) ? tsLabel(ms) : s;
  }

  // ── Memoized indicator outputs ──
  // compute() used to run three times per render (once for the legend swatch, once for the
  // crosshair tags, once for the series) — on 10k bars with a few indicators that was the
  // hitch on every toggle. One map, three consumers.
  const computed = $derived.by(() => {
    const m = new Map();
    if (!view) return m;
    for (const ind of instances) m.set(ind.id, computeInstance(ind, view) ?? []);
    return m;
  });

  // Visible indicator instances, split by pane.
  const active = $derived(instances.filter((i) => i.visible));
  const overlays = $derived(active.filter((i) => paneOf(i) === 'overlay'));
  // Distinct sub-panes, in first-seen order (catalog oscillators share one pane per type;
  // a custom indicator sent to its own pane gets one of its own).
  const oscPanes = $derived.by(() => {
    const seen = [];
    for (const i of active) {
      const key = paneKey(i);
      if (paneOf(i) === 'oscillator' && !seen.includes(key)) seen.push(key);
    }
    return seen;
  });

  // Hidden price-pane scales, appended after the per-grid axes (price, then the sub-panes):
  // one for volume (drawn inside the price pane, market-tool style) and one for custom
  // overlays. Their indices are what the crosshair tags convert against.
  const volAxisIndex = () => 1 + oscPanes.length;
  const customAxisIndex = () => 2 + oscPanes.length;

  let legendOpen = $state(true);
  // Built-in series (price + volume) are toggleable from the legend like indicators.
  let priceVisible = $state(true);
  let volumeVisible = $state(true);
  // Manual price-axis range, set by dragging the y gutter. null = auto-fit the window.
  /** Manual y ranges, keyed by pane index: 0 is the price pane, 1..n the oscillator panes.
   *  Per pane and not one for all, because an RSI pane and the price pane are two scales:
   *  zooming one has nothing to say about the other. Always reassigned, never mutated in
   *  place, so the render effect sees the change. */
  let yMan = $state({});
  /** The range a gesture is currently painting, per pane, deliberately *outside* `$state`:
   *  writing `yMan` on every pointer event rebuilt the whole option each time, which is what
   *  made scaling and panning stutter. A gesture writes here and merges the one axis it
   *  touches; `yMan` is written once, on release. */
  let yLive = {};
  const yRange = (i) => yLive[i] ?? yMan[i];
  const clearY = (i) => {
    if (yLive[i] != null) {
      const l = { ...yLive };
      delete l[i];
      yLive = l;
    }
    if (yMan[i] == null) return;
    const next = { ...yMan };
    delete next[i];
    yMan = next;
  };
  const setY = (i, r) => (yMan = { ...yMan, [i]: r });
  /** Push a pane's range onto its own axis: a merged setOption leaves the series, the
   *  dataZoom and the gesture in flight alone, so it costs a fraction of a rebuild. */
  function pushY(i, r) {
    yLive = { ...yLive, [i]: r };
    if (!chart) return;
    chart.setOption({
      yAxis: Array.from({ length: i + 1 }, (_, k) =>
        k === i ? { min: r.min, max: r.max, scale: false } : {}
      )
    });
    if (i === 0) {
      if (priceLogNow()) {
        logRange = r;
        updateLogGrid();
      }
      updateLastTag();
    }
    updateDrawings();
  }
  /** Turn what the gesture painted into the pane's stored range: one rebuild, on release. */
  function commitY() {
    const live = yLive;
    yLive = {};
    for (const k of Object.keys(live)) setY(Number(k), live[k]);
  }
  let hoverIdx = $state(null); // crosshair bar index, null when the cursor is off-chart
  let tags = $state([]); // [{ label, color, top(px), value }]
  let logGrid = $state([]); // [{ top(px), text }] custom left-axis labels for the log scale
  let lastTag = $state(null); // { top(px), text, color } — latest close on the price axis
  // Countdown to bar close. Ticks on its own clock (the tag's *value* changes every second,
  // its position only when the geometry does), and only while the setting is on.
  let nowMs = $state(Date.now());
  $effect(() => {
    if (!settings.countdown) return;
    nowMs = Date.now();
    const id = setInterval(() => (nowMs = Date.now()), 1000);
    return () => clearInterval(id);
  });
  /** ms → `1d 04:12`, `4:12:07` or `12:07`. */
  function fmtLeft(ms) {
    const s = Math.floor(ms / 1000);
    const pad = (x) => String(x).padStart(2, '0');
    const d = Math.floor(s / 86400);
    const h = Math.floor((s % 86400) / 3600);
    const m = Math.floor((s % 3600) / 60);
    if (d > 0) return `${d}d ${pad(h)}:${pad(m)}`;
    if (h > 0) return `${h}:${pad(m)}:${pad(s % 60)}`;
    return `${pad(m)}:${pad(s % 60)}`;
  }
  // The last bar is a period, not an instant: it closes one step after it opened. Nothing is
  // shown once that moment has passed — stale history has no bar still running. Renko bars
  // close on price, not on the clock, so they have no countdown at all.
  const countdown = $derived.by(() => {
    if (!settings.countdown || settings.lastPrice === false || !view || !n) return '';
    if (type === 'renko') return '';
    const closeMs = Date.parse(view.ts[n - 1]) + stepMs;
    if (!Number.isFinite(closeMs)) return '';
    const left = closeMs - nowMs;
    return left > 0 && left <= stepMs ? fmtLeft(left) : '';
  });
  let logRange = { min: 0, max: 0 }; // set by buildOption when the log scale is active
  let win = $state({ s: 0, e: 0 }); // visible window, in bar indices into the padded axis
  let gridLayout = []; // [{left,right,topPct,heightPct}] — authored by buildOption

  // ────────────────────────── Window plumbing ──────────────────────────

  function clampWin(s, e) {
    const T = total;
    if (T < 2) return { s: 0, e: 0 };
    let w = Math.min(Math.max(e - s, MIN_BARS), T - 1);
    if (s < 0) s = 0;
    e = s + w;
    if (e > T - 1) {
      e = T - 1;
      s = Math.max(0, e - w);
    }
    return { s, e };
  }

  /** True while the link is driving this chart, so following a neighbour never echoes back
   *  as a gesture of our own. Without it two linked panes push each other forever. */
  let linking = false;
  let reportTimer = 0;

  function setWin(s, e) {
    win = clampWin(s, e);
    if (!linking) reportWindow();
  }

  /** Tell the page what span is on screen, coalesced: a drag is hundreds of `setWin` calls
   *  and the neighbours only need the result. */
  function reportWindow() {
    if (!onwindow) return;
    clearTimeout(reportTimer);
    reportTimer = setTimeout(() => {
      const r = getTimeRange();
      if (r) onwindow(r);
    }, 60);
  }

  // Small sets open whole (plus the right margin); large ones open on the recent window.
  function defaultWin() {
    if (n <= DEFAULT_BARS) return clampWin(0, total - 1);
    return clampWin(total - 1 - (DEFAULT_BARS + pad), total - 1);
  }

  // dispatchAction applies the window without ECharts' roam animation. `applying` keeps the
  // resulting datazoom event (which the slider also emits) from writing back into `win`.
  let applying = false;
  let raf = 0;
  function applyWindow() {
    if (!chart) return;
    applying = true;
    chart.dispatchAction({ type: 'dataZoom', startValue: win.s, endValue: win.e });
    applying = false;
    // The price axis re-fits to the new window, so a fixed price lands on a new pixel.
    updateLastTag();
    requestCursorTag();
    updateDrawings();
  }
  function schedule() {
    if (raf || !chart) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      applyWindow();
    });
  }

  // Integer window bounds clamped to real data — for y-fitting and the legend readout.
  function dataWin() {
    return {
      from: Math.max(0, Math.floor(win.s)),
      to: Math.min(n - 1, Math.ceil(win.e))
    };
  }

  // Pixel rect of pane `i`, derived from the layout buildOption authored (no ECharts internals).
  function rect(i) {
    const g = gridLayout[i];
    if (!g || !chart) return null;
    const W = chart.getWidth();
    const H = chart.getHeight();
    return {
      x: g.left,
      y: (H * g.topPct) / 100,
      width: Math.max(1, W - g.left - g.right),
      height: Math.max(1, (H * g.heightPct) / 100)
    };
  }
  function plotBottom() {
    const last = gridLayout[gridLayout.length - 1];
    if (!last || !chart) return chart?.getHeight() ?? 0;
    return (chart.getHeight() * (last.topPct + last.heightPct)) / 100;
  }

  // ────────────────────────── Gestures ──────────────────────────

  let drag = null;
  let panning = $state(false);

  function hitZone(x, y) {
    if (!chart) return 'none';
    const bottom = plotBottom();
    // Below the panes: the date strip is draggable, the slider underneath it is ECharts'.
    if (y > bottom) return y < chart.getHeight() - SLIDER_BAND ? 'xaxis' : 'none';
    if (axisRight) {
      if (x > chart.getWidth() - GRID_AXIS) return 'yaxis';
      if (x < GRID_PAD) return 'none';
    } else {
      if (x < GRID_AXIS) return 'yaxis';
      if (x > chart.getWidth() - GRID_PAD) return 'none';
    }
    return 'plot';
  }

  /** Which pane a y pixel falls in. A gesture belongs to the scale it started on. */
  function paneAt(y) {
    if (!chart) return 0;
    const H = chart.getHeight();
    for (let i = 0; i < gridLayout.length; i++) {
      const g = gridLayout[i];
      const top = (H * g.topPct) / 100;
      if (y >= top && y <= top + (H * g.heightPct) / 100) return i;
    }
    return gridLayout.length - 1;
  }

  // A pane's current extent in data units, read through the public pixel API.
  function paneExtent(i = 0) {
    const r = rect(i);
    if (!r || !chart) return null;
    const hi = chart.convertFromPixel({ gridIndex: i }, [0, r.y]);
    const lo = chart.convertFromPixel({ gridIndex: i }, [0, r.y + r.height]);
    if (!hi || !lo) return null;
    const max = hi[1];
    const min = lo[1];
    return Number.isFinite(min) && Number.isFinite(max) && max > min ? { min, max } : null;
  }

  /** True while the pointer is over a value gutter: the only time the scale controls show.
   *  Measured from the coordinates, not the event target, so moving onto a button that sits
   *  in the gutter does not count as leaving it. */
  let axisHover = $state(false);

  function onHostMove(e) {
    if (!chart || !el) return;
    const box = el.getBoundingClientRect();
    axisHover = hitZone(e.clientX - box.left, e.clientY - box.top) === 'yaxis';
  }

  function onPointerDown(e) {
    if (e.button !== 0 || !chart) return;
    const box = el.getBoundingClientRect();
    const x = e.clientX - box.left;
    const y = e.clientY - box.top;
    const zone = hitZone(x, y);
    if (zone === 'none') return;
    quickMenu = null;
    if (quick && !armed && zone === 'plot') startQuickPress(e);
    if (!armed) selectedDrawing = null; // a press on bare chart deselects, as in every editor
    host?.focus?.({ preventScroll: true });
    drag = {
      zone,
      x0: e.clientX,
      y0: e.clientY,
      win0: { ...win },
      pane: paneAt(y),
      // The plot drag needs the extent too: it slides the pane's scale as well as time.
      ext0: zone === 'yaxis' || zone === 'plot' ? paneExtent(paneAt(y)) : null,
      moved: false
    };
    if (zone === 'yaxis' && !drag.ext0) {
      drag = null;
      return;
    }
    el.setPointerCapture?.(e.pointerId);
    if (zone === 'plot') panning = true;
    e.preventDefault();
  }

  // A pointer stream is faster than the screen (120Hz trackpads, coalesced mouse moves), and
  // every event used to redraw the chart: the gesture ran ahead of the frames and read as a
  // stutter. Keep the last position, do the work once per frame.
  let moveRaf = 0;
  let pendingMove = null;

  function onPointerMove(e) {
    // A press that travels is a pan, never a trade.
    if (quickPress && (Math.abs(e.clientX - quickPress.x0) > CLICK_SLOP || Math.abs(e.clientY - quickPress.y0) > CLICK_SLOP))
      cancelQuickPress();
    if (!drag || !chart) return;
    pendingMove = { x: e.clientX, y: e.clientY };
    if (moveRaf) return;
    moveRaf = requestAnimationFrame(() => {
      moveRaf = 0;
      flushMove();
    });
  }

  function flushMove() {
    const p = pendingMove;
    pendingMove = null;
    if (!p || !drag || !chart) return;
    const dx = p.x - drag.x0;
    const dy = p.y - drag.y0;
    if (!drag.moved && Math.abs(dx) < 2 && Math.abs(dy) < 2) return;
    drag.moved = true;

    if (drag.zone === 'plot') {
      const r = rect(0);
      if (!r) return;
      const w = drag.win0.e - drag.win0.s;
      const shift = (-dx * w) / r.width; // 1:1 with the cursor
      setWin(drag.win0.s + shift, drag.win0.e + shift);
      // A press on the plot slides the chart, it never rescales it: the pane's extent is
      // pinned at the press and translated by the cursor, so the candles keep their size
      // whichever way the drag goes. Auto-fitting mid-pan is what made a sideways drag look
      // like a zoom. Sliding therefore hands the pane a manual scale, and the "auto" button
      // on the gutter is the way back, exactly as a gutter drag is.
      if (drag.ext0) {
        const ext = drag.ext0;
        const gi = drag.pane;
        const pr = rect(gi) ?? r;
        if (gi === 0 && priceLogNow()) {
          const lo = Math.log(Math.max(ext.min, Number.EPSILON));
          const hi = Math.log(Math.max(ext.max, Number.EPSILON * 2));
          const d = ((hi - lo) * dy) / pr.height;
          pushY(gi, { min: Math.exp(lo + d), max: Math.exp(hi + d) });
        } else {
          const d = ((ext.max - ext.min) * dy) / pr.height;
          pushY(gi, { min: ext.min + d, max: ext.max + d });
        }
      }
    } else if (drag.zone === 'xaxis') {
      // Drag the date strip to zoom, right edge pinned — the market-tool convention: the
      // grabbed date follows the cursor, so pulling left away from the pinned edge stretches
      // the bars (zoom in) and pushing right compresses them (zoom out).
      const r = rect(0);
      if (!r) return;
      const f = Math.exp(dx / Math.max(80, r.width / 4));
      const w = (drag.win0.e - drag.win0.s) * f;
      setWin(drag.win0.e - w, drag.win0.e);
    } else if (drag.zone === 'yaxis') {
      // Drag a value gutter to scale that pane manually; its centre stays put.
      const gi = drag.pane;
      const r = rect(gi);
      if (!r) return;
      const f = Math.exp(dy / Math.max(80, r.height / 2));
      const ext = drag.ext0;
      if (gi === 0 && priceLogNow()) {
        const lo = Math.log(Math.max(ext.min, Number.EPSILON));
        const hi = Math.log(Math.max(ext.max, Number.EPSILON * 2));
        const mid = (lo + hi) / 2;
        const half = ((hi - lo) / 2) * f;
        pushY(gi, { min: Math.exp(mid - half), max: Math.exp(mid + half) });
      } else {
        const mid = (ext.min + ext.max) / 2;
        const half = ((ext.max - ext.min) / 2) * f;
        pushY(gi, { min: mid - half, max: mid + half });
      }
    }
  }

  function onPointerUp(e) {
    // The last move may still be queued for the next frame: land it before reading `moved`.
    if (moveRaf) {
      cancelAnimationFrame(moveRaf);
      moveRaf = 0;
      flushMove();
    }
    // Short press, no travel: one click on the chart = one entry or exit.
    if (quickPress) {
      const p = quickPress.p;
      const moved = !!drag?.moved;
      cancelQuickPress();
      if (!moved) onquickclick?.(p);
    }
    if (!drag) return;
    el?.releasePointerCapture?.(e.pointerId);
    drag = null;
    panning = false;
    commitY();
  }

  function onWheel(e) {
    if (!chart || !n) return;
    const box = el.getBoundingClientRect();
    const x = e.clientX - box.left;
    const y = e.clientY - box.top;
    if (hitZone(x, y) === 'none') return;
    e.preventDefault();

    // Normalize across mice (lines), pages, and trackpad pixel deltas.
    const scale = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 100 : 1;
    const dy = e.deltaY * scale;
    const dx = e.deltaX * scale;

    // Trackpad horizontal swipe or shift+wheel pans; everything else zooms at the cursor.
    if (!e.ctrlKey && (e.shiftKey || Math.abs(dx) > Math.abs(dy))) {
      const r = rect(0);
      if (!r) return;
      const w = win.e - win.s;
      const src = e.shiftKey && Math.abs(dx) < Math.abs(dy) ? dy : dx;
      const shift = (src * w) / r.width;
      setWin(win.s + shift, win.e + shift);
      return;
    }

    const r = rect(0);
    if (!r) return;
    const frac = Math.min(1, Math.max(0, (x - r.x) / r.width));
    const w = win.e - win.s;
    const anchor = win.s + frac * w;
    // One event = one gentle step. Raw deltas differ by an order of magnitude between a
    // mouse notch (~100) and a trackpad tick (~5), so normalize to a unit step, floor it
    // (a trackpad would otherwise barely move) and cap it (a flick must not jump several
    // zoom levels). ctrlKey = pinch, which arrives with larger deltas.
    const unit =
      Math.sign(dy) *
      Math.max(ZOOM_MIN_UNIT, Math.min(Math.abs(dy) / (e.ctrlKey ? 30 : 50), 1));
    const f = Math.exp(unit * ZOOM_STEP);
    const w2 = Math.min(Math.max(w * f, MIN_BARS), total - 1);
    setWin(anchor - frac * w2, anchor - frac * w2 + w2);
  }

  function onDblClick(e) {
    if (!chart) return;
    const box = el.getBoundingClientRect();
    const zone = hitZone(e.clientX - box.left, e.clientY - box.top);
    if (zone === 'yaxis') clearY(paneAt(e.clientY - box.top)); // that pane back to auto-fit
    else if (zone !== 'none') zoomFit();
  }

  function onKeyDown(e) {
    if (!chart || !n) return;
    // Copy, paste and duplicate, on the keys every editor uses. There is no text to copy on a
    // chart, so taking these over costs the user nothing.
    if (e.metaKey || e.ctrlKey) {
      const k = e.key.toLowerCase();
      if (k === 'c' && selectedDrawing) {
        copySelected();
        e.preventDefault();
        return;
      }
      if (k === 'v' && canPaste()) {
        pasteCopied();
        e.preventDefault();
        return;
      }
      if (k === 'd' && selectedDrawing) {
        if (copySelected()) pasteCopied();
        e.preventDefault();
        return;
      }
    }
    if (e.key === 'Delete' || e.key === 'Backspace') {
      if (!selectedDrawing) return;
      removeSelected();
      e.preventDefault();
      return;
    }
    if (e.key === 'Escape' && quickMenu) {
      quickMenu = null;
      e.preventDefault();
      return;
    }
    if (e.key === 'Escape' && (draft || selectedDrawing || armed)) {
      draft = null;
      selectedDrawing = null;
      guides = null;
      tool = 'cursor';
      updateDrawings();
      e.preventDefault();
      return;
    }
    const w = win.e - win.s;
    const step = w * (e.shiftKey ? 0.25 : 0.05);
    switch (e.key) {
      case 'ArrowLeft':
        setWin(win.s - step, win.e - step);
        break;
      case 'ArrowRight':
        setWin(win.s + step, win.e + step);
        break;
      case '+':
      case '=':
        zoomBy(1 / 1.25);
        break;
      case '-':
      case '_':
        zoomBy(1.25);
        break;
      case 'Home':
        goStart();
        break;
      case 'End':
        goEnd();
        break;
      case '0':
        zoomFit();
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  // Zoom about the window centre (keyboard / buttons; the wheel anchors at the cursor).
  function zoomBy(f) {
    const w = win.e - win.s;
    const mid = (win.s + win.e) / 2;
    const w2 = Math.min(Math.max(w * f, MIN_BARS), total - 1);
    setWin(mid - w2 / 2, mid + w2 / 2);
  }
  function zoomFit() {
    yMan = {};
    win = defaultWin();
  }
  function goStart() {
    const w = win.e - win.s;
    setWin(0, w);
  }
  function goEnd() {
    const w = win.e - win.s;
    setWin(total - 1 - w, total - 1);
  }

  // ────────────────────────── Crosshair readout ──────────────────────────

  // Price precision follows the instrument, not the magnitude (see format.js): the same
  // decimals everywhere on the chart — tags, legend, drawings, trade markers.
  const priceDigits = $derived(priceDigitsOf(view));
  const fmtPrice = $derived(makePriceFmt(priceDigits));
  // Axis labels sit on round tick values in a narrow gutter, so they keep the instrument's
  // precision as a *ceiling* and drop trailing zeros — a tag reading 63 060.00 lines up under
  // a gridline labelled 63 000, and a sub-cent instrument still gets all its decimals.
  const fmtAxisPrice = $derived(makeAxisFmt(priceDigits));
  const fmtVal = (x) =>
    Math.abs(x) >= 1000 ? x.toLocaleString(undefined, { maximumFractionDigits: 2 }) : x.toPrecision(5);
  const fmtQty = (x) => {
    const a = Math.abs(x);
    if (a >= 1e9) return `${(x / 1e9).toFixed(2)}B`;
    if (a >= 1e6) return `${(x / 1e6).toFixed(2)}M`;
    if (a >= 1e3) return `${(x / 1e3).toFixed(1)}K`;
    return x.toFixed(0);
  };

  // Bar the legend reads from: the crosshair when hovering, otherwise the newest visible bar
  // (the same fallback every market tool uses so the readout is never blank).
  const readIdx = $derived.by(() => {
    if (!n) return null;
    if (hoverIdx != null && hoverIdx >= 0 && hoverIdx < n) return hoverIdx;
    return Math.min(n - 1, Math.max(0, Math.floor(win.e)));
  });

  const ohlc = $derived.by(() => {
    const i = readIdx;
    if (i == null || !view) return null;
    const o = view.o[i];
    const c = view.c[i];
    // Change against the previous close, like every quote screen — the open-to-close move is
    // already visible in the candle itself.
    const ref = i > 0 ? view.c[i - 1] : o;
    const delta = c - ref;
    const chg = ref ? (delta / ref) * 100 : 0;
    return {
      o: fmtPrice(o),
      h: fmtPrice(view.h[i]),
      l: fmtPrice(view.l[i]),
      c: fmtPrice(c),
      delta: `${delta >= 0 ? '+' : ''}${fmtPrice(delta)}`,
      chg: `${chg >= 0 ? '+' : ''}${chg.toFixed(2)}%`,
      up: delta >= 0,
      ts: fmtTs(view.ts[i])
    };
  });

  function paletteColor(key) {
    return palette[key] ?? palette.accent;
  }

  // ── In-chart legend model (HTML overlay, replaces the ECharts legend) ──
  // One row per series: swatch + label + its value at `readIdx`. Price/Volume lead. All
  // instances are listed (visible + hidden) so a hidden one can be clicked back on.
  const legend = $derived.by(() => {
    const i = readIdx;
    const rows = [
      {
        ...(type === 'line'
          ? { label: $t('histviz.chart.close'), color: palette.accent, icon: 'line' }
          : { label: $t('histviz.chart.price'), color: palette.up, color2: palette.down, icon: 'candle' }),
        builtin: 'price',
        hidden: !priceVisible
      },
      {
        label: $t('histviz.chart.volume'),
        color: palette.up,
        color2: palette.down,
        icon: 'bar',
        builtin: 'volume',
        hidden: !volumeVisible,
        value: i != null && view ? fmtQty(view.v[i] ?? 0) : ''
      }
    ];
    for (const ind of instances) {
      const outs = computed.get(ind.id) ?? [];
      const out = outs[0];
      const swatch = ind.style?.color || (out?.color === 'updown' ? palette.up : paletteColor(out?.color));
      // Multi-output indicators (MACD, Bollinger…) read out every line, space permitting.
      // An overlay sits on the price scale, so it is read out with the price precision.
      const onPrice = paneOf(ind) === 'overlay' && !isCustom(ind);
      const vals = [];
      if (i != null) {
        for (const o of outs) {
          const x = o.data?.[i];
          if (x != null && Number.isFinite(x)) vals.push(onPrice ? fmtPrice(x) : fmtVal(x));
        }
      }
      rows.push({
        id: ind.id,
        label: isCustom(ind) ? ind.custom?.name || '' : instanceLabel(ind.type, ind.params),
        color: swatch,
        icon: out?.kind === 'scatter' ? 'dot' : out?.kind === 'bar' ? 'bar' : 'line',
        dashed: !!out?.dashed,
        hidden: !ind.visible,
        value: vals.join(' '),
        // Which pane the row belongs to: null = the price pane (overlays), otherwise the
        // sub-pane's key. A title belongs over the pane that draws the series — and a hidden
        // one has no pane, so it falls back to the price header where it can be switched
        // back on instead of disappearing with its pane.
        pane: paneOf(ind) === 'oscillator' && ind.visible ? paneKey(ind) : null
      });
    }
    return rows;
  });

  // Legend row click: toggle a built-in series locally, or an indicator via the parent.
  function toggleRow(row) {
    if (row.builtin === 'price') priceVisible = !priceVisible;
    else if (row.builtin === 'volume') volumeVisible = !volumeVisible;
    else if (row.id != null) ontoggle?.(row.id);
  }

  // ── Custom crosshair value tags (right-edge vignettes, one per visible value series) ──
  // Built from the same outputs as the chart series so colors/labels match. `gridIndex`
  // tells us which pane's y-axis to convert against.
  const tagSeries = $derived.by(() => {
    if (!view) return [];
    // No entry for the price itself: the scale shows the price *under the cursor*
    // (`cursorTag`), not the close of the bar being hovered — that is the market-tool
    // behaviour, and it is what makes the scale readable while measuring.
    const out = [];
    for (const ind of instances) {
      if (!ind.visible) continue;
      // Overlays live on pane 0; everything else on its sub-pane (2 + position).
      let gridIndex = 0;
      if (paneOf(ind) === 'oscillator') {
        const pos = oscPanes.indexOf(paneKey(ind));
        if (pos < 0) continue;
        gridIndex = 1 + pos;
      }
      for (const o of computed.get(ind.id) ?? []) {
        if (o.kind === 'bar' || o.kind === 'scatter') continue; // tag lines only
        out.push({
          label: o.name,
          color: ind.style?.color || (o.color === 'updown' ? palette.up : paletteColor(o.color)),
          data: o.data,
          gridIndex,
          // A custom overlay hangs on the price pane's hidden second scale, so its tag is
          // positioned against that axis and formatted as a plain value, not as a price.
          custom: isCustom(ind),
          onPrice: gridIndex === 0 && !isCustom(ind)
        });
      }
    }
    return out;
  });

  // convertToPixel per series per mousemove was the other hot path — coalesce into one frame.
  let tagRaf = 0;
  let tagIdx = null;
  // Last pointer position over the chart, in chart pixels. A live bar rebuilds the option,
  // which drops ECharts' axis pointer — replaying this position puts the crosshair (and our
  // tags) straight back instead of waiting for the user to jiggle the mouse.
  let lastPointer = null;
  let cursorTag = $state(null); // { top, text } — the value under the cursor, on the scale
  let cursorRaf = 0;
  function requestTags(dataIndex) {
    tagIdx = dataIndex;
    if (tagRaf) return;
    tagRaf = requestAnimationFrame(() => {
      tagRaf = 0;
      updateTags(tagIdx);
    });
  }

  function updateTags(dataIndex) {
    if (!chart || dataIndex == null || settings.crosshair === false || settings.crosshairTags === false) {
      tags = [];
      return;
    }
    const next = [];
    for (const s of tagSeries) {
      const val = s.data[dataIndex];
      if (val == null || !Number.isFinite(val)) continue;
      const target = s.custom
        ? { xAxisIndex: 0, yAxisIndex: customAxisIndex() }
        : { gridIndex: s.gridIndex };
      const px = chart.convertToPixel(target, [dataIndex, val]);
      if (!px) continue;
      next.push({ label: s.label, color: s.color, top: px[1], value: s.onPrice ? fmtPrice(val) : fmtVal(val) });
    }
    tags = next;
  }

  function color(key, p, fallback) {
    return p[key] ?? fallback ?? p.accent;
  }

  /** The scale label that rides the crosshair: the value at the cursor's own pixel, on the
   *  pane it is over — a price on the price pane, the pane's own unit below. It is *not* the
   *  hovered bar's close: the point of the label is to read the level you are pointing at. */
  function requestCursorTag() {
    if (cursorRaf) return;
    cursorRaf = requestAnimationFrame(() => {
      cursorRaf = 0;
      updateCursorTag();
    });
  }

  function updateCursorTag() {
    if (!chart || !lastPointer || settings.crosshair === false || settings.crosshairTags === false) {
      cursorTag = null;
      return;
    }
    const y = lastPointer.y;
    for (let i = 0; i < gridLayout.length; i++) {
      const r = rect(i);
      if (!r || y < r.y || y > r.y + r.height) continue;
      const val = chart.convertFromPixel({ gridIndex: i }, [0, y])?.[1];
      if (val == null || !Number.isFinite(val)) break;
      cursorTag = { top: y, text: i === 0 ? fmtPrice(val) : fmtVal(val) };
      return;
    }
    cursorTag = null;
  }

  /** Put the crosshair back where the cursor is. Rebuilding the option (a live bar, a new
   *  indicator, a settings change) drops ECharts' axis pointer; without this it only comes
   *  back on the next mouse move, which on a live chart means it blinks every few seconds. */
  function restoreCrosshair() {
    if (!chart || !lastPointer || settings.crosshair === false) return;
    chart.dispatchAction({
      type: 'updateAxisPointer',
      currTrigger: 'mousemove',
      x: lastPointer.x,
      y: lastPointer.y
    });
  }

  // ────────────────────────── Price-axis range ──────────────────────────

  // The log axis applies to the price pane only, and only when every low is positive.
  function priceLogNow() {
    return settings.scale === 'log' && type !== 'renko' && !!view && view.l.every((x) => x > 0);
  }

  /** Top of the hidden volume scale: 4× the biggest bar in view, so volume occupies the
   *  bottom quarter of the price pane and never climbs into the candles. */
  function visibleVolumeMax(v) {
    if (!v?.v?.length) return 1;
    const { from, to } = dataWin();
    let m = 0;
    for (let k = from; k <= to; k++) if (v.v[k] > m) m = v.v[k];
    return m > 0 ? m * 4 : 1;
  }

  // Log range fitted to the *visible* window, so a zoomed-in slice fills the pane instead of
  // being compressed against the full-history extent. A log axis does not honor `scale`.
  function visibleLogRange(v) {
    if (!v) return null;
    const { from, to } = dataWin();
    let lo = Infinity;
    let hi = -Infinity;
    for (let k = from; k <= to; k++) {
      if (v.l[k] < lo) lo = v.l[k];
      if (v.h[k] > hi) hi = v.h[k];
    }
    if (!Number.isFinite(lo) || !Number.isFinite(hi) || lo <= 0) {
      lo = Math.min(...v.l);
      hi = Math.max(...v.h);
    }
    if (!(lo > 0) || !(hi > lo)) return null;
    return { min: lo / 1.01, max: hi * 1.01 };
  }

  /** One candle, cut to the price range the pane actually shows. `lim` null = auto-fit, in
   *  which case the box is already inside and the values pass through untouched. */
  const OUT = ['-', '-', '-', '-']; // ECharts skips it
  function clipCandle(o, c, l, h, lim) {
    if (!lim) return [o, c, l, h];
    if (!(h >= lim.min) || !(l <= lim.max)) return OUT; // wholly above/below, or a gap
    const k = (x) => (x < lim.min ? lim.min : x > lim.max ? lim.max : x);
    let co = k(o);
    let cc = k(c);
    // A body cut flat still has a direction: keep open ≠ close or the candle changes colour.
    if (co === cc && o !== c) {
      const eps = (lim.max - lim.min) * 1e-6;
      if (o > c) cc = co - eps;
      else co = cc - eps;
    }
    return [co, cc, k(l), k(h)];
  }

  // Latest close, pinned on the price axis where it sits right now. Recomputed with the
  // other pixel overlays: the value never changes with the window, its height does.
  function updateLastTag() {
    const v = view;
    if (!chart || !v || !n || settings.lastPrice === false) {
      lastTag = null;
      return;
    }
    const last = v.c[n - 1];
    const prev = n > 1 ? v.c[n - 2] : last;
    if (!Number.isFinite(last)) {
      lastTag = null;
      return;
    }
    const px = chart.convertToPixel({ gridIndex: 0 }, [n - 1, last]);
    const r = rect(0);
    if (!px || !r) {
      lastTag = null;
      return;
    }
    const top = px[1];
    // Outside the price pane (the window is scrolled far from the last bar's level): a tag
    // clamped to the edge would point at a price that isn't there.
    if (top < r.y || top > r.y + r.height) {
      lastTag = null;
      return;
    }
    lastTag = {
      top,
      text: fmtPrice(last),
      color: last >= prev ? palette.up : palette.down
    };
  }

  // Recompute the log-scale left-axis labels/gridlines from the current pixel geometry.
  function updateLogGrid() {
    if (!chart || !priceLogNow() || !logRange.max) {
      logGrid = [];
      return;
    }
    const lgMin = Math.log(logRange.min);
    const lgMax = Math.log(logRange.max);
    const steps = 6;
    const out = [];
    for (let k = 0; k <= steps; k++) {
      const val = Math.exp(lgMin + ((lgMax - lgMin) * k) / steps);
      const px = chart.convertToPixel({ gridIndex: 0 }, [0, val]);
      if (px) out.push({ top: px[1], text: fmtAxisPrice(val) });
    }
    logGrid = out;
  }

  // ────────────────────────── Drawings ──────────────────────────
  //
  // Objects are stored in time+price (drawings.js) and projected here, exactly like the value
  // tags: every geometry change re-runs `updateDrawings`. The SVG layer is inert
  // (`pointer-events: none`) so panning still works through it; only the shapes themselves —
  // and the whole layer while a tool is armed — take the pointer.

  // Projection is a *pure* read of (drawings, window, chart geometry) — writing it from an
  // effect meant the same effect both read and wrote state, which Svelte rightly refuses.
  // `paintTick` is the one signal the geometry hooks bump; everything else derives.
  let paintTick = $state(0);
  let draft = $state(null); // two-point drawing being dragged out
  let dragging = null; // { id, mode: 'move' | 'a' | 'b', from, orig }
  let editing = $state(null); // { id, value } — the text tool's inline input
  let noteInput = $state(null);
  let svgEl = $state(null);
  // Alignment guides: the anchor being placed lined up with an anchor already on the chart.
  // Stored in data coordinates like everything else, projected in `guideLines`.
  let guides = $state(null); // { x: ms|null, y: price|null }
  let tplOpen = $state(false);
  let tplName = $state('');

  /** What the objects on this chart belong to. Two panes on one instrument share a board, so
   *  pasting between them must not offset; pasting onto another instrument must not either. */
  const instrumentKey = $derived(title ? `${title.provider}|${title.ticker}` : '');

  const armed = $derived(tool !== 'cursor');
  const clipId = `dclip-${Math.random().toString(36).slice(2, 8)}`;

  const tsMs = (i) => Date.parse(view.ts[Math.min(n - 1, Math.max(0, i))]);

  /** Bar index for an instant: interpolated between bars, extrapolated past either end so a
   *  drawing pinned in the right margin (or before the loaded history) still lands. */
  function idxFromMs(ms) {
    if (!view || !n) return 0;
    const t0 = tsMs(0);
    if (ms <= t0) return (ms - t0) / stepMs;
    const tl = tsMs(n - 1);
    if (ms >= tl) return n - 1 + (ms - tl) / stepMs;
    let lo = 0;
    let hi = n - 1;
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1;
      if (tsMs(mid) <= ms) lo = mid;
      else hi = mid;
    }
    const span = tsMs(hi) - tsMs(lo) || stepMs;
    return lo + (ms - tsMs(lo)) / span;
  }
  function msFromIdx(idx) {
    if (!view || !n) return 0;
    if (idx <= 0) return tsMs(0) + idx * stepMs;
    if (idx >= n - 1) return tsMs(n - 1) + (idx - (n - 1)) * stepMs;
    const lo = Math.floor(idx);
    return tsMs(lo) + (idx - lo) * (tsMs(lo + 1) - tsMs(lo));
  }

  // The window ECharts actually draws: a category axis parses `startValue`/`endValue` through
  // `Math.round`, while `win` keeps the fractional bounds a wheel zoom or a trackpad pan leaves
  // behind. Projecting against the fractional pair puts every overlay (drawings, quick-backtest
  // markers) up to half a bar beside its candle, and lands a click on the neighbouring bar.
  function drawWin() {
    return { s: Math.round(win.s), e: Math.round(win.e) };
  }

  // The category axis lays band `i` out centred in the visible span — the same arithmetic the
  // window plumbing already uses, so no ECharts internals are needed for x.
  function xPix(idx) {
    const r = rect(0);
    if (!r) return null;
    const w = drawWin();
    const span = w.e - w.s + 1;
    return r.x + ((idx - w.s + 0.5) / span) * r.width;
  }
  function xIdx(px) {
    const r = rect(0);
    if (!r) return 0;
    const w = drawWin();
    const span = w.e - w.s + 1;
    return w.s + ((px - r.x) / r.width) * span - 0.5;
  }
  const yPix = (price) => chart?.convertToPixel({ gridIndex: 0 }, [0, price])?.[1] ?? null;
  const yPrice = (py) => chart?.convertFromPixel({ gridIndex: 0 }, [0, py])?.[1] ?? null;

  /** Pointer event → data anchor. With the magnet on, the anchor lands on the nearest
   *  open/high/low/close of the bar under the cursor instead of a pixel between candles. */
  function dataAt(e) {
    const box = el.getBoundingClientRect();
    const idx = xIdx(e.clientX - box.left);
    const y = yPrice(e.clientY - box.top);
    if (!magnet || !view || !n || y == null) return { x: msFromIdx(idx), y };
    const i = Math.min(n - 1, Math.max(0, Math.round(idx)));
    const near = [view.o[i], view.h[i], view.l[i], view.c[i]].reduce((best, p) =>
      Math.abs(p - y) < Math.abs(best - y) ? p : best
    );
    return { x: tsMs(i), y: near };
  }

  /** Pixels within which an anchor lines up with one already on the chart. Tight enough
   *  that a deliberate placement is never moved, wide enough that a near miss is caught. */
  const SNAP_PX = 6;


  /** Anchors the one being placed can line up with: every other object's, plus the first
   *  anchor of the shape currently being dragged out (that is what makes a flat rectangle or
   *  a horizontal trend line land flat). */
  function alignTargets(skipId) {
    const out = [];
    for (const d of drawings) {
      if (d.id === skipId || d.hidden) continue;
      if (d.a) out.push(d.a);
      if (d.b) out.push(d.b);
      if (d.stop != null && d.a) out.push({ x: d.a.x, y: d.stop });
    }
    if (draft?.a && skipId !== '__draft') out.push(draft.a);
    return out;
  }

  /** Snap an anchor onto the nearest existing one and remember the guide to draw. */
  function snapToObjects(p, skipId) {
    const px = xPix(idxFromMs(p.x));
    const py = yPix(p.y);
    if (px == null || py == null) {
      guides = null;
      return p;
    }
    const hit = nearestAnchor(
      px,
      py,
      alignTargets(skipId).map((t) => ({
        x: t.x,
        y: t.y,
        px: xPix(idxFromMs(t.x)),
        py: yPix(t.y)
      })),
      SNAP_PX
    );
    guides = hit.x == null && hit.y == null ? null : hit;
    return { x: hit.x ?? p.x, y: hit.y ?? p.y };
  }

  /** Where an anchor lands: the magnet's bar price first, then the objects already drawn. */
  const anchorAt = (e, skipId) => snapToObjects(dataAt(e), skipId);

  /** Elapsed time, written the way a trader says it: "3d 4h", "45m". */
  function fmtSpan(ms) {
    const m = Math.round(ms / 60000);
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    if (h < 24) return m % 60 ? `${h}h ${m % 60}m` : `${h}h`;
    const days = Math.floor(h / 24);
    return h % 24 ? `${days}d ${h % 24}h` : `${days}d`;
  }

  function project(d, r) {
    const color = d.style?.color || palette.accent;
    const width = d.style?.width || 1.4;
    const ax = xPix(idxFromMs(d.a.x));
    const ay = yPix(d.a.y);
    if (ax == null || ay == null) return null;
    const bx = d.b ? xPix(idxFromMs(d.b.x)) : null;
    const by = d.b ? yPix(d.b.y) : null;
    const base = { id: d.id, tool: d.tool, color, width, sel: d.id === selectedDrawing, ax, ay, bx, by };
    if (d.tool === 'hline') return { ...base, x1: r.x, x2: r.x + r.width, label: fmtPrice(d.a.y) };
    if (d.tool === 'vline') return { ...base, y1: r.y, y2: r.y + r.height };
    if (d.tool === 'text') return { ...base, text: d.text ?? '' };
    if (d.tool === 'rect' && bx != null) {
      return {
        ...base,
        rx: Math.min(ax, bx),
        ry: Math.min(ay, by),
        rw: Math.abs(bx - ax),
        rh: Math.abs(by - ay)
      };
    }
    // Position tools: an entry line with a target box above (green) and a stop box below
    // (red), plus what a trader actually reads off it — the two distances and the R:R.
    if ((d.tool === 'long' || d.tool === 'short') && bx != null && d.stop != null) {
      const x1 = Math.min(ax, bx);
      const x2 = Math.max(ax, bx);
      const sy = yPix(d.stop);
      const reward = Math.abs(d.b.y - d.a.y);
      const risk = Math.abs(d.stop - d.a.y);
      const pct = (v) => (d.a.y ? ((v - d.a.y) / d.a.y) * 100 : 0);
      return {
        ...base,
        x1,
        x2,
        sy,
        entryLabel: fmtPrice(d.a.y),
        target: {
          y1: Math.min(ay, by),
          y2: Math.max(ay, by),
          label: `${fmtPrice(d.b.y)} (${pct(d.b.y) >= 0 ? '+' : ''}${pct(d.b.y).toFixed(2)}%)`
        },
        stopBox: {
          y1: Math.min(ay, sy),
          y2: Math.max(ay, sy),
          label: `${fmtPrice(d.stop)} (${pct(d.stop) >= 0 ? '+' : ''}${pct(d.stop).toFixed(2)}%)`
        },
        rr: risk > 0 ? `R:R ${(reward / risk).toFixed(2)}` : 'R:R —'
      };
    }
    // Ruler: what changed between two points — price, percent, bars and elapsed time.
    if (d.tool === 'ruler' && bx != null) {
      const dy = d.b.y - d.a.y;
      const pct = d.a.y ? (dy / d.a.y) * 100 : 0;
      const barCount = Math.round(Math.abs(idxFromMs(d.b.x) - idxFromMs(d.a.x)));
      return {
        ...base,
        rx: Math.min(ax, bx),
        ry: Math.min(ay, by),
        rw: Math.abs(bx - ax),
        rh: Math.abs(by - ay),
        up: dy >= 0,
        tint: dy >= 0 ? palette.up : palette.down,
        label: `${dy >= 0 ? '+' : ''}${fmtPrice(dy)}  (${pct >= 0 ? '+' : ''}${pct.toFixed(2)}%)`,
        sub: `${barCount} ${$t('histviz.draw.bars')} · ${fmtSpan(Math.abs(d.b.x - d.a.x))}`
      };
    }
    if (d.tool === 'fib' && bx != null) {
      const x1 = Math.min(ax, bx);
      const x2 = Math.max(ax, bx);
      const levels = FIB_LEVELS.map((lv) => ({
        lv,
        y: ay + (by - ay) * lv,
        price: fmtPrice(d.a.y + (d.b.y - d.a.y) * lv)
      }));
      return { ...base, x1, x2, levels };
    }
    return base;
  }

  /** Ask for a repaint: the pixels changed under the same data (pan, zoom, resize, re-render). */
  function updateDrawings() {
    paintTick++;
  }

  const clip = $derived.by(() => {
    void paintTick;
    return chart && view ? rect(0) : null;
  });

  const shapes = $derived.by(() => {
    void paintTick;
    const r = clip;
    if (!r || drawingsHidden) return [];
    // An object hidden from the tree is not drawn; a locked one is drawn and left alone.
    const visible = drawings.filter((d) => !d.hidden);
    const list = draft ? [...visible, { ...draft, id: '__draft' }] : visible;
    return list.map((d) => project(d, r)).filter(Boolean);
  });

  /** The guides in pixels, re-projected each frame like every other overlay. */
  const guideLines = $derived.by(() => {
    void paintTick;
    const r = clip;
    if (!guides || !r) return null;
    return {
      x: guides.x == null ? null : xPix(idxFromMs(guides.x)),
      y: guides.y == null ? null : yPix(guides.y)
    };
  });

  /** Style bar for the selected object: presets + a free colour, right above its anchor. */
  const STYLE_COLORS = $derived([
    ...new Set([palette.accent, palette.up, palette.down, palette.amber, palette.text])
  ]);
  const selShape = $derived(shapes.find((s) => s.sel && s.id !== '__draft') ?? null);
  const styleBar = $derived.by(() => {
    if (!selShape || !clip || armed) return null;
    const xs = [selShape.ax, selShape.bx].filter((v) => v != null);
    const ys = [selShape.ay, selShape.by].filter((v) => v != null);
    const left = Math.min(Math.max(clip.x, Math.min(...xs)), clip.x + clip.width - 190);
    const top = Math.max(clip.y + 4, Math.min(...ys) - 38);
    return { left, top };
  });

  function styleSelected(patch) {
    if (!selectedDrawing) return;
    drawings = drawings.map((d) =>
      d.id === selectedDrawing ? { ...d, style: { ...d.style, ...patch } } : d
    );
  }

  // ── Templates ──
  // A named style, kept in the *global* chart settings next to the colours: a trader draws
  // their supports the same way on every instrument, so a template that lived per chart
  // would have to be rebuilt on each one.

  const templates = $derived(
    Array.isArray(settings.drawTemplates) ? settings.drawTemplates : []
  );
  /** Style a new object starts with: the user's default when they set one. */
  const newStyle = () => ({ ...DEFAULT_DRAW_STYLE, ...(settings.drawDefault ?? {}) });
  const selectedObj = $derived(drawings.find((d) => d.id === selectedDrawing) ?? null);

  function saveTemplate() {
    if (!selectedObj) return;
    const name = tplName.trim() || $t(drawTool(selectedObj.tool).labelKey);
    const tpl = {
      id: `t${Date.now().toString(36)}`,
      name,
      style: { ...DEFAULT_DRAW_STYLE, ...(selectedObj.style ?? {}) }
    };
    onsettings?.({ drawTemplates: [...templates, tpl].slice(-12) });
    tplName = '';
  }

  const removeTemplate = (id) =>
    onsettings?.({ drawTemplates: templates.filter((x) => x.id !== id) });

  /** Make the selection's style the one every new object starts with. */
  function templateAsDefault() {
    if (!selectedObj) return;
    onsettings?.({ drawDefault: { ...DEFAULT_DRAW_STYLE, ...(selectedObj.style ?? {}) } });
    tplOpen = false;
  }

  // ── Copy / paste ──

  function copySelected() {
    if (!selectedObj) return false;
    return copyDrawing($state.snapshot(selectedObj), instrumentKey);
  }

  function pasteCopied() {
    const d = pasteDrawing(instrumentKey, stepMs * 3);
    if (!d) return false;
    drawings = [...drawings, d];
    selectedDrawing = d.id;
    updateDrawings();
    return true;
  }

  /** Where the inline text input sits: read from the projection, never stored. */
  const notePos = $derived.by(() => {
    const s = editing ? shapes.find((x) => x.id === editing.id) : null;
    return s ? { left: s.ax, top: s.ay } : null;
  });

  function commit(d) {
    drawings = [...drawings, d];
    selectedDrawing = d.id;
    tool = 'cursor';
  }

  function removeSelected() {
    if (!selectedDrawing) return;
    drawings = drawings.filter((d) => d.id !== selectedDrawing);
    selectedDrawing = null;
    editing = null;
    updateDrawings();
  }

  function onDrawDown(e) {
    if (e.button !== 0 || !armed || !chart) return;
    e.stopPropagation();
    e.preventDefault();
    host?.focus?.({ preventScroll: true }); // keep Delete/Escape working after a draw
    const p = anchorAt(e, null);
    if (p.y == null) return;
    if (drawTool(tool).points === 1) {
      const d = makeDrawing(tool, p, null, {
        ...(tool === 'text' ? { text: '' } : {}),
        style: newStyle()
      });
      commit(d);
      updateDrawings();
      if (d.tool === 'text') {
        editing = { id: d.id, value: '' };
        // `autofocus` on a node inserted into an already-focused document is unreliable, and
        // an effect that focuses would re-run on every repaint; one frame after the mount is
        // both deterministic and one-shot.
        requestAnimationFrame(() => noteInput?.focus());
      }
      return;
    }
    draft = { tool, a: p, b: p, style: newStyle() };
    svgEl?.setPointerCapture?.(e.pointerId);
    updateDrawings();
  }

  function onDrawMove(e) {
    if (draft) {
      const p = anchorAt(e, '__draft');
      if (p.y == null) return;
      draft = { ...draft, b: p };
      updateDrawings();
      return;
    }
    if (!dragging) return;
    // An anchor being dragged snaps onto the others; a whole shape being moved does not, as
    // there the pointer is a grab point and not the thing that has to line up.
    const p = dragging.mode === 'move' ? dataAt(e) : anchorAt(e, dragging.id);
    if (p.y == null) return;
    const next =
      dragging.mode === 'move'
        ? translate(dragging.orig, p.x - dragging.from.x, p.y - dragging.from.y)
        : moveAnchor(dragging.orig, dragging.mode, p);
    drawings = drawings.map((d) => (d.id === dragging.id ? next : d));
    updateDrawings();
  }

  function onDrawUp(e) {
    if (draft) {
      const d = draft;
      draft = null;
      // A stray click while a two-point tool is armed would otherwise leave a zero-size object.
      const far =
        Math.abs(xPix(idxFromMs(d.b.x)) - xPix(idxFromMs(d.a.x))) > 3 ||
        Math.abs(yPix(d.b.y) - yPix(d.a.y)) > 3;
      if (far) commit(makeDrawing(d.tool, d.a, d.b, { style: newStyle() }));
      else tool = 'cursor';
      updateDrawings();
    }
    if (dragging) dragging = null;
    guides = null;
    svgEl?.releasePointerCapture?.(e.pointerId);
  }

  /** Cursor mode: press a shape to select and move it, press a handle to move that anchor. */
  function startDrag(e, id, mode) {
    if (armed || e.button !== 0) return;
    // A locked object still selects (so it can be unlocked from the tree) but never moves.
    if (drawings.find((d) => d.id === id)?.locked) {
      e.stopPropagation();
      selectedDrawing = id;
      return;
    }
    e.stopPropagation();
    e.preventDefault();
    host?.focus?.({ preventScroll: true }); // ditto: selection without focus can't be deleted
    selectedDrawing = id;
    const orig = drawings.find((d) => d.id === id);
    if (!orig) return;
    dragging = { id, mode, from: dataAt(e), orig };
    svgEl?.setPointerCapture?.(e.pointerId);
    updateDrawings();
  }

  function commitText() {
    if (!editing) return;
    const { id, value } = editing;
    editing = null;
    const text = value.trim();
    if (!text) drawings = drawings.filter((d) => d.id !== id);
    else drawings = drawings.map((d) => (d.id === id ? { ...d, text } : d));
    updateDrawings();
  }

  // ────────────────────────── Quick backtest ──────────────────────────
  //
  // Two gestures on the same press: a click posts one fill, a long press opens the side/size
  // window. The chart only reports *where* — which bar and at what price — and paints the
  // markers the engine sends back; the position lives with the page.

  const LONG_PRESS_MS = 380;
  const CLICK_SLOP = 4; // px of travel that still counts as a click

  let quickPress = null;
  let quickTimer = 0;
  let quickMenu = $state(null); // { left, top, p } — the long-press window

  /** Pointer → fill anchor: the bar under the cursor, at a price inside that bar's range.
   *  Clicking above the wick would otherwise fill at a price the market never printed. */
  function quickPoint(e) {
    if (!view || !n) return null;
    const box = el.getBoundingClientRect();
    const y = yPrice(e.clientY - box.top);
    if (y == null) return null;
    const i = Math.min(n - 1, Math.max(0, Math.round(xIdx(e.clientX - box.left))));
    return { x: tsMs(i), y: Math.min(view.h[i], Math.max(view.l[i], y)) };
  }

  function startQuickPress(e) {
    const p = quickPoint(e);
    if (!p) return;
    const box = el.getBoundingClientRect();
    quickPress = { x0: e.clientX, y0: e.clientY, left: e.clientX - box.left, top: e.clientY - box.top, p };
    quickTimer = setTimeout(() => {
      quickTimer = 0;
      if (!quickPress) return;
      quickMenu = { left: quickPress.left, top: quickPress.top, p: quickPress.p };
      quickPress = null;
      drag = null; // the press became a window, not a pan
      panning = false;
    }, LONG_PRESS_MS);
  }

  function cancelQuickPress() {
    if (quickTimer) clearTimeout(quickTimer);
    quickTimer = 0;
    quickPress = null;
  }

  function quickSubmit(side) {
    const p = quickMenu?.p;
    quickMenu = null;
    if (!p) return;
    // The page owns the unit arithmetic — the chart only reports what was typed.
    onquickfill?.({ ...p, side, size: Number(quickSize) > 0 ? Number(quickSize) : 1 });
  }

  // A size sized in dollars comes back as 0.07865344051385237 units — printed raw next to a
  // marker that is a dozen pixels wide, it is a smear. Six decimals, trailing zeros dropped.
  // A marker is labelled in the user's own unit, not in the engine's. Sizing in contracts, a
  // fill of 1 contract holds 2 units when the contract is worth 2, and the label reads 1: the
  // multiplier belongs to the PnL, not to what was clicked. Sizing in notional it reads the
  // currency that fill was worth at its own price. A fill from an older session carries no
  // `lot` and is already in units.
  const markSize = (m) => formatSize(m.qty / (Number(m.lot) > 0 ? Number(m.lot) : 1));

  /** Marks in time order, so the visible slice can be found by binary search. A click-through
   *  session has a handful of them; a backtest hands over one pair per trade, which on a long
   *  run is six figures, and walking that list on every frame is what makes a chart stutter. */
  const sortedMarks = $derived(marks.length > 1 ? [...marks].sort((a, b) => a.x - b.x) : marks);
  /** Roughly one marker per bar is the point where a fill is still a shape you can aim at;
   *  past that the chart is ink, not information, so it says so and draws none until the
   *  window narrows. The floor keeps a handful readable at maximum zoom, the ceiling keeps a
   *  full-history view from painting thousands of triangles. */
  const markLimit = (visibleBars) => Math.max(40, Math.min(400, visibleBars));
  /** Sizes are only legible while the markers are far apart. */
  const MARK_LABELS = 30;

  /** Markers, projected like drawings: a triangle at the fill price, blue up for a long and
   *  red down for a short. Entries are solid, exits hollow. */
  const markView = $derived.by(() => {
    void paintTick;
    const r = clip;
    if (!r || !view || !sortedMarks.length) return { shapes: [], dense: false };
    // One bar of slack each side so a marker sitting on the edge is not clipped away.
    const t0 = msFromIdx(win.s - 1);
    const t1 = msFromIdx(win.e + 1);
    const lo = lowerBound(sortedMarks, t0);
    const hi = lowerBound(sortedMarks, t1);
    if (hi - lo > markLimit(win.e - win.s + 1)) return { shapes: [], dense: true };
    const labels = markSizes && hi - lo <= MARK_LABELS;
    const out = [];
    for (let i = lo; i < hi; i++) {
      const m = sortedMarks[i];
      const x = xPix(idxFromMs(m.x));
      const y = yPix(m.y);
      if (x == null || y == null) continue;
      const up = m.side === 'long';
      const tip = up ? y - 1 : y + 1;
      const base = up ? y + 12 : y - 12;
      out.push({
        id: m.id,
        color: up ? QUICK_COLORS.long : QUICK_COLORS.short,
        entry: m.role === 'entry' || m.role === 'add' || m.role === 'reverse',
        points: `${x},${tip} ${x - 6},${base} ${x + 6},${base}`,
        x,
        y: base + (up ? 11 : -3),
        label: labels ? markSize(m) : '',
        title: `${m.side} ${markSize(m)} @ ${fmtPrice(m.y)}`
      });
    }
    return { shapes: out, dense: false };
  });
  const markShapes = $derived(markView.shapes);

  /** First index whose instant is >= `ms`. */
  function lowerBound(list, ms) {
    let lo = 0;
    let hi = list.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (list[mid].x < ms) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** Rows the price-pane header carries: the instrument, then the overlays drawn on it. */
  const headRows = $derived(legend.slice(2).filter((r) => r.pane == null));

  /** Comparison rows in the header: the instrument, and what it reads at the crosshair.
   *  Percent for a comparison, an index for a ratio, and never a price: the whole point is
   *  that two instruments with different prices become comparable. */
  const compareRows = $derived.by(() => {
    void paintTick;
    const i = readIdx;
    return compare.map((c) => {
      const v = i != null ? c.values?.[i] : null;
      return {
        id: c.id,
        label: c.label,
        color: c.color,
        hidden: !!c.hidden,
        value:
          v == null
            ? ''
            : c.mode === 'ratio'
              ? v.toFixed(2)
              : `${v >= 0 ? '+' : ''}${v.toFixed(2)}%`
      };
    });
  });

  /** One scale control per pane, sitting on that pane's own value gutter, shown while the
   *  pointer is on that gutter: the gutter is where scaling is done, so it is where the way
   *  back belongs, and off the gutter the chart stays clean. It is lit when that pane is on a
   *  manual range and clicking it returns that pane to auto-fit. There is no single "auto scale" on a chart with several scales: the price pane
   *  and an RSI pane are independent, and one button resetting both would undo work the user
   *  did not ask about. A pane too short to hold the icon does not get one. */
  const autoBtns = $derived.by(() => {
    void paintTick;
    if (!chart || !view) return [];
    const W = chart.getWidth();
    const out = [];
    for (let i = 0; i < gridLayout.length; i++) {
      const r = rect(i);
      if (!r || r.height < 26) continue;
      out.push({
        i,
        manual: yRange(i) != null,
        left: axisRight ? W - GRID_AXIS + 4 : 4,
        top: Math.max(r.y + 2, r.y + r.height - 21)
      });
    }
    return out;
  });

  /** Sub-pane headers. Volume and each oscillator pane are titled over the pane that draws
   *  them, not in the price pane's header — that is where a charting tool puts them, and it
   *  is the only way to tell two RSIs in two panes apart. */
  const paneHeads = $derived.by(() => {
    void paintTick;
    if (!chart || !view || !legendOpen) return [];
    const out = [];
    const push = (gridIndex, rows) => {
      if (!rows.length) return;
      const r = rect(gridIndex);
      if (!r) return;
      // A few pixels of air under the pane's top edge: the title must not sit on the line
      // that separates it from the pane above.
      out.push({ key: `pane${gridIndex}`, left: r.x + 4, top: r.y + 8, rows });
    };
    oscPanes.forEach((pane, i) => push(1 + i, legend.slice(2).filter((r) => r.pane === pane)));
    return out;
  });

  // ────────────────────────── Option builder ──────────────────────────

  function buildOption(v, p) {
    const oscCount = oscPanes.length;

    // Grid layout: the price pane (large), then one pane per oscillator. Volume has no pane
    // of its own — it is drawn inside the price pane, over the bottom quarter.
    // The header floats *over* the price pane (market-tool style), so the top margin is only
    // the room a price label needs — reserving a header-sized band left a dead strip.
    const top = 2;
    const oscH = oscCount ? Math.min(16, 40 / oscCount) : 0;
    const priceBottom = 100 - oscCount * oscH - 8;
    const layout = [{ left: gridLeft, right: gridRight, topPct: top, heightPct: priceBottom - top }];
    oscPanes.forEach((_, i) => {
      layout.push({
        left: gridLeft,
        right: gridRight,
        topPct: priceBottom + i * oscH,
        heightPct: oscH - 2
      });
    });
    gridLayout = layout;
    const grids = layout.map((g) => ({
      left: g.left,
      right: g.right,
      top: `${g.topPct}%`,
      height: `${g.heightPct}%`
    }));

    const showH = settings.grid !== false;
    const showV = settings.gridV === true;
    const showCrosshair = settings.crosshair !== false;
    const showTooltip = settings.tooltip === true;
    // Value tags need the crosshair (they ride its position event).
    const showTags = showCrosshair && settings.crosshairTags !== false;

    const priceLog = priceLogNow();
    // Manual drag wins; otherwise the log axis needs an explicit fitted range and the linear
    // axis fits itself (dataZoom filterMode 'filter' drops off-window points for it).
    const range = yRange(0) ?? (priceLog ? visibleLogRange(v) : null);
    if (priceLog && range) logRange = range;

    const xAxes = grids.map((_, i) => ({
      type: 'category',
      gridIndex: i,
      data: cats,
      boundaryGap: true,
      axisLine: { lineStyle: { color: p.border } },
      axisLabel: {
        show: i === grids.length - 1,
        color: p.dim,
        fontFamily: p.mono,
        fontSize: 10,
        hideOverlap: true,
        formatter: fmtTs
      },
      axisTick: { show: false },
      // Date label on the bottom axis only; the crosshair links all panes.
      axisPointer: {
        label: {
          show: showTags && i === grids.length - 1,
          backgroundColor: p.surface,
          borderColor: p.border,
          borderWidth: 1,
          fontFamily: p.mono,
          color: p.text,
          formatter: (o) => fmtTs(o.value)
        }
      },
      // Grid lines run through every pane, not just the price one — a volume or oscillator
      // pane without them reads as a picture instead of a scale.
      splitLine: { show: showV, lineStyle: { color: p.gridLine, width: 0.5 } }
    }));

    const yAxes = grids.map((_, i) => {
      const isPrice = i === 0;
      const isLog = isPrice && priceLog;
      const fixed = isPrice ? range : yRange(i);
      const axis = {
        gridIndex: i,
        type: isLog ? 'log' : 'value',
        position: axisRight ? 'right' : 'left',
        // A fixed range (manual drag, or the log fit) replaces `scale` auto-fitting.
        scale: !isLog && !fixed,
        axisLine: { show: false },
        // The price axis is written with the instrument's own precision; the oscillator panes
        // keep ECharts' generic labels.
        //
        // Two panes touch along one line, so the label at the bottom of one lands on the
        // label at the top of the next and the two values print over each other. The margin
        // belongs to the value, not to the pane: the edge label of a shared boundary is
        // dropped rather than the panes pushed apart, which would cost chart height.
        axisLabel: {
          color: p.dim,
          fontFamily: p.mono,
          fontSize: 10,
          hideOverlap: true,
          ...(i < grids.length - 1 ? { showMinLabel: false } : {}),
          ...(i > 0 ? { showMaxLabel: false } : {}),
          ...(isPrice ? { formatter: (x) => fmtAxisPrice(x) } : {})
        },
        // Per-series values are drawn as our own tags on the axis side, so hide ECharts'.
        axisPointer: { label: { show: false } },
        splitLine: { show: showH, lineStyle: { color: p.gridLine, width: 0.5 } }
      };
      if (fixed) {
        axis.min = fixed.min;
        axis.max = fixed.max;
      }
      if (isLog) {
        // ECharts' own log-axis ticks are unreliable over a sub-decade range (often just
        // min+max), so we hide them and draw evenly log-spaced labels/lines ourselves.
        axis.minorTick = { show: false };
        axis.minorSplitLine = { show: false };
        axis.axisLabel = { ...axis.axisLabel, show: false };
        axis.splitLine = { show: false };
      }
      return axis;
    });

    // Volume lives *inside* the price pane, as on every market terminal: its own hidden
    // scale, fitted so the bars fill the bottom quarter and never reach the candles.
    const volAxis = yAxes.length;
    yAxes.push({
      gridIndex: 0,
      type: 'value',
      min: 0,
      max: visibleVolumeMax(v),
      show: false,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { show: false },
      splitLine: { show: false },
      axisPointer: { label: { show: false } }
    });

    // A custom indicator dropped on the price pane keeps its own (hidden) scale: an RSI in the
    // 0-100 range must not flatten the candles by dragging the price axis down to zero.
    const customAxis = yAxes.length;
    yAxes.push({
      gridIndex: 0,
      type: 'value',
      scale: true,
      show: false,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { show: false },
      splitLine: { show: false },
      axisPointer: { label: { show: false } }
    });

    const series = [];

    // A plain line series; `st` is an optional per-instance style override {color, width}.
    // Note: fill is intentionally NOT handled here — only band/channel indicators fill, and
    // they do so between their edges (see the band-fill block below), never down to the axis.
    const lineSeries = (name, data, col, xi, width = 1.5, dashed = false, st = null, yi = xi) => ({
      name,
      type: 'line',
      data,
      showSymbol: false,
      clip: true, // a panned pane must not paint over its neighbours
      xAxisIndex: xi,
      yAxisIndex: yi,
      sampling: 'lttb', // decimate to one point per pixel — invisible, and much cheaper
      lineStyle: {
        width: st?.width ? Number(st.width) : width,
        color: st?.color || col,
        type: dashed ? 'dashed' : 'solid'
      },
      connectNulls: false
    });

    // ── Main price series (pane 0) ──
    if (!priceVisible) {
      // skip — hidden via legend
    } else if (type === 'line') {
      series.push(lineSeries($t('histviz.chart.close'), v.c, p.accent, 0, 1.5));
    } else {
      // ECharts only truly clips candles in `large` mode (a clip path on the whole group).
      // Under `largeThreshold` it renders each box itself and `clip` merely *drops* the ones
      // fully outside the grid — a candle with one corner inside is drawn whole, over the
      // pane below. So whenever the price axis carries an imposed range (a dragged scale, or
      // the log fit) we clip the boxes ourselves. Auto-fit needs none: it contains them all.
      const data = v.ts.map((_, i) => clipCandle(v.o[i], v.c[i], v.l[i], v.h[i], range));
      series.push({
        name: $t('histviz.chart.price'),
        type: 'candlestick',
        data,
        clip: true,
        xAxisIndex: 0,
        yAxisIndex: 0,
        large: true,
        largeThreshold: 400,
        progressive: 0,
        barWidth: type === 'ohlc' ? 1 : undefined,
        itemStyle: {
          color: p.up,
          color0: p.down,
          borderColor: p.up,
          borderColor0: p.down,
          ...(type === 'ohlc' ? { borderWidth: 1.5 } : {})
        }
      });
    }

    // ── Overlay indicators (pane 0) ──
    for (const ind of overlays) {
      const def = catalogDef(ind.type);
      const outs = computed.get(ind.id) ?? [];

      // Band/channel indicators fill the region *between* their upper and lower edges when a
      // fill color is set. Implemented as two stacked series: an invisible lower baseline
      // plus the (upper−lower) delta carrying the area — so the fill hugs the band exactly.
      if (def?.fillable && ind.style?.fill) {
        const upper = outs.find((o) => o.role === 'upper')?.data;
        const lower = outs.find((o) => o.role === 'lower')?.data;
        if (upper && lower) {
          const delta = upper.map((u, i) => (u != null && lower[i] != null ? u - lower[i] : null));
          const stackId = `band-${ind.id}`;
          series.push({
            name: `${def.type}-base-${ind.id}`,
            type: 'line',
            data: lower,
            clip: true,
            stack: stackId,
            xAxisIndex: 0,
            yAxisIndex: 0,
            lineStyle: { width: 0, opacity: 0 },
            symbol: 'none',
            silent: true,
            tooltip: { show: false }
          });
          series.push({
            name: `${def.type}-fill-${ind.id}`,
            type: 'line',
            data: delta,
            clip: true,
            stack: stackId,
            xAxisIndex: 0,
            yAxisIndex: 0,
            lineStyle: { width: 0, opacity: 0 },
            areaStyle: { color: ind.style.fill, opacity: 0.15 },
            symbol: 'none',
            silent: true,
            tooltip: { show: false }
          });
        }
      }

      for (const o of outs) {
        if (o.kind === 'scatter') {
          series.push({
            name: o.name,
            type: 'scatter',
            data: o.data,
            symbolSize: 3,
            clip: true,
            large: true,
            xAxisIndex: 0,
            yAxisIndex: 0,
            itemStyle: { color: ind.style?.color || color(o.color, p) }
          });
        } else if (o.kind === 'line') {
          series.push(
            lineSeries(o.name, o.data, color(o.color, p), 0, 1.4, o.dashed, ind.style, isCustom(ind) ? customAxis : 0)
          );
        }
      }
    }

    // ── Day separators ──
    // Where the calendar day changes, on an intraday chart. Deliberately *not* session
    // shading: sessions need an exchange calendar we do not have, and shading invented hours
    // would be a claim about the market rather than about the data.
    if (settings.daySeparators && stepMs < 86400000 && n > 1) {
      const marks = [];
      let prevDay = new Date(tsMs(0)).getUTCDate();
      for (let i = 1; i < n; i++) {
        const day = new Date(tsMs(i)).getUTCDate();
        if (day !== prevDay) marks.push({ xAxis: i });
        prevDay = day;
      }
      if (marks.length && marks.length < 400) {
        series.push({
          name: 'day-separators',
          type: 'line',
          data: [],
          xAxisIndex: 0,
          yAxisIndex: 0,
          silent: true,
          animation: false,
          markLine: {
            silent: true,
            symbol: 'none',
            label: { show: false },
            lineStyle: { color: p.border, width: 1, type: 'dashed', opacity: 0.7 },
            data: marks
          }
        });
      }
    }

    // ── Previous close ──
    // The last close of the previous day, the level every intraday desk quotes against.
    if (settings.prevClose && stepMs < 86400000 && n > 1) {
      let cut = -1;
      const lastDay = new Date(tsMs(n - 1)).getUTCDate();
      for (let i = n - 1; i >= 0; i--) {
        if (new Date(tsMs(i)).getUTCDate() !== lastDay) {
          cut = i;
          break;
        }
      }
      if (cut >= 0) {
        series.push({
          name: 'prev-close',
          type: 'line',
          data: [],
          xAxisIndex: 0,
          yAxisIndex: 0,
          silent: true,
          animation: false,
          markLine: {
            silent: true,
            symbol: 'none',
            label: {
              show: true,
              position: 'insideStartTop',
              formatter: $t('histviz.chart.prevClose'),
              color: p.muted,
              fontSize: 10
            },
            lineStyle: { color: p.muted, width: 1, type: 'dotted' },
            data: [{ yAxis: v.c[cut] }]
          }
        });
      }
    }

    // ── Comparison series ──
    // Other instruments, rebased and drawn on the price pane's hidden scale. Percentages and
    // prices share no units, which is exactly why they never share an axis.
    if (type !== 'renko') {
      for (const c of compare) {
        if (c.hidden || !c.values?.length) continue;
        series.push(lineSeries(c.label, c.values, c.color, 0, 1.4, false, null, customAxis));
      }
    }

    // ── Volume (bottom of the price pane) ──
    // Split into up/down series rather than one series with a per-item color callback: the
    // callback ran per bar per frame and blocks `large`. Overlapped with barGap -100%.
    if (volumeVisible) {
      const upV = v.v.map((x, i) => (v.c[i] >= v.o[i] ? x : null));
      const dnV = v.v.map((x, i) => (v.c[i] >= v.o[i] ? null : x));
      const volBase = {
        type: 'bar',
        clip: true,
        xAxisIndex: 0,
        yAxisIndex: volAxis,
        large: true,
        largeThreshold: 400,
        // Same footprint as the candle above it, gap included, and translucent so the price
        // grid still reads through it.
        barWidth: '60%',
        barGap: '-100%',
        silent: true,
        z: 1
      };
      series.push({
        ...volBase,
        name: $t('histviz.chart.volume'),
        data: upV,
        itemStyle: { color: p.up, opacity: 0.45 }
      });
      series.push({
        ...volBase,
        name: `${$t('histviz.chart.volume')} ▾`,
        data: dnV,
        itemStyle: { color: p.down, opacity: 0.45 }
      });
    }

    // ── Oscillator panes (one per type, shared by instances of that type) ──
    oscPanes.forEach((pane, i) => {
      const xi = 1 + i;
      for (const ind of active.filter((a) => paneKey(a) === pane)) {
        for (const o of computed.get(ind.id) ?? []) {
          if (o.kind === 'bar') {
            series.push({
              name: o.name,
              type: 'bar',
              data: o.data,
              clip: true,
              xAxisIndex: xi,
              yAxisIndex: xi,
              itemStyle:
                o.color === 'updown'
                  ? { color: (pp) => ((o.data[pp.dataIndex] ?? 0) >= 0 ? p.up : p.down) }
                  : { color: ind.style?.color || color(o.color, p) }
            });
          } else {
            series.push(lineSeries(o.name, o.data, color(o.color, p), xi, 1.3, o.dashed, ind.style));
          }
        }
      }
    });

    const allX = grids.map((_, i) => i);

    return {
      animation: false,
      backgroundColor: 'transparent',
      textStyle: { color: p.text },
      legend: { show: false },
      grid: grids,
      xAxis: xAxes,
      yAxis: yAxes,
      // The tooltip component is what actually drives the crosshair on hover, so it must
      // stay active even when the user hides the tooltip box — otherwise a standalone
      // axisPointer never triggers on mousemove (that was the "no crosshair on load" bug).
      // We keep the tooltip alive to move the pointer, and blank its box when not wanted.
      axisPointer: { link: [{ xAxisIndex: 'all' }] },
      tooltip: {
        show: true,
        trigger: 'axis',
        triggerOn: 'mousemove',
        // Crosshair lines belong to the tooltip's axisPointer.
        axisPointer: { type: showCrosshair ? 'cross' : 'none', label: { show: false } },
        // When the user's tooltip is off, render an empty, invisible box (pointer still moves).
        showContent: showTooltip,
        backgroundColor: showTooltip ? p.surface : 'transparent',
        borderColor: showTooltip ? p.border : 'transparent',
        textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 }
      },
      dataZoom: [
        {
          // Inert: every gesture is handled by our own pointer/wheel/key code above. Left in
          // because it is what actually applies the window when we dispatch dataZoom.
          type: 'inside',
          xAxisIndex: allX,
          startValue: win.s,
          endValue: win.e,
          moveOnMouseMove: false,
          moveOnMouseWheel: false,
          zoomOnMouseWheel: false,
          preventDefaultMouseMove: false
        },
        {
          type: 'slider',
          xAxisIndex: allX,
          bottom: 4,
          height: 16,
          startValue: win.s,
          endValue: win.e,
          brushSelect: false,
          textStyle: { color: p.dim, fontFamily: p.mono },
          labelFormatter: (_, s) => fmtTs(s)
        }
      ],
      series
    };
  }

  // ────────────────────────── Lifecycle & effects ──────────────────────────

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => {
      chart?.resize();
      updateLogGrid(); // pixel positions changed
      updateLastTag();
      updateDrawings();
    });
    ro.observe(el);
    // Drive the custom value tags and the legend readout off the linked crosshair.
    chart.on('updateAxisPointer', (e) => {
      const xInfo = e.axesInfo?.find((a) => a.axisDim === 'x');
      const idx = xInfo ? xInfo.value : null;
      hoverIdx = idx;
      requestTags(idx);
      // Linked panes follow the *instant*, not the bar index: a 1m pane and a 1h one are
      // pointing at the same moment even though the index means nothing to the other.
      if (!linking && idx != null && idx >= 0 && idx < n) onhover?.(msFromIdx(idx));
    });
    chart.getZr().on('globalout', () => {
      hoverIdx = null;
      lastPointer = null;
      cursorTag = null;
      tags = [];
      if (!linking) onhover?.(null);
    });
    chart.getZr().on('mousemove', (e) => {
      lastPointer = { x: e.offsetX, y: e.offsetY };
      requestCursorTag();
    });
    // Only the slider can move the window behind our back; mirror it into `win`.
    chart.on('datazoom', () => {
      if (applying) return;
      const dz = chart.getOption()?.dataZoom?.[0];
      if (!dz) return;
      const T = total;
      const s = dz.startValue ?? ((dz.start ?? 0) / 100) * (T - 1);
      const e = dz.endValue ?? ((dz.end ?? 100) / 100) * (T - 1);
      if (Math.abs(s - win.s) < 0.5 && Math.abs(e - win.e) < 0.5) return;
      win = clampWin(s, e);
    });

    // Capture phase, not bubble: ECharts' roam controller enables its own mousewheel handler
    // with hardcoded `true` (the per-model zoomOnMouseWheel:false is only read later), and it
    // calls preventDefault + stopPropagation on the canvas — a bubble listener here never runs.
    el.addEventListener('wheel', onWheel, { passive: false, capture: true });
    el.addEventListener('pointerdown', onPointerDown);
    el.addEventListener('pointermove', onPointerMove);
    el.addEventListener('pointerup', onPointerUp);
    el.addEventListener('pointercancel', onPointerUp);
    el.addEventListener('dblclick', onDblClick);

    return () => {
      ro.disconnect();
      el.removeEventListener('wheel', onWheel, true);
      el.removeEventListener('pointerdown', onPointerDown);
      el.removeEventListener('pointermove', onPointerMove);
      el.removeEventListener('pointerup', onPointerUp);
      el.removeEventListener('pointercancel', onPointerUp);
      el.removeEventListener('dblclick', onDblClick);
      if (raf) cancelAnimationFrame(raf);
      if (moveRaf) cancelAnimationFrame(moveRaf);
      if (tagRaf) cancelAnimationFrame(tagRaf);
      if (cursorRaf) cancelAnimationFrame(cursorRaf);
    };
  });

  // Refit the geometry when the parent toggles fullscreen (the box changes size in one frame).
  $effect(() => {
    void fullscreen;
    requestAnimationFrame(() => {
      chart?.resize();
      updateLogGrid();
      updateLastTag();
      updateDrawings();
    });
  });

  // ── Dataset identity ──
  // A new dataset resets the window; a chart-*type* change must not (candles → line should
  // leave the viewport alone). Renko changes the bar count, so it resets through the same door.
  let lastLen = -1;
  let lastFirst = null;
  let lastLast = null;
  $effect(() => {
    const v = view;
    if (!v || !v.ts.length) return;
    const len = v.ts.length;
    const first = v.ts[0];
    const last = v.ts[len - 1];
    if (len === lastLen && first === lastFirst) return;

    // Older history prepended: same newest bar, more bars in front. Shift the window by the
    // number added so the view does not jump — the whole point of loading in the background.
    const prepended = lastLen > 0 && last === lastLast && first !== lastFirst && len > lastLen;
    // A live bar closed and a new one opened: same history, one more bar at the end. That is
    // not a new dataset — the viewport and a hand-set price scale must both survive it.
    const appended = lastLen > 0 && first === lastFirst && len > lastLen && last > lastLast;
    const added = len - lastLen;
    const wasAtEnd = untrack(() => win.e) >= lastLen - 1;
    lastLen = len;
    lastFirst = first;
    lastLast = last;
    if (prepended) {
      untrack(() => setWin(win.s + added, win.e + added));
      return;
    }
    if (appended) {
      // Follow the market only if the window was already sitting on the last bar; someone
      // reading older bars stays where they are.
      if (wasAtEnd) untrack(() => setWin(win.s + added, win.e + added));
      return;
    }

    const rr = untrack(() => restoreRange);
    let next = null;
    if (rr?.t0 && rr?.t1) {
      const i0 = idxOfTs(v.ts, rr.t0);
      const i1 = idxOfTs(v.ts, rr.t1);
      if (i1 - i0 >= MIN_BARS) next = clampWin(i0, i1);
      onrestored?.();
    }
    yMan = {};
    win = next ?? defaultWin();
  });

  // First index whose timestamp is >= ts (binary search; RFC3339 sorts lexicographically).
  function idxOfTs(arr, ts) {
    let lo = 0;
    let hi = arr.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (arr[mid] < ts) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  // Apply the window whenever it moves, one dispatch per frame.
  $effect(() => {
    void [win.s, win.e];
    if (chart) schedule();
  });

  // The left edge of the loaded history is in view: that is where the "load more" button
  // appears. Nothing is fetched automatically — a scroll must never spend an API call.
  const atLeftEdge = $derived(!!n && win.s <= 2);

  // Full option rebuild. `replaceMerge` swaps the drawn components wholesale (so removing an
  // oscillator pane really removes it) while leaving dataZoom/tooltip merged — a notMerge
  // here would rebuild the dataZoom mid-gesture and kill the drag.
  $effect(() => {
    if (!chart || !view) return;
    // Explicitly touch settings + inputs so the effect re-runs on any of them (buildOption
    // reads them internally, but we make the dependency unambiguous here).
    void [
      settings.scale,
      settings.grid,
      settings.gridV,
      settings.upColor,
      settings.downColor,
      settings.lineColor,
      settings.crosshair,
      settings.crosshairTags,
      settings.tooltip,
      settings.lastPrice,
      settings.daySeparators,
      settings.prevClose,
      settings.yAxisSide,
      type,
      brick,
      instances,
      priceVisible,
      volumeVisible,
      cats,
      yMan
    ];
    void palette;
    void computed;
    if (!priceLogNow()) logRange = { min: 0, max: 0 };
    // buildOption reads `win` — untrack it, or every pan frame rebuilds the whole option.
    const option = untrack(() => buildOption(view, palette));
    chart.setOption(option, { replaceMerge: ['series', 'xAxis', 'yAxis', 'grid'] });
    tags = []; // positions are stale after a re-render — replayed next frame, or on the next move
    updateLogGrid(); // custom left-axis labels/gridlines for the log price scale
    updateLastTag();
    // The drawing layer derives from `paintTick`, so bumping it *inside* this effect would be
    // a read-and-write of the same graph. The next frame — once ECharts has laid the panes
    // out — is both correct and outside the effect. The crosshair goes back on the same frame.
    requestAnimationFrame(() => {
      updateDrawings();
      restoreCrosshair();
      updateCursorTag();
    });
  });

  // Log scale only: refit the price axis as the window moves. A *merged* setOption touching
  // just yAxis[0] leaves the dataZoom components alone, so an in-progress drag survives.
  $effect(() => {
    void [win.s, win.e];
    if (!chart || !view || yRange(0) || !priceLogNow()) return;
    const lr = untrack(() => visibleLogRange(view));
    if (!lr) return;
    logRange = lr;
    chart.setOption({ yAxis: [{ min: lr.min, max: lr.max }] });
    updateLogGrid();
    updateLastTag();
    requestAnimationFrame(updateDrawings);
  });

  // The volume scale is fitted to the window like the log price axis: a merged setOption on
  // that one axis leaves every other component (and any in-progress drag) alone.
  $effect(() => {
    void [win.s, win.e];
    if (!chart || !view || !volumeVisible) return;
    const max = untrack(() => visibleVolumeMax(view));
    const idx = volAxisIndex();
    chart.setOption({ yAxis: Array.from({ length: idx + 1 }, (_, i) => (i === idx ? { max } : {})) });
  });

  // ── Public API (bind:this) — the parent uses this to keep the same dates across a
  // timeframe switch. Returns the visible span clamped to real bars. ──
  /** The chart as a PNG data URL, at the pixel ratio it is drawn with: what the export
   *  button hands the browser to save. Includes every overlay ECharts drew, which is why it
   *  comes from the instance rather than from a canvas we would have to redraw. */
  /** The chart as a PNG data URL: ECharts' own render with the drawing layer on top.
   *
   *  `getDataURL` only knows about the canvas, so a chart exported from it came out without
   *  a single trend line, which is exactly the part a shared chart is about. The overlay is
   *  an inert SVG, so it is serialized, rasterized and composited at the same pixel ratio.
   *  The few styles the component stylesheet gave it travel with it (a standalone SVG has no
   *  access to that sheet), and the edit affordances do not: a handle is a way to move an
   *  object, not part of the picture. */
  export async function snapshot() {
    if (!chart) return null;
    const ratio = Math.max(2, window.devicePixelRatio || 1);
    const base = chart.getDataURL({
      type: 'png',
      pixelRatio: ratio,
      backgroundColor: palette.bg || '#111'
    });
    if (!svgEl || !el) return base;
    try {
      const w = el.clientWidth;
      const h = el.clientHeight;
      const clone = svgEl.cloneNode(true);
      clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
      clone.setAttribute('width', String(w));
      clone.setAttribute('height', String(h));
      clone.setAttribute('viewBox', `0 0 ${w} ${h}`);
      const css = document.createElementNS('http://www.w3.org/2000/svg', 'style');
      css.textContent =
        '.lab{font-size:10px;font-family:ui-monospace,monospace}' +
        '.lab.big{font-size:12px;font-weight:600}' +
        '.note{font-size:12px}' +
        '.handle,.guide{display:none}';
      clone.prepend(css);
      const svgUrl =
        'data:image/svg+xml;charset=utf-8,' +
        encodeURIComponent(new XMLSerializer().serializeToString(clone));
      const load = (src) =>
        new Promise((resolve, reject) => {
          const img = new Image();
          img.onload = () => resolve(img);
          img.onerror = reject;
          img.src = src;
        });
      const [under, over] = await Promise.all([load(base), load(svgUrl)]);
      const canvas = document.createElement('canvas');
      canvas.width = under.width;
      canvas.height = under.height;
      const cx = canvas.getContext('2d');
      cx.drawImage(under, 0, 0);
      cx.drawImage(over, 0, 0, canvas.width, canvas.height);
      return canvas.toDataURL('image/png');
    } catch {
      // A layer that will not rasterize must not cost the user their chart.
      return base;
    }
  }

  export function getTimeRange() {
    if (!view || !n) return null;
    const { from, to } = dataWin();
    if (to <= from) return null;
    return { t0: view.ts[from], t1: view.ts[to] };
  }

  /**
   * Frame a span of time, centred, with air around it.
   *
   * The window is sized from the span itself (`pad` of its length on each side) so a two-bar
   * trade and a two-hundred-bar one both land readable, and never goes tighter than `minBars`,
   * which is what keeps a scalp from opening on three candles filling the screen.
   *
   * @param fromMs first instant to show, epoch ms
   * @param toMs   last instant to show, epoch ms
   */
  /** Show exactly this span, as a linked pane asks for it: no padding, no minimum, because
   *  the point is that both charts show the same window. */
  export function setTimeRange(t0, t1) {
    if (!view || !n) return;
    const a = Date.parse(t0);
    const b = Date.parse(t1);
    if (!Number.isFinite(a) || !Number.isFinite(b) || b <= a) return;
    linking = true;
    yMan = {};
    setWin(idxFromMs(a), idxFromMs(b));
    linking = false;
  }

  /** Put the crosshair on an instant a neighbour is pointing at. `null` clears it. */
  export function showCrosshairAt(ms) {
    if (!chart || settings.crosshair === false) return;
    if (ms == null) {
      linking = true;
      hoverIdx = null;
      tags = [];
      chart.dispatchAction({ type: 'hideTip' });
      chart.dispatchAction({ type: 'updateAxisPointer', currTrigger: 'leave' });
      linking = false;
      return;
    }
    if (!view || !n) return;
    const idx = Math.round(idxFromMs(ms));
    if (idx < 0 || idx >= n) return;
    linking = true;
    chart.dispatchAction({ type: 'showTip', seriesIndex: 0, dataIndex: idx });
    hoverIdx = idx;
    requestTags(idx);
    linking = false;
  }

  export function focusRange(fromMs, toMs, { minBars = 60, pad = 0.6 } = {}) {
    if (!view || !n || !Number.isFinite(fromMs)) return;
    const i0 = idxFromMs(fromMs);
    const i1 = Number.isFinite(toMs) ? idxFromMs(toMs) : i0;
    const len = Math.max(1, i1 - i0);
    const span = Math.max(minBars, Math.round(len * (1 + 2 * pad)));
    const mid = (i0 + i1) / 2;
    yMan = {};
    setWin(Math.round(mid - span / 2), Math.round(mid + span / 2));
  }
</script>

<!-- role=application: this is a direct-manipulation surface with its own key bindings
     (arrows pan, +/- zoom, Home/End jump), not a document region. The a11y rules below flag
     a focusable div with key handlers, which is exactly what an application region is. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="chart-host"
  class:fs={fullscreen}
  class:axis-right={axisRight}
  bind:this={host}
  tabindex="0"
  role="application"
  aria-label={$t('histviz.chart.ariaChart')}
  onkeydown={onKeyDown}
  onpointermove={onHostMove}
  onpointerleave={() => (axisHover = false)}
>
  <div class="chart" class:panning bind:this={el}></div>

  <!-- Drawing layer. Inert by default so the chart keeps every gesture; the shapes opt into
       the pointer themselves, and the whole layer takes it while a tool is armed. -->
  {#if view}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <svg
      class="draw"
      class:armed
      bind:this={svgEl}
      onpointerdown={onDrawDown}
      onpointermove={onDrawMove}
      onpointerup={onDrawUp}
      onpointercancel={onDrawUp}
    >
      {#if clip}
        <defs>
          <clipPath id={clipId}>
            <rect x={clip.x} y={clip.y} width={clip.width} height={clip.height} />
          </clipPath>
        </defs>
      {/if}
      <g clip-path={clip ? `url(#${clipId})` : undefined}>
        <!-- Alignment guides: what the anchor being placed has lined up with. -->
        {#if guideLines && clip}
          {#if guideLines.y != null}
            <line
              class="guide"
              x1={clip.x}
              x2={clip.x + clip.width}
              y1={guideLines.y}
              y2={guideLines.y}
            />
          {/if}
          {#if guideLines.x != null}
            <line
              class="guide"
              x1={guideLines.x}
              x2={guideLines.x}
              y1={clip.y}
              y2={clip.y + clip.height}
            />
          {/if}
        {/if}
        {#each shapes as s (s.id)}
          <g class="shape" class:sel={s.sel} stroke={s.color} fill="none">
            {#if s.tool === 'trend' && s.bx != null}
              <line
                x1={s.ax}
                y1={s.ay}
                x2={s.bx}
                y2={s.by}
                stroke-width={s.width}
                class="hit"
                onpointerdown={(e) => startDrag(e, s.id, 'move')}
              />
            {:else if s.tool === 'hline'}
              <line
                x1={s.x1}
                y1={s.ay}
                x2={s.x2}
                y2={s.ay}
                stroke-width={s.width}
                class="hit"
                onpointerdown={(e) => startDrag(e, s.id, 'move')}
              />
              <text class="lab" x={s.x2 - 4} y={s.ay - 4} text-anchor="end" fill={s.color} stroke="none"
                >{s.label}</text
              >
            {:else if s.tool === 'vline'}
              <line
                x1={s.ax}
                y1={s.y1}
                x2={s.ax}
                y2={s.y2}
                stroke-width={s.width}
                class="hit"
                onpointerdown={(e) => startDrag(e, s.id, 'move')}
              />
            {:else if s.tool === 'rect' && s.rw != null}
              <rect
                x={s.rx}
                y={s.ry}
                width={s.rw}
                height={s.rh}
                stroke-width={s.width}
                fill={s.color}
                fill-opacity="0.08"
                class="hit fill"
                onpointerdown={(e) => startDrag(e, s.id, 'move')}
              />
            {:else if s.tool === 'fib' && s.levels}
              {#each s.levels as lvl (lvl.lv)}
                <line
                  x1={s.x1}
                  y1={lvl.y}
                  x2={s.x2}
                  y2={lvl.y}
                  stroke-width={s.width}
                  stroke-opacity={lvl.lv === 0 || lvl.lv === 1 ? 1 : 0.6}
                  class="hit"
                  onpointerdown={(e) => startDrag(e, s.id, 'move')}
                />
                <text class="lab" x={s.x1 + 4} y={lvl.y - 3} fill={s.color} stroke="none"
                  >{lvl.lv.toFixed(3)} · {lvl.price}</text
                >
              {/each}
            {:else if s.tool === 'text'}
              <text
                class="note"
                x={s.ax}
                y={s.ay}
                fill={s.color}
                stroke="none"
                onpointerdown={(e) => startDrag(e, s.id, 'move')}>{s.text}</text
              >
            {:else if (s.tool === 'long' || s.tool === 'short') && s.sy != null}
              <!-- Target box (green), stop box (red), entry line between them. -->
              <rect
                x={s.x1}
                y={s.target.y1}
                width={s.x2 - s.x1}
                height={Math.max(1, s.target.y2 - s.target.y1)}
                fill={palette.up}
                fill-opacity="0.12"
                stroke="none"
                class="hit fill"
                onpointerdown={(e) => startDrag(e, s.id, 'move')}
              />
              <rect
                x={s.x1}
                y={s.stopBox.y1}
                width={s.x2 - s.x1}
                height={Math.max(1, s.stopBox.y2 - s.stopBox.y1)}
                fill={palette.down}
                fill-opacity="0.12"
                stroke="none"
                class="hit fill"
                onpointerdown={(e) => startDrag(e, s.id, 'move')}
              />
              <line x1={s.x1} y1={s.ay} x2={s.x2} y2={s.ay} stroke-width={s.width} class="hit" />
              <line
                x1={s.x1}
                y1={s.by}
                x2={s.x2}
                y2={s.by}
                stroke={palette.up}
                stroke-width="1"
                stroke-dasharray="4 3"
              />
              <line
                x1={s.x1}
                y1={s.sy}
                x2={s.x2}
                y2={s.sy}
                stroke={palette.down}
                stroke-width="1"
                stroke-dasharray="4 3"
              />
              <text class="lab" x={s.x1 + 4} y={s.by - 3} fill={palette.up} stroke="none"
                >{s.target.label}</text
              >
              <text class="lab" x={s.x1 + 4} y={s.sy + 10} fill={palette.down} stroke="none"
                >{s.stopBox.label}</text
              >
              <text class="lab" x={s.x1 + 4} y={s.ay - 3} fill={s.color} stroke="none"
                >{s.entryLabel} · {s.rr}</text
              >
            {:else if s.tool === 'ruler' && s.rw != null}
              <rect
                x={s.rx}
                y={s.ry}
                width={s.rw}
                height={Math.max(1, s.rh)}
                fill={s.tint}
                fill-opacity="0.12"
                stroke={s.tint}
                stroke-width="1"
                class="hit fill"
                onpointerdown={(e) => startDrag(e, s.id, 'move')}
              />
              <line
                x1={s.ax}
                y1={s.ay}
                x2={s.ax}
                y2={s.by}
                stroke={s.tint}
                stroke-width="1.5"
                marker-end=""
              />
              <text class="lab big" x={s.rx + s.rw / 2} y={s.up ? s.ry - 14 : s.ry + s.rh + 14} text-anchor="middle" fill={s.tint} stroke="none"
                >{s.label}</text
              >
              <text class="lab" x={s.rx + s.rw / 2} y={s.up ? s.ry - 4 : s.ry + s.rh + 24} text-anchor="middle" fill={s.tint} stroke="none"
                >{s.sub}</text
              >
            {/if}

            {#if s.sel && s.id !== '__draft' && s.sy != null}
              <circle
                class="handle"
                cx={(s.x1 + s.x2) / 2}
                cy={s.sy}
                r="4"
                fill={palette.down}
                onpointerdown={(e) => startDrag(e, s.id, 'stop')}
              />
            {/if}
            {#if s.sel && s.id !== '__draft'}
              <circle
                class="handle"
                cx={s.ax}
                cy={s.ay}
                r="4"
                fill={s.color}
                onpointerdown={(e) => startDrag(e, s.id, 'a')}
              />
              {#if s.bx != null}
                <circle
                  class="handle"
                  cx={s.bx}
                  cy={s.by}
                  r="4"
                  fill={s.color}
                  onpointerdown={(e) => startDrag(e, s.id, 'b')}
                />
              {/if}
            {/if}
          </g>
        {/each}

        <!-- Quick backtest markers. Clickable only while the mode is on, so an arrow never
             steals a pan from someone who is just reading the chart. -->
        <g class="marks" class:live={quick}>
          {#each markShapes as m (m.id)}
            <polygon
              class="mark"
              points={m.points}
              fill={m.entry ? m.color : 'none'}
              stroke={m.color}
              stroke-width="1.5"
              onpointerdown={(e) => {
                if (!quick) return;
                e.stopPropagation();
                e.preventDefault();
                onquicktoggle?.(m.id);
              }}
            >
              <title>{m.title}</title>
            </polygon>
            {#if m.label && m.label !== '1'}
              <text class="qty" x={m.x} y={m.y} text-anchor="middle" fill={m.color} stroke="none"
                >{m.label}</text
              >
            {/if}
          {/each}
        </g>
      </g>
    </svg>
  {/if}

  <!-- Long press: side and size, where the finger was. -->
  {#if quickMenu}
    <div class="quick-menu" style:left="{quickMenu.left}px" style:top="{quickMenu.top}px">
      <div class="qrow">
        <button type="button" class="qbtn long" onclick={() => quickSubmit('long')}>
          <Icon name="arrow-up" size={12} /> {$t('analysis.long')}
        </button>
        <button type="button" class="qbtn short" onclick={() => quickSubmit('short')}>
          <Icon name="arrow-down" size={12} /> {$t('analysis.short')}
        </button>
      </div>
      <label class="qqty">
        {$t('histviz.quick.qty')}
        <input
          type="number"
          min="0"
          step="any"
          bind:value={quickSize}
          onkeydown={(e) => {
            if (e.key === 'Enter') quickSubmit('long');
            e.stopPropagation();
          }}
        />
        {#if quickUnitLabel}<span class="qunit">{quickUnitLabel}</span>{/if}
      </label>
      <p class="qhint">{$t('histviz.quick.priceAt', { price: fmtPrice(quickMenu.p.y) })}</p>
    </div>
  {/if}

  <!-- The text tool types straight on the chart; an empty entry drops the object. -->
  {#if editing && notePos}
    <input
      class="note-input"
      style:left="{notePos.left}px"
      style:top="{notePos.top - 10}px"
      bind:this={noteInput}
      bind:value={editing.value}
      onblur={commitText}
      onkeydown={(e) => {
        if (e.key === 'Enter') commitText();
        else if (e.key === 'Escape') {
          editing.value = '';
          commitText();
        }
        e.stopPropagation();
      }}
    />
  {/if}

  <!-- Selection style bar: the few controls a drawing needs, where the drawing is. -->
  {#if styleBar}
    {@const cur = drawings.find((d) => d.id === selectedDrawing)}
    <div class="style-bar" style:left="{styleBar.left}px" style:top="{styleBar.top}px">
      {#each STYLE_COLORS as c, ci (ci)}
        <button
          type="button"
          class="swatch"
          class:on={(cur?.style?.color || STYLE_COLORS[0]) === c}
          style:background={c}
          title={$t('histviz.draw.color')}
          aria-label={$t('histviz.draw.color')}
          onclick={() => styleSelected({ color: c })}
        ></button>
      {/each}
      <label class="pick" title={$t('histviz.draw.color')}>
        <Icon name="droplet" size={12} />
        <input
          type="color"
          value={cur?.style?.color || palette.accent}
          oninput={(e) => styleSelected({ color: e.currentTarget.value })}
        />
      </label>
      <span class="bar-sep"></span>
      {#each [1, 2, 3] as w (w)}
        <button
          type="button"
          class="width"
          class:on={Math.round(cur?.style?.width || 1.4) === w}
          title={$t('histviz.draw.width')}
          aria-label={`${$t('histviz.draw.width')} ${w}`}
          onclick={() => styleSelected({ width: w })}
        >
          <span style:height="{w}px"></span>
        </button>
      {/each}
      <span class="bar-sep"></span>
      <button
        type="button"
        class="bar-act"
        class:on={tplOpen}
        title={$t('histviz.draw.templates')}
        aria-label={$t('histviz.draw.templates')}
        onclick={() => (tplOpen = !tplOpen)}
      >
        <Icon name="star" size={12} />
      </button>
      <button
        type="button"
        class="bar-act"
        title={$t('histviz.draw.copy')}
        aria-label={$t('histviz.draw.copy')}
        onclick={copySelected}
      >
        <Icon name="copy" size={12} />
      </button>
      <button
        type="button"
        class="bar-del"
        title={$t('histviz.draw.remove')}
        aria-label={$t('histviz.draw.remove')}
        onclick={removeSelected}
      >
        <Icon name="trash" size={12} />
      </button>
      {#if tplOpen}
        <div class="tpl-pop">
          {#each templates as tpl (tpl.id)}
            <div class="tpl-row">
              <button
                type="button"
                class="tpl-apply"
                onclick={() => styleSelected({ ...tpl.style })}
              >
                <span class="swatch" style:background={tpl.style?.color || palette.accent}
                ></span>
                <span class="tpl-name">{tpl.name}</span>
              </button>
              <button
                type="button"
                class="tpl-del"
                title={$t('common.remove')}
                aria-label={$t('common.remove')}
                onclick={() => removeTemplate(tpl.id)}
              >
                <Icon name="x" size={10} />
              </button>
            </div>
          {:else}
            <p class="tpl-empty">{$t('histviz.draw.templateEmpty')}</p>
          {/each}
          <div class="tpl-new">
            <input
              type="text"
              bind:value={tplName}
              placeholder={$t('histviz.draw.templateName')}
              onkeydown={(e) => e.key === 'Enter' && saveTemplate()}
            />
            <button type="button" onclick={saveTemplate}>{$t('histviz.draw.templateSave')}</button>
          </div>
          <button type="button" class="tpl-def" onclick={templateAsDefault}>
            {$t('histviz.draw.templateDefault')}
          </button>
        </div>
      {/if}
    </div>
  {/if}

  {#if view && logGrid.length}
    <div class="loggrid">
      {#each logGrid as g, gi (gi)}
        {#if settings.grid !== false}<span class="lg-line" style:top="{g.top}px"></span>{/if}
        <span class="lg-label" style:top="{g.top}px">{g.text}</span>
      {/each}
    </div>
  {/if}

  {#if view && tags.length}
    <div class="tags">
      {#each tags as t, ti (ti)}
        <span class="tag" style:top="{t.top}px" style:background={t.color}>{t.value}</span>
      {/each}
    </div>
  {/if}

  <!-- The crosshair's own scale label: the value the cursor is pointing at. -->
  {#if view && cursorTag}
    <span class="cursor-tag" style:top="{cursorTag.top}px">{cursorTag.text}</span>
  {/if}

  <!-- Latest close, on the price axis it belongs to (left gutter), tinted like the candle
       that produced it. -->
  {#if view && lastTag}
    <span class="last-tag" style:top="{lastTag.top}px" style:background={lastTag.color}
      >{lastTag.text}</span
    >
    <!-- Time left on the bar that is still running, directly under the price it belongs to. -->
    {#if countdown}
      <span class="count-tag" style:top="{lastTag.top + 8}px" style:color={lastTag.color}
        >{countdown}</span
      >
    {/if}
  {/if}

  {#if view}
    <!-- Three icons, no words: the chart is the content and this is navigation furniture.
         Fullscreen is not repeated here, the pane bar above already owns it. -->
    <div class="nav" role="toolbar" aria-label={$t('histviz.chart.ariaChart')}>
      <button
        onclick={goStart}
        title={$t('histviz.chart.jumpToStart')}
        aria-label={$t('histviz.chart.start')}
      >
        <Icon name="chevrons-left" size={13} />
      </button>
      <button
        onclick={zoomFit}
        title={$t('histviz.chart.fitToDefault')}
        aria-label={$t('histviz.chart.fit')}
      >
        <Icon name="move-horizontal" size={13} />
      </button>
      <button
        onclick={goEnd}
        title={$t('histviz.chart.jumpToLatest')}
        aria-label={$t('histviz.chart.end')}
      >
        <Icon name="chevrons-right" size={13} />
      </button>
    </div>
  {/if}

  {#each autoBtns as b (b.i)}
    <button
      class="autoscale"
      class:manual={b.manual}
      class:show={axisHover}
      style:left="{b.left}px"
      style:top="{b.top}px"
      disabled={!b.manual}
      title={b.manual ? $t('histviz.chart.autoScaleHint') : $t('histviz.chart.autoScaleOn')}
      aria-label={$t('histviz.chart.autoScale')}
      aria-pressed={!b.manual}
      onclick={() => clearY(b.i)}
    >
      <Icon name="chevrons-up-down" size={12} />
    </button>
  {/each}

  <!-- Too many fills in the window to read: say it, rather than paint a wall of triangles. -->
  {#if markView.dense}
    <span class="marks-dense">{$t('histviz.chart.marksDense')}</span>
  {/if}

  <!-- History control, pinned to the left edge where the loaded data ends. Clicking is the
       only way older bars are fetched; when the provider refuses, its reason rides along as
       the tooltip instead of the button vanishing. -->
  {#if view && atLeftEdge}
    {#if atHistoryStart}
      <span class="hist-edge">{$t('histviz.chart.historyStart')}</span>
    {:else}
      <button
        class="hist-more"
        class:blocked={!!loadMoreNotice}
        disabled={loadingMore}
        title={loadMoreNotice || $t('histviz.chart.loadMoreHint')}
        onclick={() => onloadmore?.()}
      >
        {#if loadingMore}
          <Icon name="refresh-cw" size={12} /> {$t('histviz.chart.loadingMore')}
        {:else}
          <Icon name="chevron-left" size={12} /> {loadMoreLabel || $t('histviz.chart.loadMoreShort')}
        {/if}
      </button>
    {/if}
  {/if}

  <!-- ── On-chart header ──
       The instrument, its OHLC readout, then one line per series, the way a market tool
       writes it over the candles. Each line only shows its controls on hover, so the chart
       stays clean until you reach for them. -->
  {#if view && legend.length}
    {@const price = legend[0]}
    <div class="head" class:collapsed={!legendOpen}>
      <div class="line sym-line">
        {#if title}
          <!-- The symbol is a button, as on every market terminal: it opens the picker. -->
          <button class="sym" title={$t('histviz.data.pick')} onclick={() => onsymbol?.()}>
            {title.ticker}
          </button>
          <span class="dot-sep">·</span>
          <span class="tf">{title.timeframe}</span>
          <span class="dot-sep">·</span>
          <span class="ex">{title.provider}</span>
        {:else}
          <span class="sym">{price.label}</span>
        {/if}
        {#if ohlc && !price.hidden}
          <span class="ohlc" class:up={ohlc.up}>
            <span>O<b>{ohlc.o}</b></span>
            <span>H<b>{ohlc.h}</b></span>
            <span>L<b>{ohlc.l}</b></span>
            <span>C<b>{ohlc.c}</b></span>
            <span class="chg">{ohlc.delta} ({ohlc.chg})</span>
          </span>
        {/if}
        <span class="btns">
          <button
            class="hbtn"
            title={price.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
            aria-label={price.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
            onclick={() => toggleRow(price)}
          >
            <Icon name={price.hidden ? 'eye-off' : 'eye'} size={12} />
          </button>
        </span>
      </div>

      {#if legendOpen}
        <!-- Volume is drawn in this pane, so it is titled in this header. -->
        {@const volume = legend[1]}
        <div class="line" class:off={volume.hidden}>
          <span class="ico bar" style:--c={volume.color} style:--c2={volume.color2}></span>
          <span class="lbl">{volume.label}</span>
          <span class="val">{volume.value}</span>
          <span class="btns">
            <button
              class="hbtn"
              title={volume.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
              aria-label={volume.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
              onclick={() => toggleRow(volume)}
            >
              <Icon name={volume.hidden ? 'eye-off' : 'eye'} size={12} />
            </button>
          </span>
        </div>

        {#each compareRows as row (row.id)}
          <div class="line ind" class:off={row.hidden}>
            <span class="ico line-ico" style:--c={row.color}></span>
            <span class="lbl">{row.label}</span>
            {#if row.value}<span class="val">{row.value}</span>{/if}
            <span class="btns">
              <button
                class="hbtn"
                title={row.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
                aria-label={row.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
                onclick={() => oncomparetoggle?.(row.id)}
              >
                <Icon name={row.hidden ? 'eye-off' : 'eye'} size={12} />
              </button>
              <button
                class="hbtn"
                title={$t('histviz.panel.remove')}
                aria-label={$t('histviz.panel.remove')}
                onclick={() => oncompareremove?.(row.id)}
              >
                <Icon name="x" size={12} />
              </button>
            </span>
          </div>
        {/each}

        {#each headRows as row (row.id)}
          <div class="line ind" class:off={row.hidden}>
            {#if row.icon === 'dot'}
              <span class="ico dot" style:background={row.color}></span>
            {:else if row.icon === 'bar'}
              <span class="ico bar" style:--c={row.color} style:--c2={row.color2 ?? row.color}></span>
            {:else}
              <span class="ico line-ico" class:dashed={row.dashed} style:--c={row.color}></span>
            {/if}
            <span class="lbl">{row.label}</span>
            {#if row.value}<span class="val">{row.value}</span>{/if}
            <span class="btns">
              <button
                class="hbtn"
                title={row.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
                aria-label={row.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
                onclick={() => toggleRow(row)}
              >
                <Icon name={row.hidden ? 'eye-off' : 'eye'} size={12} />
              </button>
              <button
                class="hbtn"
                title={$t('histviz.panel.edit')}
                aria-label={$t('histviz.panel.edit')}
                onclick={() => onedit?.(row.id)}
              >
                <Icon name="settings" size={12} />
              </button>
              <button
                class="hbtn"
                title={$t('histviz.panel.remove')}
                aria-label={$t('histviz.panel.remove')}
                onclick={() => onremove?.(row.id)}
              >
                <Icon name="x" size={12} />
              </button>
            </span>
          </div>
        {/each}
      {/if}

      <button
        class="head-toggle"
        title={legendOpen ? $t('histviz.chart.toggleLegend') : $t('histviz.chart.series')}
        aria-label={$t('histviz.chart.toggleLegend')}
        aria-expanded={legendOpen}
        onclick={() => (legendOpen = !legendOpen)}
      >
        <Icon name={legendOpen ? 'chevron-up' : 'chevron-down'} size={12} />
        {#if !legendOpen && legend.length > 2}<span class="count">{legend.length - 2}</span>{/if}
      </button>
    </div>

    <!-- One title per sub-pane, over the pane that draws it. -->
    {#each paneHeads as ph (ph.key)}
      <div class="head pane-head" style:left="{ph.left}px" style:top="{ph.top}px">
        {#each ph.rows as row (row.builtin ?? row.id)}
          <div class="line ind" class:off={row.hidden}>
            {#if row.icon === 'dot'}
              <span class="ico dot" style:background={row.color}></span>
            {:else if row.icon === 'bar'}
              <span class="ico bar" style:--c={row.color} style:--c2={row.color2 ?? row.color}></span>
            {:else}
              <span class="ico line-ico" class:dashed={row.dashed} style:--c={row.color}></span>
            {/if}
            <span class="lbl">{row.label}</span>
            {#if row.value}<span class="val">{row.value}</span>{/if}
            <span class="btns">
              <button
                class="hbtn"
                title={row.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
                aria-label={row.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
                onclick={() => toggleRow(row)}
              >
                <Icon name={row.hidden ? 'eye-off' : 'eye'} size={12} />
              </button>
              {#if row.id != null}
                <button
                  class="hbtn"
                  title={$t('histviz.panel.edit')}
                  aria-label={$t('histviz.panel.edit')}
                  onclick={() => onedit?.(row.id)}
                >
                  <Icon name="settings" size={12} />
                </button>
                <button
                  class="hbtn"
                  title={$t('histviz.panel.remove')}
                  aria-label={$t('histviz.panel.remove')}
                  onclick={() => onremove?.(row.id)}
                >
                  <Icon name="x" size={12} />
                </button>
              {/if}
            </span>
          </div>
        {/each}
      </div>
    {/each}
  {/if}
</div>

<style>
  /* `isolation: isolate` contains the overlay children below (crosshair, tags, nav).
     Their z-indexes are a private 1..5 order *within* this chart, not rungs on the
     global --z-* ladder; without a stacking context they'd leak into the page's root
     order and silently compete with real layers. */
  .draw .guide {
    stroke: var(--muted);
    stroke-width: 1;
    stroke-dasharray: 3 3;
    opacity: 0.75;
    pointer-events: none;
  }

  /* ── Drawing layer ──
     The svg spans the whole chart but stays out of the way: only an armed tool (or a shape
     under the cursor) takes the pointer, so pan/zoom keep working through it. */
  .draw {
    position: absolute;
    inset: 0;
    /* An <svg> is a replaced element: its 300×150 intrinsic size wins over `inset` alone. */
    width: 100%;
    height: 100%;
    z-index: 3;
    pointer-events: none;
  }
  .draw.armed {
    pointer-events: auto;
    cursor: crosshair;
  }
  .shape .hit {
    pointer-events: stroke;
    /* A hairline is impossible to grab; a fat transparent stroke under it is the hit area. */
    stroke-linecap: round;
    paint-order: stroke;
  }
  .shape .hit.fill {
    pointer-events: all;
  }
  .draw:not(.armed) .shape .hit {
    cursor: move;
  }
  .shape.sel .hit {
    stroke-dasharray: none;
    filter: drop-shadow(0 0 2px currentColor);
  }
  .shape .lab,
  .shape .note {
    font-size: 10px;
    font-family: var(--mono, ui-monospace, monospace);
    pointer-events: none;
  }
  .shape .note {
    font-size: 12px;
    font-family: inherit;
    pointer-events: all;
  }
  .shape .lab.big {
    font-size: 12px;
    font-weight: 600;
  }
  .shape .handle {
    pointer-events: all;
    cursor: grab;
    stroke: var(--surface);
    stroke-width: 1.5;
  }
  /* ── Quick backtest markers ── */
  .marks {
    pointer-events: none;
  }
  .marks.live .mark {
    pointer-events: all;
    cursor: pointer;
  }
  .marks.live .mark:hover {
    stroke-width: 2.5;
  }
  .qty {
    font-size: 9px;
    font-family: var(--mono, ui-monospace, monospace);
    pointer-events: none;
  }
  .quick-menu {
    position: absolute;
    z-index: 5;
    transform: translate(-50%, -50%);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-2);
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg, 0 6px 18px rgb(0 0 0 / 0.3));
  }
  .quick-menu .qrow {
    display: flex;
    gap: var(--space-1);
  }
  .qbtn {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 3px 10px;
    border-radius: var(--radius);
    border: 1px solid currentColor;
    background: transparent;
    font-size: var(--text-xs);
    cursor: pointer;
  }
  /* Blue long / red short, the quick-backtest convention — fixed, so it never collides with
     a user-recoloured candle palette. */
  .qbtn.long {
    color: #3b82f6;
  }
  .qbtn.short {
    color: #ef4444;
  }
  .qbtn:hover {
    background: color-mix(in srgb, currentColor 14%, transparent);
  }
  .qqty {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .qqty input {
    width: 68px;
    padding: 1px var(--space-1);
    font-size: var(--text-xs);
  }
  .qunit {
    font-size: var(--text-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .qhint {
    font-size: var(--text-xs);
    font-family: var(--mono);
    color: var(--muted);
  }

  .style-bar {
    position: absolute;
    z-index: 4;
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 3px 5px;
    background: color-mix(in srgb, var(--surface) 94%, transparent);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg, 0 6px 18px rgb(0 0 0 / 0.3));
    backdrop-filter: blur(2px);
  }
  .style-bar .swatch {
    width: 14px;
    height: 14px;
    border: 1px solid var(--border);
    border-radius: 50%;
    padding: 0;
    cursor: pointer;
  }
  .style-bar .swatch.on {
    box-shadow: 0 0 0 2px var(--surface), 0 0 0 3px currentColor;
  }
  .style-bar .pick {
    display: inline-flex;
    align-items: center;
    color: var(--muted);
    cursor: pointer;
  }
  .style-bar .pick input {
    width: 16px;
    height: 16px;
    padding: 0;
    border: none;
    background: transparent;
    cursor: pointer;
  }
  .style-bar .width {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 18px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
  }
  .style-bar .width span {
    display: block;
    width: 12px;
    background: var(--muted);
  }
  .style-bar .width.on {
    border-color: var(--border-control);
  }
  .style-bar .width.on span {
    background: var(--text);
  }
  .style-bar .bar-sep {
    width: 1px;
    height: 14px;
    background: var(--border);
    margin: 0 2px;
  }
  .style-bar .bar-del {
    display: inline-flex;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 2px;
  }
  .style-bar .bar-del:hover {
    color: var(--red);
  }
  .style-bar .bar-act {
    display: inline-flex;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 2px;
  }
  .style-bar .bar-act:hover,
  .style-bar .bar-act.on {
    color: var(--text);
  }

  /* Template list, hanging under the style bar it belongs to. */
  .tpl-pop {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 5;
    width: 190px;
    padding: var(--space-1);
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg, 0 6px 18px rgb(0 0 0 / 0.3));
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .tpl-row {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .tpl-apply {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px var(--space-1);
    background: none;
    border: 0;
    border-radius: var(--radius);
    color: var(--text);
    font-size: 0.74rem;
    text-align: left;
    cursor: pointer;
  }
  .tpl-apply:hover {
    background: var(--surface-2);
  }
  .tpl-apply .swatch {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex: none;
  }
  .tpl-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tpl-del {
    display: inline-flex;
    padding: 2px;
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .tpl-del:hover {
    color: var(--red);
  }
  .tpl-empty {
    margin: 0;
    padding: 2px var(--space-1);
    color: var(--muted);
    font-size: 0.72rem;
  }
  .tpl-new {
    display: flex;
    gap: 2px;
    margin-top: 2px;
    border-top: 1px solid var(--border);
    padding-top: var(--space-1);
  }
  .tpl-new input {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 0 var(--space-1);
    font-size: 0.74rem;
  }
  .tpl-new button,
  .tpl-def {
    padding: 2px var(--space-2);
    background: var(--surface-2);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    font-size: 0.72rem;
    cursor: pointer;
  }
  .tpl-def {
    margin-top: 2px;
    text-align: center;
  }
  .tpl-new button:hover,
  .tpl-def:hover {
    border-color: var(--accent);
  }

  .note-input {
    position: absolute;
    z-index: 4;
    width: 160px;
    height: 22px;
    padding: 0 var(--space-1);
    background: var(--surface);
    border: 1px solid var(--accent);
    color: var(--text);
    font-size: var(--text-sm);
  }

  .chart-host {
    position: relative;
    isolation: isolate;
    container-type: inline-size;
    width: 100%;
    height: 100%;
    min-height: 320px;
    outline: none;
  }

  /* The host is focusable so arrows/+/- reach the chart; show that only for keyboard users. */
  .chart-host:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .chart-host.fs {
    background: var(--surface);
    padding: var(--space-3);
  }
  .chart {
    width: 100%;
    height: 100%;
    min-height: 320px;
    cursor: crosshair;
    touch-action: none; /* we own the pan/zoom gestures */
  }
  .chart.panning {
    cursor: grabbing;
  }
  /* Custom log-scale left axis: gridlines from the y-axis edge, labels to its left. */
  .loggrid {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 3;
    overflow: hidden;
  }
  .lg-line {
    position: absolute;
    left: 50px;
    right: 16px;
  }
  .axis-right .lg-line {
    left: 16px;
    right: 50px;
    height: 0;
    border-top: 0.5px solid var(--grid-line);
  }
  .lg-label {
    position: absolute;
    left: 0;
    width: 46px;
    transform: translateY(-50%);
    text-align: right;
  }
  .axis-right .lg-label {
    left: auto;
    right: 0;
    text-align: left;
    color: var(--dim);
    font-family: var(--mono);
    font-size: 10px;
    line-height: 1;
  }
  .tags {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 4;
    overflow: hidden;
  }
  /* Crosshair price tag. Its background is set inline from the series color, which the
     user can override to any hex in chart settings — so no theme token can be the right
     ink here. White plus the dark 1px ring is the fixed pair, like a label on a marker. */
  .tag {
    position: absolute;
    left: 2px;
    transform: translateY(-50%);
    padding: 0 4px;
    border-radius: 0;
    font-family: var(--mono);
    font-size: var(--text-xs);
    line-height: 1.4;
    color: #fff;
    white-space: nowrap;
    outline: 1px solid var(--border);
    outline-offset: 0;
  }
  .axis-right .tag {
    left: auto;
    right: 2px;
  }
  /* The crosshair's scale label. Neutral ink on purpose — it is the cursor speaking, not a
     series, so it must not borrow a series colour. */
  .cursor-tag {
    position: absolute;
    left: 0;
    z-index: 5;
    transform: translateY(-50%);
    min-width: 46px;
    padding: 0 4px;
    text-align: right;
    background: var(--text);
    color: var(--bg);
    font-family: var(--mono);
    font-size: var(--text-xs);
    line-height: 1.4;
    white-space: nowrap;
    pointer-events: none;
  }
  .axis-right .cursor-tag {
    left: auto;
    right: 0;
    text-align: left;
  }
  /* Same ink as the crosshair tag (white on the series color, which the user can set to
     any hex), but pinned to the price gutter on the left. */
  .last-tag {
    position: absolute;
    left: 0;
    z-index: 4;
    transform: translateY(-50%);
    min-width: 46px;
    padding: 0 4px;
    text-align: right;
    font-family: var(--mono);
    font-size: var(--text-xs);
    line-height: 1.4;
    color: #fff;
    white-space: nowrap;
    pointer-events: none;
    outline: 1px solid var(--border);
  }
  .axis-right .last-tag {
    left: auto;
    right: 0;
    text-align: left;
  }
  .count-tag {
    position: absolute;
    left: 0;
    z-index: 4;
    min-width: 46px;
    padding: 0 4px;
    text-align: right;
    font-family: var(--mono);
    font-size: var(--text-xs);
    line-height: 1.4;
    background: var(--surface);
    white-space: nowrap;
    pointer-events: none;
    outline: 1px solid var(--border);
  }
  .axis-right .count-tag {
    left: auto;
    right: 0;
    text-align: left;
  }
  /* Top-right: the header now owns the top-left corner and can run long, so a centred nav
     collided with the OHLC readout. */
  .nav {
    position: absolute;
    top: 6px;
    right: 56px;
    z-index: 5;
    display: flex;
    gap: 2px;
    background: color-mix(in srgb, var(--surface) 82%, transparent);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    backdrop-filter: blur(2px);
  }
  .nav button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .nav button:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  /* Only shown once the price axis has been dragged off auto — the way back. */
  /* One per manually scaled pane, on that pane's gutter. An icon, not a word: it sits in a
     50px axis strip and an oscillator pane is barely 40px tall. */
  /* One per pane, on that pane's gutter. An icon, not a word: it sits in a 50px axis strip
     and an oscillator pane is barely 40px tall. Quiet while the pane auto-fits, lit and
     clickable once the scale has been dragged. */
  .autoscale {
    position: absolute;
    z-index: 5;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 18px;
    padding: 0;
    background: color-mix(in srgb, var(--surface) 88%, transparent);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--faint);
    cursor: default;
    backdrop-filter: blur(2px);
    opacity: 0;
    pointer-events: none;
    transition: opacity 90ms linear;
  }
  .autoscale.show {
    opacity: 1;
    pointer-events: auto;
  }
  .autoscale.manual {
    border-color: var(--accent);
    color: var(--accent);
    cursor: pointer;
  }
  .autoscale.manual:hover {
    background: var(--surface-2);
  }
  /* Sits under the top-right controls, out of the header's way. */
  .marks-dense {
    position: absolute;
    right: 6px;
    top: 34px;
    z-index: 5;
    background: color-mix(in srgb, var(--surface) 88%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px 8px;
    color: var(--muted);
    font-size: var(--text-xs);
    backdrop-filter: blur(2px);
    pointer-events: none;
  }
  /* History control: vertically centred against the price pane, at the left gutter — it
     marks where the loaded data ends, so it reads as the edge of history itself. */
  .hist-more,
  .hist-edge {
    position: absolute;
    left: 56px;
    top: 50%;
    transform: translateY(-50%);
    z-index: 5;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: color-mix(in srgb, var(--surface) 92%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 3px 8px;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .hist-more {
    cursor: pointer;
  }
  .hist-more:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border-control);
  }
  .hist-more:disabled {
    cursor: default;
  }
  /* The provider said no (depth, quota, key): keep the button, flag the reason. */
  .hist-more.blocked {
    color: var(--amber);
    border-color: var(--amber);
  }
  /* ── On-chart header (market-tool style) ──
     No box, no border: lines of text over the candles, each with its own controls that only
     appear on hover. */
  .head {
    position: absolute;
    top: 6px;
    left: 8px;
    z-index: 5;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    max-width: min(70%, 720px);
    font-size: var(--text-xs);
    pointer-events: none;
  }
  /* A sub-pane's title sits at that pane's own top-left corner (positions come from the
     layout, in pixels), so a windowed indicator is labelled where it is drawn. */
  .pane-head {
    top: auto;
    left: auto;
    max-width: 60%;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 5px;
    min-height: 17px;
    padding: 0 4px 0 2px;
    border-radius: var(--radius);
    color: var(--text);
    line-height: 1.35;
    pointer-events: auto;
  }
  .line:hover {
    background: color-mix(in srgb, var(--surface) 88%, transparent);
  }
  .line.off {
    opacity: 0.45;
  }
  .sym-line {
    font-size: var(--text-base);
  }
  .sym {
    padding: 0;
    background: transparent;
    border: none;
    color: var(--text);
    font: inherit;
    font-weight: var(--fw-medium);
    cursor: pointer;
  }
  .sym:hover {
    color: var(--accent);
  }
  .dot-sep {
    color: var(--muted);
  }
  .tf,
  .ex {
    color: var(--text);
  }
  .ohlc {
    display: flex;
    flex-wrap: wrap;
    gap: 0 8px;
    margin-left: var(--space-2);
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .ohlc b {
    margin-left: 3px;
    font-weight: var(--fw-medium);
    color: var(--red);
  }
  .ohlc.up b {
    color: var(--green);
  }
  .ohlc .chg {
    color: var(--red);
  }
  .ohlc.up .chg {
    color: var(--green);
  }
  .lbl {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .val {
    font-family: var(--mono);
    color: var(--muted);
    white-space: nowrap;
  }
  /* Per-line controls: hidden until the line is hovered, like every charting tool. */
  .btns {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    margin-left: 4px;
    opacity: 0;
    transition: opacity 0.1s ease;
  }
  .line:hover .btns,
  .btns:focus-within {
    opacity: 1;
  }
  .hbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 16px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .hbtn:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .head-toggle {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin-top: 1px;
    padding: 1px 5px;
    background: color-mix(in srgb, var(--surface) 82%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    pointer-events: auto;
  }
  .head-toggle:hover {
    color: var(--text);
    border-color: var(--border-control);
  }
  .count {
    font-family: var(--mono);
    color: var(--text);
  }
  /* Condensed icons (12×10) */
  .ico {
    flex: none;
    width: 12px;
    height: 10px;
    display: inline-block;
  }
  .ico.line-ico {
    height: 0;
    border-top: 2px solid var(--c);
    align-self: center;
  }
  .ico.line-ico.dashed {
    border-top-style: dashed;
  }
  .ico.dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    align-self: center;
  }
  /* Two-tone up/down swatch: left half --c, right half --c2 — a hard split, no gradient. */
  .ico.bar {
    position: relative;
    background: var(--c);
    border-radius: 0;
  }
  .ico.bar::after {
    content: '';
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 50%;
    background: var(--c2);
  }
  /* A pane in a 3x4 grid is a quarter of the width one has on its own, and the furniture a
     chart wears (the OHLC readout, the jump buttons) does not shrink with it: it wraps over
     the candles instead. Below these widths the overlays that have another way in are
     dropped, in the order the chart can most afford. The gestures they duplicate all stay:
     the crosshair still reads values out, and Home/End/0 still jump. */
  @container (max-width: 560px) {
    .nav {
      display: none;
    }
  }
  @container (max-width: 420px) {
    .head .ohlc {
      display: none;
    }
  }
  @container (max-width: 300px) {
    .head .dot-sep,
    .head .ex {
      display: none;
    }
  }
</style>
