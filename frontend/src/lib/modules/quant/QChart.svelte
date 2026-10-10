<script>
  // Thin ECharts host for the quant panels: the caller hands a `build(palette)` function and
  // this owns init, resize and the theme repaint (the palette is read inside the effect, so a
  // theme flip rebuilds the option).
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';

  let { build, height = 260 } = $props();

  let el;
  let chart;
  const colors = $derived(chartColors());

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  });
  onDestroy(() => chart?.dispose());

  $effect(() => {
    if (chart && build) chart.setOption(build(colors), true);
  });
</script>

<div class="chart" bind:this={el} style:height={`${height}px`}></div>

<style>
  .chart {
    width: 100%;
  }
</style>
