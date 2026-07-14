<script>
  // Result chart: candlesticks with entry/exit markers (▲ long entry, ▼ short, ● exit) and
  // SL/TP markLines per trade, plus an equity-curve pane below. Bars come from the histdata
  // endpoint; trades/equity from the run result. x-axis is the bar timestamps (category),
  // trades are placed by matching their entry/exit ts to the bar index.
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // `assets` (optional): [{ id, ticker }] for a multi-asset run; `activeAssetId` is the dataset
  // whose bars are charted (the parent owns the switcher). Trades are filtered to the active
  // ticker when assets are given.
  let {
    bars = null,
    trades = [],
    equity = [],
    benchmark = [],
    assets = [],
    activeAssetId = null
  } = $props();

  let el;
  let wrap;
  let chart;
  let fullscreen = $state(false);

  // Trades shown on the price pane are the active asset's (single-asset runs carry no ticker,
  // so everything shows). Chart markers get cluttered past ~1 trade per 10 visible bars, so we
  // cull them when the *visible* window is too dense and surface a "zoom in" hint instead.
  const activeTicker = $derived(assets.find((a) => a.id === activeAssetId)?.ticker ?? null);
  const shownTrades = $derived(activeTicker ? trades.filter((t) => t.ticker === activeTicker) : trades);
  const MAX_PER_10_BARS = 1; // density ceiling: 1 trade per 10 bars in the visible window
  // Visible bar window [start,end] as category indices; updated on zoom. null = full range.
  let zoomRange = $state(null);

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
    // Track the visible bar window so trade markers can be culled/revealed by zoom level.
    chart.on('dataZoom', () => {
      const n = bars?.ts?.length ?? 0;
      if (!n) return;
      const opt = chart.getOption();
      const dz = opt.dataZoom?.[0] ?? {};
      let start, end;
      if (dz.startValue != null && dz.endValue != null) {
        start = dz.startValue;
        end = dz.endValue;
      } else {
        start = Math.floor(((dz.start ?? 0) / 100) * (n - 1));
        end = Math.ceil(((dz.end ?? 100) / 100) * (n - 1));
      }
      zoomRange = [start, end];
    });
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

  function build(b, p, tr, range) {
    const idx = new Map(b.ts.map((t, i) => [t, i]));
    const ohlc = b.ts.map((_, i) => [b.o[i], b.c[i], b.l[i], b.h[i]]);

    // Density gate: count trades whose entry falls in the visible window, against the visible
    // bar span. Above the ceiling we drop all markers and show a "zoom in" hint (they reappear
    // as the window narrows). null range = full extent.
    const [vs, ve] = range ?? [0, b.ts.length - 1];
    const visibleBars = Math.max(1, ve - vs + 1);
    const visibleTrades = tr.filter((t) => {
      const ei = idx.get(t.entry_ts);
      return ei != null && ei >= vs && ei <= ve;
    }).length;
    const tooDense = visibleTrades > (visibleBars / 10) * MAX_PER_10_BARS;

    // Entry/exit scatter points.
    const entries = [];
    const exits = [];
    const slLines = [];
    const tpLines = [];
    if (!tooDense) {
      for (const t of tr) {
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
      for (const t of tr) {
        const ei = idx.get(t.entry_ts);
        const xi = idx.get(t.exit_ts);
        if (ei == null || xi == null) continue;
        // We don't have the exact SL/TP price unless it triggered; draw the triggered level.
        if (t.exit_reason === 'stop_loss')
          slLines.push([{ coord: [ei, t.exit_price] }, { coord: [xi, t.exit_price] }]);
        if (t.exit_reason === 'take_profit')
          tpLines.push([{ coord: [ei, t.exit_price] }, { coord: [xi, t.exit_price] }]);
      }
    }

    return {
      graphic: tooDense
        ? [
            {
              type: 'text',
              right: 20,
              top: '8%',
              z: 10,
              style: {
                text: $t('backtest.chart.zoomHint'),
                fill: p.muted,
                fontSize: 11,
                backgroundColor: p.surface,
                padding: [3, 8],
                borderColor: p.border,
                borderWidth: 1
              }
            }
          ]
        : [],
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

  // Full rebuild when the underlying data changes (bars / trade set / active asset). Resets
  // the zoom window, so clear our cached range too.
  $effect(() => {
    if (!chart || !bars) return;
    void shownTrades; // rebuild when the shown trade set changes (asset switch)
    const p = palette();
    zoomRange = null;
    chart.setOption(build(bars, p, shownTrades, null), { notMerge: true });
  });

  // Lightweight re-render on zoom only: recompute markers/hint for the visible window without
  // resetting the dataZoom (merge, not notMerge).
  $effect(() => {
    if (!chart || !bars || !zoomRange) return;
    const p = palette();
    const o = build(bars, p, shownTrades, zoomRange);
    chart.setOption({ graphic: o.graphic, series: o.series }, { replaceMerge: ['series', 'graphic'] });
  });

  function palette() {
    return {
      green: cssVar('--green', '#26a69a'),
      red: cssVar('--red', '#ef5350'),
      accent: cssVar('--accent', '#4a90d9'),
      text: cssVar('--text', '#e6e6e6'),
      muted: cssVar('--muted', '#888'),
      border: cssVar('--border', '#333'),
      surface: cssVar('--surface', '#1a1a1a')
    };
  }
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
