<script>
  import Icon from '$lib/ui/Icon.svelte';
  // ECharts price chart. Main pane: candlestick / OHLC bar / line / Renko. Below: volume,
  // then one shared sub-pane per distinct oscillator *type* in use (RSI, MACD, …). Overlay
  // indicators (SMA/EMA/Bollinger/…) draw on the price pane. Indicators come from the
  // `instances` prop ({id, type, params, visible}); the catalog's compute() produces the
  // series generically, so adding a catalog entry needs no change here.
  import { onMount, onDestroy, untrack } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t } from '$lib/i18n';
  import { renko, catalogDef, instanceLabel } from './indicators.js';

  let { bars = null, type = 'candlestick', instances = [], brick = 0, settings = {}, ontoggle } = $props();

  let el;
  let host;
  let fullscreen = $state(false);
  // Reactive so the render effect re-runs once the chart is initialized in onMount
  // (otherwise the first setOption is skipped by the `!chart` guard and never retried).
  let chart = $state(null);

  function toggleFullscreen() {
    if (document.fullscreenElement) document.exitFullscreen?.();
    else host?.requestFullscreen?.();
  }

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => {
      chart?.resize();
      updateLogGrid(); // pixel positions changed
    });
    ro.observe(el);
    const onFsChange = () => {
      fullscreen = document.fullscreenElement === host;
      requestAnimationFrame(() => {
        chart?.resize();
        updateLogGrid();
      });
    };
    document.addEventListener('fullscreenchange', onFsChange);
    // Drive the custom value tags off the linked crosshair (x-category index).
    chart.on('updateAxisPointer', (e) => {
      const xInfo = e.axesInfo?.find((a) => a.axisDim === 'x');
      updateTags(xInfo ? xInfo.value : null);
    });
    chart.getZr().on('globalout', () => (tags = []));
    // Track the visible x-window so the log price axis can fit the *visible* range, not the
    // whole dataset (otherwise a zoomed-in slice looks tiny against full-history bounds).
    chart.on('datazoom', () => {
      const dz = chart.getOption()?.dataZoom?.[0];
      if (dz) zoomPct = { start: dz.start ?? 0, end: dz.end ?? 100 };
    });
    return () => {
      ro.disconnect();
      document.removeEventListener('fullscreenchange', onFsChange);
    };
  });
  onDestroy(() => chart?.dispose());

  // Theme palette, with per-setting color overrides (blank setting = keep theme).
  // chartColors() is what makes this derived depend on the theme: a bare
  // getComputedStyle read is not reactive, so the canvas would keep the old
  // palette until the next data change.
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

  // ── In-chart legend model (HTML overlay, replaces the ECharts legend) ──
  // One row per active indicator instance: small color swatch + label. Price/Volume lead.
  function paletteColor(key) {
    const p = palette;
    return p[key] ?? p.accent;
  }
  // All instances are listed (visible + hidden) so a hidden one can be clicked back on.
  const legend = $derived.by(() => {
    const rows = [
      {
        ...(type === 'line'
          ? { label: $t('histviz.chart.close'), color: palette.accent, icon: 'line' }
          : { label: $t('histviz.chart.price'), color: palette.up, color2: palette.down, icon: 'candle' }),
        builtin: 'price',
        hidden: !priceVisible
      },
      { label: $t('histviz.chart.volume'), color: palette.up, color2: palette.down, icon: 'bar', builtin: 'volume', hidden: !volumeVisible }
    ];
    for (const ind of instances) {
      const def = catalogDef(ind.type);
      const out = view ? def?.compute(view, ind.params)?.[0] : null;
      const swatch =
        ind.style?.color || (out?.color === 'updown' ? palette.up : paletteColor(out?.color));
      rows.push({
        id: ind.id,
        label: instanceLabel(ind.type, ind.params),
        color: swatch,
        icon: out?.kind === 'scatter' ? 'dot' : out?.kind === 'bar' ? 'bar' : 'line',
        dashed: !!out?.dashed,
        hidden: !ind.visible
      });
    }
    return rows;
  });

  let legendOpen = $state(true);
  // Built-in series (price + volume) are toggleable from the legend like indicators.
  let priceVisible = $state(true);
  let volumeVisible = $state(true);
  let defaultStart = 0; // last-built default zoom start (%), used by the Fit button

  // Legend row click: toggle a built-in series locally, or an indicator via the parent.
  function toggleRow(row) {
    if (row.builtin === 'price') priceVisible = !priceVisible;
    else if (row.builtin === 'volume') volumeVisible = !volumeVisible;
    else if (row.id != null) ontoggle?.(row.id);
  }

  // Read the current x zoom window [start,end] in percent from the inside dataZoom.
  function currentWindow() {
    const dz = chart?.getOption()?.dataZoom?.[0];
    return { start: dz?.start ?? 0, end: dz?.end ?? 100 };
  }
  function setWindow(start, end) {
    chart?.dispatchAction({ type: 'dataZoom', start, end });
  }
  // Fit: back to the default view (recent window for large sets, all for small).
  function zoomFit() {
    setWindow(defaultStart, 100);
  }
  // Jump to the beginning / end while keeping the current window width.
  function goStart() {
    const w = currentWindow();
    const width = Math.max(1, w.end - w.start);
    setWindow(0, width);
  }
  function goEnd() {
    const w = currentWindow();
    const width = Math.max(1, w.end - w.start);
    setWindow(100 - width, 100);
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
      const kind = def.kind;
      // Overlays live on pane 0; oscillators on their shared pane (2 + position).
      let gridIndex = 0;
      if (kind === 'oscillator') {
        const pos = oscTypes.indexOf(ind.type);
        if (pos < 0) continue;
        gridIndex = 2 + pos;
      }
      const outs = def.compute(view, ind.params);
      for (const o of outs) {
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

  let tags = $state([]); // [{ label, color, top(px), value }]
  let zoomPct = $state({ start: 0, end: 100 }); // current visible x-window (percent)
  let logGrid = $state([]); // [{ top(px), text }] custom left-axis labels for log price scale
  let logRange = { min: 0, max: 0 }; // set by buildOption when log scale is active

  const fmtPrice = (v) => {
    if (v >= 1000) return v.toLocaleString(undefined, { maximumFractionDigits: 0 });
    if (v >= 1) return v.toFixed(2);
    return v.toPrecision(3);
  };

  // Recompute the log-scale left-axis labels/gridlines from the current pixel geometry.
  function updateLogGrid() {
    if (!chart || settings.scale !== 'log' || !logRange.max) {
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
  const fmtVal = (x) =>
    Math.abs(x) >= 1000 ? x.toLocaleString(undefined, { maximumFractionDigits: 2 }) : x.toPrecision(5);

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

  // Price-pane log range fitted to the *visible* window (from the current zoom), so a
  // zoomed-in slice fills the pane instead of being compressed against the full-history
  // extent. Returns null when the log scale doesn't apply. Reads zoomPct — callers decide
  // whether that read is tracked.
  function visibleLogRange(v) {
    if (settings.scale !== 'log' || type === 'renko' || !v || !v.l.every((x) => x > 0)) return null;
    const n = v.ts.length;
    const from = Math.max(0, Math.floor((zoomPct.start / 100) * (n - 1)));
    const to = Math.min(n - 1, Math.ceil((zoomPct.end / 100) * (n - 1)));
    let lo = Infinity;
    let hi = -Infinity;
    for (let k = from; k <= to; k++) {
      if (v.l[k] < lo) lo = v.l[k];
      if (v.h[k] > hi) hi = v.h[k];
    }
    if (!Number.isFinite(lo) || !Number.isFinite(hi)) {
      lo = Math.min(...v.l);
      hi = Math.max(...v.h);
    }
    return { min: lo / 1.01, max: hi * 1.01 };
  }

  function buildOption(v, p) {
    const n = v.ts.length;
    const oscCount = oscTypes.length;

    // Grid layout: price (large), volume (thin), then oscillator panes.
    const top = 8;
    const volH = 12;
    const oscH = oscCount ? Math.min(16, 40 / oscCount) : 0;
    const priceBottom = 100 - volH - oscCount * oscH - 8;
    const grids = [{ left: 50, right: 16, top: `${top}%`, height: `${priceBottom - top}%` }];
    const volTop = priceBottom + 2;
    grids.push({ left: 50, right: 16, top: `${volTop}%`, height: `${volH - 2}%` });
    oscTypes.forEach((_, i) => {
      grids.push({
        left: 50,
        right: 16,
        top: `${volTop + volH + i * oscH}%`,
        height: `${oscH - 2}%`
      });
    });

    // Price-pane grid lines are user-toggleable; log scale applies to the price pane only
    // (a value axis with non-positive values can't be logarithmic).
    const showH = settings.grid !== false;
    const showV = settings.gridV === true;
    const showCrosshair = settings.crosshair !== false;
    const showTooltip = settings.tooltip === true;
    // Value tags need the crosshair (they ride its position event).
    const showTags = showCrosshair && settings.crosshairTags !== false;
    // Log scale applies to the price pane only, and only when all lows are positive
    // (a log axis can't plot ≤0). A log axis does NOT honor `scale` to tighten its range,
    // so we pin min/max to the visible extent (see visibleLogRange) — otherwise it spans
    // from a low power of 10 up past the data and squashes everything into a flat sliver.
    const lr = visibleLogRange(v);
    const priceLog = !!lr;
    let logMin = 0;
    let logMax = 0;
    if (lr) {
      logMin = lr.min;
      logMax = lr.max;
      logRange = lr;
    }

    const xAxes = grids.map((_, i) => ({
      type: 'category',
      gridIndex: i,
      data: v.ts,
      boundaryGap: true,
      axisLine: { lineStyle: { color: p.border } },
      axisLabel: { show: i === grids.length - 1, color: p.dim, fontFamily: p.mono, fontSize: 10 },
      axisTick: { show: false },
      // Date label on the bottom axis only; the crosshair links all panes.
      axisPointer: {
        label: {
          show: showTags && i === grids.length - 1,
          backgroundColor: p.surface,
          borderColor: p.border,
          borderWidth: 1,
          fontFamily: p.mono,
          color: p.text
        }
      },
      splitLine: { show: i === 0 && showV, lineStyle: { color: p.gridLine, width: 0.5 } }
    }));
    const yAxes = grids.map((_, i) => {
      const isLog = i === 0 && priceLog;
      const axis = {
        gridIndex: i,
        type: isLog ? 'log' : 'value',
        scale: !isLog, // value axes fit via `scale`; log axes fit via explicit min/max below
        axisLine: { show: false },
        axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 },
        // Per-series values are drawn as custom right-edge tags, so hide the axis pointer label.
        axisPointer: { label: { show: false } },
        splitLine: { show: i === 0 && showH, lineStyle: { color: p.gridLine, width: 0.5 } }
      };
      if (isLog) {
        // Pin the range to the data so the price fills the pane. ECharts' own log-axis ticks
        // are unreliable over a sub-decade range (often just min+max), so we hide them and
        // draw our own evenly log-spaced labels/gridlines as an overlay (see logGrid).
        axis.min = logMin;
        axis.max = logMax;
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
    const lineSeries = (name, data, col, xi, width = 1.5, dashed = false, st = null) => {
      const w = st?.width ? Number(st.width) : width;
      const c = st?.color || col;
      return {
        name,
        type: 'line',
        data,
        showSymbol: false,
        xAxisIndex: xi,
        yAxisIndex: xi,
        lineStyle: { width: w, color: c, type: dashed ? 'dashed' : 'solid' },
        connectNulls: false
      };
    };

    // ── Main price series (pane 0) ──
    if (!priceVisible) {
      // skip — hidden via legend
    } else if (type === 'line') {
      series.push(lineSeries($t('histviz.chart.close'), v.c, p.accent, 0, 1.5));
    } else {
      const ohlc = v.ts.map((_, i) => [v.o[i], v.c[i], v.l[i], v.h[i]]);
      series.push({
        name: $t('histviz.chart.price'),
        type: 'candlestick',
        data: ohlc,
        xAxisIndex: 0,
        yAxisIndex: 0,
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
      const outs = def.compute(v, ind.params);

      // Band/channel indicators fill the region *between* their upper and lower edges when a
      // fill color is set. Implemented as two stacked series: an invisible lower baseline
      // plus the (upper−lower) delta carrying the area — so the fill hugs the band exactly.
      if (def.fillable && ind.style?.fill) {
        const upper = outs.find((o) => o.role === 'upper')?.data;
        const lower = outs.find((o) => o.role === 'lower')?.data;
        if (upper && lower) {
          const delta = upper.map((u, i) =>
            u != null && lower[i] != null ? u - lower[i] : null
          );
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
    if (volumeVisible) {
      series.push({
        name: $t('histviz.chart.volume'),
        type: 'bar',
        data: v.v,
        xAxisIndex: 1,
        yAxisIndex: 1,
        itemStyle: {
          color: (params) => (v.c[params.dataIndex] >= v.o[params.dataIndex] ? p.up : p.down)
        }
      });
    }

    // ── Oscillator panes (one per type, shared by instances of that type) ──
    oscTypes.forEach((ot, i) => {
      const xi = 2 + i;
      for (const ind of active.filter((a) => a.type === ot)) {
        const def = catalogDef(ind.type);
        const outs = def.compute(v, ind.params);
        for (const o of outs) {
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
    const zoomStart = Math.max(0, 100 - (n > 200 ? 20000 / n : 100));
    defaultStart = zoomStart; // remembered for the Fit button

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
          type: 'inside',
          xAxisIndex: allX,
          // Preserve the current window across re-renders so setOption doesn't reset the zoom
          // (which would fight the datazoom→refit loop). Defaults are applied on dataset load.
          start: zoomPct.start,
          end: zoomPct.end,
          // Left-drag inside the chart pans the window; wheel zooms. Without these the inside
          // dataZoom only responds to the wheel, so the chart feels stuck horizontally.
          moveOnMouseMove: true,
          moveOnMouseWheel: false,
          zoomOnMouseWheel: true,
          // Gentler wheel zoom — the default (1) makes a single notch jump too far.
          zoomOnMouseWheelSpeed: 0.1,
          // ECharts' internal pan dispatch always animates over 100ms (roams.js, not
          // overridable via the global `animation:false`). With a small/zero throttle the
          // animations stack and interrupt each other, so a drag only nudges the window a
          // little. Match the throttle to the 100ms animation so each pan settles before the
          // next fires → the window tracks the drag smoothly.
          throttle: 100
        },
        { type: 'slider', xAxisIndex: allX, bottom: 4, height: 16, start: zoomPct.start, end: zoomPct.end, textStyle: { color: p.dim, fontFamily: p.mono } }
      ],
      series
    };
  }

  // On a new dataset (or chart-type change that rebuilds the series), reset the zoom window
  // to the default so we don't carry a stale window onto different-length data.
  let lastViewLen = -1;
  let lastType = null;
  $effect(() => {
    const len = view?.ts?.length ?? 0;
    if (len && (len !== lastViewLen || type !== lastType)) {
      lastViewLen = len;
      lastType = type;
      const start = Math.max(0, 100 - (len > 200 ? 20000 / len : 100));
      zoomPct = { start, end: 100 };
    }
  });

  $effect(() => {
    if (!chart || !view) return;
    // Explicitly touch settings + inputs so the effect re-runs on any of them (buildOption
    // reads them internally, but we make the dependency unambiguous here).
    void [settings.scale, settings.grid, settings.gridV, settings.upColor, settings.downColor, settings.lineColor, settings.crosshair, settings.crosshairTags, settings.tooltip, type, brick, instances, priceVisible, volumeVisible];
    void palette;
    if (settings.scale !== 'log') logRange = { min: 0, max: 0 };
    // buildOption reads zoomPct (dataZoom start/end) — untrack it, or every drag-move's
    // datazoom event re-runs this effect, and a notMerge setOption mid-gesture rebuilds
    // the inside dataZoom and kills the drag (chart pans a hair, then sticks).
    const option = untrack(() => buildOption(view, palette));
    chart.setOption(option, { notMerge: true });
    tags = []; // positions are stale after a re-render; next cursor move repopulates
    updateLogGrid(); // custom left-axis labels/gridlines for the log price scale
  });

  // Log scale only: refit the price axis to the visible window as the user zooms/pans.
  // A *merged* setOption touching just yAxis[0] min/max leaves the dataZoom components
  // alone, so an in-progress drag survives (unlike the notMerge rebuild above).
  $effect(() => {
    void [zoomPct.start, zoomPct.end];
    if (!chart || !view || settings.scale !== 'log') return;
    const lr = visibleLogRange(view);
    if (!lr) return;
    logRange = lr;
    chart.setOption({ yAxis: [{ min: lr.min, max: lr.max }] });
    updateLogGrid();
  });
</script>

<div class="chart-host" class:fs={fullscreen} bind:this={host}>
  <div class="chart" bind:this={el}></div>

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

  {#if view}
    <div class="nav">
      <button onclick={goStart} title={$t('histviz.chart.jumpToStart')}><Icon name="chevron-left" size={12} /> {$t('histviz.chart.start')}</button>
      <button onclick={zoomFit} title={$t('histviz.chart.fitToDefault')}>⤢ {$t('histviz.chart.fit')}</button>
      <button onclick={goEnd} title={$t('histviz.chart.jumpToLatest')}>{$t('histviz.chart.end')} <Icon name="chevron-right" size={12} /></button>
      <button onclick={toggleFullscreen} title={fullscreen ? $t('histviz.chart.exitFullscreen') : $t('histviz.chart.fullscreen')} aria-label={$t('histviz.chart.toggleFullscreen')}>
        <Icon name={fullscreen ? 'chevron-down' : 'maximize'} size={12} />
      </button>
    </div>
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
            </button>
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
  }
  .chart-host.fs {
    background: var(--surface);
    padding: var(--space-3);
  }
  .chart {
    width: 100%;
    height: 100%;
    min-height: 320px;
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
    height: 0;
    border-top: 0.5px solid var(--grid-line);
  }
  .lg-label {
    position: absolute;
    left: 0;
    width: 46px;
    transform: translateY(-50%);
    text-align: right;
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
    right: 2px;
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
  .legend {
    position: absolute;
    top: 6px;
    left: 6px;
    z-index: 5;
    max-width: 220px;
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
    max-height: 220px;
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
