<script>
  // Tracked-hours bar chart (uPlot) over day/week/month buckets.
  import { onDestroy } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { t } from '$lib/i18n';

  let { points = [] } = $props();

  let el = $state(null);
  let plot = null;
  let ro;

  const data = $derived.by(() => [points.map((_, i) => i), points.map((p) => p.hours)]);

  function cssVar(name, fallback) {
    if (typeof window === 'undefined') return fallback;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
  }

  function bars(u, seriesIdx, idx0, idx1) {
    const path = new Path2D();
    // One "slot" wide per bucket. With a single point the x-range spans 2 slots
    // (min-0.5..max+0.5 widened below), so cap the slot to keep the bar narrow.
    const slot = u.bbox.width / Math.max(points.length, 1);
    const width = Math.max(2, Math.min(slot, 64) * 0.6);
    for (let i = idx0; i <= idx1; i++) {
      const x = u.valToPos(u.data[0][i], 'x', true);
      const y = u.valToPos(u.data[seriesIdx][i], 'y', true);
      const y0 = u.valToPos(0, 'y', true);
      path.rect(x - width / 2, Math.min(y, y0), width, Math.abs(y0 - y));
    }
    return { fill: path, stroke: path };
  }

  function make() {
    if (!el) return;
    plot?.destroy();
    const accent = cssVar('--accent', '#5b8cff');
    const muted = cssVar('--muted', '#8a8f98');
    const border = cssVar('--border', '#2a2d34');
    plot = new uPlot(
      {
        width: el.clientWidth || 600,
        height: 240,
        padding: [12, 24, 0, 8],
        scales: {
          // Pad the x-scale by half a bucket on each side so the first/last bars
          // are drawn fully inside the plot box instead of being clipped in half.
          x: { time: false, range: (u, min, max) => [min - 0.6, max + 0.6] },
          y: { range: (u, min, max) => [0, max * 1.1 || 1] }
        },
        axes: [
          {
            stroke: muted,
            grid: { show: false },
            ticks: { stroke: border },
            values: (u, splits) => splits.map((i) => (points[i] ? points[i].bucket.slice(5) : ''))
          },
          {
            stroke: muted,
            grid: { stroke: border, width: 0.5 },
            ticks: { stroke: border },
            size: 48,
            values: (u, splits) => splits.map((v) => `${v}h`)
          }
        ],
        series: [{}, { label: 'Hours', stroke: accent, fill: accent + '55', paths: bars, points: { show: false } }],
        legend: { show: false },
        cursor: { x: false, y: false }
      },
      data,
      el
    );
  }

  onDestroy(() => {
    ro?.disconnect();
    plot?.destroy();
  });

  // `el` lives inside `{#if points.length > 0}`, so it only appears once there's
  // data. Rebuild whenever the bound element or the data changes — this is what
  // makes the chart show on first load without toggling tabs.
  $effect(() => {
    void data;
    if (!el) {
      plot?.destroy();
      plot = null;
      return;
    }
    make();
    if (!ro) {
      ro = new ResizeObserver(() => plot && el && plot.setSize({ width: el.clientWidth, height: 240 }));
    }
    ro.disconnect();
    ro.observe(el);
  });
</script>

{#if points.length === 0}
  <div class="empty">{$t('time.chart.empty')}</div>
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
