<script>
  // Net worth per category as a vertical bar chart (uPlot). Bars ranked high→low (tallest at
  // the left). One color per category. Hovering a bar shows the category name + amount.
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  // `byCategory` is a { category: amount } map in the display currency ("" = uncategorized).
  let { byCategory = {}, currency = 'USD' } = $props();

  let el = $state(null);
  let plot = null;
  let ro;
  let tip = $state(null);

  const PALETTE = [
    '#5b8cff', '#22c55e', '#f59e0b', '#ef4444', '#a855f7', '#06b6d4',
    '#ec4899', '#84cc16', '#f97316', '#14b8a6', '#6366f1', '#eab308'
  ];

  const points = $derived(
    Object.entries(byCategory)
      .map(([cat, amount]) => ({ label: cat || $t('wealth.categoryBars.uncategorized'), amount }))
      .filter((p) => p.amount > 0)
      .sort((a, b) => b.amount - a.amount || a.label.localeCompare(b.label))
  );

  const data = $derived.by(() => [points.map((_, i) => i), points.map((p) => p.amount)]);

  function cssVar(name, fallback) {
    if (typeof window === 'undefined') return fallback;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
  }

  function barsPaths(u, seriesIdx, idx0, idx1) {
    const path = new Path2D();
    const width = Math.max(2, (u.bbox.width / Math.max(points.length, 1)) * 0.6);
    for (let i = idx0; i <= idx1; i++) {
      const x = u.valToPos(u.data[0][i], 'x', true);
      const y = u.valToPos(u.data[seriesIdx][i], 'y', true);
      const y0 = u.valToPos(0, 'y', true);
      const c = u.ctx;
      c.fillStyle = PALETTE[i % PALETTE.length];
      c.fillRect(x - width / 2, Math.min(y, y0), width, Math.abs(y0 - y));
    }
    return { fill: path, stroke: path };
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    tip = null;
    const muted = cssVar('--muted', '#8a8f98');
    const border = cssVar('--border', '#2a2d34');
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
    make();
    ro = new ResizeObserver(() => {
      if (plot && el) plot.setSize({ width: el.clientWidth, height: 240 });
    });
    if (el) ro.observe(el);
  });

  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });

  $effect(() => {
    void data;
    if (plot) make();
  });
</script>

{#if points.length === 0}
  <div class="empty">{$t('wealth.categoryBars.empty')}</div>
{:else}
  <div class="chart-wrap" onmousemove={hover} onmouseleave={() => (tip = null)}>
    <div class="chart" bind:this={el}></div>
    {#if tip}
      <div class="tip" style="left:{tip.left}px; top:{tip.top}px;">
        <span class="tip-label">{tip.label}</span>
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
  .tip {
    position: absolute;
    transform: translate(-50%, -100%);
    margin-top: -8px;
    pointer-events: none;
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 8px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    white-space: nowrap;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.25);
    z-index: 2;
  }
  .tip-label {
    font-size: 0.62rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .tip-val {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text);
  }
  .empty {
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    padding: var(--space-6);
    text-align: center;
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
