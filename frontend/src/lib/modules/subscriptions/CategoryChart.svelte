<script>
  // Monthly-equivalent spend per category as a vertical bar chart (uPlot). Bars are ranked
  // high→low (tallest at the left). One color per category. Hovering a bar shows the
  // category name + amount.
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { fmtMoney } from './api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  // `months` carries per-period `by_category`; `categories` gives the high→low ranking order.
  // `mode` selects whether bars total the current month or the current calendar year.
  let { months = [], years = [], categories = [], currency = 'USD', mode = 'month' } = $props();

  let el = $state(null);
  let plot = null;
  let ro;
  let mounted = $state(false);
  let tip = $state(null); // { left, top, label, value } | null

  // The theme's categorical ramp, read as concrete values (canvas can't use var()).
  const colors = $derived(chartColors());

  /**
   * Color per category NAME, fixed for the life of the page.
   *
   * The bars are sorted by amount, so indexing the ramp by bar position would make the
   * color follow the rank: a month where Music overtakes Cloud would swap their colors,
   * and a filter that drops a category would repaint every survivor. Keying on the
   * category's position in the (stable) `categories` list keeps a category's color its
   * own. Past the ramp everything shares the last slot rather than cycling back onto
   * slot 1, which would claim two categories are the same thing.
   */
  const colorFor = $derived.by(() => {
    const ramp = colors.series;
    const order = new Map(categories.map((c, i) => [c.category, i]));
    return (label) => ramp[Math.min(order.get(label) ?? ramp.length - 1, ramp.length - 1)];
  });

  // Per-category totals for the current period. In 'month' mode: the current calendar month's
  // monthly-equivalent. In 'year' mode: the current year's FULL annual cost, taken straight
  // from the backend `years` (not prorated), so a future year shows complete spend.
  const totals = $derived.by(() => {
    const now = new Date();
    const curMonth = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}`;
    const curYear = String(now.getFullYear());
    const acc = new Map();
    if (mode === 'year') {
      const yr = years.find((y) => y.month.slice(0, 4) === curYear);
      for (const [cat, v] of Object.entries(yr?.by_category ?? {})) acc.set(cat, v);
      return acc;
    }
    const mo = months.find((m) => m.month.slice(0, 7) === curMonth);
    for (const [cat, v] of Object.entries(mo?.by_category ?? {})) acc.set(cat, v);
    return acc;
  });

  // Keep the backend's high→low ranking order, but use the period-scoped totals.
  const points = $derived(
    categories
      .map((c) => ({ label: c.category, amount: totals.get(c.category) ?? 0 }))
      .filter((p) => p.amount > 0)
      .sort((a, b) => b.amount - a.amount || a.label.localeCompare(b.label))
  );

  const data = $derived.by(() => [points.map((_, i) => i), points.map((p) => p.amount)]);

  // Bars path builder; each bar is painted with its own category's color.
  function barsPaths(u, seriesIdx, idx0, idx1) {
    const path = new Path2D();
    const width = Math.max(2, (u.bbox.width / Math.max(points.length, 1)) * 0.6);
    for (let i = idx0; i <= idx1; i++) {
      const x = u.valToPos(u.data[0][i], 'x', true);
      const y = u.valToPos(u.data[seriesIdx][i], 'y', true);
      const y0 = u.valToPos(0, 'y', true);
      const w = width;
      const c = u.ctx;
      c.fillStyle = colorFor(points[i]?.label);
      c.fillRect(x - w / 2, Math.min(y, y0), w, Math.abs(y0 - y));
    }
    // We painted directly; return an empty path so uPlot's own fill/stroke is a no-op.
    return { fill: path, stroke: path };
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    plot = null;
    tip = null;
    if (points.length === 0) return;
    const { muted, border } = colors;
    const maxY = points.reduce((mx, p) => Math.max(mx, p.amount), 0);
    const opts = {
      width: el.clientWidth || 600,
      height: 240,
      padding: [12, 12, 0, 0],
      scales: {
        // Half-slot padding each side so the first/last bar sits fully inside the plot.
        x: { time: false, range: () => [-0.5, Math.max(points.length - 1, 0) + 0.5] },
        y: { range: () => [0, maxY * 1.1 || 1] }
      },
      axes: [
        {
          stroke: muted,
          grid: { show: false },
          ticks: { stroke: border },
          values: (u, splits) => splits.map((i) => points[i]?.label ?? '')
        },
        {
          stroke: muted,
          grid: { stroke: border, width: 0.5 },
          ticks: { stroke: border },
          size: 64,
          values: (u, splits) =>
            splits.map((v) => v.toLocaleString(undefined, { maximumFractionDigits: 0 }))
        }
      ],
      series: [{}, { label: 'Spend', paths: barsPaths, points: { show: false } }],
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

  // Rebuild on data change AND on a theme change: the bar colors are painted straight
  // onto the canvas, so new tokens need a repaint. Also covers empty↔non-empty, where
  // the {#if} recreates `el`.
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
  <EmptyState icon="pie-chart" description={$t('subscriptions.categoryChart.empty')} compact />
{:else}
  <!-- The subscriptions table on this page carries the same numbers, so the canvas is a
       redundant view rather than an unlabelled image. -->
  <div class="chart-wrap" aria-hidden="true" onmousemove={hover} onmouseleave={() => (tip = null)}>
    <div class="chart" bind:this={el}></div>
    {#if tip}
      <div class="tip" style="left:{tip.left}px; top:{tip.top}px;">
        <span class="tip-cat">
          <!-- The swatch ties the tooltip to its bar: identity is never color-alone,
               but it must not be name-alone either when eight bars sit side by side. -->
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
  /* Level 2: it floats over the plot, so it carries the shadow and drops the border. */
  .tip {
    position: absolute;
    transform: translate(-50%, -100%);
    margin-top: -8px;
    pointer-events: none;
    background: var(--surface);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    display: flex;
    flex-direction: column;
    gap: 1px;
    white-space: nowrap;
    box-shadow: var(--shadow-2);
    z-index: var(--z-dropdown);
  }
  .tip-cat {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .swatch {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex: none;
  }
  /* Tabular so the value doesn't shimmy as the cursor moves between bars. */
  .tip-val {
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
</style>
