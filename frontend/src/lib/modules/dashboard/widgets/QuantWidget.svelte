<script>
  // Analysis reads the historical-data catalog, never a market-data provider.
  import { quantApi } from '$lib/modules/quant/api.js';
  import { fmtPct, fmtFixed } from '$lib/format';
  import { t, locale } from '$lib/i18n';
  import { livePulse } from '../live.svelte.js';
  import { compatibleDataset, correlationData, correlationPairs, minimumBars, seasonValue } from './insights.js';
  import WidgetState from './WidgetState.svelte';
  import Buckets from './parts/Buckets.svelte';
  import Matrix from './parts/Matrix.svelte';
  import TrendChart from './parts/TrendChart.svelte';
  import Stat from './parts/Stat.svelte';

  let { item, editing, onconfigure = null } = $props();
  const variant = $derived(item.config?.variant ?? 'count');
  let data = $state.raw(null);
  let err = $state('');
  let empty = $state('');
  let busy = $state(false);
  let asked = '';
  const live = livePulse(300);

  const day = (value) => value ? new Date(value).toLocaleDateString($locale, { year: 'numeric', month: 'short', day: 'numeric', timeZone: 'UTC' }) : '—';
  const pc = (v) => Number.isFinite(v) ? fmtPct(v * 100, 2) : '—';
  const asPoints = (rows) => rows.map((r) => ({ at: r.ts, value: r.dd * 100 }));
  const href = (ds, tab = 'single') => `/quant?tab=${tab}${ds ? `&dataset=${encodeURIComponent(ds.id)}` : ''}`;
  const matrix = $derived(correlationData(data?.basket));
  const season = $derived(data?.season?.result);
  const months = $derived(Array.from({ length: 12 }, (_, i) => new Date(2024, i, 1).toLocaleDateString($locale, { month: 'short' })));
  const weekdays = $derived(Array.from({ length: 7 }, (_, i) => new Date(2024, 0, 1 + i).toLocaleDateString($locale, { weekday: 'short' })));
  const seasonMax = $derived(Math.max(0.0001, ...(season?.month_weekday ?? []).flat().filter(Number.isFinite).map(Math.abs)));
  const regime = $derived(data?.regime?.result);
  const stateIndex = $derived(regime?.current?.reduce((best, v, i, a) => v > a[best] ? i : best, 0) ?? 0);
  const vol = $derived(data?.vol?.result);
  const cone = $derived(vol?.cones?.find((c) => c.window === vol.window));
  const confidence = $derived(data?.single?.result?.confidence ?? 0.95);

  $effect(() => {
    const cfg = { ...item.config };
    const v = variant;
    const signature = JSON.stringify([v, cfg.datasetId, cfg.datasetIds, cfg.metric, cfg.confidence, cfg.window, cfg.states]);
    live.n;
    if (editing) return;
    let alive = true;
    if (signature !== asked) { asked = signature; data = null; empty = ''; }
    err = '';
    busy = true;
    async function load() {
      const datasets = await quantApi.datasets();
      if (v === 'count') return { datasets, runs: await quantApi.backtestRuns() };
      if (v === 'correlation') {
        const ids = cfg.datasetIds ?? [];
        if (ids.length < 2) return { empty: $t('dashboard.widgets.quant.pickBasket') };
        const selected = ids.map((id) => datasets.find((d) => d.id === id));
        if (selected.some((d) => !d)) return { empty: $t('dashboard.widgets.insights.datasetMissing') };
        if (selected.some((d) => !compatibleDataset(selected[0], d))) throw new Error($t('dashboard.widgets.insights.compatibleBasket'));
        return { basket: await quantApi.portfolio(ids, { samples: 1000 }), datasets: selected };
      }
      const required = minimumBars(v, cfg.states ?? 2);
      const ds = cfg.datasetId ? datasets.find((d) => d.id === cfg.datasetId) : datasets.find((d) => d.bar_count >= required);
      if (!ds) return { empty: $t(cfg.datasetId ? 'dashboard.widgets.insights.datasetMissing' : 'dashboard.widgets.quant.pickDataset') };
      if (ds.bar_count < required) return { empty: $t('dashboard.widgets.insights.minimumHistory', { n: required }) };
      if (v === 'risk') return { ds, single: await quantApi.single(ds.id, cfg.confidence ?? 0.95) };
      if (v === 'seasonality') return { ds, season: await quantApi.seasonality(ds.id, { metric: cfg.metric ?? 'return' }) };
      if (v === 'volatility') return { ds, vol: await quantApi.volatility(ds.id, { window: cfg.window ?? 21 }) };
      if (v === 'regime') return { ds, regime: await quantApi.regimes(ds.id, { states: cfg.states ?? 2 }) };
      return { empty: $t('dashboard.widgets.quant.pickDataset') };
    }
    load().then((out) => {
      if (alive) { data = out; empty = out.empty ?? ''; }
    }).catch((e) => { if (alive) err = e.message; }).finally(() => { if (alive) busy = false; });
    return () => { alive = false; };
  });
</script>

<WidgetState {editing} error={data === null ? err : ''} loading={data === null} empty={!!empty}
  preview={$t('dashboard.widgets.quant.preview')} emptyText={empty}
  emptyAction={$t('dashboard.widgets.insights.chooseData')} onemptyaction={onconfigure} rows={4}>
  <div class="insight w-body" aria-busy={busy}>
    {#if variant === 'count'}
      <Buckets align="left" cells={[
        { value: data.datasets.length, label: $t('dashboard.widgets.quant.datasets'), href: '/histdata' },
        { value: data.runs.length, label: $t('dashboard.widgets.quant.backtestRuns'), href: '/backtest' }
      ]} />
    {:else if variant === 'risk'}
      {@const r = data.single.result}
      <a class="context identity" href={href(data.ds)}>{data.single.ticker} · {data.single.timeframe} · {data.ds.provider}</a>
      <Buckets cells={[
        { value: pc(r.hv_annual), label: $t('dashboard.widgets.insights.annualVol'), tip: $t('dashboard.widgets.insights.annualization', { n: fmtFixed(r.periods_per_year, 1) }) },
        { value: pc(-Math.abs(r.max_drawdown)), label: $t('dashboard.widgets.quant.maxDd'), tone: 'neg', sub: $t('dashboard.widgets.insights.fullHistory') },
        { value: pc(-Math.abs(r.var_hist)), label: $t('dashboard.widgets.insights.historicalVar'), tone: 'neg', sub: `${fmtPct(confidence * 100, 0)} · ${data.single.timeframe}`, tip: $t('dashboard.widgets.insights.varHint') }
      ]} />
      {#if r.drawdown_curve?.length > 1}
        <div class="w-fill chart"><TrendChart points={asPoints(r.drawdown_curve)} format={(v) => fmtPct(v, 1)} tone="neg" showValue={false} showPill={false} ranges={['1y', 'all']} label={$t('dashboard.widgets.quant.maxDd')} /></div>
      {/if}
      <p class="context">{day(r.drawdown_curve?.[0]?.ts)} – {day(r.drawdown_curve?.at(-1)?.ts)} · {$t('quant.seasonality.samples', { n: r.periods })}</p>
    {:else if variant === 'correlation'}
      {@const b = data.basket}
      <p class="context">{day(b.alignment?.from)} – {day(b.alignment?.to)} · {b.measured_at} · {$t('quant.seasonality.samples', { n: Math.max(0, b.periods - 1) })}</p>
      <Matrix rows={matrix.labels} columns={matrix.labels} values={matrix.matrix} format={(v) => fmtFixed(v, 2)} mode="correlation" label={$t('quant.page.correlationMatrix')}
        compact={correlationPairs(matrix.labels, matrix.matrix)} compactLabel={$t('dashboard.widgets.insights.strongestPairs')} />
      {#if b.resampled}<p class="context">{$t('dashboard.widgets.insights.resampled', { from: b.resampled.from, to: b.resampled.to })}</p>{/if}
      <a class="context" href={`/quant?tab=portfolio&datasets=${encodeURIComponent(data.datasets.map((d) => d.id).join(','))}`}>{$t('dashboard.widgets.insights.openAnalysis')}</a>
    {:else if variant === 'seasonality'}
      <a class="context identity" href={href(data.ds, 'seasonality')}>{data.season.ticker} · {data.season.timeframe} · {$t(`dashboard.widgets.insights.season.${season.metric}`)}</a>
      <Matrix rows={months} columns={weekdays} values={season.month_weekday} counts={season.month_weekday_counts ?? []}
        format={(v) => seasonValue(v, season.metric, fmtFixed, fmtPct)} mode={season.metric} max={seasonMax} label={$t('dashboard.widgets.insights.seasonTitle')}
        compact={season.month.map((b) => ({ label: months[b.key], value: b.count ? b.value : null, count: b.count }))} compactLabel={$t('quant.seasonality.byMonth')} />
      <p class="context">{day(data.season.from ?? data.ds.range_from)} – {day(data.season.until ?? data.ds.range_to)} · {$t('quant.seasonality.samples', { n: season.periods })} · UTC</p>
      <p class="context">{$t('dashboard.widgets.insights.historicalPattern')}</p>
    {:else if variant === 'volatility'}
      <a class="context identity" href={href(data.ds, 'volatility')}>{data.vol.ticker} · {data.vol.timeframe} · {$t('quant.vol.rollingTitle', { n: vol.window })}</a>
      <Stat value={pc(cone?.current ?? vol.rolling?.at(-1)?.cc)} note={$t('dashboard.widgets.insights.annualVol')} />
      {#if cone}
        <Buckets cells={[
          { value: pc(cone.p10), label: 'P10' },
          { value: pc(cone.median), label: $t('quant.vol.median') },
          { value: pc(cone.p90), label: 'P90' }
        ]} />
        <p class="context">{$t('dashboard.widgets.insights.percentile', { n: fmtFixed(cone.current_rank * 100, 0), samples: cone.samples })}</p>
      {/if}
      {#if vol.rolling?.length > 1}<div class="w-fill chart"><TrendChart points={vol.rolling.filter((r) => r.cc != null).map((r) => ({ at: r.ts, value: r.cc * 100 }))} format={(v) => fmtPct(v, 1)} tone="accent" showValue={false} showPill={false} ranges={['1y', 'all']} label={$t('quant.vol.closeToClose')} /></div>{/if}
      <p class="context">{day(data.vol.from ?? data.ds.range_from)} – {day(data.vol.until ?? data.ds.range_to)} · {$t('dashboard.widgets.insights.bars', { n: vol.bars })}</p>
    {:else if variant === 'regime'}
      <a class="context identity" href={href(data.ds, 'regimes')}>{data.regime.ticker} · {data.regime.timeframe} · HMM</a>
      <Stat value={regime.k === 2 ? $t(stateIndex === 0 ? 'quant.regimes.calm' : 'quant.regimes.turbulent') : $t('quant.regimes.stateN', { n: stateIndex + 1 })}
        note={$t('dashboard.widgets.insights.stateProbability', { n: fmtPct(regime.current[stateIndex] * 100, 1) })} />
      <Buckets cells={[
        { value: pc(regime.states[stateIndex].vol_annual), label: $t('quant.regimes.volAnnual') },
        { value: fmtFixed(regime.states[stateIndex].expected_duration, 1), label: $t('quant.regimes.duration'), sub: data.regime.timeframe }
      ]} />
      <p class="context">{day(data.regime.from ?? data.ds.range_from)} – {day(data.regime.until ?? data.ds.range_to)} · {$t('dashboard.widgets.insights.bars', { n: data.regime.bars ?? data.ds.bar_count })}</p>
      {#if !regime.converged}<p class="context w-warn">{$t('dashboard.widgets.insights.notConverged')}</p>{/if}
      <p class="context">{$t('dashboard.widgets.insights.regimeEstimate')}</p>
    {/if}
    {#if data.ds}<p class="context">{$t('dashboard.widgets.insights.updated', { date: day(data.ds.last_updated) })}</p>{/if}
    {#if err}<p class="context w-warn" role="status">{$t('dashboard.widgets.insights.refreshFailed', { error: err })}</p>{/if}
  </div>
</WidgetState>

<style>
  .insight { gap: var(--space-2); }
  .context { margin: 0; font-size: 11px; line-height: 1.4; color: var(--muted); overflow-wrap: anywhere; }
  a.context { text-decoration: none; }
  a.context:hover { color: var(--accent); text-decoration: underline; }
  .identity { font-weight: var(--fw-medium); color: var(--text); }
  .chart { min-height: 85px; }
  .context.w-warn { color: var(--amber-ink); }
</style>
