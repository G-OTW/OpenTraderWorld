<script>
  // Subscription spend as a bar chart (uPlot). Takes the breakdown's `months`
  // ([{ month, amount }]) and draws one bar per period. In `month` mode each bar is a
  // calendar month; in `year` mode the months are summed into calendar-year totals.
  // Hovering a bar shows a tooltip with that period's total.
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { monthLabel, fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  let { months = [], years = [], subs = [], currency = 'USD', mode = 'month', color = 'single' } = $props();

  let el = $state(null);
  let plot = null;
  let ro;

  // Hover tooltip state (the period total under the cursor), positioned over `.chart-wrap`.
  let tip = $state(null); // { left, top, label, value } | null

  // Distinct-ish color palette for per-sub segments (cycled if more subs than colors).
  const PALETTE = [
    '#5b8cff', '#22c55e', '#f59e0b', '#ef4444', '#a855f7', '#06b6d4',
    '#ec4899', '#84cc16', '#f97316', '#14b8a6', '#6366f1', '#eab308'
  ];

  // Points actually plotted: per-month, or aggregated per calendar year. Each is
  // { key, label, amount, bySub: { subId: amount } }. Future periods are kept; the half-slot
  // x-margin (see `make`) ensures the last bar is never visually truncated.
  const points = $derived.by(() => {
    if (mode === 'year') {
      // Backend `years` already hold each year's FULL annual cost (no proration), so future
      // years aren't understated. Just map them straight through.
      return years.map((y) => ({
        key: y.month,
        label: y.month.slice(0, 4),
        amount: y.amount,
        bySub: y.by_sub ?? {}
      }));
    }
    return months.map((m) => ({
      key: m.month,
      label: monthLabel(m.month),
      amount: m.amount,
      bySub: m.by_sub ?? {}
    }));
  });

  // Stacking order: stable list of sub ids (from the breakdown legend, falling back to any
  // id seen in the data). In single-color mode there's one series; in multi, one per sub.
  const subIds = $derived.by(() => {
    if (color !== 'multi') return [];
    const ids = subs.map((s) => s.id);
    const seen = new Set(ids);
    for (const p of points) {
      for (const id of Object.keys(p.bySub)) if (!seen.has(id)) { seen.add(id); ids.push(id); }
    }
    return ids;
  });

  // uPlot data: x indices + one y-series per sub (multi) or a single total series (single).
  const data = $derived.by(() => {
    const xs = points.map((_, i) => i);
    if (color === 'multi' && subIds.length) {
      return [xs, ...subIds.map((id) => points.map((p) => p.bySub[id] ?? 0))];
    }
    return [xs, points.map((p) => p.amount)];
  });

  function cssVar(name, fallback) {
    if (typeof window === 'undefined') return fallback;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
  }

  // A bars path builder. For a stacked chart each series draws from the cumulative sum of the
  // series below it (1..seriesIdx-1) up to its own cumulative sum, so segments stack.
  function barsPaths(u, seriesIdx, idx0, idx1) {
    const path = new Path2D();
    const width = Math.max(2, (u.bbox.width / Math.max(points.length, 1)) * 0.6);
    for (let i = idx0; i <= idx1; i++) {
      const x = u.valToPos(u.data[0][i], 'x', true);
      let base = 0;
      for (let s = 1; s < seriesIdx; s++) base += u.data[s][i] ?? 0;
      const top = base + (u.data[seriesIdx][i] ?? 0);
      if (top === base) continue;
      const yBase = u.valToPos(base, 'y', true);
      const yTop = u.valToPos(top, 'y', true);
      path.rect(x - width / 2, Math.min(yTop, yBase), width, Math.abs(yBase - yTop));
    }
    return { fill: path, stroke: path };
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    tip = null;
    const accent = cssVar('--accent', '#5b8cff');
    const muted = cssVar('--muted', '#8a8f98');
    const border = cssVar('--border', '#2a2d34');
    const multi = color === 'multi' && subIds.length > 0;
    // Stacked total per period drives the y-axis top (per-series max would clip the stack).
    const stackMax = points.reduce((mx, p) => Math.max(mx, p.amount), 0);
    const barSeries = multi
      ? subIds.map((id, i) => {
          const c = PALETTE[i % PALETTE.length];
          return { stroke: c, fill: c + 'cc', paths: barsPaths, points: { show: false } };
        })
      : [{ label: 'Spend', stroke: accent, fill: accent + '55', paths: barsPaths, points: { show: false } }];
    const opts = {
      width: el.clientWidth || 600,
      height: 240,
      padding: [12, 12, 0, 0],
      scales: {
        // Half-slot padding each side so the first/last bar sits fully inside the plot.
        x: { time: false, range: () => [-0.5, Math.max(points.length - 1, 0) + 0.5] },
        y: { range: () => [0, stackMax * 1.1 || 1] }
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
      series: [{}, ...barSeries],
      legend: { show: false },
      // We drive our own tooltip from native mouse events (see `hover`), so disable
      // uPlot's built-in cursor entirely.
      cursor: { show: false }
    };
    plot = new uPlot(opts, data, el);
  }

  // Map a mouse event over `.chart-wrap` to the nearest bar and position a tooltip on it.
  function hover(e) {
    if (!plot || points.length === 0) {
      tip = null;
      return;
    }
    const wrap = e.currentTarget;
    const wrapRect = wrap.getBoundingClientRect();
    const overRect = plot.over.getBoundingClientRect();
    // Mouse X within the plotting area (CSS pixels).
    const xInPlot = e.clientX - overRect.left;
    if (xInPlot < 0 || xInPlot > overRect.width) {
      tip = null;
      return;
    }
    // Nearest data index for that x. valToPos(...'x') returns CSS px within the plot area.
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
    const barLeft = plot.valToPos(best, 'x'); // within plot area
    const barTop = plot.valToPos(points[best].amount, 'y');
    // Convert to coordinates relative to `.chart-wrap` (the tooltip's positioning parent).
    const offsetX = overRect.left - wrapRect.left;
    const offsetY = overRect.top - wrapRect.top;
    tip = {
      left: offsetX + barLeft,
      top: offsetY + barTop,
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

  // Rebuild fully on data change (bar count / values / mode).
  $effect(() => {
    void data;
    if (plot) make();
  });
</script>

{#if points.length === 0}
  <div class="empty">{$t('subscriptions.monthlyChart.empty')}</div>
{:else}
  <div class="chart-wrap" onmousemove={hover} onmouseleave={() => (tip = null)}>
    <div class="chart" bind:this={el}></div>
    {#if tip}
      <div class="tip" style="left:{tip.left}px; top:{tip.top}px;">
        <span class="tip-month">{tip.label}</span>
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
  .tip-month {
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
