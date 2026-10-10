<script>
  // Cumulative net per trade, the real exits against each compared signal. The x axis is
  // the trade number in exit order: the lab compares exits, not calendars.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  // series: [{ label, color, curve: number[] }]; baseline: number[]
  let { baseline = [], series = [], currency = 'USD' } = $props();

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
    if (!chart) return;
    const p = colors;
    const line = (name, data, color, width, dashed = false) => ({
      name,
      type: 'line',
      data: [0, ...data],
      showSymbol: false,
      lineStyle: { color, width, type: dashed ? 'dashed' : 'solid' },
      itemStyle: { color }
    });
    chart.setOption(
      {
        animation: false,
        backgroundColor: 'transparent',
        legend: { top: 0, textStyle: { color: p.dim, fontSize: 10 } },
        grid: { left: 64, right: 16, top: 32, bottom: 28 },
        xAxis: {
          type: 'category',
          data: [0, ...baseline.map((_, i) => i + 1)],
          name: $t('journal.exits.chart.tradeNo'),
          nameLocation: 'middle',
          nameGap: 20,
          nameTextStyle: { color: p.dim, fontSize: 10 },
          axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 },
          axisLine: { lineStyle: { color: p.border } },
          axisTick: { show: false }
        },
        yAxis: {
          scale: true,
          axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10 },
          splitLine: { lineStyle: { color: p.gridLine, width: 0.5 } }
        },
        tooltip: {
          trigger: 'axis',
          backgroundColor: p.surface,
          borderColor: p.border,
          textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 },
          valueFormatter: (v) => fmtMoney(v, currency)
        },
        series: [
          line($t('journal.exits.baseline'), baseline, p.text, 1.5, true),
          ...series.map((s) => line(s.label, s.curve, s.color, 1.5))
        ]
      },
      true
    );
  });
</script>

<div class="curves" bind:this={el}></div>

<style>
  .curves {
    width: 100%;
    height: 280px;
  }
</style>
