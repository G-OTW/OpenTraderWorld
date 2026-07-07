<script>
  // Efficient frontier: scatter of the random-portfolio cloud (vol × return, colored by
  // Sharpe), with the min-volatility and max-Sharpe portfolios highlighted. Clicking a
  // highlighted point reports its weights via the onpick callback.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { t } from '$lib/i18n';

  let { frontier, onpick = () => {} } = $props(); // { labels, cloud, min_vol, max_sharpe }

  let el;
  let chart;

  function cssVar(name, fb) {
    if (typeof window === 'undefined') return fb;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fb;
  }

  function pct(n) {
    return +(n * 100).toFixed(2);
  }

  function build(f) {
    const muted = cssVar('--muted', '#888');
    const border = cssVar('--border', '#333');
    const green = cssVar('--green', '#2ca02c');
    const accent = cssVar('--accent', '#3b82f6');

    const cloud = f.cloud.map((p) => [pct(p.vol), pct(p.ret), +p.sharpe.toFixed(3)]);
    const sharpes = f.cloud.map((p) => p.sharpe);

    return {
      animation: false,
      grid: { left: 56, right: 20, top: 20, bottom: 70 },
      tooltip: {
        formatter: (p) =>
          p.seriesName === 'cloud'
            ? $t('quant.frontier.tooltipCloud', { vol: p.value[0], ret: p.value[1], sharpe: p.value[2] })
            : $t('quant.frontier.tooltipPoint', { name: p.seriesName, vol: p.value[0], ret: p.value[1] })
      },
      xAxis: {
        type: 'value',
        name: $t('quant.frontier.volatilityAxis'),
        nameLocation: 'middle',
        nameGap: 28,
        axisLabel: { color: muted },
        axisLine: { lineStyle: { color: border } },
        splitLine: { lineStyle: { color: border, opacity: 0.3 } }
      },
      yAxis: {
        type: 'value',
        name: $t('quant.frontier.returnAxis'),
        axisLabel: { color: muted },
        axisLine: { lineStyle: { color: border } },
        splitLine: { lineStyle: { color: border, opacity: 0.3 } }
      },
      visualMap: {
        min: Math.min(...sharpes),
        max: Math.max(...sharpes),
        dimension: 2,
        orient: 'horizontal',
        left: 'center',
        bottom: 0,
        text: [$t('quant.frontier.highSharpe'), $t('quant.frontier.lowSharpe')],
        textStyle: { color: muted },
        inRange: { color: ['#6b7280', accent, green] },
        seriesIndex: 0
      },
      series: [
        { name: 'cloud', type: 'scatter', symbolSize: 5, data: cloud, itemStyle: { opacity: 0.55 } },
        {
          name: $t('quant.frontier.maxSharpe'),
          type: 'scatter',
          symbol: 'diamond',
          symbolSize: 18,
          data: [[pct(f.max_sharpe.vol), pct(f.max_sharpe.ret)]],
          itemStyle: { color: green },
          label: { show: true, formatter: '★', color: '#fff', fontSize: 11 }
        },
        {
          name: $t('quant.frontier.minVolatility'),
          type: 'scatter',
          symbol: 'triangle',
          symbolSize: 16,
          data: [[pct(f.min_vol.vol), pct(f.min_vol.ret)]],
          itemStyle: { color: accent }
        }
      ]
    };
  }

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    chart.on('click', (p) => {
      if (p.seriesName === $t('quant.frontier.maxSharpe')) onpick('max_sharpe');
      else if (p.seriesName === $t('quant.frontier.minVolatility')) onpick('min_vol');
    });
    return () => ro.disconnect();
  });
  onDestroy(() => chart?.dispose());

  $effect(() => {
    if (chart && frontier) chart.setOption(build(frontier), true);
  });
</script>

<div class="chart" bind:this={el}></div>

<style>
  .chart {
    width: 100%;
    height: 380px;
  }
</style>
