<script>
  // Two stacked charts for one asset: the drawdown curve (underwater plot) and the return
  // distribution histogram with the VaR threshold marked. Redraws when `result` changes.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t } from '$lib/i18n';

  let { result } = $props();

  let el;
  let chart;

  // Read inside the effect below, so a theme flip repaints the canvas.
  const colors = $derived(chartColors());

  function build(r, pal) {
    // Red on the drawdown and the VaR line is a loss semantic, not a series color.
    const { red, series, muted, dim, gridLine, border, surface, text, mono } = pal;
    const chart1 = series[0];

    const ddX = r.drawdown_curve.map((p) => p.ts);
    const ddY = r.drawdown_curve.map((p) => +(p.dd * 100).toFixed(3));

    // Histogram: bar at each bin midpoint, width from edges. VaR marker as a vertical line.
    const edges = r.histogram.edges;
    const bins = r.histogram.counts.map((c, i) => {
      const mid = ((edges[i] + edges[i + 1]) / 2) * 100;
      return [+mid.toFixed(3), c];
    });
    const varX = -(r.var_hist * 100);

    return {
      animation: false,
      grid: [
        { left: 56, right: 20, top: 30, height: 150 },
        { left: 56, right: 20, top: 240, height: 150 }
      ],
      title: [
        { text: $t('quant.charts.drawdownPct'), left: 56, top: 4, textStyle: { fontSize: 11, fontWeight: 500, color: muted } },
        { text: $t('quant.charts.returnDistributionPct'), left: 56, top: 214, textStyle: { fontSize: 11, fontWeight: 500, color: muted } }
      ],
      tooltip: {
        trigger: 'axis',
        // Free-moving vertical cursor on the distribution: don't snap to bin midpoints.
        axisPointer: { type: 'line', snap: false },
        backgroundColor: surface,
        borderColor: border,
        textStyle: { color: text, fontFamily: mono, fontSize: 11 }
      },
      xAxis: [
        { type: 'category', data: ddX, gridIndex: 0, axisLabel: { show: false }, axisLine: { lineStyle: { color: border } }, axisTick: { show: false } },
        {
          type: 'value',
          gridIndex: 1,
          axisLine: { lineStyle: { color: border } },
          axisTick: { show: false },
          axisLabel: { color: dim, fontFamily: mono, fontSize: 10 },
          axisPointer: { snap: false }
        }
      ],
      yAxis: [
        { type: 'value', gridIndex: 0, max: 0, axisLine: { lineStyle: { color: border } }, axisTick: { show: false }, axisLabel: { color: dim, fontFamily: mono, fontSize: 10 }, splitLine: { lineStyle: { color: gridLine, width: 0.5 } } },
        { type: 'value', gridIndex: 1, axisLine: { lineStyle: { color: border } }, axisTick: { show: false }, axisLabel: { color: dim, fontFamily: mono, fontSize: 10 }, splitLine: { lineStyle: { color: gridLine, width: 0.5 } } }
      ],
      series: [
        {
          type: 'line',
          xAxisIndex: 0,
          yAxisIndex: 0,
          data: ddY,
          lineStyle: { color: red, width: 1.5 },
          symbol: 'none'
        },
        {
          type: 'bar',
          xAxisIndex: 1,
          yAxisIndex: 1,
          data: bins,
          barWidth: '90%',
          itemStyle: { color: chart1, opacity: 0.7 },
          markLine: {
            symbol: 'none',
            silent: true,
            data: [{ xAxis: +varX.toFixed(3), label: { formatter: $t('quant.charts.varLabel'), color: red, position: 'insideEndTop' }, lineStyle: { color: red, type: 'dashed' } }]
          }
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
    height: 420px;
  }
</style>
