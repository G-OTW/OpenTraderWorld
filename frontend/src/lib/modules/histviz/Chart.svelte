<script>
  import Icon from '$lib/ui/Icon.svelte';
  // ECharts price chart. Main pane: candlestick / OHLC bar / line / Renko. Below: volume,
  // then one shared sub-pane per distinct oscillator *type* in use (RSI, MACD, …). Overlay
  // indicators (SMA/EMA/Bollinger/…) draw on the price pane. Indicators come from the
  // `instances` prop ({id, type, params, visible}); the catalog's compute() produces the
  // series generically, so adding a catalog entry needs no change here.
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
  import { renko, catalogDef, instanceLabel } from './indicators.js';

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
    ontoggle,
    onfullscreen,
    onrestored,
    onloadmore
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

  onDestroy(() => chart?.dispose());

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

  // RFC3339 in, compact label out. Slicing beats Date formatting here: it runs per axis tick
  // on every frame, and the strings are already normalized UTC from the API.
  function fmtTs(s) {
    if (typeof s !== 'string' || s.length < 10) return s ?? '';
    return intraday ? `${s.slice(5, 10)} ${s.slice(11, 16)}` : s.slice(0, 10);
  }

  // ── Memoized indicator outputs ──
  // compute() used to run three times per render (once for the legend swatch, once for the
  // crosshair tags, once for the series) — on 10k bars with a few indicators that was the
  // hitch on every toggle. One map, three consumers.
  const computed = $derived.by(() => {
    const m = new Map();
    if (!view) return m;
    for (const ind of instances) {
      const def = catalogDef(ind.type);
      if (def) m.set(ind.id, def.compute(view, ind.params) ?? []);
    }
    return m;
  });

  // Visible indicator instances, split by pane kind.
  const active = $derived(instances.filter((i) => i.visible));
  const overlays = $derived(active.filter((i) => catalogDef(i.type)?.kind === 'overlay'));
  // Distinct oscillator types, in first-seen order → one shared pane each.
  const oscTypes = $derived.by(() => {
    const seen = [];
    for (const i of active) {
      const d = catalogDef(i.type);
      if (d?.kind === 'oscillator' && !seen.includes(i.type)) seen.push(i.type);
    }
    return seen;
  });

  let legendOpen = $state(true);
  // Built-in series (price + volume) are toggleable from the legend like indicators.
  let priceVisible = $state(true);
  let volumeVisible = $state(true);
  // Manual price-axis range, set by dragging the y gutter. null = auto-fit the window.
  let yMan = $state(null);
  let hoverIdx = $state(null); // crosshair bar index, null when the cursor is off-chart
  let tags = $state([]); // [{ label, color, top(px), value }]
  let logGrid = $state([]); // [{ top(px), text }] custom left-axis labels for the log scale
  let lastTag = $state(null); // { top(px), text, color } — latest close on the price axis
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

  function setWin(s, e) {
    win = clampWin(s, e);
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

  // Current price-pane extent in data units, read through the public pixel API.
  function priceExtent() {
    const r = rect(0);
    if (!r || !chart) return null;
    const hi = chart.convertFromPixel({ gridIndex: 0 }, [0, r.y]);
    const lo = chart.convertFromPixel({ gridIndex: 0 }, [0, r.y + r.height]);
    if (!hi || !lo) return null;
    const max = hi[1];
    const min = lo[1];
    return Number.isFinite(min) && Number.isFinite(max) && max > min ? { min, max } : null;
  }

  function onPointerDown(e) {
    if (e.button !== 0 || !chart) return;
    const box = el.getBoundingClientRect();
    const x = e.clientX - box.left;
    const y = e.clientY - box.top;
    const zone = hitZone(x, y);
    if (zone === 'none') return;
    host?.focus?.({ preventScroll: true });
    drag = {
      zone,
      x0: e.clientX,
      y0: e.clientY,
      win0: { ...win },
      ext0: zone === 'yaxis' ? priceExtent() : null,
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

  function onPointerMove(e) {
    if (!drag || !chart) return;
    const dx = e.clientX - drag.x0;
    const dy = e.clientY - drag.y0;
    if (!drag.moved && Math.abs(dx) < 2 && Math.abs(dy) < 2) return;
    drag.moved = true;

    if (drag.zone === 'plot') {
      const r = rect(0);
      if (!r) return;
      const w = drag.win0.e - drag.win0.s;
      const shift = (-dx * w) / r.width; // 1:1 with the cursor
      setWin(drag.win0.s + shift, drag.win0.e + shift);
    } else if (drag.zone === 'xaxis') {
      // Drag the date strip to zoom, right edge pinned — the market-tool convention.
      const r = rect(0);
      if (!r) return;
      const f = Math.exp(-dx / Math.max(80, r.width / 4));
      const w = (drag.win0.e - drag.win0.s) * f;
      setWin(drag.win0.e - w, drag.win0.e);
    } else if (drag.zone === 'yaxis') {
      // Drag the price gutter to scale it manually; the pane centre stays put.
      const r = rect(0);
      if (!r) return;
      const f = Math.exp(dy / Math.max(80, r.height / 2));
      const ext = drag.ext0;
      if (priceLogNow()) {
        const lo = Math.log(Math.max(ext.min, Number.EPSILON));
        const hi = Math.log(Math.max(ext.max, Number.EPSILON * 2));
        const mid = (lo + hi) / 2;
        const half = ((hi - lo) / 2) * f;
        yMan = { min: Math.exp(mid - half), max: Math.exp(mid + half) };
      } else {
        const mid = (ext.min + ext.max) / 2;
        const half = ((ext.max - ext.min) / 2) * f;
        yMan = { min: mid - half, max: mid + half };
      }
    }
  }

  function onPointerUp(e) {
    if (!drag) return;
    el?.releasePointerCapture?.(e.pointerId);
    drag = null;
    panning = false;
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
    if (zone === 'yaxis') yMan = null; // back to auto-fit
    else if (zone !== 'none') zoomFit();
  }

  function onKeyDown(e) {
    if (!chart || !n) return;
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
    yMan = null;
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

  const fmtPrice = (v) => {
    if (v >= 1000) return v.toLocaleString(undefined, { maximumFractionDigits: 0 });
    if (v >= 1) return v.toFixed(2);
    return v.toPrecision(3);
  };
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
    const chg = o ? ((c - o) / o) * 100 : 0;
    return {
      o: fmtPrice(o),
      h: fmtPrice(view.h[i]),
      l: fmtPrice(view.l[i]),
      c: fmtPrice(c),
      chg: `${chg >= 0 ? '+' : ''}${chg.toFixed(2)}%`,
      up: c >= o,
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
      const vals = [];
      if (i != null) {
        for (const o of outs) {
          const x = o.data?.[i];
          if (x != null && Number.isFinite(x)) vals.push(fmtVal(x));
        }
      }
      rows.push({
        id: ind.id,
        label: instanceLabel(ind.type, ind.params),
        color: swatch,
        icon: out?.kind === 'scatter' ? 'dot' : out?.kind === 'bar' ? 'bar' : 'line',
        dashed: !!out?.dashed,
        hidden: !ind.visible,
        value: vals.join(' ')
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
    const out = [];
    if (priceVisible) {
      out.push({
        label: type === 'line' ? $t('histviz.chart.close') : $t('histviz.chart.price'),
        color: palette.accent,
        data: view.c,
        gridIndex: 0
      });
    }
    for (const ind of instances) {
      if (!ind.visible) continue;
      const def = catalogDef(ind.type);
      if (!def) continue;
      // Overlays live on pane 0; oscillators on their shared pane (2 + position).
      let gridIndex = 0;
      if (def.kind === 'oscillator') {
        const pos = oscTypes.indexOf(ind.type);
        if (pos < 0) continue;
        gridIndex = 2 + pos;
      }
      for (const o of computed.get(ind.id) ?? []) {
        if (o.kind === 'bar' || o.kind === 'scatter') continue; // tag lines only
        out.push({
          label: o.name,
          color: ind.style?.color || (o.color === 'updown' ? palette.up : paletteColor(o.color)),
          data: o.data,
          gridIndex
        });
      }
    }
    return out;
  });

  // convertToPixel per series per mousemove was the other hot path — coalesce into one frame.
  let tagRaf = 0;
  let tagIdx = null;
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
      const px = chart.convertToPixel({ gridIndex: s.gridIndex }, [dataIndex, val]);
      if (!px) continue;
      next.push({ label: s.label, color: s.color, top: px[1], value: fmtVal(val) });
    }
    tags = next;
  }

  function color(key, p, fallback) {
    return p[key] ?? fallback ?? p.accent;
  }

  // ────────────────────────── Price-axis range ──────────────────────────

  // The log axis applies to the price pane only, and only when every low is positive.
  function priceLogNow() {
    return settings.scale === 'log' && type !== 'renko' && !!view && view.l.every((x) => x > 0);
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
      if (px) out.push({ top: px[1], text: fmtPrice(val) });
    }
    logGrid = out;
  }

  // ────────────────────────── Option builder ──────────────────────────

  function buildOption(v, p) {
    const oscCount = oscTypes.length;

    // Grid layout: price (large), volume (thin), then oscillator panes.
    const top = 8;
    const volH = 12;
    const oscH = oscCount ? Math.min(16, 40 / oscCount) : 0;
    const priceBottom = 100 - volH - oscCount * oscH - 8;
    const layout = [{ left: gridLeft, right: gridRight, topPct: top, heightPct: priceBottom - top }];
    const volTop = priceBottom + 2;
    layout.push({ left: gridLeft, right: gridRight, topPct: volTop, heightPct: volH - 2 });
    oscTypes.forEach((_, i) => {
      layout.push({
        left: gridLeft,
        right: gridRight,
        topPct: volTop + volH + i * oscH,
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
    const range = yMan ?? (priceLog ? visibleLogRange(v) : null);
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
      splitLine: { show: i === 0 && showV, lineStyle: { color: p.gridLine, width: 0.5 } }
    }));

    const yAxes = grids.map((_, i) => {
      const isPrice = i === 0;
      const isLog = isPrice && priceLog;
      const axis = {
        gridIndex: i,
        type: isLog ? 'log' : 'value',
        position: axisRight ? 'right' : 'left',
        // A fixed range (manual drag, or the log fit) replaces `scale` auto-fitting.
        scale: !isLog && !(isPrice && range),
        axisLine: { show: false },
        axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 },
        // Per-series values are drawn as our own tags on the axis side, so hide ECharts'.
        axisPointer: { label: { show: false } },
        splitLine: { show: i === 0 && showH, lineStyle: { color: p.gridLine, width: 0.5 } }
      };
      if (isPrice && range) {
        axis.min = range.min;
        axis.max = range.max;
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

    const series = [];

    // A plain line series; `st` is an optional per-instance style override {color, width}.
    // Note: fill is intentionally NOT handled here — only band/channel indicators fill, and
    // they do so between their edges (see the band-fill block below), never down to the axis.
    const lineSeries = (name, data, col, xi, width = 1.5, dashed = false, st = null) => ({
      name,
      type: 'line',
      data,
      showSymbol: false,
      xAxisIndex: xi,
      yAxisIndex: xi,
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
      const data = v.ts.map((_, i) => [v.o[i], v.c[i], v.l[i], v.h[i]]);
      series.push({
        name: $t('histviz.chart.price'),
        type: 'candlestick',
        data,
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
      if (def.fillable && ind.style?.fill) {
        const upper = outs.find((o) => o.role === 'upper')?.data;
        const lower = outs.find((o) => o.role === 'lower')?.data;
        if (upper && lower) {
          const delta = upper.map((u, i) => (u != null && lower[i] != null ? u - lower[i] : null));
          const stackId = `band-${ind.id}`;
          series.push({
            name: `${def.type}-base-${ind.id}`,
            type: 'line',
            data: lower,
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
            large: true,
            xAxisIndex: 0,
            yAxisIndex: 0,
            itemStyle: { color: ind.style?.color || color(o.color, p) }
          });
        } else if (o.kind === 'line') {
          series.push(lineSeries(o.name, o.data, color(o.color, p), 0, 1.4, o.dashed, ind.style));
        }
      }
    }

    // ── Volume (pane 1) ──
    // Split into up/down series rather than one series with a per-item color callback: the
    // callback ran per bar per frame and blocks `large`. Overlapped with barGap -100%.
    if (volumeVisible) {
      const upV = v.v.map((x, i) => (v.c[i] >= v.o[i] ? x : null));
      const dnV = v.v.map((x, i) => (v.c[i] >= v.o[i] ? null : x));
      const volBase = {
        type: 'bar',
        xAxisIndex: 1,
        yAxisIndex: 1,
        large: true,
        largeThreshold: 400,
        barGap: '-100%'
      };
      series.push({ ...volBase, name: $t('histviz.chart.volume'), data: upV, itemStyle: { color: p.up } });
      series.push({ ...volBase, name: `${$t('histviz.chart.volume')} ▾`, data: dnV, itemStyle: { color: p.down } });
    }

    // ── Oscillator panes (one per type, shared by instances of that type) ──
    oscTypes.forEach((ot, i) => {
      const xi = 2 + i;
      for (const ind of active.filter((a) => a.type === ot)) {
        for (const o of computed.get(ind.id) ?? []) {
          if (o.kind === 'bar') {
            series.push({
              name: o.name,
              type: 'bar',
              data: o.data,
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
    });
    ro.observe(el);
    // Drive the custom value tags and the legend readout off the linked crosshair.
    chart.on('updateAxisPointer', (e) => {
      const xInfo = e.axesInfo?.find((a) => a.axisDim === 'x');
      const idx = xInfo ? xInfo.value : null;
      hoverIdx = idx;
      requestTags(idx);
    });
    chart.getZr().on('globalout', () => {
      hoverIdx = null;
      tags = [];
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
      if (tagRaf) cancelAnimationFrame(tagRaf);
    };
  });

  // Refit the geometry when the parent toggles fullscreen (the box changes size in one frame).
  $effect(() => {
    void fullscreen;
    requestAnimationFrame(() => {
      chart?.resize();
      updateLogGrid();
      updateLastTag();
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
    const added = len - lastLen;
    lastLen = len;
    lastFirst = first;
    lastLast = last;
    if (prepended) {
      untrack(() => setWin(win.s + added, win.e + added));
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
    yMan = null;
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
    tags = []; // positions are stale after a re-render; the next cursor move repopulates
    updateLogGrid(); // custom left-axis labels/gridlines for the log price scale
    updateLastTag();
  });

  // Log scale only: refit the price axis as the window moves. A *merged* setOption touching
  // just yAxis[0] leaves the dataZoom components alone, so an in-progress drag survives.
  $effect(() => {
    void [win.s, win.e];
    if (!chart || !view || yMan || !priceLogNow()) return;
    const lr = untrack(() => visibleLogRange(view));
    if (!lr) return;
    logRange = lr;
    chart.setOption({ yAxis: [{ min: lr.min, max: lr.max }] });
    updateLogGrid();
    updateLastTag();
  });

  // ── Public API (bind:this) — the parent uses this to keep the same dates across a
  // timeframe switch. Returns the visible span clamped to real bars. ──
  export function getTimeRange() {
    if (!view || !n) return null;
    const { from, to } = dataWin();
    if (to <= from) return null;
    return { t0: view.ts[from], t1: view.ts[to] };
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
>
  <div class="chart" class:panning bind:this={el}></div>

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

  <!-- Latest close, on the price axis it belongs to (left gutter), tinted like the candle
       that produced it. -->
  {#if view && lastTag}
    <span class="last-tag" style:top="{lastTag.top}px" style:background={lastTag.color}
      >{lastTag.text}</span
    >
  {/if}

  {#if view}
    <div class="nav">
      <button onclick={goStart} title={$t('histviz.chart.jumpToStart')}>
        <Icon name="chevron-left" size={12} /> {$t('histviz.chart.start')}
      </button>
      <button onclick={zoomFit} title={$t('histviz.chart.fitToDefault')}>⤢ {$t('histviz.chart.fit')}</button>
      <button onclick={goEnd} title={$t('histviz.chart.jumpToLatest')}>
        {$t('histviz.chart.end')} <Icon name="chevron-right" size={12} />
      </button>
      <button
        onclick={() => onfullscreen?.()}
        title={fullscreen ? $t('histviz.chart.exitFullscreen') : $t('histviz.chart.fullscreen')}
        aria-label={$t('histviz.chart.toggleFullscreen')}
      >
        <Icon name={fullscreen ? 'chevrons-down-up' : 'maximize'} size={12} />
      </button>
    </div>
  {/if}

  {#if yMan}
    <button class="autoscale" onclick={() => (yMan = null)} title={$t('histviz.chart.autoScaleHint')}>
      {$t('histviz.chart.autoScale')}
    </button>
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

  {#if view && legend.length}
    <div class="legend" class:collapsed={!legendOpen}>
      <button class="legend-toggle" onclick={() => (legendOpen = !legendOpen)} title={$t('histviz.chart.toggleLegend')}>
        <span class="caret"><Icon name={legendOpen ? 'chevron-down' : 'chevron-right'} size={12} /></span>
        <span class="ttl">{$t('histviz.chart.series')}</span>
        {#if !legendOpen}<span class="count">{legend.length}</span>{/if}
      </button>
      {#if legendOpen}
        <div class="legend-body">
          {#each legend as row, li (li)}
            <button
              type="button"
              class="legend-row clickable"
              class:hidden={row.hidden}
              title={row.hidden ? $t('histviz.panel.show') : $t('histviz.panel.hide')}
              onclick={() => toggleRow(row)}
            >
              {#if row.icon === 'candle'}
                <span class="ico candle" style:--c={row.color} style:--c2={row.color2}></span>
              {:else if row.icon === 'bar'}
                <span class="ico bar" style:--c={row.color} style:--c2={row.color2 ?? row.color}></span>
              {:else if row.icon === 'dot'}
                <span class="ico dot" style:background={row.color}></span>
              {:else}
                <span class="ico line" class:dashed={row.dashed} style:--c={row.color}></span>
              {/if}
              <span class="lbl">{row.label}</span>
              {#if row.value}<span class="val">{row.value}</span>{/if}
            </button>
            <!-- The price row carries the OHLC readout market tools put in the top-left
                 corner: it follows the crosshair and falls back to the newest visible bar. -->
            {#if row.builtin === 'price' && ohlc && !row.hidden}
              <div class="ohlc" class:up={ohlc.up}>
                <span>O <b>{ohlc.o}</b></span>
                <span>H <b>{ohlc.h}</b></span>
                <span>L <b>{ohlc.l}</b></span>
                <span>C <b>{ohlc.c}</b></span>
                <span class="chg">{ohlc.chg}</span>
              </div>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* `isolation: isolate` contains the overlay children below (crosshair, tags, nav).
     Their z-indexes are a private 1..5 order *within* this chart, not rungs on the
     global --z-* ladder; without a stacking context they'd leak into the page's root
     order and silently compete with real layers. */
  .chart-host {
    position: relative;
    isolation: isolate;
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
  .nav {
    position: absolute;
    top: 6px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 5;
    display: flex;
    gap: 4px;
  }
  .nav button {
    background: color-mix(in srgb, var(--surface) 82%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px 8px;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
    backdrop-filter: blur(2px);
  }
  .nav button:hover {
    color: var(--text);
    border-color: var(--border-control);
  }
  /* Only shown once the price axis has been dragged off auto — the way back. */
  .autoscale {
    position: absolute;
    right: 6px;
    bottom: 28px;
    z-index: 5;
    background: color-mix(in srgb, var(--surface) 88%, transparent);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    padding: 2px 8px;
    color: var(--text);
    font-size: var(--text-xs);
    cursor: pointer;
    backdrop-filter: blur(2px);
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
  .legend {
    position: absolute;
    top: 6px;
    left: 6px;
    z-index: 5;
    max-width: 280px;
    background: color-mix(in srgb, var(--surface) 82%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    backdrop-filter: blur(2px);
    font-size: var(--text-xs);
    overflow: hidden;
  }
  .legend-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    background: transparent;
    border: none;
    padding: 2px 6px;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .legend-toggle:hover {
    color: var(--text);
  }
  .caret {
    font-size: var(--text-xs);
  }
  .count {
    margin-left: auto;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0 4px;
    color: var(--text);
  }
  .legend-body {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 2px 6px 4px;
    max-height: 240px;
    overflow-y: auto;
  }
  .legend-row {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    background: transparent;
    border: none;
    padding: 1px 2px;
    border-radius: var(--radius);
    color: var(--text);
    font-size: inherit;
    line-height: 1.35;
    text-align: left;
  }
  .legend-row.clickable {
    cursor: pointer;
  }
  .legend-row.clickable:hover {
    background: var(--surface-2);
  }
  .legend-row.hidden {
    opacity: 0.4;
  }
  .legend-row.hidden .lbl {
    text-decoration: line-through;
  }
  .lbl {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .val {
    margin-left: auto;
    padding-left: var(--space-2);
    font-family: var(--mono);
    color: var(--muted);
    white-space: nowrap;
  }
  .ohlc {
    display: flex;
    flex-wrap: wrap;
    gap: 0 6px;
    padding: 0 2px 2px 19px;
    font-family: var(--mono);
    font-size: var(--text-xs);
    line-height: 1.3;
    color: var(--muted);
  }
  .ohlc b {
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .ohlc .chg {
    color: var(--red);
  }
  .ohlc.up .chg {
    color: var(--green);
  }
  /* Condensed icons (12×10) */
  .ico {
    flex: none;
    width: 12px;
    height: 10px;
    display: inline-block;
  }
  .ico.line {
    height: 0;
    border-top: 2px solid var(--c);
    align-self: center;
  }
  .ico.line.dashed {
    border-top-style: dashed;
  }
  .ico.dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    align-self: center;
  }
  /* Two-tone up/down swatch: left half --c, right half --c2 — a hard split, no gradient. */
  .ico.candle,
  .ico.bar {
    position: relative;
    background: var(--c);
    border-radius: 0;
  }
  .ico.candle::after,
  .ico.bar::after {
    content: '';
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 50%;
    background: var(--c2);
  }
</style>
