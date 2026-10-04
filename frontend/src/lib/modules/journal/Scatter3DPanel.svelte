<script>
  // Trade scatter in three dimensions: one dot per closed trade, all three axes chosen by
  // the reader, the box turned by hand. It answers what the flat scatter cannot: whether
  // two variables only matter together (long holds pay, but only in the US session).
  //
  // Same axis catalogue as the flat scatter (`scatter.js`), plus the market session of the
  // entry, read against the sessions defined in Settings. The backtest reuses it over its
  // own trades, with the axes and colourings a simulated trade can fill.
  import { onMount, untrack } from 'svelte';
  import {
    METRICS,
    scatterCtx,
    sessionMetric,
    COLOR_MODES,
    colorKey,
    groupRows
  } from './scatter.js';
  import { fmtMoney, fmtPct, fmtNum, ASSET_CLASSES } from './api.js';
  import { settingsApi } from '$lib/settings/api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  // ECharts and its WebGL extension load when the tab opens, never with the journal page.
  // The extension registers `grid3D` / `scatter3D` on the ECharts instance it imports, so
  // both come from the same module graph. `$state.raw`: a module namespace is never proxied.
  let echarts = $state.raw(null);
  let glError = $state(false);
  // `null` until loaded, and on a failed load: the session axis then reads "not recorded".
  let sessions = $state.raw(null);

  onMount(async () => {
    settingsApi
      .marketSessions()
      .then((r) => {
        sessions = r.sessions ?? [];
      })
      .catch(() => {
        sessions = null;
      });
    try {
      const lib = await import('echarts');
      await import('echarts-gl');
      echarts = lib;
    } catch {
      glError = true;
    }
  });

  let {
    points = [],
    currency = 'USD',
    // Subset of the axis ids offered (null = all of them) and of the colourings.
    metricIds = null,
    colorModes = COLOR_MODES,
    storageKey = 'otw.journal.scatter3d.v1'
  } = $props();

  const catalogue = $derived([...METRICS, sessionMetric(sessions)]);
  const pick = (id) => catalogue.find((m) => m.id === id) ?? catalogue[0];

  // ── Reader's choices, kept across visits ──
  // Fixed for the component's life: a caller does not swap its storage key or catalogue.
  const KEY = untrack(() => storageKey);
  const saved = (() => {
    try {
      return JSON.parse(localStorage.getItem(KEY)) ?? {};
    } catch {
      return {};
    }
  })();
  const IDS = untrack(() => metricIds) ?? [...METRICS.map((m) => m.id), 'session'];
  const MODES = untrack(() => colorModes);
  const known = (id, fallback) => (IDS.includes(id) ? id : fallback);

  let xId = $state(known(saved.x, 'hold'));
  let yId = $state(known(saved.y, 'session'));
  let zId = $state(known(saved.z, 'net'));
  let colorMode = $state(MODES.includes(saved.color) ? saved.color : 'result');

  $effect(() => {
    const state = { x: xId, y: yId, z: zId, color: colorMode };
    try {
      localStorage.setItem(KEY, JSON.stringify(state));
    } catch {
      /* private mode: the chart still works, it just forgets */
    }
  });

  // ── Data ──
  const ctx = $derived(scatterCtx(points));
  const xm = $derived(pick(xId));
  const ym = $derived(pick(yId));
  const zm = $derived(pick(zId));

  // A trade missing any of the three axes is dropped and counted, never coerced to zero.
  const plotted = $derived.by(() => {
    const rows = [];
    let skipped = 0;
    points.forEach((p, i) => {
      const x = xm.of(p, i, ctx);
      const y = ym.of(p, i, ctx);
      const z = zm.of(p, i, ctx);
      if ([x, y, z].some((v) => v == null || !Number.isFinite(v))) {
        skipped += 1;
        return;
      }
      rows.push({ x, y, z, p, key: colorKey(p, colorMode) });
    });
    return { rows, skipped };
  });

  const groups = $derived(groupRows(plotted.rows));

  // ── Formatting ──
  const WEEKDAYS = Array.from({ length: 7 }, (_, i) =>
    // 2024-01-01 was a Monday, so +i walks Monday → Sunday.
    new Date(2024, 0, 1 + i).toLocaleDateString(undefined, { weekday: 'short' })
  );

  function dur(min) {
    const m = Math.round(min);
    if (Math.abs(m) < 60) return $t('journal.analytics.scatter.durMin', { n: m });
    if (Math.abs(m) < 1440) return $t('journal.analytics.scatter.durHour', { n: +(m / 60).toFixed(1) });
    return $t('journal.analytics.scatter.durDay', { n: +(m / 1440).toFixed(1) });
  }

  function sessionName(v) {
    const i = Math.round(v);
    if (!sessions || i < 0 || i > sessions.length) return '';
    return i === sessions.length ? $t('journal.analytics.scatter3d.offSession') : sessions[i].name;
  }

  /** Full precision, for the tooltip. */
  function fmtVal(kind, v) {
    switch (kind) {
      case 'money':
        return fmtMoney(v, currency);
      case 'percent':
        return fmtPct(v);
      case 'r':
        return `${fmtNum(v)}R`;
      case 'duration':
        return dur(v);
      case 'time':
        return new Date(v).toLocaleString();
      case 'hour':
        return `${Math.round(v)}h`;
      case 'weekday':
        return WEEKDAYS[((Math.round(v) % 7) + 7) % 7] ?? '';
      case 'session':
        return sessionName(v);
      case 'count':
        return String(Math.round(v));
      default:
        return fmtNum(v);
    }
  }

  /** Short enough for a tick. */
  function axisVal(kind, v) {
    if (kind === 'money') {
      const a = Math.abs(v);
      if (a >= 1e6) return `${fmtMoney(v / 1e6, currency, 1)}M`;
      if (a >= 1e4) return `${fmtMoney(v / 1e3, currency, 0)}k`;
      return fmtMoney(v, currency, a < 10 && a > 0 ? 2 : 0);
    }
    if (kind === 'percent') return `${+v.toFixed(1)}%`;
    if (kind === 'num') return String(+v.toFixed(2));
    if (kind === 'time') return new Date(v).toLocaleDateString();
    return fmtVal(kind, v);
  }

  const metricLabel = (id) => $t(`journal.analytics.metric.${id}`);
  const METRIC_OPTIONS = $derived(IDS.map((id) => ({ value: id, label: metricLabel(id) })));
  const COLOR_OPTIONS = $derived(
    MODES.map((m) => ({ value: m, label: $t(`journal.analytics.scatter.by.${m}`) }))
  );

  function groupLabel(key) {
    if (key === '__other') return $t('journal.analytics.scatter.other');
    if (key === '') return $t('journal.analytics.group.unassigned');
    if (colorMode === 'result') return $t(`journal.analytics.scatter.${key}`);
    if (colorMode === 'side') return $t(`journal.side.${key}`);
    if (colorMode === 'assetClass') return ASSET_CLASSES.find((a) => a.id === key)?.label ?? key;
    if (colorMode === 'regime') return $t(`journal.market.regime.${key}`);
    if (colorMode === 'trend') return $t(`journal.market.trend.${key}`);
    return key;
  }

  // ── Chart ──
  let el = $state(null);
  // Write-only in the init effect, read by the paint effect (see ScatterPanel).
  let chart = $state.raw(null);
  const colors = $derived(chartColors());

  // The camera the reader left the box at. A filter or an axis change repaints the chart;
  // it must not spin the view back to its default.
  const DEFAULT_VIEW = { alpha: 20, beta: 40, distance: 220, center: [0, 0, 0] };

  function axis3D(m, pal) {
    const { dim, mono } = pal;
    const base = {
      type: m.kind === 'time' ? 'time' : 'value',
      name: metricLabel(m.id),
      nameTextStyle: { color: dim, fontFamily: mono, fontSize: 11 },
      axisLabel: {
        textStyle: { color: dim, fontFamily: mono, fontSize: 10 },
        formatter: (v) => axisVal(m.kind, v)
      },
      scale: m.kind !== 'time'
    };
    if (m.kind === 'weekday') Object.assign(base, { min: 0, max: 6, interval: 1, scale: false });
    if (m.kind === 'hour') Object.assign(base, { min: 0, max: 23, interval: 3, scale: false });
    if (m.kind === 'session') {
      Object.assign(base, { min: 0, max: sessions?.length ?? 0, interval: 1, scale: false });
    }
    return base;
  }

  function build(pal, view) {
    const { series: ramp, muted, dim, border, gridLine, surface, text, mono, green, red, accent } =
      pal;
    const dense = plotted.rows.length > 600;

    const seriesColor = (key, i) => {
      if (colorMode !== 'result') return key === '__other' ? muted : ramp[i % ramp.length];
      return key === 'win' ? green : key === 'loss' ? red : muted;
    };

    const cloud = groups.map((g, i) => ({
      name: groupLabel(g.key),
      type: 'scatter3D',
      symbolSize: dense ? 5 : 8,
      itemStyle: { color: seriesColor(g.key, i), opacity: dense ? 0.7 : 0.9 },
      emphasis: { itemStyle: { color: accent }, label: { show: false } },
      data: g.items.map((r) => ({ value: [r.x, r.y, r.z], row: r }))
    }));

    return {
      animation: false,
      legend:
        groups.length > 1
          ? {
              top: 0,
              right: 0,
              icon: 'circle',
              itemWidth: 8,
              itemHeight: 8,
              textStyle: { color: dim, fontFamily: mono, fontSize: 10 },
              inactiveColor: border,
              data: cloud.map((s) => s.name)
            }
          : { show: false },
      tooltip: {
        trigger: 'item',
        backgroundColor: surface,
        borderColor: border,
        textStyle: { color: text, fontFamily: mono, fontSize: 11 },
        formatter: (p) => {
          const row = p.data?.row;
          if (!row) return '';
          const date = new Date(row.p.at).toLocaleDateString();
          const head = row.p.ticker ? `${row.p.ticker} · ${date}` : date;
          const lines = [
            `${metricLabel(xm.id)}: ${fmtVal(xm.kind, row.x)}`,
            `${metricLabel(ym.id)}: ${fmtVal(ym.kind, row.y)}`,
            `${metricLabel(zm.id)}: ${fmtVal(zm.kind, row.z)}`
          ];
          if (![xm.id, ym.id, zm.id].includes('net')) {
            lines.push(`${metricLabel('net')}: ${fmtMoney(row.p.net, currency)}`);
          }
          return `<b>${head}</b><br>${lines.join('<br>')}`;
        }
      },
      grid3D: {
        boxWidth: 100,
        boxDepth: 100,
        boxHeight: 80,
        environment: 'none',
        axisLine: { lineStyle: { color: border } },
        axisTick: { show: false },
        splitLine: { lineStyle: { color: gridLine, width: 1 } },
        splitArea: { show: false },
        axisPointer: { lineStyle: { color: accent }, label: { show: false } },
        viewControl: {
          projection: 'perspective',
          autoRotate: false,
          rotateSensitivity: 1,
          zoomSensitivity: 1,
          panMouseButton: 'right',
          rotateMouseButton: 'left',
          minDistance: 80,
          maxDistance: 400,
          ...view
        }
      },
      xAxis3D: axis3D(xm, pal),
      yAxis3D: axis3D(ym, pal),
      zAxis3D: axis3D(zm, pal),
      series: cloud
    };
  }

  function currentView(c) {
    const vc = c.getOption()?.grid3D?.[0]?.viewControl;
    if (!vc) return DEFAULT_VIEW;
    const { alpha, beta, distance, center } = vc;
    return { alpha, beta, distance, center };
  }

  $effect(() => {
    const node = el;
    const lib = echarts;
    if (!node || !lib) return;
    let c;
    try {
      c = lib.init(node, null, { renderer: 'canvas' });
    } catch {
      glError = true;
      return;
    }
    chart = c;
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(node);
    return () => {
      ro.disconnect();
      c.dispose();
      chart = null;
    };
  });

  $effect(() => {
    // Read every input so a theme flip, an axis change or a filter repaints the box.
    const c = chart;
    if (!c) return;
    c.setOption(build(colors, currentView(c)), true);
  });

  function resetView() {
    chart?.setOption(build(colors, DEFAULT_VIEW), true);
  }
</script>

<section class="card">
  <div class="controls">
    <div class="axes">
      <Select label={$t('journal.analytics.scatter.x')} options={METRIC_OPTIONS} bind:value={xId} />
      <Select label={$t('journal.analytics.scatter.y')} options={METRIC_OPTIONS} bind:value={yId} />
      <Select label={$t('journal.analytics.scatter3d.z')} options={METRIC_OPTIONS} bind:value={zId} />
      <Select
        label={$t('journal.analytics.scatter.color')}
        options={COLOR_OPTIONS}
        bind:value={colorMode}
      />
    </div>
    <div class="chips">
      <button type="button" class="chip" onclick={resetView}>
        {$t('journal.analytics.scatter3d.reset')}
      </button>
    </div>
  </div>

  {#if glError}
    <EmptyState
      compact
      icon="alert-triangle"
      title={$t('journal.analytics.scatter3d.noGl')}
      description={$t('journal.analytics.scatter3d.noGlHint')}
    />
  {:else if plotted.rows.length === 0}
    <EmptyState
      compact
      icon="bar-chart"
      title={$t('journal.analytics.scatter.noData')}
      description={$t('journal.analytics.scatter3d.noDataHint')}
    />
  {:else if !echarts}
    <Skeleton height="520px" />
  {:else}
    <div class="chart" bind:this={el}></div>
    <p class="caption">
      <span>{$t('journal.analytics.scatter.count', { count: plotted.rows.length })}</span>
      {#if plotted.skipped > 0}
        <span class="skipped">
          {$t('journal.analytics.scatter3d.skipped', { count: plotted.skipped })}
        </span>
      {/if}
      <span class="hint">{$t('journal.analytics.scatter3d.hint')}</span>
    </p>
  {/if}
</section>

<style>
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    padding: var(--space-4);
  }
  .controls {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
    margin-bottom: var(--space-4);
  }
  .axes {
    display: grid;
    grid-template-columns: repeat(4, minmax(140px, 1fr));
    align-items: end;
    gap: var(--space-2);
    flex: 1 1 560px;
  }
  .chips {
    display: flex;
    gap: var(--space-1);
  }
  /* Same frame as the flat scatter's chips (ScatterPanel), one control row, one baseline. */
  .chip {
    height: var(--control-h);
    padding: 0 var(--space-3);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    color: var(--text);
    font-size: 11.5px;
    letter-spacing: 0.04em;
    line-height: 1;
    cursor: pointer;
    white-space: nowrap;
  }
  .chip:hover {
    border-color: var(--accent);
  }
  .chart {
    width: 100%;
    height: 520px;
  }
  .caption {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
    font-size: 11px;
    color: var(--dim);
    font-family: var(--mono);
    padding-top: var(--space-2);
  }
  .skipped {
    color: var(--faint);
  }
  .hint {
    margin-left: auto;
    color: var(--faint);
  }
  @media (max-width: 720px) {
    .axes {
      grid-template-columns: 1fr;
    }
  }
</style>
