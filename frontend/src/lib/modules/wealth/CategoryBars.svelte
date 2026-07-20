<script>
  // Net worth per category as a vertical bar chart (uPlot). Bars ranked high→low (tallest at
  // the left). One color per category. Hovering a bar shows the category name + amount.
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { fmtMoney } from './api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  // `byCategory` is a { category: amount } map in the display currency ("" = uncategorized).
  let { byCategory = {}, currency = 'USD' } = $props();

  let el = $state(null);
  let plot = null;
  let ro;
  let mounted = $state(false);
  let tip = $state(null);

  // The theme's categorical ramp, read as concrete values (canvas can't use var()).
  const colors = $derived(chartColors());

  const points = $derived(
    Object.entries(byCategory)
      .map(([cat, amount]) => ({ label: cat || $t('wealth.categoryBars.uncategorized'), amount }))
      .filter((p) => p.amount > 0)
      .sort((a, b) => b.amount - a.amount || a.label.localeCompare(b.label))
  );

  /**
   * Color per category NAME, stable across renders.
   *
   * The bars are sorted by amount, so indexing the ramp by bar position would tie the
   * color to the rank: a month where Property overtakes Cash would swap their colors.
   * Alphabetical order is the stable key here — it doesn't move when the amounts do.
   * Past the ramp everything shares the last slot rather than cycling back onto slot 1.
   */
  const colorFor = $derived.by(() => {
    const ramp = colors.series;
    const names = points.map((p) => p.label).sort((a, b) => a.localeCompare(b));
    const order = new Map(names.map((n, i) => [n, i]));
    return (label) => ramp[Math.min(order.get(label) ?? ramp.length - 1, ramp.length - 1)];
  });

  const data = $derived.by(() => [points.map((_, i) => i), points.map((p) => p.amount)]);

  function barsPaths(u, seriesIdx, idx0, idx1) {
    const path = new Path2D();
    const width = Math.max(2, (u.bbox.width / Math.max(points.length, 1)) * 0.6);
    for (let i = idx0; i <= idx1; i++) {
      const x = u.valToPos(u.data[0][i], 'x', true);
      const y = u.valToPos(u.data[seriesIdx][i], 'y', true);
      const y0 = u.valToPos(0, 'y', true);
      const c = u.ctx;
      c.fillStyle = colorFor(points[i]?.label);
      c.fillRect(x - width / 2, Math.min(y, y0), width, Math.abs(y0 - y));
    }
    return { fill: path, stroke: path };
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    plot = null;
    tip = null;
    if (points.length === 0) return;
    const { dim, gridLine, mono } = colors;
    const axisFont = `10px ${mono}`;
    const maxY = points.reduce((mx, p) => Math.max(mx, p.amount), 0);
    const opts = {
      width: el.clientWidth || 600,
      height: 240,
      padding: [12, 12, 0, 0],
      scales: {
        x: { time: false, range: () => [-0.5, Math.max(points.length - 1, 0) + 0.5] },
        y: { range: () => [0, maxY * 1.1 || 1] }
      },
      axes: [
        {
          stroke: dim,
          font: axisFont,
          grid: { show: false },
          ticks: { stroke: gridLine },
          values: (u, splits) => splits.map((i) => points[i]?.label ?? '')
        },
        {
          stroke: dim,
          font: axisFont,
          grid: { stroke: gridLine, width: 0.5 },
          ticks: { stroke: gridLine },
          size: 64,
          values: (u, splits) =>
            splits.map((v) => v.toLocaleString(undefined, { maximumFractionDigits: 0 }))
        }
      ],
      series: [{}, { label: $t('wealth.categoryBars.valueSeries'), paths: barsPaths, points: { show: false } }],
      legend: { show: false },
      cursor: { show: false }
    };
    plot = new uPlot(opts, data, el);
  }

  function hover(e) {
    if (!plot || points.length === 0) {
      tip = null;
      return;
    }
    const wrap = e.currentTarget;
    const wrapRect = wrap.getBoundingClientRect();
    const overRect = plot.over.getBoundingClientRect();
    const xInPlot = e.clientX - overRect.left;
    if (xInPlot < 0 || xInPlot > overRect.width) {
      tip = null;
      return;
    }
    let best = 0;
    let bestDist = Infinity;
    for (let i = 0; i < points.length; i++) {
      const px = plot.valToPos(i, 'x');
      const d = Math.abs(px - xInPlot);
      if (d < bestDist) {
        bestDist = d;
        best = i;
      }
    }
    const barLeft = plot.valToPos(best, 'x');
    const barTop = plot.valToPos(points[best].amount, 'y');
    tip = {
      left: overRect.left - wrapRect.left + barLeft,
      top: overRect.top - wrapRect.top + barTop,
      label: points[best].label,
      value: fmtMoney(points[best].amount, currency)
    };
  }

  onMount(() => {
    mounted = true;
    ro = new ResizeObserver(() => {
      if (plot && el) plot.setSize({ width: el.clientWidth, height: 240 });
    });
  });

  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });

  // Rebuild on data change AND on a theme change: the bars are painted straight onto the
  // canvas, so new tokens need a repaint. Also covers empty↔non-empty, where the {#if}
  // recreates `el` — a one-shot onMount build lost the chart the first time it emptied.
  $effect(() => {
    void data;
    void colors;
    if (!mounted) return;
    if (el && points.length > 0) {
      make();
      if (plot && el) ro?.observe(el);
    } else {
      plot?.destroy();
      plot = null;
      tip = null;
    }
  });
</script>

{#if points.length === 0}
  <EmptyState icon="bar-chart" description={$t('wealth.categoryBars.empty')} compact />
{:else}
  <!-- The assets table on this page carries the same numbers, so the canvas is a
       redundant view rather than an unlabelled image to assistive tech. -->
  <div class="chart-wrap" aria-hidden="true" onmousemove={hover} onmouseleave={() => (tip = null)}>
    <div class="chart" bind:this={el}></div>
    {#if tip}
      <div class="tip" style="left:{tip.left}px; top:{tip.top}px;">
        <span class="tip-label">
          <!-- The swatch ties the tooltip to its bar: with eight bars side by side, the
               name alone makes you count across to find which one you're reading. -->
          <span class="swatch" style:background={colorFor(tip.label)}></span>
          {tip.label}
        </span>
        <span class="tip-val">{tip.value}</span>
      </div>
    {/if}
  </div>
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
  .tip-label {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .swatch {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  /* Tabular so the value doesn't shimmy as the cursor moves between bars. */
  .tip-val {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
</style>
