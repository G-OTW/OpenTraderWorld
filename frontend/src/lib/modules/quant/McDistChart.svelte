<script>
  // Two distribution histograms from the Monte-Carlo run: final equity (with the starting
  // capital marked) and max drawdown (positive fraction). Redraws on data/theme change.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t } from '$lib/i18n';

  let { result } = $props(); // MonteCarloResult
  let el;
  let chart;
  const colors = $derived(chartColors());

  // Histogram → [midpoint, count] bars, plus the bar width from the edges.
  function bars(h, scale = 1) {
    const { edges, counts } = h;
    return counts.map((c, i) => [+(((edges[i] + edges[i + 1]) / 2) * scale).toFixed(3), c]);
  }

  function build(r, p) {
    const eqBars = bars(r.final_histogram, 1);
    const ddBars = bars(r.drawdown_histogram, 100); // fraction → percent
    const startCap = +r.start_capital.toFixed(2);

    return {
      animation: false,
      grid: [
        { left: 56, right: 24, top: 30, width: '40%', height: 200 },
        { right: 24, left: '58%', top: 30, height: 200 }
      ],
      title: [
        { text: $t('quant.mc.finalEquityDist'), left: 56, top: 4, textStyle: { fontSize: 11, fontWeight: 500, color: p.muted } },
        { text: $t('quant.mc.maxDrawdownDist'), left: '58%', top: 4, textStyle: { fontSize: 11, fontWeight: 500, color: p.muted } }
      ],
      tooltip: {
        trigger: 'axis',
        axisPointer: { type: 'line', snap: false },
        backgroundColor: p.surface,
        borderColor: p.border,
        textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 }
      },
      xAxis: [
        { type: 'value', gridIndex: 0, scale: true, axisLine: { lineStyle: { color: p.border } }, axisTick: { show: false }, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 } },
        { type: 'value', gridIndex: 1, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: '{value}%' }, axisLine: { lineStyle: { color: p.border } }, axisTick: { show: false } }
      ],
      yAxis: [
        { type: 'value', gridIndex: 0, axisLine: { lineStyle: { color: p.border } }, axisTick: { show: false }, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 }, splitLine: { lineStyle: { color: p.gridLine, width: 0.5 } } },
        { type: 'value', gridIndex: 1, axisLine: { lineStyle: { color: p.border } }, axisTick: { show: false }, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 }, splitLine: { lineStyle: { color: p.gridLine, width: 0.5 } } }
      ],
      series: [
        {
          type: 'bar',
          xAxisIndex: 0,
          yAxisIndex: 0,
          data: eqBars,
          barWidth: '90%',
          itemStyle: { color: p.accent, opacity: 0.75 },
          markLine: {
            symbol: 'none',
            silent: true,
            data: [{ xAxis: startCap, label: { formatter: $t('quant.mc.start'), color: p.muted, position: 'insideEndTop' }, lineStyle: { color: p.muted, type: 'dashed' } }]
          }
        },
        {
          type: 'bar',
          xAxisIndex: 1,
          yAxisIndex: 1,
          data: ddBars,
          barWidth: '90%',
          itemStyle: { color: p.red, opacity: 0.7 }
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
    if (chart && result) chart.setOption(build(result, colors), true);
  });
</script>

<div class="chart" bind:this={el}></div>

<style>
  .chart {
    width: 100%;
    height: 260px;
  }
</style>
