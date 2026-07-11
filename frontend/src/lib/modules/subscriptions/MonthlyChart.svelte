<script>
  // Subscription spend as a bar chart (uPlot). Takes the breakdown's `months`
  // ([{ month, amount }]) and draws one bar per period. In `month` mode each bar is a
  // calendar month; in `year` mode the months are summed into calendar-year totals.
  // Hovering a bar shows a tooltip with that period's total.
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { monthLabel, fmtMoney } from './api.js';
  import { chartColors, withAlpha } from '$lib/theme/chart.svelte.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { months = [], years = [], subs = [], currency = 'USD', mode = 'month', color = 'single' } = $props();

  let el = $state(null);
  let plot = null;
  let ro;
  let mounted = $state(false);

  // Hover tooltip state (the period total under the cursor), positioned over `.chart-wrap`.
  let tip = $state(null); // { left, top, label, value } | null

  // The theme's categorical ramp, read as concrete values (canvas can't use var()).
  // Reactive: a theme flip re-runs the build effect and repaints.
  const colors = $derived(chartColors());

  // Named series get a fixed slot; everything past the ramp folds into one "Other"
  // segment. The palette is never cycled — a 9th subscription reusing the 1st's color
  // would say the two are the same thing.
  const MAX_SERIES = 7; // 7 named + 1 "Other" = the ramp's 8 slots

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

  // The stacked series actually drawn: the first MAX_SERIES subscriptions keep their own
  // slot; the tail is summed into a single "Other" segment. `ids` is what each segment
  // sums over, so the stack still totals the period amount.
  const stackSeries = $derived.by(() => {
    if (color !== 'multi' || subIds.length === 0) return [];
    const named = subIds.slice(0, MAX_SERIES).map((id) => ({ id, ids: [id] }));
    const tail = subIds.slice(MAX_SERIES);
    return tail.length ? [...named, { id: '__other', ids: tail }] : named;
  });

  const sumOf = (p, ids) => ids.reduce((n, id) => n + (p.bySub[id] ?? 0), 0);

  // uPlot data: x indices + one y-series per stacked segment (multi), or one total (single).
  const data = $derived.by(() => {
    const xs = points.map((_, i) => i);
    if (stackSeries.length) {
      return [xs, ...stackSeries.map((s) => points.map((p) => sumOf(p, s.ids)))];
    }
    return [xs, points.map((p) => p.amount)];
  });

  // A bars path builder. For a stacked chart each series draws from the cumulative sum of the
  // series below it (1..seriesIdx-1) up to its own cumulative sum, so segments stack.
  //
  // A 2px gap is cut off the bottom of every segment above the first: without it,
  // adjacent fills of similar lightness merge into one block and the stack stops being
  // readable. The gap eats into the segment, never into the total.
  const STACK_GAP = 2;

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
      const y = Math.min(yTop, yBase);
      let h = Math.abs(yBase - yTop);
      // Only segments resting on another one need the separator, and only if they can
      // spare the pixels.
      if (base > 0 && h > STACK_GAP * 2) h -= STACK_GAP;
      path.rect(x - width / 2, y, width, h);
    }
    return { fill: path, stroke: path };
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    plot = null;
    tip = null;
    if (points.length === 0) return;

    const { accent, muted, border, series: ramp } = colors;
    // Stacked total per period drives the y-axis top (per-series max would clip the stack).
    const stackMax = points.reduce((mx, p) => Math.max(mx, p.amount), 0);
    // Fixed-order assignment: segment i always wears ramp[i]. The "Other" fold means the
    // index never runs past the ramp, so no color is ever reused for a second entity.
    const barSeries = stackSeries.length
      ? stackSeries.map((s, i) => {
          const c = ramp[i];
          return { stroke: c, fill: withAlpha(c, 0.8), paths: barsPaths, points: { show: false } };
        })
      : [
          {
            label: 'Spend',
            stroke: accent,
            fill: withAlpha(accent, 0.33),
            paths: barsPaths,
            points: { show: false }
          }
        ];
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
    mounted = true;
    ro = new ResizeObserver(() => {
      if (plot && el) plot.setSize({ width: el.clientWidth, height: 240 });
    });
  });

  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });

  // Rebuild on data change (bar count / values / mode) AND on a theme change: stroke and
  // fill are baked into the uPlot instance at construction, so new colors need a rebuild.
  // Also covers the empty↔non-empty transition, where the {#if} recreates `el`.
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
  <EmptyState icon="bar-chart" description={$t('subscriptions.monthlyChart.empty')} compact />
{:else}
  <!-- The subscriptions table below this chart carries the same numbers, so the canvas is
       a redundant view: hidden from assistive tech rather than announced as a blank
       image. The hover tooltip is a pointer-only enhancement. -->
  <div
    class="chart-wrap"
    aria-hidden="true"
    onmousemove={hover}
    onmouseleave={() => (tip = null)}
  >
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
  .tip-month {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  /* Tabular so the value doesn't shimmy as the cursor moves between bars. */
  .tip-val {
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
</style>
