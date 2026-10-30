<script>
  // Net-worth line/area chart (uPlot) over the breakdown's points ([{ at, net_worth }]).
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { fmtMoney } from './api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t, locale } from '$lib/i18n';

  let { points = [], currency = 'USD', granularity = 'month', shape = 'line' } = $props();
  const yearly = $derived(granularity === 'year');
  const bars = $derived(shape === 'bars');

  let el = $state(null);
  let plot = null;
  let ro;
  let mounted = $state(false);
  // Hover tooltip (the point under the cursor), positioned over `.chart-wrap`.
  let tip = $state(null); // { left, top, label, value } | null

  // Canvas can't consume CSS custom properties, so the palette is read as concrete
  // values. Reactive: flipping the theme re-runs the build effect below.
  const colors = $derived(chartColors());

  // Institutional axis label font: monospace 10px in --dim.
  const axisFont = $derived(`10px ${colors.mono}`);

  // Line: real timestamps, so gaps between samples are drawn to scale.
  // Bars: a categorical index instead. Samples are period *ends* and the last one is clamped
  // to today, so on a time axis the final bar sits days after the previous one instead of a
  // month/year later — one narrow bar with the rest of the period empty. One slot per period
  // gives every bar the same width.
  const data = $derived.by(() => {
    const ys = points.map((p) => p.net_worth);
    const xs = bars
      ? points.map((_, i) => i)
      : points.map((p) => Math.floor(new Date(p.at + 'T00:00:00').getTime() / 1000));
    return [xs, ys];
  });

  // Period label for a sample date: "2026" yearly, else the short month ("Aug 26").
  function periodLabel(at) {
    if (!at) return '';
    const d = new Date(at + 'T00:00:00');
    if (yearly) return String(d.getFullYear());
    return d.toLocaleDateString($locale, { month: 'short', year: '2-digit' });
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    plot = null;
    if (points.length === 0) return;

    plot = new uPlot(
      {
        width: el.clientWidth || 600,
        height: 260,
        padding: [12, 12, 0, 0],
        scales: {
          // Bars: index scale padded by half a slot each side, so the first and last bar
          // are not flush against the plot edges.
          x: bars
            ? { time: false, range: () => [-0.5, points.length - 0.5] }
            : { time: true },
          // Bars are read by their length, so they have to start at zero — a clipped
          // baseline would exaggerate small differences. The line keeps uPlot's auto range.
          ...(bars ? { y: { range: (u, min, max) => [Math.min(0, min), max] } } : {})
        },
        // No crosshair over bars (it reads as a gridline between them); the line keeps it.
        cursor: bars ? { show: false } : { x: true, y: false, points: { size: 8 } },
        axes: [
          {
            // X axis: no vertical gridlines — spec is horizontal grid only.
            stroke: colors.dim,
            font: axisFont,
            grid: { show: false },
            ticks: { stroke: colors.gridLine, size: 4 },
            // One tick per sampled period, named by that period. uPlot's default time ticks
            // pick their own interval (Mar/Jun/Sep/Dec…), labelling months we never sampled
            // and repeating the same year four times.
            ...(bars
              ? {
                  // Index scale: one label per slot, thinned to ~8 so "Aug 26"-width labels
                  // never collide. Anchored on the last slot — the newest period is the one
                  // worth naming.
                  splits: () => {
                    const step = Math.ceil(points.length / 8);
                    const last = points.length - 1;
                    return points.map((_, i) => i).filter((i) => (last - i) % step === 0);
                  },
                  values: (u, splits) => splits.map((i) => periodLabel(points[i]?.at ?? ''))
                }
              : yearly
                ? {
                    splits: () => data[0],
                    values: (u, splits) => splits.map((s) => new Date(s * 1000).getFullYear())
                  }
                : {})
          },
          {
            stroke: colors.dim,
            font: axisFont,
            grid: { stroke: colors.gridLine, width: 0.5 },
            ticks: { stroke: colors.gridLine, size: 4 },
            size: 70
          }
        ],
        series: [
          {},
          {
            // Accent, not --green: net worth is a magnitude that can fall, and
            // green/red are reserved for gain/loss semantics. A permanently green
            // line would claim a result the number doesn't carry. No area fill under
            // the curve — institutional fact-sheet style.
            label: $t('wealth.netWorthChart.series'),
            stroke: colors.accent,
            width: 1.5,
            ...(bars
              ? {
                  // Bars are filled; the stroke stays for the outline. Width is a fraction
                  // of the slot (one slot = one period), capped so a short series doesn't
                  // draw slabs.
                  fill: colors.accent,
                  paths: uPlot.paths.bars({ align: 0, size: [0.65, 60] }),
                  points: { show: false }
                }
              : {
                  points: {
                    show: points.length < 40,
                    size: 5,
                    stroke: colors.accent,
                    fill: colors.bg
                  }
                })
          }
        ],
        // One series: the surrounding heading names it, so a legend box repeats itself.
        legend: { show: false }
      },
      data,
      el
    );
  }

  // Map a mouse event over `.chart-wrap` to the nearest point and position a tooltip on it.
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
    const [xs, ys] = data;
    let best = 0;
    let bestDist = Infinity;
    for (let i = 0; i < xs.length; i++) {
      const d = Math.abs(plot.valToPos(xs[i], 'x') - xInPlot);
      if (d < bestDist) {
        bestDist = d;
        best = i;
      }
    }
    tip = {
      left: overRect.left - wrapRect.left + plot.valToPos(xs[best], 'x'),
      top: overRect.top - wrapRect.top + plot.valToPos(ys[best], 'y'),
      label: fmtDate(points[best].at),
      value: fmtMoney(ys[best], currency)
    };
  }

  // Point dates are plain `YYYY-MM-DD`; read as local so a month-end sample can't slip
  // back a day in a negative-offset zone.
  function fmtDate(at) {
    const d = new Date(at + 'T00:00:00');
    return d.toLocaleDateString($locale, { year: 'numeric', month: 'short', day: 'numeric' });
  }

  onMount(() => {
    mounted = true;
    ro = new ResizeObserver(() => plot && el && plot.setSize({ width: el.clientWidth, height: 260 }));
  });
  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });

  // Track the palette so a theme flip lands here: stroke/fill/axis colors are baked
  // into the uPlot instance at construction, so a color change has to rebuild, while
  // a plain data change takes the cheap setData path. Rebuilding also covers the
  // empty↔non-empty transition, where the {#if} recreates the bound element — a
  // one-shot onMount build lost the chart the first time a filter emptied it.
  let lastPalette = null;
  let lastGranularity = null;
  let lastShape = null;
  $effect(() => {
    void data;
    const palette = colors.accent + colors.border + colors.muted;
    if (!mounted) return;

    if (el && points.length > 0) {
      // The axis tick formatter and the series path builder are baked in at construction,
      // so switching month↔year or line↔bars has to rebuild rather than take setData.
      const rebuild =
        palette !== lastPalette || granularity !== lastGranularity || shape !== lastShape;
      if (plot && plot.root.isConnected && !rebuild) {
        plot.setData(data);
      } else {
        make();
        if (plot && el) ro?.observe(el);
      }
      lastPalette = palette;
      lastGranularity = granularity;
      lastShape = shape;
    } else {
      plot?.destroy();
      plot = null;
      tip = null;
    }
  });
</script>

{#if points.length === 0}
  <EmptyState icon="trending-up" description={$t('wealth.netWorthChart.empty')} compact />
{:else}
  <!-- The rail beside the chart carries the same net-worth figure, so the canvas is a
       redundant view rather than an unlabelled image. -->
  <div class="chart-wrap" aria-hidden="true" onmousemove={hover} onmouseleave={() => (tip = null)}>
    <div class="chart" bind:this={el}></div>
    {#if tip}
      <div class="tip" style="left:{tip.left}px; top:{tip.top}px;">
        <span class="tip-date">{tip.label}</span>
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
  .tip-date {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
  }
  /* Tabular so the value doesn't shimmy as the cursor moves along the curve. */
  .tip-val {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
</style>
