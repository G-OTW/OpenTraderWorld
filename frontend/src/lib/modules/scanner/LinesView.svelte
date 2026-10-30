<script>
  // Lines view: small multiples, one line per item over its recent closes normalized to 100
  // at the first one, so items of any price compare. Each line wears the item's assigned
  // color; its hits are markers on the bar they hit. One ECharts instance, one grid per item,
  // one shared vertical scale per tile (never two axes on one plot).
  import { onMount, onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { chartUrl } from './api.js';
  import { normalized, sortRows, itemColor, concreteColor, rowMetrics } from './board.js';
  import { fmtSignedPct } from '$lib/format.js';
  import { goto } from '$app/navigation';
  import { t } from '$lib/i18n';

  let { items = [], hives = [], hits = [], live = {}, sort = 'ticker', size = 'm', palette, hitsOnly = false } = $props();

  /** Tiles drawn at most: past it a lines view stops being readable; the grid shows them all. */
  const MAX = 48;
  const COLS = { s: 2, m: 3, l: 6 };
  const TILE_H = 92;

  let el = $state();
  let chart = $state(null);

  const rows = $derived(sortRows(hitsOnly ? items.filter((i) => i.last_hit_at) : items, sort, live).slice(0, MAX));
  const total = $derived(hitsOnly ? items.filter((i) => i.last_hit_at).length : items.length);
  const cols = $derived(COLS[size] ?? 3);
  const height = $derived(Math.max(1, Math.ceil(rows.length / cols)) * TILE_H + 8);

  onMount(() => {
    chart = echarts.init(el, null, { renderer: 'canvas' });
    chart.on('click', (p) => {
      const it = rows[p.seriesIndex >> 1];
      if (it) goto(chartUrl(it));
    });
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  });
  onDestroy(() => chart?.dispose());

  function option() {
    const grids = [];
    const xAxis = [];
    const yAxis = [];
    const series = [];
    const titles = [];
    const byItem = new Map();
    for (const h of hits) {
      const list = byItem.get(h.item_id) ?? [];
      list.push(Date.parse(h.bar_ts));
      byItem.set(h.item_id, list);
    }
    const w = 100 / cols;
    rows.forEach((it, i) => {
      const col = i % cols;
      const row = Math.floor(i / cols);
      const line = normalized(it.spark);
      const color = concreteColor(itemColor(it, hives), palette.series) || palette.series[0];
      const top = row * TILE_H + 22;
      grids.push({ left: `${col * w + 1.5}%`, width: `${w - 3}%`, top, height: TILE_H - 34 });
      xAxis.push({ type: 'time', gridIndex: i, show: false });
      yAxis.push({ type: 'value', gridIndex: i, scale: true, show: false });
      const m = rowMetrics(it, live[it.id]);
      titles.push({
        text: it.ticker,
        subtext: m.change_pct != null ? fmtSignedPct(m.change_pct) : '',
        left: `${col * w + 1.5}%`,
        top: row * TILE_H + 2,
        textStyle: { fontSize: 11, fontFamily: palette.mono, color: palette.text, fontWeight: 'normal' },
        subtextStyle: { fontSize: 10, color: m.change_pct >= 0 ? palette.green : palette.red },
        itemGap: 2
      });
      series.push({
        type: 'line',
        xAxisIndex: i,
        yAxisIndex: i,
        data: line,
        showSymbol: false,
        lineStyle: { width: 2, color },
        itemStyle: { color },
        emphasis: { disabled: true },
        silent: false
      });
      // Hits: a marker on the closest drawn point at or before the hit's bar.
      const marks = (byItem.get(it.id) ?? [])
        .map((ts) => {
          let best = null;
          for (const p of line) if (p[0] <= ts) best = p;
          return best;
        })
        .filter(Boolean);
      series.push({
        type: 'scatter',
        xAxisIndex: i,
        yAxisIndex: i,
        data: marks,
        symbolSize: 8,
        itemStyle: { color, borderColor: palette.surface, borderWidth: 2 },
        tooltip: { show: false }
      });
    });
    return {
      animation: false,
      title: titles,
      grid: grids,
      xAxis,
      yAxis,
      series,
      tooltip: {
        trigger: 'axis',
        backgroundColor: palette.surface,
        borderColor: palette.borderControl,
        textStyle: { color: palette.text, fontSize: 11 },
        formatter: (ps) => {
          const p = Array.isArray(ps) ? ps[0] : ps;
          if (!p) return '';
          const it = rows[p.seriesIndex >> 1];
          const d = new Date(p.value[0]).toLocaleString();
          return `${it?.ticker ?? ''}<br/>${d}<br/>${(p.value[1] - 100 >= 0 ? '+' : '')}${(p.value[1] - 100).toFixed(2)}%`;
        }
      }
    };
  }

  $effect(() => {
    if (!chart) return;
    const opt = option();
    chart.setOption(opt, true);
    chart.resize();
  });
</script>

{#if !rows.length}
  <p class="hint pad">{hitsOnly ? $t('scanner.board.noHits') : $t('scanner.board.noItems')}</p>
{/if}
<div class="lines" bind:this={el} style="height:{rows.length ? height : 0}px" role="img" aria-label={$t('scanner.view.lines')}></div>
{#if total > MAX}
  <p class="hint pad">{$t('scanner.board.linesCap', { max: MAX, n: total })}</p>
{/if}

<style>
  .lines {
    width: 100%;
  }
  .pad {
    padding: var(--space-2) var(--space-4);
  }
</style>
