<script>
  // One trade on its candles: the entry, the real exit, the planned stop, and where each
  // compared signal would have closed it. Bars come from /api/journal/exits/bars.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t } from '$lib/i18n';

  // bars: { ts[], o[], h[], l[], c[], entry_idx, exit_idx, entry_price, exit_price, stop, side }
  // exits: [{ label, color, exit_bar (ISO), exit_price }]
  let { bars = null, exits = [] } = $props();

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

  function fmtTs(s) {
    const d = new Date(s * 1000);
    return d.getUTCHours() === 0 && d.getUTCMinutes() === 0
      ? d.toLocaleDateString()
      : d.toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
  }

  $effect(() => {
    if (!chart || !bars) return;
    const p = colors;
    const b = bars;
    const idx = new Map(b.ts.map((s, i) => [s, i]));
    const long = b.side !== 'short';
    const ohlc = b.ts.map((_, i) => [b.o[i], b.c[i], b.l[i], b.h[i]]);
    const sim = exits
      .map((x) => {
        const i = idx.get(Math.floor(new Date(x.exit_bar).getTime() / 1000));
        return i == null || x.exit_price == null
          ? null
          : {
              name: x.label,
              type: 'scatter',
              symbol: 'diamond',
              symbolSize: 11,
              z: 6,
              itemStyle: { color: x.color, borderColor: p.bg, borderWidth: 1 },
              data: [[i, x.exit_price]]
            };
      })
      .filter(Boolean);
    const lines = [];
    if (b.stop != null) lines.push({ yAxis: b.stop, lineStyle: { color: p.red, type: 'dashed' } });
    lines.push({ yAxis: b.entry_price, lineStyle: { color: p.dim, type: 'dotted' } });

    chart.setOption(
      {
        animation: false,
        backgroundColor: 'transparent',
        textStyle: { color: p.text },
        legend: { top: 0, textStyle: { color: p.dim, fontSize: 10 } },
        grid: { left: 56, right: 16, top: 32, bottom: 44 },
        xAxis: {
          type: 'category',
          data: b.ts.map(fmtTs),
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
          axisPointer: { type: 'cross' },
          backgroundColor: p.surface,
          borderColor: p.border,
          textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 }
        },
        dataZoom: [
          { type: 'inside' },
          { type: 'slider', bottom: 4, height: 14, textStyle: { color: p.dim, fontFamily: p.mono } }
        ],
        series: [
          {
            name: $t('journal.exits.chart.price'),
            type: 'candlestick',
            data: ohlc,
            itemStyle: { color: p.green, color0: p.red, borderColor: p.green, borderColor0: p.red },
            markLine: { symbol: 'none', silent: true, label: { show: false }, data: lines },
            markArea: {
              silent: true,
              itemStyle: { color: p.surface2, opacity: 0.6 },
              data: [[{ xAxis: b.entry_idx }, { xAxis: b.exit_idx }]]
            }
          },
          {
            name: $t('journal.exits.chart.entry'),
            type: 'scatter',
            symbol: 'triangle',
            symbolRotate: long ? 0 : 180,
            symbolSize: 12,
            z: 5,
            itemStyle: { color: long ? p.green : p.red },
            data: [[b.entry_idx, b.entry_price]]
          },
          {
            name: $t('journal.exits.chart.realExit'),
            type: 'scatter',
            symbol: 'circle',
            symbolSize: 10,
            z: 5,
            itemStyle: { color: p.text },
            data: b.exit_price != null ? [[b.exit_idx, b.exit_price]] : []
          },
          ...sim
        ]
      },
      true
    );
  });
</script>

<div class="exit-chart" bind:this={el}></div>

<style>
  .exit-chart {
    width: 100%;
    height: 380px;
  }
</style>
