<script>
  // Trade scatter: one dot per closed trade, both axes chosen by the reader. The point
  // of the screen is the shape of the cloud — does the edge come from long holds, big
  // size, a time of day — so everything else stays quiet: no fill, no gradient, colour
  // spent only on the grouping the reader asked for.
  //
  // Axis catalogue and the fit live in `scatter.js`; this file owns the chart.
  import { onMount } from 'svelte';
  import {
    METRICS,
    metric,
    scatterCtx,
    COLOR_MODES,
    colorKey,
    groupRows,
    fit
  } from './scatter.js';
  import { fmtMoney, fmtPct, fmtNum, ASSET_CLASSES } from './api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Select from '$lib/ui/Select.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  // ECharts is a megabyte of parse work and this is the only chart of the module that
  // needs it: loading it statically would put that on every journal page view. It comes
  // in when the tab is actually opened, and the browser caches it for the app's other
  // chart pages. `$state.raw`: a module namespace must never be proxied.
  let echarts = $state.raw(null);
  onMount(async () => {
    echarts = await import('echarts');
  });

  let { points = [], currency = 'USD' } = $props();

  // ── Reader's choices, kept across visits like the filter bar's ──
  const KEY = 'otw.journal.scatter.v1';
  const saved = (() => {
    try {
      return JSON.parse(localStorage.getItem(KEY)) ?? {};
    } catch {
      return {};
    }
  })();
  const known = (id, fallback) => (METRICS.some((m) => m.id === id) ? id : fallback);

  let xId = $state(known(saved.x, 'date'));
  let yId = $state(known(saved.y, 'net'));
  let colorMode = $state(COLOR_MODES.includes(saved.color) ? saved.color : 'result');
  let trend = $state(saved.trend ?? true);

  $effect(() => {
    const state = { x: xId, y: yId, color: colorMode, trend };
    try {
      localStorage.setItem(KEY, JSON.stringify(state));
    } catch {
      /* private mode: the chart still works, it just forgets */
    }
  });

  // ── Data ──
  const ctx = $derived(scatterCtx(points));
  const xm = $derived(metric(xId));
  const ym = $derived(metric(yId));

  // A trade missing either axis is not plottable (no stop = no R). It is dropped and
  // counted, never silently coerced to zero.
  const plotted = $derived.by(() => {
    const rows = [];
    let skipped = 0;
    points.forEach((p, i) => {
      const x = xm.of(p, i, ctx);
      const y = ym.of(p, i, ctx);
      if (x == null || y == null || !Number.isFinite(x) || !Number.isFinite(y)) {
        skipped += 1;
        return;
      }
      rows.push({ x, y, p, key: colorKey(p, colorMode) });
    });
    return { rows, skipped };
  });

  const groups = $derived(groupRows(plotted.rows));
  const line = $derived(fit(plotted.rows.map((r) => r.x), plotted.rows.map((r) => r.y)));

  // ── Formatting ──
  const WEEKDAYS = Array.from({ length: 7 }, (_, i) =>
    // 2024-01-01 was a Monday, so +i walks Monday → Sunday.
    new Date(2024, 0, 1 + i).toLocaleDateString(undefined, { weekday: 'short' })
  );

  function dur(min) {
    const m = Math.round(min);
    if (Math.abs(m) < 60) return $t('journal.analytics.scatter.durMin', { n: m });
    if (Math.abs(m) < 1440) return $t('journal.analytics.scatter.durHour', { n: +(m / 60).toFixed(1) });
    return $t('journal.analytics.scatter.durDay', { n: +(m / 1440).toFixed(1) });
  }

  /** Full precision, for the tooltip. */
  function fmtVal(kind, v) {
    switch (kind) {
      case 'money':
        return fmtMoney(v, currency);
      case 'percent':
        return fmtPct(v);
      case 'r':
        return `${fmtNum(v)}R`;
      case 'duration':
        return dur(v);
      case 'time':
        return new Date(v).toLocaleString();
      case 'hour':
        return `${Math.round(v)}h`;
      case 'weekday':
        return WEEKDAYS[((Math.round(v) % 7) + 7) % 7] ?? '';
      case 'count':
        return String(Math.round(v));
      default:
        return fmtNum(v);
    }
  }

  /** Short enough for a tick. */
  function axisVal(kind, v) {
    if (kind === 'money') {
      const a = Math.abs(v);
      if (a >= 1e6) return `${fmtMoney(v / 1e6, currency, 1)}M`;
      if (a >= 1e4) return `${fmtMoney(v / 1e3, currency, 0)}k`;
      return fmtMoney(v, currency, a < 10 && a > 0 ? 2 : 0);
    }
    if (kind === 'percent') return `${+v.toFixed(1)}%`;
    if (kind === 'num') return String(+v.toFixed(2));
    return fmtVal(kind, v);
  }

  const metricLabel = (id) => $t(`journal.analytics.metric.${id}`);
  const METRIC_OPTIONS = $derived(METRICS.map((m) => ({ value: m.id, label: metricLabel(m.id) })));
  const COLOR_OPTIONS = $derived(
    COLOR_MODES.map((m) => ({ value: m, label: $t(`journal.analytics.scatter.by.${m}`) }))
  );

  function groupLabel(key) {
    if (key === '__other') return $t('journal.analytics.scatter.other');
    if (key === '') return $t('journal.analytics.group.unassigned');
    if (colorMode === 'result') return $t(`journal.analytics.scatter.${key}`);
    if (colorMode === 'side') return $t(`journal.side.${key}`);
    if (colorMode === 'assetClass') return ASSET_CLASSES.find((a) => a.id === key)?.label ?? key;
    return key;
  }

  // ── Chart ──
  let el = $state(null);
  // `$state.raw` and write-only inside the effect below: `chart` is what tells the paint
  // effect the canvas exists. Reading a state *and* writing it in the same effect makes
  // that effect depend on itself, which Svelte runs until it gives up
  // (`effect_update_depth_exceeded`) — here that meant re-initialising ECharts on a loop.
  let chart = $state.raw(null);
  const colors = $derived(chartColors());

  function axis(m, pal, isX) {
    const { dim, border, gridLine, mono } = pal;
    const base = {
      type: m.kind === 'time' ? 'time' : 'value',
      name: metricLabel(m.id),
      nameTextStyle: { color: dim, fontFamily: mono, fontSize: 9 },
      axisLabel: {
        color: dim,
        fontFamily: mono,
        fontSize: 10,
        hideOverlap: true,
        formatter: (v) => (m.kind === 'time' ? new Date(v).toLocaleDateString() : axisVal(m.kind, v))
      },
      axisLine: { lineStyle: { color: border } },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: gridLine, width: 0.5 } },
      scale: m.kind !== 'time'
    };
    if (m.kind === 'weekday') Object.assign(base, { min: 0, max: 6, interval: 1, scale: false });
    if (m.kind === 'hour') Object.assign(base, { min: 0, max: 23, interval: 3, scale: false });
    return isX
      ? { ...base, nameLocation: 'middle', nameGap: 30 }
      : { ...base, nameGap: 12, nameTextStyle: { ...base.nameTextStyle, align: 'left' } };
  }

  function build(pal) {
    const { series: ramp, muted, dim, border, surface, text, mono, green, red, accent } = pal;
    const rows = plotted.rows;
    const dense = rows.length > 600;

    const seriesColor = (key, i) => {
      if (colorMode !== 'result') return key === '__other' ? muted : ramp[i % ramp.length];
      return key === 'win' ? green : key === 'loss' ? red : muted;
    };

    const cloud = groups.map((g, i) => ({
      name: groupLabel(g.key),
      type: 'scatter',
      symbolSize: dense ? 5 : 7,
      itemStyle: { color: seriesColor(g.key, i), opacity: dense ? 0.6 : 0.78 },
      emphasis: { scale: 1.5, itemStyle: { opacity: 1 } },
      data: g.items.map((r) => ({ value: [r.x, r.y], row: r }))
    }));

    // Zero lines, drawn only where an axis actually crosses zero — a reference the eye
    // needs on a PnL axis and noise anywhere else. Own series, so toggling a legend
    // entry cannot take the reference away with it.
    const marks = [];
    const crosses = (vals) => vals.length > 0 && Math.min(...vals) < 0 && Math.max(...vals) > 0;
    if (xm.kind !== 'time' && crosses(rows.map((r) => r.x))) marks.push({ xAxis: 0 });
    if (crosses(rows.map((r) => r.y))) marks.push({ yAxis: 0 });

    const overlays = [];
    if (marks.length > 0) {
      overlays.push({
        type: 'line',
        data: [],
        silent: true,
        tooltip: { show: false },
        markLine: {
          silent: true,
          symbol: 'none',
          label: { show: false },
          lineStyle: { color: border, width: 0.5, type: 'solid' },
          data: marks
        }
      });
    }
    if (trend && line) {
      const xs = rows.map((r) => r.x);
      const x0 = Math.min(...xs);
      const x1 = Math.max(...xs);
      overlays.push({
        name: 'trend',
        type: 'line',
        silent: true,
        showSymbol: false,
        tooltip: { show: false },
        lineStyle: { color: accent, width: 1, type: 'dashed' },
        data: [
          [x0, line.intercept + line.slope * x0],
          [x1, line.intercept + line.slope * x1]
        ]
      });
    }

    return {
      animation: false,
      grid: { left: 64, right: 24, top: groups.length > 1 ? 40 : 24, bottom: 56 },
      legend:
        groups.length > 1
          ? {
              top: 0,
              right: 0,
              icon: 'circle',
              itemWidth: 8,
              itemHeight: 8,
              textStyle: { color: dim, fontFamily: mono, fontSize: 10 },
              inactiveColor: border,
              data: cloud.map((s) => s.name)
            }
          : { show: false },
      tooltip: {
        trigger: 'item',
        backgroundColor: surface,
        borderColor: border,
        textStyle: { color: text, fontFamily: mono, fontSize: 11 },
        formatter: (p) => {
          const row = p.data?.row;
          if (!row) return '';
          const head = `${row.p.ticker || '—'} · ${new Date(row.p.at).toLocaleDateString()}`;
          const lines = [
            `${metricLabel(xm.id)}: ${fmtVal(xm.kind, row.x)}`,
            `${metricLabel(ym.id)}: ${fmtVal(ym.kind, row.y)}`
          ];
          if (xm.id !== 'net' && ym.id !== 'net') {
            lines.push(`${metricLabel('net')}: ${fmtMoney(row.p.net, currency)}`);
          }
          return `<b>${head}</b><br>${lines.join('<br>')}`;
        }
      },
      // Ctrl + wheel zooms, drag pans: a bare wheel keeps scrolling the page, which is
      // what a reader expects of a chart sitting inside a long analytics view.
      dataZoom: [
        { type: 'inside', xAxisIndex: 0, filterMode: 'none', zoomOnMouseWheel: 'ctrl' },
        { type: 'inside', yAxisIndex: 0, filterMode: 'none', zoomOnMouseWheel: 'ctrl' }
      ],
      xAxis: axis(xm, pal, true),
      yAxis: axis(ym, pal, false),
      series: [...cloud, ...overlays]
    };
  }

  $effect(() => {
    const node = el;
    const lib = echarts;
    if (!node || !lib) return;
    const c = lib.init(node, null, { renderer: 'canvas' });
    chart = c;
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(node);
    return () => {
      ro.disconnect();
      c.dispose();
      chart = null;
    };
  });

  $effect(() => {
    // Read every input so a theme flip, an axis change or a filter repaints the canvas.
    const opt = build(colors);
    chart?.setOption(opt, true);
  });

  function resetZoom() {
    chart?.dispatchAction({ type: 'dataZoom', start: 0, end: 100 });
  }

  function swap() {
    const prev = xId;
    xId = yId;
    yId = prev;
  }
</script>

<section class="card">
  <div class="controls">
    <div class="axes">
      <Select label={$t('journal.analytics.scatter.x')} options={METRIC_OPTIONS} bind:value={xId} />
      <Button
        icon="move-horizontal"
        onclick={swap}
        title={$t('journal.analytics.scatter.swap')}
        aria-label={$t('journal.analytics.scatter.swap')}
      />
      <Select label={$t('journal.analytics.scatter.y')} options={METRIC_OPTIONS} bind:value={yId} />
      <Select
        label={$t('journal.analytics.scatter.color')}
        options={COLOR_OPTIONS}
        bind:value={colorMode}
      />
    </div>
    <div class="chips">
      <button type="button" class="chip" class:on={trend} onclick={() => (trend = !trend)}>
        {$t('journal.analytics.scatter.trend')}
      </button>
      <button type="button" class="chip" onclick={resetZoom}>
        {$t('journal.analytics.scatter.reset')}
      </button>
    </div>
  </div>

  {#if plotted.rows.length === 0}
    <EmptyState
      compact
      icon="bar-chart"
      title={$t('journal.analytics.scatter.noData')}
      description={$t('journal.analytics.scatter.noDataHint')}
    />
  {:else if !echarts}
    <Skeleton height="420px" />
  {:else}
    <div class="chart" bind:this={el}></div>
    <p class="caption">
      <span>{$t('journal.analytics.scatter.count', { count: plotted.rows.length })}</span>
      {#if line}
        <span class="stat">
          r <b class:pos={line.r > 0.2} class:neg={line.r < -0.2}>{fmtNum(line.r)}</b>
        </span>
        <span class="stat">R² <b>{fmtNum(line.r * line.r)}</b></span>
      {/if}
      {#if plotted.skipped > 0}
        <span class="skipped">
          {$t('journal.analytics.scatter.skipped', { count: plotted.skipped })}
        </span>
      {/if}
      <span class="hint">{$t('journal.analytics.scatter.zoomHint')}</span>
    </p>
  {/if}
</section>

<style>
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    padding: var(--space-4);
  }
  .controls {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
    margin-bottom: var(--space-4);
  }
  /* The two axes sit next to their swap, the grouping after them: the reading order of
     the question being asked. */
  .axes {
    display: grid;
    grid-template-columns: minmax(140px, 1fr) auto minmax(140px, 1fr) minmax(140px, 1fr);
    align-items: end;
    gap: var(--space-2);
    flex: 1 1 480px;
  }
  .chips {
    display: flex;
    gap: var(--space-1);
  }
  /* Same frame and the same height as the pickers next to them: one control row, one
     baseline. Values mirror the .btn layer (theme/components.css). */
  .chip {
    height: var(--control-h);
    padding: 0 var(--space-3);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    color: var(--text);
    font-size: 11.5px;
    letter-spacing: 0.04em;
    line-height: 1;
    cursor: pointer;
    white-space: nowrap;
  }
  .chip:hover {
    color: var(--text);
    border-color: var(--accent);
  }
  .chip.on {
    color: var(--text);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .chart {
    width: 100%;
    height: 420px;
  }
  .caption {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
    font-size: 11px;
    color: var(--dim);
    font-family: var(--mono);
    padding-top: var(--space-2);
  }
  .caption b {
    color: var(--text);
    font-weight: var(--fw-normal);
  }
  .caption b.pos {
    color: var(--green);
  }
  .caption b.neg {
    color: var(--red);
  }
  .skipped {
    color: var(--faint);
  }
  .hint {
    margin-left: auto;
    color: var(--faint);
  }
  @media (max-width: 720px) {
    .axes {
      grid-template-columns: 1fr;
    }
  }
</style>
