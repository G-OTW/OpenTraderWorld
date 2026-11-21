<script>
  // Tracked-time bar chart (uPlot) over day/week/month buckets.
  //
  // Two metrics: `time` (hours) and `cost` (value at each project's hourly rate, already
  // converted to the display currency server-side). Two colorings: `single` (one accent
  // bar per bucket) and `multi` (one stacked segment per project, wearing that project's
  // own color — the same color as its card in the list).
  import { onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { chartColors, withAlpha } from '$lib/theme/chart.svelte.js';
  import { fmtMoney } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    points = [],
    projects = [],
    metric = 'time', // 'time' | 'cost'
    color = 'single', // 'single' | 'multi'
    currency = 'USD'
  } = $props();

  let el = $state(null);
  let plot = null;
  let ro;

  // Hover tooltip (the bucket total under the cursor), positioned over `.chart-wrap`.
  let tip = $state(null); // { left, top, label, value } | null

  // Read inside the effect below, so a theme flip rebuilds the canvas.
  const colors = $derived(chartColors());

  const isCost = $derived(metric === 'cost');

  const total = (p) => (isCost ? (p.value ?? 0) : (p.hours ?? 0));
  const perProject = (p) => (isCost ? (p.value_by_project ?? {}) : (p.by_project ?? {}));

  const fmtTotal = (v) => (isCost ? fmtMoney(v, currency) : `${v.toFixed(1)}h`);

  // Named series get a fixed slot; everything past the ramp folds into one "Other" segment,
  // so no color is ever reused for a second project.
  const MAX_SERIES = 7;

  // Stacking order: the legend's order (biggest first, from the server), plus any project id
  // seen in the data but missing from the legend.
  const projectIds = $derived.by(() => {
    if (color !== 'multi') return [];
    const ids = projects.map((p) => p.id);
    const seen = new Set(ids);
    for (const p of points) {
      for (const id of Object.keys(perProject(p))) if (!seen.has(id)) { seen.add(id); ids.push(id); }
    }
    return ids;
  });

  // The stacked segments actually drawn: the first MAX_SERIES projects keep their own slot,
  // the tail is summed into one "Other" segment, so the stack still totals the bucket.
  const stackSeries = $derived.by(() => {
    if (color !== 'multi' || projectIds.length === 0) return [];
    const byId = new Map(projects.map((p) => [p.id, p]));
    const named = projectIds.slice(0, MAX_SERIES).map((id) => ({ id, ids: [id], color: byId.get(id)?.color }));
    const tail = projectIds.slice(MAX_SERIES);
    return tail.length ? [...named, { id: '__other', ids: tail, color: null }] : named;
  });

  const sumOf = (p, ids) => {
    const map = perProject(p);
    return ids.reduce((n, id) => n + (map[id] ?? 0), 0);
  };

  // uPlot data: x indices + one y-series per stacked segment (multi), or one total (single).
  const data = $derived.by(() => {
    const xs = points.map((_, i) => i);
    if (stackSeries.length) return [xs, ...stackSeries.map((s) => points.map((p) => sumOf(p, s.ids)))];
    return [xs, points.map((p) => total(p))];
  });

  // A 2px gap is cut off the bottom of every segment above the first, so adjacent fills of
  // similar lightness don't merge into one block. The gap eats into the segment, never the total.
  const STACK_GAP = 2;

  function bars(u, seriesIdx, idx0, idx1) {
    const path = new Path2D();
    // One "slot" wide per bucket. With a single point the x-range spans 2 slots
    // (min-0.5..max+0.5 widened below), so cap the slot to keep the bar narrow.
    const slot = u.bbox.width / Math.max(points.length, 1);
    const width = Math.max(2, Math.min(slot, 64) * 0.6);
    for (let i = idx0; i <= idx1; i++) {
      const x = u.valToPos(u.data[0][i], 'x', true);
      // Stacked: draw from the cumulative sum of the series below up to our own.
      let base = 0;
      for (let s = 1; s < seriesIdx; s++) base += u.data[s][i] ?? 0;
      const top = base + (u.data[seriesIdx][i] ?? 0);
      if (top === base) continue;
      const yBase = u.valToPos(base, 'y', true);
      const yTop = u.valToPos(top, 'y', true);
      const y = Math.min(yTop, yBase);
      let h = Math.abs(yBase - yTop);
      if (base > 0 && h > STACK_GAP * 2) h -= STACK_GAP;
      path.rect(x - width / 2, y, width, h);
    }
    return { fill: path, stroke: path };
  }

  function make(pal) {
    if (!el) return;
    plot?.destroy();
    plot = null;
    tip = null;
    if (points.length === 0) return;

    const { accent, dim, gridLine, mono, series: ramp } = pal;
    const axisFont = `10px ${mono}`;
    // Stacked total per bucket drives the y-axis top (a per-series max would clip the stack).
    const stackMax = points.reduce((mx, p) => Math.max(mx, total(p)), 0);
    // Each project wears its own color (same as its card in the list); the ramp only fills in
    // for a project with no color set, and for the "Other" fold.
    const barSeries = stackSeries.length
      ? stackSeries.map((s, i) => {
          const c = s.color || ramp[i % ramp.length];
          return { stroke: c, fill: withAlpha(c, 0.8), paths: bars, points: { show: false } };
        })
      : [
          {
            label: isCost ? 'Cost' : 'Hours',
            stroke: accent,
            fill: withAlpha(accent, 1 / 3),
            paths: bars,
            points: { show: false }
          }
        ];
    plot = new uPlot(
      {
        width: el.clientWidth || 600,
        height: 240,
        padding: [12, 24, 0, 8],
        scales: {
          // Pad the x-scale by half a bucket on each side so the first/last bars
          // are drawn fully inside the plot box instead of being clipped in half.
          x: { time: false, range: (u, min, max) => [min - 0.6, max + 0.6] },
          y: { range: () => [0, stackMax * 1.1 || 1] }
        },
        axes: [
          {
            stroke: dim,
            font: axisFont,
            grid: { show: false },
            ticks: { show: false },
            values: (u, splits) => splits.map((i) => (points[i] ? points[i].bucket.slice(5) : ''))
          },
          {
            stroke: dim,
            font: axisFont,
            grid: { stroke: gridLine, width: 0.5 },
            ticks: { show: false },
            size: isCost ? 64 : 48,
            values: (u, splits) =>
              splits.map((v) =>
                isCost ? v.toLocaleString(undefined, { maximumFractionDigits: 0 }) : `${v}h`
              )
          }
        ],
        series: [{}, ...barSeries],
        legend: { show: false },
        // We drive our own tooltip from native mouse events (see `hover`).
        cursor: { show: false }
      },
      data,
      el
    );
  }

  // Map a mouse event over `.chart-wrap` to the nearest bar and position a tooltip on it.
  function hover(e) {
    if (!plot || points.length === 0) {
      tip = null;
      return;
    }
    const wrapRect = e.currentTarget.getBoundingClientRect();
    const overRect = plot.over.getBoundingClientRect();
    const xInPlot = e.clientX - overRect.left;
    if (xInPlot < 0 || xInPlot > overRect.width) {
      tip = null;
      return;
    }
    let best = 0;
    let bestDist = Infinity;
    for (let i = 0; i < points.length; i++) {
      const d = Math.abs(plot.valToPos(i, 'x') - xInPlot);
      if (d < bestDist) {
        bestDist = d;
        best = i;
      }
    }
    const p = points[best];
    tip = {
      left: overRect.left - wrapRect.left + plot.valToPos(best, 'x'),
      top: overRect.top - wrapRect.top + plot.valToPos(total(p), 'y'),
      label: p.bucket,
      // Both metrics, whichever one the bars are drawing — the plotted one leads.
      value: fmtTotal(total(p)),
      alt: isCost ? `${(p.hours ?? 0).toFixed(1)}h` : fmtMoney(p.value ?? 0, currency)
    };
  }

  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });

  // `el` lives inside `{#if points.length > 0}`, so it only appears once there's
  // data. Rebuild whenever the bound element or the data changes — this is what
  // makes the chart show on first load without toggling tabs. Metric/coloring changes
  // flow through `data`; stroke and fill are baked in at construction, so a theme flip
  // needs a rebuild too.
  $effect(() => {
    void data;
    void metric;
    if (!el) {
      plot?.destroy();
      plot = null;
      tip = null;
      return;
    }
    make(colors);
    if (!ro) {
      ro = new ResizeObserver(() => plot && el && plot.setSize({ width: el.clientWidth, height: 240 }));
    }
    ro.disconnect();
    ro.observe(el);
  });
</script>

{#if points.length === 0}
  <div class="empty">{$t('time.chart.empty')}</div>
{:else}
  <!-- The numbers are carried by the rail beside this chart, so the canvas is a redundant
       view: hidden from assistive tech rather than announced as a blank image. -->
  <div class="chart-wrap" aria-hidden="true" onmousemove={hover} onmouseleave={() => (tip = null)}>
    <div class="chart" bind:this={el}></div>
    {#if tip}
      <div class="tip" style="left:{tip.left}px; top:{tip.top}px;">
        <span class="tip-bucket">{tip.label}</span>
        <span class="tip-val">{tip.value}</span>
        <span class="tip-alt">{tip.alt}</span>
      </div>
    {/if}
  </div>
  {#if color === 'multi' && stackSeries.length > 0}
    <ul class="legend">
      {#each stackSeries as s, i}
        <li>
          <span class="swatch" style:background={s.color || colors.series[i % colors.series.length]}></span>
          {s.id === '__other' ? $t('time.breakdown.otherProjects') : (projects.find((p) => p.id === s.id)?.name ?? '—')}
        </li>
      {/each}
    </ul>
  {/if}
{/if}

<style>
  .chart-wrap {
    position: relative;
    width: 100%;
  }
  .chart {
    width: 100%;
  }
  /* Floats over the plot: a hairline filet, no shadow, no radius. */
  .tip {
    position: absolute;
    transform: translate(-50%, -100%);
    margin-top: -8px;
    pointer-events: none;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-2) var(--space-3);
    display: flex;
    flex-direction: column;
    gap: 1px;
    white-space: nowrap;
    z-index: var(--z-dropdown);
  }
  .tip-bucket {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
  }
  /* Tabular so the value doesn't shimmy as the cursor moves between bars. */
  .tip-val {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  /* The metric that isn't plotted, kept secondary. */
  .tip-alt {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
    margin-top: var(--space-3);
    list-style: none;
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .swatch {
    width: 8px;
    height: 8px;
    flex: none;
  }
  .empty {
    border: 0.5px dashed var(--border);
    border-radius: 0;
    padding: var(--space-6);
    text-align: center;
    color: var(--dim);
    font-size: var(--text-base);
  }
</style>
