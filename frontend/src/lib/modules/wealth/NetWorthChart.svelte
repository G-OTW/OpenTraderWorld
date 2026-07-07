<script>
  // Net-worth line/area chart (uPlot) over the breakdown's points ([{ at, net_worth }]).
  import { onMount, onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { t } from '$lib/i18n';

  let { points = [] } = $props();

  let el = $state(null);
  let plot = null;
  let ro;

  const data = $derived.by(() => {
    const xs = points.map((p) => Math.floor(new Date(p.at + 'T00:00:00').getTime() / 1000));
    const ys = points.map((p) => p.net_worth);
    return [xs, ys];
  });

  function cssVar(name, fallback) {
    if (typeof window === 'undefined') return fallback;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    const accent = cssVar('--accent', '#5b8cff');
    const green = cssVar('--green', '#22c55e');
    const muted = cssVar('--muted', '#8a8f98');
    const border = cssVar('--border', '#2a2d34');
    plot = new uPlot(
      {
        width: el.clientWidth || 600,
        height: 260,
        padding: [12, 12, 0, 0],
        scales: { x: { time: true } },
        axes: [
          { stroke: muted, grid: { stroke: border, width: 0.5 }, ticks: { stroke: border } },
          { stroke: muted, grid: { stroke: border, width: 0.5 }, ticks: { stroke: border }, size: 70 }
        ],
        series: [
          {},
          {
            label: $t('wealth.netWorthChart.series'),
            stroke: green,
            width: 2,
            fill: green + '22',
            points: { show: points.length < 40 }
          }
        ],
        legend: { show: false }
      },
      data,
      el
    );
  }

  onMount(() => {
    make();
    ro = new ResizeObserver(() => plot && el && plot.setSize({ width: el.clientWidth, height: 260 }));
    if (el) ro.observe(el);
  });
  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });
  $effect(() => {
    void data;
    if (plot) plot.setData(data);
  });
</script>

{#if points.length === 0}
  <div class="empty">{$t('wealth.netWorthChart.empty')}</div>
{:else}
  <div class="chart" bind:this={el}></div>
{/if}

<style>
  .chart {
    width: 100%;
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
