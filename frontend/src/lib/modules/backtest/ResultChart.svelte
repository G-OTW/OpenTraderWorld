<script>
  // Result chart: candlesticks with entry/exit markers (▲ long entry, ▼ short, ● exit) and
  // SL/TP markLines per trade, plus an equity-curve pane below. Bars come from the histdata
  // endpoint; trades/equity from the run result. x-axis is the bar timestamps (category),
  // trades are placed by matching their entry/exit ts to the bar index.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let { bars = null, trades = [], equity = [], benchmark = [] } = $props();

  let el;
  let wrap;
  let chart;
  let fullscreen = $state(false);

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    const onFsChange = () => {
      fullscreen = document.fullscreenElement === wrap;
      // Let the layout settle before echarts re-measures.
      requestAnimationFrame(() => chart?.resize());
    };
    document.addEventListener('fullscreenchange', onFsChange);
    return () => {
      ro.disconnect();
      document.removeEventListener('fullscreenchange', onFsChange);
    };
  });
  onDestroy(() => chart?.dispose());

  function toggleFullscreen() {
    if (document.fullscreenElement) document.exitFullscreen?.();
    else wrap?.requestFullscreen?.();
  }

  function cssVar(name, fb) {
    if (typeof window === 'undefined') return fb;
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fb;
  }

  function build(b, p) {
    const idx = new Map(b.ts.map((t, i) => [t, i]));
    const ohlc = b.ts.map((_, i) => [b.o[i], b.c[i], b.l[i], b.h[i]]);

    // Entry/exit scatter points.
    const entries = [];
    const exits = [];
    for (const t of trades) {
      const ei = idx.get(t.entry_ts);
      const xi = idx.get(t.exit_ts);
      if (ei != null)
        entries.push({
          value: [ei, t.entry_price],
          itemStyle: { color: t.direction === 'long' ? p.green : p.red },
          symbol: t.direction === 'long' ? 'triangle' : 'pin',
          symbolRotate: t.direction === 'long' ? 0 : 180
        });
      if (xi != null)
        exits.push({
          value: [xi, t.exit_price],
          itemStyle: { color: t.pnl >= 0 ? p.green : p.red }
        });
    }

    // SL/TP horizontal segments spanning each trade's lifetime.
    const slLines = [];
    const tpLines = [];
    for (const t of trades) {
      const ei = idx.get(t.entry_ts);
      const xi = idx.get(t.exit_ts);
      if (ei == null || xi == null) continue;
      const dir = t.direction === 'long' ? 1 : -1;
      if (t.exit_reason === 'stop_loss' || trades.sl) {
        /* drawn via markLine below using entry price only when reason matches */
      }
      // We don't have the exact SL/TP price unless it triggered; draw the triggered level.
      if (t.exit_reason === 'stop_loss')
        slLines.push([{ coord: [ei, t.exit_price] }, { coord: [xi, t.exit_price] }]);
      if (t.exit_reason === 'take_profit')
        tpLines.push([{ coord: [ei, t.exit_price] }, { coord: [xi, t.exit_price] }]);
      void dir;
    }

    return {
      animation: false,
      backgroundColor: 'transparent',
      textStyle: { color: p.text },
      legend: { top: 0, textStyle: { color: p.muted, fontSize: 10 } },
      grid: [
        { left: 56, right: 16, top: '8%', height: '56%' },
        { left: 56, right: 16, top: '72%', height: '20%' }
      ],
      xAxis: [
        { type: 'category', data: b.ts, gridIndex: 0, axisLabel: { show: false }, axisLine: { lineStyle: { color: p.border } } },
        { type: 'category', data: b.ts, gridIndex: 1, axisLabel: { color: p.muted, fontSize: 10 }, axisLine: { lineStyle: { color: p.border } } }
      ],
      yAxis: [
        { scale: true, gridIndex: 0, axisLabel: { color: p.muted, fontSize: 10 }, splitLine: { lineStyle: { color: p.border, opacity: 0.3 } } },
        { scale: true, gridIndex: 1, name: $t('backtest.chart.equity'), nameTextStyle: { color: p.muted }, axisLabel: { color: p.muted, fontSize: 10 }, splitLine: { show: false } }
      ],
      axisPointer: { link: [{ xAxisIndex: 'all' }] },
      tooltip: { trigger: 'axis', axisPointer: { type: 'cross' }, backgroundColor: p.surface, borderColor: p.border, textStyle: { color: p.text, fontSize: 11 } },
      dataZoom: [
        { type: 'inside', xAxisIndex: [0, 1] },
        { type: 'slider', xAxisIndex: [0, 1], bottom: 4, height: 14, textStyle: { color: p.muted } }
      ],
      series: [
        {
          name: $t('backtest.chart.price'),
          type: 'candlestick',
          data: ohlc,
          xAxisIndex: 0,
          yAxisIndex: 0,
          itemStyle: { color: p.green, color0: p.red, borderColor: p.green, borderColor0: p.red },
          markLine: {
            symbol: 'none',
            silent: true,
            lineStyle: { type: 'dashed', width: 1 },
            data: [
              ...slLines.map((seg) => seg.map((s) => ({ ...s, lineStyle: { color: p.red } }))),
              ...tpLines.map((seg) => seg.map((s) => ({ ...s, lineStyle: { color: p.green } })))
            ]
          }
        },
        { name: $t('backtest.chart.entry'), type: 'scatter', data: entries, xAxisIndex: 0, yAxisIndex: 0, symbolSize: 11, z: 5 },
        { name: $t('backtest.chart.exit'), type: 'scatter', data: exits, xAxisIndex: 0, yAxisIndex: 0, symbol: 'circle', symbolSize: 7, z: 5 },
        {
          name: $t('backtest.chart.equity'),
          type: 'line',
          data: equity.map((e) => e.equity),
          xAxisIndex: 1,
          yAxisIndex: 1,
          showSymbol: false,
          lineStyle: { color: p.accent, width: 1.5 },
          areaStyle: { color: p.accent, opacity: 0.08 }
        },
        {
          // Same starting capital passively holding the asset(s) — the benchmark to beat.
          // Prefer the engine's equal-weight, fee-aware curve (keyed by ts); fall back to a
          // naive close-ratio line when it isn't present (e.g. an old saved run).
          name: $t('backtest.chart.buyHold'),
          type: 'line',
          data:
            benchmark.length
              ? (() => {
                  const byTs = new Map(benchmark.map((e) => [e.ts, e.equity]));
                  let last = benchmark[0]?.equity ?? null;
                  return b.ts.map((ts) => {
                    if (byTs.has(ts)) last = byTs.get(ts);
                    return last;
                  });
                })()
              : b.c[0] > 0 && equity.length
                ? b.c.map((c) => (equity[0].equity * c) / b.c[0])
                : [],
          xAxisIndex: 1,
          yAxisIndex: 1,
          showSymbol: false,
          lineStyle: { color: p.muted, width: 1, type: 'dashed' }
        }
      ]
    };
  }

  $effect(() => {
    if (!chart || !bars) return;
    const p = {
      green: cssVar('--green', '#26a69a'),
      red: cssVar('--red', '#ef5350'),
      accent: cssVar('--accent', '#4a90d9'),
      text: cssVar('--text', '#e6e6e6'),
      muted: cssVar('--muted', '#888'),
      border: cssVar('--border', '#333'),
      surface: cssVar('--surface', '#1a1a1a')
    };
    chart.setOption(build(bars, p), { notMerge: true });
  });
</script>

<div class="chart-wrap" class:fs={fullscreen} bind:this={wrap}>
  <button
    class="fs-btn"
    onclick={toggleFullscreen}
    title={fullscreen ? $t('backtest.chart.exitFullscreen') : $t('backtest.chart.fullscreen')}
    aria-label={fullscreen ? $t('backtest.chart.exitFullscreen') : $t('backtest.chart.fullscreen')}
  >
    <Icon name={fullscreen ? 'chevron-down' : 'maximize'} size={15} />
  </button>
  <div class="chart" bind:this={el}></div>
</div>

<style>
  .chart-wrap {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .chart-wrap.fs {
    background: var(--surface);
    padding: var(--space-4);
  }
  .chart {
    width: 100%;
    height: 100%;
    min-height: 380px;
  }
  .fs-btn {
    position: absolute;
    top: var(--space-2);
    right: var(--space-2);
    z-index: 5;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--surface-2) 85%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: 4px;
    cursor: pointer;
    opacity: 0.7;
    transition: opacity 0.12s ease, color 0.12s ease, border-color 0.12s ease;
  }
  .fs-btn:hover {
    opacity: 1;
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }
</style>
