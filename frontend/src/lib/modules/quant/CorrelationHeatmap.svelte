<script>
  // Correlation matrix as a heatmap. Diverging color scale: blue (−1) → neutral → red (+1).
  // Answers "am I diversified or buying the same thing twice?".
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { t } from '$lib/i18n';

  let { corr } = $props(); // { labels, matrix }

  let el;
  let chart;

  function cssVar(name, fb) {
    if (typeof window === 'undefined') return fb;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fb;
  }

  function build(c) {
    const muted = cssVar('--muted', '#888');
    const data = [];
    for (let i = 0; i < c.labels.length; i++) {
      for (let j = 0; j < c.labels.length; j++) {
        data.push([j, i, +c.matrix[i][j].toFixed(2)]);
      }
    }
    return {
      animation: false,
      tooltip: {
        formatter: (p) => `${c.labels[p.value[1]]} ↔ ${c.labels[p.value[0]]}: ${p.value[2]}`
      },
      grid: { left: 90, right: 20, top: 20, bottom: 60 },
      xAxis: { type: 'category', data: c.labels, axisLabel: { color: muted, rotate: 30 }, splitArea: { show: true } },
      yAxis: { type: 'category', data: c.labels, axisLabel: { color: muted }, splitArea: { show: true } },
      visualMap: {
        min: -1,
        max: 1,
        calculable: true,
        orient: 'horizontal',
        left: 'center',
        bottom: 0,
        inRange: { color: ['#3b82f6', '#e5e7eb', '#e5484d'] },
        textStyle: { color: muted }
      },
      series: [
        {
          type: 'heatmap',
          data,
          label: { show: true, formatter: (p) => p.value[2], color: '#111' },
          emphasis: { itemStyle: { shadowBlur: 6, shadowColor: 'rgba(0,0,0,0.4)' } }
        }
      ]
    };
  }

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  });
  onDestroy(() => chart?.dispose());

  $effect(() => {
    if (chart && corr) chart.setOption(build(corr), true);
  });
</script>

<div class="chart" bind:this={el}></div>

<style>
  .chart {
    width: 100%;
    height: 360px;
  }
</style>
