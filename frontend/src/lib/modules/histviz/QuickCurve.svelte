<script>
  // Performance tab of the quick backtest — the strategy-tester chart: cumulative P&L over
  // time (green while the session is up, red once it is under water), the run-up and heat
  // each trade went through as bars around the zero line, and the drawdown in a pane below.
  // Everything is derived from the closed trades, so it redraws the moment a marker is
  // flipped or deleted.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { t } from '$lib/i18n';
  import { equityCurve } from './quicktest.js';

  let { trades = [] } = $props();

  const curve = $derived(equityCurve(trades));

  // Series the legend can switch off, the way a tester lets you strip the chart down.
  let showExcursions = $state(true);
  let showDrawdown = $state(true);

  let el;
  let chart;

  function cssVar(name, fb) {
    if (typeof window === 'undefined') return fb;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fb;
  }

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  });
  onDestroy(() => chart?.dispose());

  const money = (v) => (v >= 0 ? '+' : '') + v.toLocaleString(undefined, { maximumFractionDigits: 2 });

  function build(points, withExcursions, withDrawdown) {
    const p = {
      green: cssVar('--green', '#22c55e'),
      red: cssVar('--red', '#ef4444'),
      dim: cssVar('--muted', '#8b8b8b'),
      line: cssVar('--border', '#2a2a2a'),
      surface: cssVar('--surface', '#141414'),
      text: cssVar('--text', '#e5e5e5'),
      mono: cssVar('--mono', 'monospace')
    };
    // The curve starts flat at zero just before the first close, so a single trade still
    // draws a segment instead of a lone dot.
    const eq = points.map((x) => [x.ts, x.equity]);
    const dd = points.map((x) => [x.ts, x.dd]);
    const up = points.map((x) => [x.ts, x.runup]);
    const down = points.map((x) => [x.ts, x.mae]);
    if (points.length) {
      eq.unshift([points[0].ts - 1, 0]);
      dd.unshift([points[0].ts - 1, 0]);
    }
    const byTs = new Map(points.map((x) => [x.ts, x]));
    // Bounds the colour map spans, always straddling zero so both pieces exist.
    const vals = points.map((x) => x.equity);
    const hiV = Math.max(0, ...vals);
    const loV = Math.min(0, ...vals);
    const pad = Math.max(hiV - loV, 1) * 0.2;
    const hi = hiV + pad;
    const lo = loV - pad;
    const grids = withDrawdown
      ? [
          { left: 66, right: 12, top: 16, height: '50%' },
          { left: 66, right: 12, top: '72%', bottom: 24 }
        ]
      : [{ left: 66, right: 12, top: 16, bottom: 24 }];
    const axis = (gridIndex, showLabel) => ({
      type: 'time',
      gridIndex,
      axisLine: { lineStyle: { color: p.line } },
      axisTick: { show: false },
      axisLabel: { show: showLabel, color: p.dim, fontFamily: p.mono, fontSize: 10, hideOverlap: true },
      splitLine: { show: false }
    });
    // `splitNumber` is a ceiling here: these panes are a couple of hundred pixels tall and
    // ECharts' default tick count stacks labels on top of each other.
    const value = (gridIndex, name, splitNumber) => ({
      type: 'value',
      gridIndex,
      name,
      nameTextStyle: { color: p.dim, fontSize: 10, align: 'left' },
      nameGap: 6,
      scale: true,
      splitNumber,
      axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, hideOverlap: true },
      splitLine: { lineStyle: { color: p.line, opacity: 0.5 } }
    });
    const series = [
      {
        name: $t('histviz.quick.equity'),
        type: 'line',
        data: eq,
        step: 'end', // P&L lands when a trade closes, it does not drift between two
        showSymbol: false,
        clip: true,
        z: 3,
        lineStyle: { width: 1.6 },
        // Filled between the line and zero, not down to the pane floor — the tester's read.
        areaStyle: { opacity: 0.14, origin: 0 }
      }
    ];
    // The zero line is a two-point series, not a markLine: a markLine renders in its own
    // progressive task, and one throw there aborts the rest of the frame — the whole chart
    // went blank the day this pane grew a second series.
    if (points.length)
      series.push({
        name: 'zero',
        type: 'line',
        data: [
          [points[0].ts - 1, 0],
          [points.at(-1).ts, 0]
        ],
        showSymbol: false,
        silent: true,
        clip: true,
        z: 0,
        lineStyle: { color: p.line, type: 'dashed', width: 1 },
        tooltip: { show: false }
      });
    if (withExcursions) {
      const bar = (name, data, color) => ({
        name,
        type: 'bar',
        data,
        clip: true,
        barGap: '-100%',
        barMaxWidth: 6,
        silent: true,
        z: 1,
        itemStyle: { color, opacity: 0.55 }
      });
      series.push(bar($t('histviz.quick.runup'), up, p.green));
      series.push(bar($t('histviz.quick.mae'), down, p.red));
    }
    if (withDrawdown)
      series.push({
        name: $t('histviz.quick.drawdown'),
        type: 'line',
        data: dd,
        step: 'end',
        xAxisIndex: 1,
        yAxisIndex: 1,
        showSymbol: false,
        clip: true,
        lineStyle: { color: p.red, width: 1.2 },
        areaStyle: { color: p.red, opacity: 0.18 }
      });
    return {
      animation: false,
      backgroundColor: 'transparent',
      grid: grids,
      axisPointer: { link: [{ xAxisIndex: 'all' }] },
      // One tooltip per trade, in the tester's own words: where the session stood, and how
      // far that trade ran each way before it closed.
      tooltip: {
        trigger: 'axis',
        axisPointer: { type: 'line', lineStyle: { color: p.dim } },
        backgroundColor: p.surface,
        borderColor: p.line,
        textStyle: { color: p.text, fontFamily: p.mono, fontSize: 11 },
        formatter: (items) => {
          const ts = items?.[0]?.axisValue;
          const x = byTs.get(+new Date(ts));
          if (!x) return '';
          const row = (label, v, color) =>
            `<div style="display:flex;gap:12px;justify-content:space-between">
               <span style="color:${color}">● ${label}</span><b>${money(v)}</b></div>`;
          return [
            `<div style="color:${p.dim};text-align:center;margin-bottom:4px">${$t('histviz.quick.tradeN', {
              n: x.n,
              side: x.dir > 0 ? $t('histviz.quick.long') : $t('histviz.quick.short')
            })}</div>`,
            row($t('histviz.quick.equity'), x.equity, x.equity >= 0 ? p.green : p.red),
            row($t('histviz.quick.pnl'), x.pnl, x.pnl >= 0 ? p.green : p.red),
            withExcursions ? row($t('histviz.quick.runup'), x.runup, p.green) : '',
            withExcursions ? row($t('histviz.quick.mae'), x.mae, p.red) : '',
            withDrawdown ? row($t('histviz.quick.drawdown'), x.dd, p.red) : ''
          ].join('');
        }
      },
      // The line and its fill take the colour of the side of zero they are on — the tester's
      // read at a glance: green while the session is up, red once it is under water.
      // The pieces must be *bounded*: a line series turns its visual map into a gradient, and
      // an open-ended piece contributes no stop — with none at all ECharts throws mid-render
      // and the whole frame is lost.
      visualMap: {
        show: false,
        seriesIndex: 0,
        dimension: 1,
        pieces: [
          { min: lo, max: 0, color: p.red },
          { min: 0, max: hi, color: p.green }
        ]
      },
      xAxis: withDrawdown ? [axis(0, false), axis(1, true)] : [axis(0, true)],
      yAxis: withDrawdown
        ? [value(0, $t('histviz.quick.pnl'), 4), value(1, $t('histviz.quick.drawdown'), 2)]
        : [value(0, $t('histviz.quick.pnl'), 4)],
      series
    };
  }

  $effect(() => {
    const pts = curve.points;
    const ex = showExcursions;
    const dw = showDrawdown;
    if (!chart) return;
    chart.setOption(build(pts, ex, dw), { replaceMerge: ['series', 'xAxis', 'yAxis', 'grid'] });
  });
</script>

<div class="curve">
  <div class="legend">
    <span class="item on"><i style:background="var(--green)"></i>{$t('histviz.quick.equity')}</span>
    <button class="item" class:on={showExcursions} onclick={() => (showExcursions = !showExcursions)}>
      <i style:background="var(--green)"></i><i style:background="var(--red)"></i>
      {$t('histviz.quick.excursions')}
    </button>
    <button class="item" class:on={showDrawdown} onclick={() => (showDrawdown = !showDrawdown)}>
      <i style:background="var(--red)"></i>{$t('histviz.quick.drawdown')}
    </button>
  </div>
  <div class="ec" bind:this={el}></div>
  {#if !curve.points.length}
    <p class="hint">{$t('histviz.quick.noClosed')}</p>
  {/if}
</div>

<style>
  .curve {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 160px;
  }
  .legend {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0 0 var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: none;
    padding: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
    opacity: 0.5;
  }
  .item.on {
    color: var(--text);
    opacity: 1;
  }
  .item i {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    display: inline-block;
  }
  .ec {
    flex: 1;
    width: 100%;
    min-height: 0;
  }
  .hint {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
</style>
