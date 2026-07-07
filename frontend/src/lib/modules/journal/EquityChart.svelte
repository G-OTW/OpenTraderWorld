<script>
  // Equity curve rendered with uPlot. Takes the breakdown's equity_curve points and
  // draws cumulative equity over time. Rebuilds on data change and container resize,
  // and shows a hover tooltip (date + equity) for the nearest point.
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  let { points = [], currency = 'USD' } = $props();

  let el = $state(null);
  let plot = null;
  let ro;
  let mounted = $state(false);

  // Hover tooltip state, positioned over the plot.
  let tip = $state(null); // { x, y, label, value } | null

  // [timestamps[], equity[]] in uPlot's column-oriented shape.
  const data = $derived.by(() => {
    const xs = points.map((p) => Math.floor(new Date(p.at).getTime() / 1000));
    const ys = points.map((p) => p.equity);
    return [xs, ys];
  });

  function cssVar(name, fallback) {
    if (typeof window === 'undefined') return fallback;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
  }

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
    const accent = cssVar('--accent', '#5b8cff');
    const muted = cssVar('--muted', '#8a8f98');
    const border = cssVar('--border', '#2a2d34');
    const opts = {
      width: el.clientWidth || 600,
      height: 260,
      padding: [12, 12, 0, 0],
      scales: { x: { time: true } },
      cursor: { x: true, y: true, points: { size: 7 } },
      plugins: [cursorTip()],
      axes: [
        { stroke: muted, grid: { stroke: border, width: 0.5 }, ticks: { stroke: border } },
        {
          stroke: muted,
          grid: { stroke: border, width: 0.5 },
          ticks: { stroke: border },
          size: 60
        }
      ],
      series: [
        { value: (u, v) => (v == null ? '' : new Date(v * 1000).toLocaleDateString()) },
        {
          label: $t('journal.equityChart.series.equity'),
          stroke: accent,
          width: 2,
          fill: accent + '22',
          points: { show: points.length < 40 }
        }
      ],
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

  // Build/teardown the plot whenever the bound element or the dataset changes. This
  // covers the empty↔non-empty transition (the element is recreated by the {#if}),
  // which a one-shot onMount build would miss — so the curve no longer vanishes when
  // a filter changes the result set.
  $effect(() => {
    void data; // track dataset changes
    if (!mounted) return;
    if (el && points.length > 0) {
      if (plot && plot.root.isConnected) {
        plot.setData(data);
      } else {
        make();
        if (plot && el) ro?.observe(el);
      }
    } else {
      plot?.destroy();
      plot = null;
      tip = null;
    }
  });
</script>

{#if points.length === 0}
  <div class="empty">{$t('journal.equityChart.empty')}</div>
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
  /* Hover vignette: floats above the cursor point, doesn't intercept the mouse. */
  .tip {
    position: absolute;
    transform: translate(-50%, -130%);
    pointer-events: none;
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35);
    padding: 5px 9px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    white-space: nowrap;
    z-index: 20;
  }
  .tip-date {
    font-size: 0.68rem;
    color: var(--muted);
  }
  .tip-val {
    font-size: 0.82rem;
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
