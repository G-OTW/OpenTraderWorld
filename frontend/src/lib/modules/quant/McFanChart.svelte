<script>
  // Equity fan chart: the p5–p95 and p25–p75 percentile bands of the resampled equity paths
  // across trade steps, with the median line and the actual realized curve overlaid. Bands are
  // drawn as a base (transparent) series plus a stacked filled band on top. Redraws on
  // data/theme change.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t } from '$lib/i18n';

  let { result } = $props(); // MonteCarloResult
  let el;
  let chart;
  const colors = $derived(chartColors());

  function build(r, p) {
    const steps = r.fan.map((f) => f.step);
    const p5 = r.fan.map((f) => +f.p5.toFixed(2));
    const p25 = r.fan.map((f) => +f.p25.toFixed(2));
    const p50 = r.fan.map((f) => +f.p50.toFixed(2));
    const p75 = r.fan.map((f) => +f.p75.toFixed(2));
    const p95 = r.fan.map((f) => +f.p95.toFixed(2));
    // Stacked-band trick: base = lower bound, then the delta to the upper bound is stacked and
    // filled. Two bands (5–95 lighter, 25–75 darker) share the same base pattern.
    const outerBase = p5;
    const outerSpan = p95.map((v, i) => +(v - p5[i]).toFixed(2));
    const innerBase = p25;
    const innerSpan = p75.map((v, i) => +(v - p25[i]).toFixed(2));
    // Fan steps may be subsampled (long horizons); place the realized curve on the same
    // sampled x positions so it lines up with the bands.
    const curve = r.actual_curve ?? [];
    const actual = steps.map((s) => (s < curve.length ? +curve[s].toFixed(2) : null));
    const ruin = +r.ruin_level.toFixed(2);

    const band = (name, base, span, stackId, opacity) => [
      { name: `${name}-base`, type: 'line', stack: stackId, data: base, lineStyle: { opacity: 0 }, symbol: 'none', areaStyle: { opacity: 0 }, silent: true, z: 1 },
      { name, type: 'line', stack: stackId, data: span, lineStyle: { opacity: 0 }, symbol: 'none', areaStyle: { color: p.accent, opacity }, silent: true, z: 1 }
    ];

    return {
      animation: false,
      grid: { left: 64, right: 20, top: 20, bottom: 56 },
      tooltip: {
        trigger: 'axis',
        backgroundColor: p.surface,
        borderColor: p.border,
        textStyle: { color: p.text, fontSize: 11 },
        formatter: (items) => {
          const fp = r.fan[items[0]?.dataIndex];
          const f = (v) => (v == null ? '—' : v.toLocaleString(undefined, { maximumFractionDigits: 0 }));
          return `${$t('quant.mc.trade')} ${fp?.step}<br/>`
            + `p95 ${f(fp?.p95)}<br/>p75 ${f(fp?.p75)}<br/><b>p50 ${f(fp?.p50)}</b><br/>p25 ${f(fp?.p25)}<br/>p5 ${f(fp?.p5)}`;
        }
      },
      xAxis: {
        type: 'category',
        data: steps,
        name: $t('quant.mc.tradeNumber'),
        nameLocation: 'middle',
        nameGap: 30,
        nameTextStyle: { color: p.muted },
        axisLine: { lineStyle: { color: p.border } },
        axisLabel: { color: p.muted }
      },
      yAxis: {
        type: 'value',
        scale: true,
        name: $t('quant.mc.equity'),
        nameTextStyle: { color: p.muted },
        axisLabel: { color: p.muted },
        splitLine: { lineStyle: { color: p.border, opacity: 0.3 } }
      },
      series: [
        ...band('p5-95', outerBase, outerSpan, 'outer', 0.14),
        ...band('p25-75', innerBase, innerSpan, 'inner', 0.28),
        { name: $t('quant.mc.median'), type: 'line', data: p50, lineStyle: { color: p.accent, width: 2 }, symbol: 'none', z: 3 },
        { name: $t('quant.mc.actual'), type: 'line', data: actual, lineStyle: { color: p.text, width: 1.5, type: 'dashed' }, symbol: 'none', z: 4 },
        {
          name: $t('quant.mc.ruinLevel'),
          type: 'line',
          data: [],
          markLine: {
            symbol: 'none',
            silent: true,
            data: [{ yAxis: ruin, label: { formatter: $t('quant.mc.ruin'), color: p.red, position: 'insideEndTop' }, lineStyle: { color: p.red, type: 'dashed' } }]
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
    height: 360px;
  }
</style>
