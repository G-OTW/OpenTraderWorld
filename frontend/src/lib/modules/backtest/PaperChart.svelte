<script>
  // The same chart, indicators and fill markers as the backtest result. This adapter only
  // supplies the paper book and its dataset's candles, on the strategy's own timeframe.
  import { onMount, untrack } from 'svelte';
  import Chart from '$lib/modules/histviz/Chart.svelte';
  import IndicatorModal from '$lib/modules/histviz/IndicatorModal.svelte';
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { mergeLiveBar } from '$lib/modules/histviz/bars.js';
  import { DEFAULT_SETTINGS, normalizeSettings } from '$lib/modules/histviz/settings.js';
  import Button from '$lib/ui/Button.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { strategyInstances, tradeMarks } from './chart.js';
  import { paperChartTrades } from './paper-dashboard.js';
  import { paperApi } from './paper.js';
  import { t } from '$lib/i18n';

  let { session, instruments = [], book, assetId = $bindable('') } = $props();
  const label = (key) => $t(`backtest.paper.dashboard.${key}`);
  const instrument = $derived(instruments.find((i) => i.id === assetId) ?? instruments[0]);
  const options = $derived(instruments.map((i) => ({ value: i.id, label: `${i.ticker} · ${i.timeframe} · ${i.provider}` })));
  const marks = $derived(tradeMarks(paperChartTrades(book, instrument?.ticker)));
  const strategyKey = $derived(JSON.stringify(session.settings ?? {}));
  const sourceKey = $derived(JSON.stringify(instrument ? {
    id: instrument.id, provider: instrument.provider, asset_type: instrument.asset_type,
    ticker: instrument.ticker, timeframe: instrument.timeframe, connector_id: instrument.connector_id,
    stream: instrument.chart_stream, window: session.window_bars, from: session.start_ts
  } : null));
  let bars = $state(null);
  let loading = $state(false);
  let error = $state('');
  let preferencesError = $state(false);
  let streamStatus = $state(null);
  let settings = $state({ ...DEFAULT_SETTINGS });
  let instances = $state([]);
  let editing = $state(null);
  let indicatorOpen = $state(false);
  let epoch = $state(0);
  let chartRef;

  $effect(() => {
    const frozen = JSON.parse(strategyKey);
    const library = Object.entries(frozen.indicators ?? {}).map(([id, definition]) => ({ id, name: id, definition }));
    instances = strategyInstances(frozen, library);
  });
  onMount(() => {
    let alive = true;
    histvizApi.chartSettings().then((saved) => { if (alive) settings = normalizeSettings(saved); })
      .catch(() => { if (alive) preferencesError = true; });
    return () => { alive = false; };
  });

  $effect(() => {
    epoch;
    const source = JSON.parse(sourceKey);
    if (!source) return;
    let alive = true;
    let stream;
    let pendingBar;
    let flushTimer;
    let request;
    let fetching = false;
    let needsCatchup = false;
    const limit = Math.min(50000, Math.max(1, source.window ?? 5000));
    untrack(() => { bars = null; error = ''; streamStatus = null; loading = true; });

    function receive(event) {
      try {
        const payload = JSON.parse(event.data);
        if (event.type === 'status') {
          streamStatus = payload;
          if (payload.state === 'stopped') { stream.close(); stream = null; }
          return;
        }
        if (!Number.isFinite(payload.c)) return;
        if (payload.closed) {
          pendingBar = null;
          bars = mergeLiveBar(bars, payload);
        } else {
          pendingBar = payload;
          // Match the visualization pane's forming-candle cadence.
          if (!flushTimer) flushTimer = setTimeout(() => {
            flushTimer = null;
            if (alive && pendingBar) { bars = mergeLiveBar(bars, pendingBar); pendingBar = null; }
          }, 400);
        }
      } catch { /* Ignore malformed frames; the next snapshot restores the stream. */ }
    }
    function connect() {
      if (!alive || !source.stream || stream || document.hidden) return;
      streamStatus = { state: 'connecting' };
      stream = new EventSource(paperApi.streamsUrl([{ ...source, since: bars?.ts?.at(-1) }]));
      for (const kind of ['snapshot', 'bar', 'status']) stream.addEventListener(kind, receive);
      stream.onerror = () => { if (alive) streamStatus = { state: 'retrying' }; };
    }
    async function load() {
      if (fetching) return;
      fetching = true;
      request = new AbortController();
      try {
        const next = await histvizApi.bars(source.id, { limit, from: source.from, signal: request.signal });
        if (!alive) return;
        bars = next;
        error = '';
        if (!document.hidden) { needsCatchup = false; connect(); }
      } catch (e) {
        if (alive && e.name !== 'AbortError') error = e.message;
      } finally {
        fetching = false;
        if (alive) loading = false;
      }
    }
    const visibility = () => {
      if (document.hidden) {
        stream?.close(); stream = null;
        clearTimeout(flushTimer); flushTimer = null; pendingBar = null;
        needsCatchup = true;
      } else load();
    };
    load();
    // Unsupported/refused live feeds still follow newly stored candles. After a long
    // background interval, reload before subscribing so missing candles are not skipped.
    const timer = setInterval(() => {
      if (!document.hidden && (needsCatchup || !stream || streamStatus?.state !== 'live')) load();
    }, 15000);
    document.addEventListener('visibilitychange', visibility);
    return () => {
      alive = false;
      request?.abort();
      stream?.close();
      clearTimeout(flushTimer);
      clearInterval(timer);
      document.removeEventListener('visibilitychange', visibility);
    };
  });

  function saveIndicator(next) {
    instances = instances.map((i) => i.id === editing?.id ? { ...i, ...next } : i);
    indicatorOpen = false;
  }
</script>

<section class="paper-chart" data-paper-chart>
  <div class="toolbar">
    <div class="picker"><Select label={label('instrument')} value={instrument?.id ?? ''} {options} onpick={(id) => (assetId = id)} /></div>
    <div class="context"><span>{label('chartHint')}</span><small class:live={streamStatus?.state === 'live'}>{label(!instrument?.chart_stream ? 'chartStored' : streamStatus?.state === 'live' ? 'chartLive' : !streamStatus || streamStatus.state === 'connecting' ? 'chartConnecting' : 'chartError')}</small></div>
    <Button size="sm" icon="refresh-cw" loading={loading} onclick={() => epoch++}>{label('chartRetry')}</Button>
  </div>
  {#if error}<div class="error" role="alert">{error}</div>{/if}
  {#if streamStatus?.message}<div class="notice">{streamStatus.message}</div>{/if}
  {#if preferencesError}<div class="notice">{label('chartSettingsError')}</div>{/if}
  <div class="canvas">
    {#if bars?.ts?.length}
      <Chart bind:this={chartRef} {bars} {marks} {instances} {settings} markSizes={false}
        title={{ ticker: instrument.ticker, timeframe: instrument.timeframe, provider: instrument.provider }}
        ontoggle={(id) => { instances = instances.map((i) => i.id === id ? { ...i, visible: i.visible === false } : i); }}
        onremove={(id) => { instances = instances.filter((i) => i.id !== id); }}
        onedit={(id) => { editing = instances.find((i) => i.id === id); indicatorOpen = true; }} />
    {:else}
      <div class="empty" aria-live="polite"><Icon name="candlestick" size={28} /><span>{label(loading ? 'chartLoading' : 'chartEmpty')}</span></div>
    {/if}
  </div>
</section>
<IndicatorModal bind:open={indicatorOpen} edit={editing} onsave={saveIndicator} />

<style>
  .paper-chart { display: flex; flex-direction: column; min-height: 500px; height: max(500px, calc(100dvh - 350px)); min-width: 0; border: 1px solid var(--border-control); border-radius: var(--radius-lg); background: var(--surface); overflow: hidden; }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 14px; padding: 12px 16px; border-bottom: 1px solid var(--border); }
  .picker { min-width: 220px; max-width: 100%; }.context { flex: 1; color: var(--muted); font-size: var(--text-xs); line-height: 1.7; }.context small { display: block; }.live { color: var(--green); }
  .canvas { flex: 1; min-height: 0; position: relative; }.empty { height: 100%; display: flex; align-items: center; justify-content: center; flex-direction: column; gap: 14px; color: var(--muted); font-size: var(--text-sm); }
  .error, .notice { padding: 8px 16px; font-size: var(--text-xs); }.error { color: var(--red); }.notice { color: var(--amber); }
  @media (max-width: 560px) { .toolbar { gap: 10px; }.picker { width: 100%; }.paper-chart { height: 650px; } }
</style>
