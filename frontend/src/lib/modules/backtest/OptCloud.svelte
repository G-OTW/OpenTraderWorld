<script>
  // The grid as a cloud: one dot per variant, three dimensions chosen by the reader (a metric or
  // a swept parameter), the box turned by hand. Same frame as the journal's 3D scatter.
  //
  // Each chosen dimension gets a min/max slider. Filtering and thinning are the server's job: a
  // grid can hold a quarter of a million variants, so the browser asks for what survives the
  // sliders, capped at what this machine can turn smoothly. When the cap bites, the best tenth
  // on the ranking metric is always kept and the rest is an even sample.
  import { onMount, untrack } from 'svelte';
  import { backtestApi, fmtNum } from './api.js';
  import { OPT_METRICS, metricById, displayValue, paramsAt } from './optimize.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Select from '$lib/ui/Select.svelte';
  import RangeSlider from '$lib/ui/RangeSlider.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  let {
    jobId,
    axes = [],
    /** Ranking metric and direction: the best of it survive the thinning. */
    sort = 'sharpe',
    dir = 'desc',
    /** Finished variant count; a change refetches, so a running grid fills in. */
    done = 0,
    onpick = () => {}
  } = $props();

  // ECharts and its WebGL extension load when the tab opens, never with the page.
  let echarts = $state.raw(null);
  let glError = $state(false);
  onMount(async () => {
    try {
      const lib = await import('echarts');
      await import('echarts-gl');
      echarts = lib;
    } catch {
      glError = true;
    }
  });

  // ── Point budget ──
  // What a WebGL cloud still rotates smoothly, from what the browser says about the machine.
  // `deviceMemory` is Chromium only; elsewhere the core count decides.
  const BUDGET = (() => {
    const cores = navigator.hardwareConcurrency || 4;
    const mem = navigator.deviceMemory || 8;
    if (cores <= 4 || mem <= 2) return 5000;
    if (cores <= 8 || mem <= 4) return 15000;
    return 30000;
  })();

  // ── Dimensions ──
  const dayNames = $derived({
    all: $t('backtest.opt.everyDay'),
    short: ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'].map((k) => $t(`common.weekday.${k}`))
  });

  /** A parameter plots its own value when every value is a number, else its position. */
  const numericAxis = (a) => a.values?.length > 0 && a.values.every((v) => typeof v === 'number');

  const catalogue = $derived([
    ...axes.map((a, k) => ({
      id: `p${k}`,
      kind: 'param',
      axis: a,
      numeric: numericAxis(a),
      label: a.label || a.path || `#${k + 1}`
    })),
    ...OPT_METRICS.map((m) => ({ id: m.id, kind: 'metric', metric: m, label: $t(`backtest.opt.metric.${m.id}`) }))
  ]);
  const dimById = (id) => catalogue.find((d) => d.id === id);

  const KEY = 'otw.backtest.cloud.v1';
  const saved = (() => {
    try {
      return JSON.parse(localStorage.getItem(KEY)) ?? {};
    } catch {
      return {};
    }
  })();
  const defaults = (() => {
    if (axes.length >= 2) return ['p0', 'p1', sort];
    if (axes.length === 1) return ['p0', 'max_drawdown_pct', sort];
    return ['return_pct', 'max_drawdown_pct', sort];
  })();
  const valid = (id) => /^p\d+$/.test(id ?? '') ? Number(id.slice(1)) < axes.length : !!metricById(id);
  let xId = $state(valid(saved.x) ? saved.x : defaults[0]);
  let yId = $state(valid(saved.y) ? saved.y : defaults[1]);
  let zId = $state(valid(saved.z) ? saved.z : defaults[2]);

  $effect(() => {
    const s = { x: xId, y: yId, z: zId };
    try {
      localStorage.setItem(KEY, JSON.stringify(s));
    } catch {
      /* private mode: the chart still works, it just forgets */
    }
  });

  const dims = $derived([dimById(xId), dimById(yId), dimById(zId)]);
  const OPTIONS = $derived(catalogue.map((d) => ({ value: d.id, label: d.label })));

  // ── Sliders ──
  // A slider works in integer positions. A metric runs over STEPS positions across the grid's
  // observed range; a parameter has one position per value it took. The two ends mean "no bound",
  // so a variant sitting exactly on the edge is never lost to float rounding.
  const STEPS = 200;
  /** Positions per dimension id: `{ lo, hi }`. Missing = the full range. */
  let sel = $state({});
  let bounds = $state.raw({});

  /** Sorted coordinates a parameter dimension can take. */
  function ticks(d) {
    if (!d.numeric) return d.axis.values.map((_, i) => i);
    return [...new Set(d.axis.values)].sort((a, b) => a - b);
  }

  function slider(d) {
    if (d.kind === 'param') {
      const tk = ticks(d);
      return { max: Math.max(0, tk.length - 1), ticks: tk };
    }
    return { max: STEPS, range: bounds[d.id] ?? null };
  }

  const sliders = $derived(
    [...new Map(dims.filter(Boolean).map((d) => [d.id, d])).values()].map((d) => ({ d, ...slider(d) }))
  );

  const posOf = (id, max) => ({ lo: sel[id]?.lo ?? 0, hi: Math.min(sel[id]?.hi ?? max, max) });

  /** Coordinate at a slider position, or null past the ends. */
  function valueAt(s, pos) {
    if (s.d.kind === 'param') return s.ticks[pos];
    if (!s.range) return null;
    const [lo, hi] = s.range;
    return lo + ((hi - lo) * pos) / STEPS;
  }

  /** `[id, min, max]` for every narrowed slider. Parameter bounds sit halfway to the next value. */
  const filters = $derived(
    sliders.flatMap((s) => {
      const { lo, hi } = posOf(s.d.id, s.max);
      if (lo <= 0 && hi >= s.max) return [];
      if (s.d.kind === 'metric' && !s.range) return [];
      const edge = (pos, side) => {
        if (s.d.kind === 'param') {
          const nb = s.ticks[pos + side];
          return nb == null ? side * 1e30 : (s.ticks[pos] + nb) / 2;
        }
        return valueAt(s, pos);
      };
      return [[s.d.id, lo <= 0 ? -1e30 : edge(lo, -1), hi >= s.max ? 1e30 : edge(hi, 1)]];
    })
  );

  // Compared as text: new bounds rebuild `filters`, and an unchanged filter must not refetch.
  const filterKey = $derived(JSON.stringify(filters));

  function setRange(id, lo, hi) {
    sel = { ...sel, [id]: { lo, hi } };
  }

  function resetFilters() {
    sel = {};
  }

  // ── Data ──
  let data = $state.raw(null);
  let error = $state('');
  let seq = 0;
  let timer = 0;

  async function fetchCloud(req, n) {
    try {
      const res = await backtestApi.optimizeCloud(jobId, req);
      if (n !== seq) return;
      error = '';
      data = res;
      const b = {};
      req.dims.forEach((id, k) => {
        if (res.bounds?.[k]) b[id] = res.bounds[k];
      });
      bounds = { ...bounds, ...b };
    } catch (e) {
      if (n === seq) error = e.message;
    }
  }

  // Slider drags and a running grid both ask again; a short debounce keeps a drag to a few
  // requests, and a stale answer never lands over a fresher one.
  $effect(() => {
    const req = {
      x: xId,
      y: yId,
      z: zId,
      dims: [xId, yId, zId],
      filters: JSON.parse(filterKey),
      max: BUDGET,
      sort,
      dir
    };
    void done;
    const n = ++seq;
    clearTimeout(timer);
    timer = setTimeout(() => fetchCloud(req, n), untrack(() => data) ? 150 : 0);
    return () => clearTimeout(timer);
  });

  // Bounds are per dimension; an axis swap clears the slider of the dimension that left.
  $effect(() => {
    const keep = new Set([xId, yId, zId]);
    const pruned = Object.fromEntries(Object.entries(sel).filter(([id]) => keep.has(id)));
    if (Object.keys(pruned).length !== Object.keys(sel).length) sel = pruned;
  });

  // Rows grouped by outcome, like the journal's "result" colouring.
  const groups = $derived.by(() => {
    const out = { win: [], loss: [], flat: [] };
    for (const p of data?.points ?? []) {
      const r = p[4];
      out[r == null || r === 0 ? 'flat' : r > 0 ? 'win' : 'loss'].push(p);
    }
    return Object.entries(out)
      .filter(([, items]) => items.length)
      .map(([key, items]) => ({ key, items }));
  });
  const shown = $derived(data?.points?.length ?? 0);
  const thinned = $derived(data ? shown < data.matched : false);

  // ── Formatting ──
  function fmtDim(d, v, short = false) {
    if (v == null || !Number.isFinite(v)) return '–';
    if (d.kind === 'param') {
      const a = d.axis;
      if (!d.numeric) return displayValue(a, a.values[Math.round(v)], dayNames);
      return `${displayValue(a, v, dayNames)}${a.unit ?? ''}`;
    }
    const m = d.metric;
    const digits = short ? Math.min(m.digits, Math.abs(v) >= 100 ? 0 : 2) : m.digits;
    return `${fmtNum(v, digits)}${m.suffix ?? ''}`;
  }

  function sliderText(s, pos, side) {
    if (s.d.kind === 'metric') {
      if (!s.range) return '–';
      return fmtDim(s.d, pos <= 0 ? s.range[0] : pos >= s.max ? s.range[1] : valueAt(s, pos), true);
    }
    return fmtDim(s.d, s.ticks[pos] ?? (side < 0 ? s.ticks[0] : s.ticks.at(-1)), true);
  }

  // ── Chart ──
  let el = $state(null);
  let chart = $state.raw(null);
  const colors = $derived(chartColors());
  const DEFAULT_VIEW = { alpha: 20, beta: 40, distance: 220, center: [0, 0, 0] };

  function axis3D(d, pal) {
    const { dim, mono } = pal;
    const base = {
      type: 'value',
      name: d.label,
      nameTextStyle: { color: dim, fontFamily: mono, fontSize: 11 },
      axisLabel: {
        textStyle: { color: dim, fontFamily: mono, fontSize: 10 },
        formatter: (v) => fmtDim(d, v, true)
      },
      scale: true
    };
    if (d.kind === 'param' && !d.numeric) {
      Object.assign(base, { min: 0, max: Math.max(0, d.axis.values.length - 1), interval: 1, scale: false });
    }
    return base;
  }

  function build(pal, view) {
    const { muted, dim, border, gridLine, surface, text, mono, green, red, accent } = pal;
    const size = shown > 10000 ? 3 : shown > 2000 ? 4 : shown > 600 ? 5 : 8;
    const [xd, yd, zd] = dims;
    const tone = { win: green, loss: red, flat: muted };
    const cloud = groups.map((g) => ({
      name: $t(`backtest.opt.cloud.${g.key}`),
      type: 'scatter3D',
      symbolSize: size,
      itemStyle: { color: tone[g.key], opacity: shown > 600 ? 0.7 : 0.9 },
      emphasis: { itemStyle: { color: accent }, label: { show: false } },
      data: g.items.map((p) => [p[0], p[1], p[2]])
    }));

    return {
      animation: false,
      legend:
        cloud.length > 1
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
          const pt = groups[p.seriesIndex]?.items[p.dataIndex];
          if (!pt) return '';
          const params = paramsAt(pt[3], axes);
          const lines = axes.map(
            (a, k) => `${a.label || a.path}: ${displayValue(a, params[k], dayNames)}${a.unit ?? ''}`
          );
          const onAxes = [xd, yd, zd].filter((d) => d.kind === 'metric');
          for (const d of onAxes) lines.push(`${d.label}: ${fmtDim(d, [pt[0], pt[1], pt[2]][dims.indexOf(d)])}`);
          if (!onAxes.some((d) => d.id === 'return_pct')) {
            lines.push(`${$t('backtest.opt.metric.return_pct')}: ${pt[4] == null ? '–' : `${fmtNum(pt[4], 2)}%`}`);
          }
          return `<b>${$t('backtest.opt.cloud.variant', { n: pt[3] + 1 })}</b><br>${lines.join('<br>')}`;
        }
      },
      grid3D: {
        boxWidth: 100,
        boxDepth: 100,
        boxHeight: 80,
        environment: 'none',
        axisLine: { lineStyle: { color: border } },
        axisTick: { show: false },
        splitLine: { lineStyle: { color: gridLine, width: 1 } },
        splitArea: { show: false },
        axisPointer: { lineStyle: { color: accent }, label: { show: false } },
        viewControl: {
          projection: 'perspective',
          autoRotate: false,
          rotateSensitivity: 1,
          zoomSensitivity: 1,
          panMouseButton: 'right',
          rotateMouseButton: 'left',
          minDistance: 80,
          maxDistance: 400,
          ...view
        }
      },
      xAxis3D: axis3D(xd, pal),
      yAxis3D: axis3D(yd, pal),
      zAxis3D: axis3D(zd, pal),
      series: cloud
    };
  }

  function currentView(c) {
    const vc = c.getOption()?.grid3D?.[0]?.viewControl;
    if (!vc) return DEFAULT_VIEW;
    const { alpha, beta, distance, center } = vc;
    return { alpha, beta, distance, center };
  }

  $effect(() => {
    const node = el;
    const lib = echarts;
    if (!node || !lib) return;
    let c;
    try {
      c = lib.init(node, null, { renderer: 'canvas' });
    } catch {
      glError = true;
      return;
    }
    // A click opens the variant in the breakdown tab, like a ranking row.
    c.on('click', (p) => {
      const pt = groups[p.seriesIndex]?.items[p.dataIndex];
      if (pt) onpick({ i: pt[3], rank: null, params: paramsAt(pt[3], axes) });
    });
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
    const c = chart;
    if (!c || !dims.every(Boolean)) return;
    c.setOption(build(colors, currentView(c)), true);
  });

  function resetView() {
    chart?.setOption(build(colors, DEFAULT_VIEW), true);
  }
</script>

<section class="card">
  <div class="controls">
    <div class="axes">
      <Select label={$t('journal.analytics.scatter.x')} options={OPTIONS} bind:value={xId} />
      <Select label={$t('journal.analytics.scatter.y')} options={OPTIONS} bind:value={yId} />
      <Select label={$t('journal.analytics.scatter3d.z')} options={OPTIONS} bind:value={zId} />
    </div>
    <div class="chips">
      <button type="button" class="chip" onclick={resetFilters} disabled={!filters.length}>
        {$t('backtest.opt.cloud.resetFilters')}
      </button>
      <button type="button" class="chip" onclick={resetView}>
        {$t('journal.analytics.scatter3d.reset')}
      </button>
    </div>
  </div>

  <div class="sliders">
    {#each sliders as s (s.d.id)}
      {@const pos = posOf(s.d.id, s.max)}
      <RangeSlider
        label={s.d.label}
        max={s.max}
        lo={pos.lo}
        hi={pos.hi}
        disabled={s.max <= 0 || (s.d.kind === 'metric' && !s.range)}
        loText={sliderText(s, pos.lo, -1)}
        hiText={sliderText(s, pos.hi, 1)}
        oninput={(lo, hi) => setRange(s.d.id, lo, hi)}
      />
    {/each}
  </div>

  {#if error}
    <p class="err" role="alert">{error}</p>
  {/if}

  {#if glError}
    <EmptyState
      compact
      icon="alert-triangle"
      title={$t('journal.analytics.scatter3d.noGl')}
      description={$t('journal.analytics.scatter3d.noGlHint')}
    />
  {:else if !echarts || !data}
    <Skeleton height="520px" />
  {:else if !data.total}
    <EmptyState compact icon="bar-chart" title={$t('backtest.opt.noRowsYet')} />
  {:else}
    <div class="chart-wrap">
      <div class="chart" bind:this={el}></div>
      {#if !shown}
        <p class="none">{$t('backtest.opt.cloud.noMatch')}</p>
      {/if}
    </div>
    <p class="caption">
      <span>{$t('backtest.opt.cloud.matched', { matched: fmtNum(data.matched, 0), total: fmtNum(data.total, 0) })}</span>
      {#if thinned}
        <span class="thinned" title={$t('backtest.opt.cloud.thinnedHint', { metric: $t(`backtest.opt.metric.${sort}`) })}>
          {$t('backtest.opt.cloud.thinned', { shown: fmtNum(shown, 0) })}
        </span>
      {/if}
      {#if data.skipped > 0}
        <span class="skipped">{$t('backtest.opt.cloud.skipped', { count: fmtNum(data.skipped, 0) })}</span>
      {/if}
      <span class="hint">{$t('backtest.opt.cloud.hint')}</span>
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
  .axes {
    display: grid;
    grid-template-columns: repeat(3, minmax(140px, 1fr));
    align-items: end;
    gap: var(--space-2);
    flex: 1 1 480px;
  }
  .chips {
    display: flex;
    gap: var(--space-1);
  }
  /* Same frame as the journal scatter's chips. */
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
  .chip:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .chip:disabled {
    color: var(--faint);
    cursor: default;
  }
  .sliders {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-2) var(--space-6);
    margin-bottom: var(--space-4);
  }
  .err {
    margin: 0 0 var(--space-3);
    padding: var(--space-3);
    background: var(--negative-soft);
    color: var(--red-ink);
    font-size: var(--text-sm);
  }
  .chart-wrap {
    position: relative;
  }
  .chart {
    width: 100%;
    height: 520px;
  }
  .none {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
    pointer-events: none;
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
    margin: 0;
  }
  .thinned {
    color: var(--amber-ink);
    cursor: help;
  }
  .skipped {
    color: var(--faint);
  }
  .hint {
    margin-left: auto;
    color: var(--faint);
  }
  @media (max-width: 720px) {
    .axes,
    .sliders {
      grid-template-columns: 1fr;
    }
  }
</style>
