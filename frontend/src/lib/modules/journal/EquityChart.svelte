<script>
  // Equity curve rendered with uPlot. Takes the breakdown's equity_curve points and
  // draws cumulative equity over time. Rebuilds on data change and container resize,
  // and shows a hover tooltip (date + equity) for the nearest point.
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { fmtMoney } from './api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { points = [], currency = 'USD' } = $props();

  let el = $state(null);
  let plot = null;
  let ro;
  let mounted = $state(false);

  // Canvas can't consume CSS custom properties, so the palette is read as concrete
  // values. Reactive: flipping the theme re-runs the build effect below, which is
  // what makes the curve repaint instead of keeping the old theme's colors.
  const colors = $derived(chartColors());

  // Hover tooltip state, positioned over the plot.
  let tip = $state(null); // { x, y, label, value } | null

  // [timestamps[], equity[]] in uPlot's column-oriented shape.
  const data = $derived.by(() => {
    const xs = points.map((p) => Math.floor(new Date(p.at).getTime() / 1000));
    const ys = points.map((p) => p.equity);
    return [xs, ys];
  });

  // Cursor plugin: read the hovered index and surface a tooltip vignette.
  function cursorTip() {
    return {
      hooks: {
        setCursor: (u) => {
          const i = u.cursor.idx;
          if (i == null || u.cursor.left < 0) {
            tip = null;
            return;
          }
          const at = u.data[0][i];
          const eq = u.data[1][i];
          if (at == null || eq == null) {
            tip = null;
            return;
          }
          tip = {
            x: u.cursor.left,
            y: u.valToPos(eq, 'y'),
            label: new Date(at * 1000).toLocaleDateString(),
            value: fmtMoney(eq, currency)
          };
        }
      }
    };
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    plot = null;
    if (points.length === 0) return;

    // Institutional equity curve: gold 1.5px stroke, hollow data points
    // (bg-filled, 1px gold ring, r=2), horizontal grid --grid-line 0.5px,
    // axes/labels monospace 10px --dim, NO area fill, NO gradient.
    const axisFont = `10px ${colors.mono}`;
    const opts = {
      width: el.clientWidth || 600,
      height: 260,
      padding: [12, 12, 0, 0],
      scales: { x: { time: true } },
      cursor: { x: true, y: false, points: { size: 6 } },
      plugins: [cursorTip()],
      axes: [
        {
          stroke: colors.dim,
          font: axisFont,
          // Horizontal gridlines only (spec): the x-axis draws no vertical rules.
          grid: { show: false },
          ticks: { show: false }
        },
        {
          stroke: colors.dim,
          font: axisFont,
          // Grid is scaffolding: hairline, under the data, never competing.
          grid: { stroke: colors.gridLine, width: 0.5 },
          ticks: { show: false },
          size: 52
        }
      ],
      series: [
        { value: (u, v) => (v == null ? '' : new Date(v * 1000).toLocaleDateString()) },
        {
          label: $t('journal.equityChart.series.equity'),
          stroke: colors.accent,
          width: 1.5,
          // No fill under the curve.
          points: {
            show: points.length < 40,
            size: 4, // r=2
            stroke: colors.accent,
            width: 1,
            fill: colors.bg
          }
        }
      ],
      // One series: the heading names it, so a legend box would only repeat itself.
      legend: { show: false }
    };
    plot = new uPlot(opts, data, el);
  }

  onMount(() => {
    mounted = true;
    ro = new ResizeObserver(() => {
      if (plot && el) plot.setSize({ width: el.clientWidth, height: 260 });
    });
  });

  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });

  // Track the palette so a theme flip lands here. `setData` only swaps numbers —
  // stroke/fill/axis colors are baked into the uPlot instance at construction — so a
  // color change has to rebuild, while a plain data change can take the cheap path.
  let lastPalette = null;

  // Build/teardown the plot whenever the bound element, the dataset, or the theme
  // changes. This covers the empty↔non-empty transition (the element is recreated by
  // the {#if}), which a one-shot onMount build would miss — so the curve no longer
  // vanishes when a filter changes the result set.
  $effect(() => {
    void data; // track dataset changes
    const palette = colors.accent + colors.gridLine + colors.dim; // track theme changes
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
      tip = null;
    }
  });
</script>

{#if points.length === 0}
  <EmptyState icon="trending-up" description={$t('journal.equityChart.empty')} compact />
{:else}
  <div class="chart-wrap">
    <div class="chart" bind:this={el}></div>
    {#if tip}
      <div class="tip" style:left="{tip.x}px" style:top="{tip.y}px">
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
  /* Hover vignette: floats above the cursor point, doesn't intercept the mouse.
     Institutional: hairline filet, no shadow, no radius. */
  .tip {
    position: absolute;
    transform: translate(-50%, -130%);
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
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--faint);
  }
  /* The value is the reason the tooltip exists; tabular so it doesn't shimmy as
     the cursor moves between points of different widths. */
  .tip-val {
    font-family: var(--mono);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
</style>
