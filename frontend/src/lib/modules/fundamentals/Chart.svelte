<script>
  // One ECharts canvas for every Fundamentals plot: time series (macro, prices, short
  // interest), category bars (statements, segments) and a plain category line (yield
  // curve). Repaints on theme flip through `chartColors()`.
  //
  //   <Chart series={[{ name: 'CPI', data: [[ts, v], ...] }]} shade={RECESSIONS} />
  //   <Chart categories={['FY21', ...]} series={[{ name: 'Revenue', type: 'bar', data: [..] }]} />
  //
  // props: series ([{ name, data, type ('line'|'bar'), yAxisIndex, stack, area }]),
  //        categories (category x axis when set), shade ([[from, to]] time ranges),
  //        height, yFormat (fn), dualAxis (bool), legend (bool)
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';

  let {
    series = [],
    categories = null,
    shade = [],
    height = 280,
    yFormat = null,
    dualAxis = false,
    legend = true
  } = $props();

  let el;
  let chart = $state(null);
  const c = $derived(chartColors());

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  });
  onDestroy(() => chart?.dispose());

  $effect(() => {
    if (!chart) return;
    chart.setOption(build(series, categories, shade, c, dualAxis, legend), true);
  });

  function build(list, cats, ranges, p, dual, showLegend) {
    const axisLabel = { color: p.dim, fontFamily: p.mono, fontSize: 10, hideOverlap: true };
    const yAxis = (i) => ({
      type: 'value',
      scale: true,
      position: i === 0 ? 'left' : 'right',
      axisLabel: { ...axisLabel, formatter: yFormat ?? undefined },
      splitLine: { show: i === 0, lineStyle: { color: p.gridLine } }
    });
    return {
      animation: false,
      color: p.series,
      grid: { left: 56, right: dual ? 56 : 16, top: showLegend ? 32 : 12, bottom: 28 },
      legend: showLegend ? { top: 0, left: 0, textStyle: { color: p.muted, fontSize: 11 }, icon: 'roundRect', itemWidth: 10, itemHeight: 4 } : undefined,
      tooltip: {
        trigger: 'axis',
        backgroundColor: p.surface,
        borderColor: p.border,
        textStyle: { color: p.text, fontSize: 11 },
        valueFormatter: (v) => (v == null ? '·' : yFormat ? yFormat(v) : Number(v).toLocaleString(undefined, { maximumFractionDigits: 2 }))
      },
      xAxis: cats
        ? { type: 'category', data: cats, axisLabel, axisLine: { lineStyle: { color: p.gridLine } }, axisTick: { show: false } }
        : { type: 'time', axisLabel, axisLine: { lineStyle: { color: p.gridLine } }, axisTick: { show: false }, splitLine: { show: false } },
      yAxis: dual ? [yAxis(0), yAxis(1)] : [yAxis(0)],
      series: list.map((s, i) => ({
        name: s.name,
        type: s.type ?? 'line',
        data: s.data,
        yAxisIndex: dual ? (s.yAxisIndex ?? 0) : 0,
        stack: s.stack,
        showSymbol: false,
        smooth: false,
        barMaxWidth: 28,
        lineStyle: { width: 1.6 },
        areaStyle: s.area ? { opacity: 0.12 } : undefined,
        markArea:
          i === 0 && ranges.length && !cats
            ? { silent: true, itemStyle: { color: p.faint, opacity: 0.35 }, data: ranges.map(([a, b]) => [{ xAxis: a }, { xAxis: b }]) }
            : undefined
      }))
    };
  }
</script>

<div class="chart" bind:this={el} style="height: {height}px"></div>

<style>
  .chart {
    width: 100%;
    min-width: 0;
  }
</style>
