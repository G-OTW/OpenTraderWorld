<script>
  // Equity curve rendered with uPlot. Takes the breakdown's equity_curve points and
  // draws cumulative equity over time. Rebuilds on data change and container resize,
  // and shows a hover tooltip (date + equity) for the nearest point.
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { fmtMoney } from './api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { trendColor, trendFill } from '$lib/ui/charts/palette.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { rebasePct, fmtSignedPct } from './pnlpct.js';

  let {
    points = [],
    currency = 'USD',
    header = null,
    // Shows the range buttons and the currency / percent switch in the head row.
    controls = false,
    // 'ALL' | '7D' | '1M' | '6M' | '1Y': the window drawn, ending today.
    range = $bindable('ALL'),
    // 'money' | 'pct': percent is the change from the first point drawn.
    unit = $bindable('money')
  } = $props();

  const RANGES = ['7D', '1M', '6M', '1Y'];
  const pct = $derived(unit === 'pct');

  // Start of the window, or null for the whole curve.
  function cutoff(r) {
    const d = new Date();
    if (r === '7D') d.setDate(d.getDate() - 7);
    else if (r === '1M') d.setMonth(d.getMonth() - 1);
    else if (r === '6M') d.setMonth(d.getMonth() - 6);
    else if (r === '1Y') d.setFullYear(d.getFullYear() - 1);
    else return null;
    return d;
  }

  // The window's points. The equity held when the window opens is carried to its start,
  // and the last equity to today, so a quiet week still draws as a flat line.
  const shown = $derived.by(() => {
    const from = cutoff(range);
    if (!from || points.length === 0) return points;
    const t0 = from.getTime();
    const inside = points.filter((p) => new Date(p.at).getTime() >= t0);
    const before = points.filter((p) => new Date(p.at).getTime() < t0).at(-1);
    const out = before ? [{ at: from.toISOString(), equity: before.equity }, ...inside] : inside;
    if (out.length === 0) return out;
    return [...out, { at: new Date().toISOString(), equity: out[out.length - 1].equity }];
  });

  function pickRange(r) {
    range = range === r ? 'ALL' : r;
  }

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
    const xs = shown.map((p) => Math.floor(new Date(p.at).getTime() / 1000));
    const eq = shown.map((p) => p.equity);
    return [xs, pct ? rebasePct(eq) : eq];
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
            value: pct ? fmtSignedPct(eq) : fmtMoney(eq, currency)
          };
        }
      }
    };
  }

  // Back to the whole curve after a drag-zoom: x spans every point, y re-autoscales.
  function fitAll() {
    range = 'ALL';
    const xs = plot?.data[0];
    if (!xs?.length) return;
    plot.setScale('x', { min: xs[0], max: xs[xs.length - 1] });
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    plot = null;
    if (shown.length === 0) return;

    // Trend equity curve: green when the window ends at or above its start, red
    // below, 2px stroke over a gradient area fading to transparent, small data
    // points, horizontal grid --grid-line 0.5px, axes/labels monospace 10px --dim.
    const axisFont = `10px ${colors.mono}`;
    const tone = trendColor(data[1], colors);
    const opts = {
      width: el.clientWidth || 600,
      height: 260,
      // Top room for the rescale button, so the curve never runs under it.
      padding: [28, 12, 0, 0],
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
          size: 52,
          // Percent labels only; amounts keep uPlot's own tick format. The unit is part
          // of the rebuild key below, since axis options are fixed at construction.
          ...(pct ? { values: (u, vals) => vals.map((v) => `${+v.toFixed(2)}%`) } : {})
        }
      ],
      series: [
        { value: (u, v) => (v == null ? '' : new Date(v * 1000).toLocaleDateString()) },
        {
          label: $t('journal.equityChart.series.equity'),
          stroke: tone,
          width: 2,
          fill: trendFill(tone),
          fillTo: (u) => u.scales.y.min,
          points: {
            show: shown.length <= 400,
            size: 3,
            stroke: tone,
            width: 1,
            fill: tone
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
    // Track theme, unit and trend changes: the curve colour is baked in like the theme.
    const palette = colors.green + colors.red + colors.gridLine + colors.dim + unit + trendColor(data[1], colors);
    if (!mounted) return;

    if (el && shown.length > 0) {
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

<!-- Title row owned here so the fit button shares the title's line. -->
<div class="head">
  {@render header?.()}
  {#if points.length > 0}
    {#if controls}
      <div class="ctl">
        <div class="seg" role="group" aria-label={$t('journal.equityChart.range.group')}>
          {#each RANGES as r (r)}
            <button class:active={range === r} aria-pressed={range === r} onclick={() => pickRange(r)}>{$t(`journal.equityChart.range.${r}`)}</button>
          {/each}
        </div>
        <div class="seg" role="group" aria-label={$t('journal.pnlUnit.group')}>
          <button class:active={!pct} aria-pressed={!pct} onclick={() => (unit = 'money')}>{currency}</button>
          <button class:active={pct} aria-pressed={pct} onclick={() => (unit = 'pct')}>%</button>
        </div>
      </div>
    {/if}
    <button
      class="fit"
      title={$t('journal.equityChart.fitAll')}
      aria-label={$t('journal.equityChart.fitAll')}
      onclick={fitAll}
    >
      <Icon name="move-horizontal" size={13} />
    </button>
  {/if}
</div>
{#if shown.length === 0}
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
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
  }
  /* Pushed right, the fit button after it. */
  .ctl {
    display: flex;
    gap: var(--space-2);
    margin-left: auto;
  }
  .seg {
    display: inline-flex;
    border: 0.5px solid var(--border-control);
  }
  .seg button {
    background: transparent;
    border: none;
    border-left: 0.5px solid var(--border-control);
    border-radius: 0;
    color: var(--muted);
    padding: 2px 7px;
    font: inherit;
    font-family: var(--mono);
    font-size: 10.5px;
    line-height: 1.4;
    cursor: pointer;
  }
  .seg button:first-child {
    border-left: none;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.active {
    background: var(--surface-3);
    color: var(--text);
  }
  .fit {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    background: transparent;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .fit:hover {
    color: var(--text);
    border-color: var(--accent);
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
