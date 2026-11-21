<script>
  // A chart the agent drew. The `chart` tool result names a stored, already-reduced chart
  // (the series never went through the model); it is fetched once the block scrolls into
  // view, so a long conversation does not load every chart up front, and drawn on canvas.
  import { onDestroy } from 'svelte';
  import * as echarts from 'echarts';
  import { t } from '$lib/i18n';
  import { chartColors, axisStyle, xAxisStyle, tooltipStyle, withAlpha } from '$lib/theme/chart.svelte.js';
  import { agentApi } from '$lib/modules/agent/api.js';

  // result: the tool result text (JSON summary carrying chart_id; may arrive cut for the UI).
  let { result = '' } = $props();

  const meta = $derived.by(() => {
    try {
      return JSON.parse(result);
    } catch {
      return null;
    }
  });
  const chartId = $derived(
    meta?.chart_id ?? String(result ?? '').match(/"chart_id":"([0-9a-f-]{36})"/)?.[1] ?? null
  );

  let el = $state();
  let chart;
  let visible = $state(false);
  let row = $state(null);
  let failed = $state(false);
  const colors = $derived(chartColors());

  $effect(() => {
    if (!el) return;
    const io = new IntersectionObserver(
      ([e]) => {
        if (e.isIntersecting) {
          visible = true;
          io.disconnect();
        }
      },
      { rootMargin: '200px' }
    );
    io.observe(el);
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => {
      io.disconnect();
      ro.disconnect();
    };
  });

  $effect(() => {
    if (!visible || !chartId || row || failed) return;
    agentApi
      .getChart(chartId)
      .then((r) => (row = r))
      .catch(() => (failed = true));
  });

  $effect(() => {
    const data = row?.data;
    if (!el || !data) return;
    chart ??= echarts.init(el, null, { renderer: 'canvas' });
    chart.setOption(build(data, colors), true);
  });

  onDestroy(() => chart?.dispose());

  const fmt = (v) =>
    typeof v === 'number' ? v.toLocaleString(undefined, { maximumFractionDigits: Math.abs(v) < 1 ? 6 : 2 }) : v;

  function build(d, c) {
    // Labels can come from third-party content (mail, feeds): richText keeps the tooltip on
    // the canvas instead of injecting HTML.
    const tooltip = { ...tooltipStyle(c), renderMode: 'richText', valueFormatter: fmt };
    const base = { animation: false, textStyle: { fontFamily: c.mono } };

    if (d.type === 'donut') {
      const n = d.slices.length;
      return {
        ...base,
        tooltip: { ...tooltip, trigger: 'item' },
        legend: { type: 'scroll', orient: 'vertical', right: 8, top: 'middle', textStyle: { color: c.muted, fontSize: 11 } },
        series: [
          {
            type: 'pie',
            radius: ['48%', '74%'],
            center: ['38%', '50%'],
            label: { show: false },
            data: d.slices.map((s, i) => {
              // The server folds the tail into a last "Other" slice: drawn as a remainder.
              const other = i === n - 1 && s.name === 'Other' && n > 1;
              return {
                name: other ? $t('agent.chart.other') : s.name,
                value: s.value,
                itemStyle: { color: other ? c.faint : c.series[i % c.series.length] }
              };
            })
          }
        ]
      };
    }

    if (d.type === 'histogram') {
      const bars = d.counts.map((n, i) => [(d.edges[i] + d.edges[i + 1]) / 2, n]);
      return {
        ...base,
        grid: { left: 48, right: 16, top: 12, bottom: 28 },
        tooltip: { ...tooltip, trigger: 'axis' },
        xAxis: { type: 'value', scale: true, name: d.unit || '', ...xAxisStyle(c) },
        yAxis: { type: 'value', ...axisStyle(c) },
        series: [{ type: 'bar', data: bars, barWidth: '92%', itemStyle: { color: c.accent, opacity: 0.8 } }]
      };
    }

    const xType = { time: 'time', category: 'category' }[d.x_kind] ?? 'value';
    const many = d.series.length > 1;
    return {
      ...base,
      grid: { left: 56, right: 16, top: many ? 32 : 12, bottom: 28 },
      tooltip: { ...tooltip, trigger: d.type === 'scatter' ? 'item' : 'axis' },
      legend: many ? { top: 0, textStyle: { color: c.muted, fontSize: 11 } } : undefined,
      xAxis: { type: xType, scale: xType === 'value', ...xAxisStyle(c) },
      yAxis: { type: 'value', scale: true, name: d.unit || '', nameTextStyle: { color: c.dim }, ...axisStyle(c) },
      series: d.series.map((s, i) => {
        const color = c.series[i % c.series.length];
        const common = { name: s.name, data: s.points, itemStyle: { color } };
        if (d.type === 'bar') return { ...common, type: 'bar' };
        if (d.type === 'scatter') return { ...common, type: 'scatter', symbolSize: 4, large: true };
        return {
          ...common,
          type: 'line',
          showSymbol: false,
          sampling: 'lttb',
          lineStyle: { color, width: 1.5 },
          areaStyle: d.type === 'area' ? { color: withAlpha(color, 0.15) } : undefined
        };
      })
    };
  }
</script>

<figure class="chart-block">
  {#if row?.data?.title}
    <figcaption class="title">{row.data.title}</figcaption>
  {/if}
  <div class="plot" bind:this={el}>
    {#if failed || !chartId}
      <span class="status err">{$t('agent.chart.error')}</span>
    {:else if !row}
      <span class="status">{$t('agent.chart.loading')}</span>
    {/if}
  </div>
  {#if row && row.points_raw > row.points_shown}
    <div class="foot">{$t('agent.chart.reduced', { raw: fmt(row.points_raw), shown: fmt(row.points_shown) })}</div>
  {/if}
</figure>

<style>
  .chart-block {
    margin: 0;
    width: 100%;
    max-width: 760px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-2) var(--space-3);
  }
  .title {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text);
    margin-bottom: var(--space-1);
  }
  .plot {
    position: relative;
    width: 100%;
    height: 260px;
  }
  .status {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .status.err {
    color: var(--red);
  }
  .foot {
    font-size: var(--text-xs);
    color: var(--muted);
    text-align: right;
  }
</style>
