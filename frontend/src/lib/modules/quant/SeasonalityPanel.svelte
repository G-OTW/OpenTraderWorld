<script>
  // Seasonality of one dataset's period returns: a month × weekday heatmap plus three 1-D
  // strips (by month, by weekday, and — intraday only — by hour). Green = positive mean return
  // (or higher vol under the volatility metric), red = negative; symmetric around 0. Hover a
  // cell for the exact value, sample count and win rate. Redraws on data/theme change.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { t } from '$lib/i18n';

  let { result } = $props(); // SeasonalityResult (carries its own metric: 'return' | 'volatility')

  let el;
  let chart;
  const colors = $derived(chartColors());

  const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const WD = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  // Value → percent string for tooltips; both metrics are per-period fractions.
  const pct = (v, d = 3) => (v == null ? '—' : `${(v * 100).toFixed(d)}%`);

  function build(r, p) {
    // Read the metric off the result, not the form control — the chart must describe the data
    // it shows, even when the user flips the metric toggle without re-analyzing.
    const isVol = r.metric === 'volatility';
    // For returns, center the color scale on 0 (green up / red down). For volatility, 0..max
    // with a single green→red ramp reads "calm → wild".
    const flat = [];
    for (const row of r.month_weekday) for (const v of row) if (v != null) flat.push(Math.abs(v));
    for (const b of [...r.month, ...r.weekday, ...(r.hour ?? [])]) flat.push(Math.abs(b.value));
    const mag = flat.length ? Math.max(...flat) : 0.01;

    // Green(low/neg) → neutral → red is the correlation semantic; here we want green=good, so
    // build our own ramp: red(neg) → neutral → green(pos) for returns; neutral → red for vol.
    const ramp = isVol
      ? [p.surface2, p.amber, p.red]
      : [p.red, p.surface2, p.green];
    const vmMin = isVol ? 0 : -mag;
    const vmMax = mag;

    // month × weekday matrix cells: [wdCol, monthRow, value%]
    const mw = [];
    for (let m = 0; m < 12; m++) {
      for (let w = 0; w < 7; w++) {
        const v = r.month_weekday[m]?.[w];
        mw.push([w, m, v == null ? '-' : +(v * 100).toFixed(3)]);
      }
    }

    // 1-D strips laid out as single-row heatmaps under the matrix.
    const monthStrip = r.month.map((b, i) => [i, 0, +(b.value * 100).toFixed(3)]);
    const wdStrip = r.weekday.map((b, i) => [i, 0, +(b.value * 100).toFixed(3)]);
    const hasHour = r.has_hour && r.hour.length > 0;
    const hourStrip = hasHour ? r.hour.map((b, i) => [i, 0, +(b.value * 100).toFixed(3)]) : [];

    const countByMonth = Object.fromEntries(r.month.map((b) => [b.key, b]));
    const countByWd = Object.fromEntries(r.weekday.map((b) => [b.key, b]));

    // Grid layout: big matrix on top, then month strip, weekday strip, (hour strip).
    // The matrix has top-positioned x-axis labels (weekdays), so its grid starts well below
    // the title to keep the title from stacking under the "Mon…Sun" row.
    const grids = [
      { left: 90, right: 24, top: 48, height: 210 }, // month × weekday matrix
      { left: 90, right: 24, top: 310, height: 34 }, // by month
      { left: 90, right: 24, top: 384, height: 34 } // by weekday
    ];
    if (hasHour) grids.push({ left: 90, right: 24, top: 458, height: 34 });

    const titleColor = p.muted;
    const titleStyle = { fontSize: 11, fontWeight: 500, color: titleColor };
    const titles = [
      { text: $t(isVol ? 'quant.seasonality.byMonthWeekdayVol' : 'quant.seasonality.byMonthWeekday'), left: 90, top: 4, textStyle: titleStyle },
      { text: $t('quant.seasonality.byMonth'), left: 90, top: 290, textStyle: titleStyle },
      { text: $t('quant.seasonality.byWeekday'), left: 90, top: 364, textStyle: titleStyle }
    ];
    if (hasHour) titles.push({ text: $t('quant.seasonality.byHour'), left: 90, top: 438, textStyle: titleStyle });

    const axisLbl = { color: p.dim, fontFamily: p.mono, fontSize: 10 };
    const cat = (data, gridIndex, extra = {}) => ({
      type: 'category', data, gridIndex, axisLine: { lineStyle: { color: p.border } }, axisTick: { show: false }, splitArea: { show: true }, axisLabel: axisLbl, ...extra
    });

    return {
      animation: false,
      grid: grids,
      title: titles,
      tooltip: {
        backgroundColor: p.surface,
        borderColor: p.border,
        textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 },
        formatter: (c) => {
          const [x, , v] = c.value;
          if (v === '-' || v == null) return $t('quant.seasonality.noData');
          if (c.seriesIndex === 0) {
            const m = c.value[1];
            return `${MONTHS[m]} · ${WD[x]}: <b>${v}%</b>`;
          }
          if (c.seriesIndex === 1) {
            const b = countByMonth[x];
            return `${MONTHS[x]}: <b>${v}%</b><br/>${$t('quant.seasonality.samples', { n: b?.count ?? 0 })} · ${$t('quant.seasonality.winRate', { r: pct(b?.win_rate ?? 0, 0) })}`;
          }
          if (c.seriesIndex === 2) {
            const b = countByWd[x];
            return `${WD[x]}: <b>${v}%</b><br/>${$t('quant.seasonality.samples', { n: b?.count ?? 0 })} · ${$t('quant.seasonality.winRate', { r: pct(b?.win_rate ?? 0, 0) })}`;
          }
          const hb = r.hour[x];
          return `${String(x).padStart(2, '0')}:00 — <b>${v}%</b><br/>${$t('quant.seasonality.samples', { n: hb?.count ?? 0 })}`;
        }
      },
      visualMap: {
        min: +(vmMin * 100).toFixed(3),
        max: +(vmMax * 100).toFixed(3),
        calculable: true,
        orient: 'horizontal',
        left: 'center',
        bottom: 0,
        inRange: { color: ramp },
        textStyle: { color: p.dim, fontFamily: p.mono, fontSize: 9 },
        formatter: (v) => `${(+v).toFixed(2)}%`
      },
      xAxis: [
        cat(WD, 0, { position: 'top' }),
        cat(MONTHS, 1),
        cat(WD, 2),
        ...(hasHour ? [cat([...Array(24).keys()].map((h) => String(h).padStart(2, '0')), 3, { axisLabel: { ...axisLbl, interval: 1 } })] : [])
      ],
      yAxis: [
        { type: 'category', data: MONTHS, gridIndex: 0, inverse: true, axisLine: { lineStyle: { color: p.border } }, axisTick: { show: false }, splitArea: { show: true }, axisLabel: axisLbl },
        { type: 'category', data: [''], gridIndex: 1, axisLabel: { show: false }, axisTick: { show: false }, axisLine: { show: false } },
        { type: 'category', data: [''], gridIndex: 2, axisLabel: { show: false }, axisTick: { show: false }, axisLine: { show: false } },
        ...(hasHour ? [{ type: 'category', data: [''], gridIndex: 3, axisLabel: { show: false }, axisTick: { show: false }, axisLine: { show: false } }] : [])
      ],
      series: [
        { type: 'heatmap', xAxisIndex: 0, yAxisIndex: 0, data: mw, itemStyle: { borderColor: p.surface, borderWidth: 2 }, emphasis: { itemStyle: { borderColor: p.text } } },
        { type: 'heatmap', xAxisIndex: 1, yAxisIndex: 1, data: monthStrip, itemStyle: { borderColor: p.surface, borderWidth: 2 }, emphasis: { itemStyle: { borderColor: p.text } } },
        { type: 'heatmap', xAxisIndex: 2, yAxisIndex: 2, data: wdStrip, itemStyle: { borderColor: p.surface, borderWidth: 2 }, emphasis: { itemStyle: { borderColor: p.text } } },
        ...(hasHour ? [{ type: 'heatmap', xAxisIndex: 3, yAxisIndex: 3, data: hourStrip, itemStyle: { borderColor: p.surface, borderWidth: 2 }, emphasis: { itemStyle: { borderColor: p.text } } }] : [])
      ]
    };
  }

  const chartHeight = $derived(result?.has_hour && result?.hour?.length ? 564 : 494);

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  });
  onDestroy(() => chart?.dispose());

  $effect(() => {
    if (chart && result) chart.setOption(build(result, colors), true);
    // Height depends on whether the hour axis is present; resize after the DOM updates.
    if (chart) queueMicrotask(() => chart?.resize());
  });
</script>

<div class="chart" bind:this={el} style:height={`${chartHeight}px`}></div>

<style>
  .chart {
    width: 100%;
  }
</style>
