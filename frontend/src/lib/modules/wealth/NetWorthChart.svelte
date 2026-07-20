<script>
  // Net-worth line/area chart (uPlot) over the breakdown's points ([{ at, net_worth }]).
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { points = [] } = $props();

  let el = $state(null);
  let plot = null;
  let ro;
  let mounted = $state(false);

  // Canvas can't consume CSS custom properties, so the palette is read as concrete
  // values. Reactive: flipping the theme re-runs the build effect below.
  const colors = $derived(chartColors());

  // Institutional axis label font: monospace 10px in --dim.
  const axisFont = $derived(`10px ${colors.mono}`);

  const data = $derived.by(() => {
    const xs = points.map((p) => Math.floor(new Date(p.at + 'T00:00:00').getTime() / 1000));
    const ys = points.map((p) => p.net_worth);
    return [xs, ys];
  });

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
        scales: { x: { time: true } },
        cursor: { x: true, y: false, points: { size: 8 } },
        axes: [
          {
            // X axis: no vertical gridlines — spec is horizontal grid only.
            stroke: colors.dim,
            font: axisFont,
            grid: { show: false },
            ticks: { stroke: colors.gridLine, size: 4 }
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
            points: {
              show: points.length < 40,
              size: 5,
              stroke: colors.accent,
              fill: colors.bg
            }
          }
        ],
        // One series: the surrounding heading names it, so a legend box repeats itself.
        legend: { show: false }
      },
      data,
      el
    );
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
  $effect(() => {
    void data;
    const palette = colors.accent + colors.border + colors.muted;
    if (!mounted) return;

    if (el && points.length > 0) {
      const themeChanged = palette !== lastPalette;
      if (plot && plot.root.isConnected && !themeChanged) {
        plot.setData(data);
      } else {
        make();
        if (plot && el) ro?.observe(el);
      }
      lastPalette = palette;
    } else {
      plot?.destroy();
      plot = null;
    }
  });
</script>

{#if points.length === 0}
  <EmptyState icon="trending-up" description={$t('wealth.netWorthChart.empty')} compact />
{:else}
  <div class="chart" bind:this={el}></div>
{/if}

<style>
  .chart {
    width: 100%;
  }
</style>
