<script>
  import { onMount, untrack } from 'svelte';
  import PaperChart from './PaperChart.svelte';
  import DataGaps from './DataGaps.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import QChart from '$lib/modules/quant/QChart.svelte';
  import TradeStats from '$lib/analysis/TradeStats.svelte';
  import TradeCurve from '$lib/analysis/TradeCurve.svelte';
  import { stats as closedTradeStats } from '$lib/analysis/trades.js';
  import Scatter3DPanel from '$lib/modules/journal/Scatter3DPanel.svelte';
  import StreaksPanel from '$lib/modules/journal/StreaksPanel.svelte';
  import { toAnalysisTrades, toPlotPoints, PLOT_METRICS } from './api.js';
  import { paperApi } from './paper.js';
  import { finite, projectBook, sessionState } from './paper-dashboard.js';
  import { t } from '$lib/i18n';

  let { sessionId, onedit = () => {}, onchanged = () => {} } = $props();
  const prefix = 'backtest.paper.dashboard.';
  const label = (key, params) => $t(prefix + key, params);
  const num = (value, digits = 2) => finite(value) ? value.toLocaleString(undefined, { maximumFractionDigits: digits, minimumFractionDigits: digits }) : '—';
  const signed = (value) => finite(value) ? `${value > 0 ? '+' : ''}${num(value)}` : '—';
  const when = (value) => value && Number.isFinite(Date.parse(value)) ? new Date(value).toLocaleString() : '—';
  const tone = (value) => !finite(value) || value === 0 ? '' : value > 0 ? 'positive' : 'negative';
  let data = $state(null);
  let quotes = $state({});
  let feedStates = $state({});
  let error = $state('');
  let actionError = $state('');
  let busy = $state('');
  let refreshing = $state(false);
  let updated = $state(null);
  let now = $state(Date.now());
  let visible = $state(true);
  let streamEpoch = $state(0);
  let view = $state('overview');
  let chartAssetId = $state('');
  let tab = $state('positions');
  let page = $state(0);
  let controller;
  let alive = true;
  let inFlight = null;
  const session = $derived(data?.session);
  const book = $derived(projectBook(data, quotes));
  const state = $derived(busy === 'run' ? 'running' : sessionState(session));
  const instruments = $derived(data?.instruments ?? []);
  const streamKey = $derived(JSON.stringify(instruments.filter((i) => i.stream).map((i) => ({
    id: i.id, provider: i.provider, asset_type: i.asset_type, ticker: i.ticker,
    timeframe: i.stream_timeframe, connector_id: i.connector_id
  }))));
  // Analytics read the same closed book as the P&L and win rate above, not the engine's
  // last simulation window, so every figure on the page counts the same trades.
  const analysisTrades = $derived(toAnalysisTrades(book.closed));
  const closedStats = $derived(closedTradeStats(analysisTrades));
  const plotPoints = $derived(toPlotPoints(book.closed));
  const signals = $derived((session?.settings?.kind ?? 'signals') === 'signals');
  const pages = $derived(Math.max(1, Math.ceil(book.closed.length / 25)));
  const history = $derived([...book.closed].reverse().slice(Math.min(page, pages - 1) * 25, (Math.min(page, pages - 1) + 1) * 25));

  async function refresh() {
    if (inFlight) return inFlight;
    refreshing = true;
    controller = new AbortController();
    inFlight = (async () => {
      try {
        const next = await paperApi.dashboard(sessionId, controller.signal);
        if (!alive) return;
        data = next;
        updated = new Date().toISOString();
        error = '';
      } catch (e) {
        if (alive && e.name !== 'AbortError') error = e.message;
      } finally {
        if (alive) refreshing = false;
        inFlight = null;
      }
    })();
    return inFlight;
  }

  onMount(() => {
    visible = !document.hidden;
    refresh();
    const timer = setInterval(() => { now = Date.now(); if (!document.hidden && !busy) refresh(); }, 3000);
    const visibility = () => { visible = !document.hidden; if (visible) refresh(); };
    document.addEventListener('visibilitychange', visibility);
    return () => {
      alive = false;
      controller?.abort();
      clearInterval(timer);
      document.removeEventListener('visibilitychange', visibility);
    };
  });

  // One multiplexed stream for the whole book; reconnect only when its instruments change.
  $effect(() => {
    streamEpoch;
    const items = JSON.parse(streamKey);
    if (!visible || !items.length) return;
    untrack(() => { feedStates = Object.fromEntries(items.map((i) => [i.id, { state: 'connecting' }])); });
    // The shared stream endpoint accepts twelve instruments per connection.
    const sources = [];
    for (let offset = 0; offset < items.length; offset += 12) {
      const batch = items.slice(offset, offset + 12);
      const source = new EventSource(paperApi.streamsUrl(batch));
      sources.push(source);
      const stopped = new Set();
      const receive = (event) => {
        try {
          const payload = JSON.parse(event.data);
          const item = batch[payload.i];
          if (!item) return;
          if (event.type === 'status') {
            feedStates = { ...feedStates, [item.id]: payload };
            if (payload.state === 'stopped') stopped.add(item.id);
            if (stopped.size === batch.length) source.close();
          } else if (finite(payload.c)) {
            quotes = { ...quotes, [item.id]: { price: payload.c, at: new Date(Date.now() - Math.max(0, payload.lag_ms ?? 0)).toISOString(), source: 'live' } };
          }
        } catch { /* A malformed stream frame must not break the dashboard. */ }
      };
      for (const kind of ['snapshot', 'bar', 'status']) source.addEventListener(kind, receive);
      source.onerror = () => {
        const retrying = batch.filter((i) => !stopped.has(i.id)).map((i) => [i.id, { state: 'retrying' }]);
        feedStates = { ...feedStates, ...Object.fromEntries(retrying) };
      };
    }
    return () => sources.forEach((source) => source.close());
  });

  function quoteFor(instrument) {
    return quotes[instrument.id] ?? { price: instrument.price, at: instrument.price_at, source: 'stored' };
  }
  export function reload() { return refreshNow(); }

  function refreshNow() {
    streamEpoch += 1;
    return refresh();
  }
  function quoteState(instrument) {
    const quote = quoteFor(instrument);
    if (!finite(quote.price)) return 'unavailable';
    if (quote.source !== 'live') return 'stored';
    if (!visible || now - Date.parse(quote.at) > 90000) return 'stale';
    return feedStates[instrument.id]?.state === 'live' ? 'live' : 'stale';
  }
  async function act(kind) {
    if (busy) return;
    busy = kind;
    actionError = '';
    try {
      await inFlight;
      if (kind === 'run') {
        const result = await paperApi.runNow(sessionId);
        if (result.error) throw new Error(result.error);
      } else {
        await paperApi.setStatus(sessionId, session.status === 'active' ? 'paused' : 'active');
      }
      await refresh();
      onchanged();
    } catch (e) { if (alive) actionError = e.message; }
    finally { if (alive) busy = ''; }
  }
  function chartOptions(c) {
    const points = book.curve;
    return {
      animation: false,
      grid: { left: 66, right: 22, top: 18, bottom: 30 },
      tooltip: { trigger: 'axis', renderMode: 'richText', valueFormatter: (v) => num(v) },
      xAxis: { type: 'time', axisLabel: { color: c.muted, hideOverlap: true }, axisLine: { lineStyle: { color: c.borderControl } }, splitLine: { show: false } },
      yAxis: { type: 'value', axisLabel: { color: c.muted }, splitLine: { lineStyle: { color: c.gridLine, type: 'dashed' } } },
      series: [{ name: label('realized'), type: 'line', step: 'end', showSymbol: points.length < 2,
        symbolSize: 6, lineStyle: { width: 2, color: c.accent }, itemStyle: { color: c.accent },
        data: points.map((p) => [p.ts, p.value]) }]
    };
  }
</script>

<div class="dashboard" data-paper-dashboard>
    <div class="page-bar"><a class="back btn sm" href="/backtest?paper=1"><Icon name="arrow-left" size={14} />{label('back')}</a><span>{label('title')}</span></div>
    {#if error}
      <div class="notice error" role="alert"><Icon name="alert-triangle" /> <span>{label(data ? 'refreshFailed' : 'loadFailed')} {error}</span><Button size="sm" onclick={refreshNow} loading={refreshing}>{label('retry')}</Button></div>
    {/if}
    {#if !data}
      <div class="loading" aria-live="polite"><Icon name="grid" size={28} /><p>{label(error ? 'loadFailed' : 'loading')}</p></div>
    {:else}
      <header class="hero">
        <div class="identity">
          <div class="eyebrow"><Icon name="flask" size={13} /> {label('paperOnly')} <span>·</span> {session.settings?.kind ?? 'signals'}</div>
          <h2>{session.name}</h2>
          <div class="status-line"><span class="status {state}"><span class="dot"></span>{label(`state.${state}`)}</span><span>{label(`stateHint.${state}`)}</span></div>
        </div>
        <div class="controls">
          <div class="buttons">
            <Button size="sm" icon="settings" disabled={!!busy} title={label('settings')} aria-label={label('settings')} onclick={() => onedit(session, instruments.map((i) => i.ticker))} />
            <Button size="sm" icon="refresh-cw" disabled={!!busy || session.running} loading={busy === 'run'} onclick={() => act('run')}>{$t('backtest.paper.runNow')}</Button>
            <Button size="sm" icon={session.status === 'active' ? 'stop' : 'play'} variant={session.status === 'active' ? 'danger' : 'primary'} disabled={!!busy || session.running} loading={busy === 'toggle'} onclick={() => act('toggle')}>{label(session.status === 'active' ? 'stop' : 'start')}</Button>
          </div>
          <small>{label('stopHint')}</small>
        </div>
      </header>
      {#if actionError || session.last_error}<div class="notice error" role="alert"><Icon name="alert-triangle" /><span>{actionError || session.last_error}</span></div>{/if}
      <!-- The session fills its own holes; this only says where that stands. -->
      <DataGaps gaps={session.stats?.data_gaps ?? []} excluded={session.stats?.excluded_trades ?? []} />
      <div class="timing">
        <span><Icon name="clock" size={13} /> {label('next')} <b>{when(session.next_run_at)}</b></span>
        <span>{label('last')} <b>{when(session.last_run_at)}</b></span>
        <span class="sync" title={when(updated)}><span class="dot" class:healthy={!error}></span>{label(error ? 'disconnected' : 'autoRefresh')}</span>
        <button class="icon-button" disabled={refreshing || !!busy} onclick={refreshNow} aria-label={label('refresh')} title={label('refresh')}><Icon name="refresh-cw" size={13} /></button>
      </div>
      <nav class="view-tabs" aria-label={label('title')}>
        <button class:active={view === 'overview'} aria-pressed={view === 'overview'} onclick={() => (view = 'overview')}><Icon name="grid" size={14} />{label('overview')}</button>
        <button class:active={view === 'chart'} aria-pressed={view === 'chart'} onclick={() => (view = 'chart')}><Icon name="candlestick" size={14} />{label('chart')}</button>
        <button class:active={view === 'stats'} aria-pressed={view === 'stats'} onclick={() => (view = 'stats')}>{$t('backtest.result.statistics')}</button>
        <button class:active={view === 'perf'} aria-pressed={view === 'perf'} onclick={() => (view = 'perf')}>{$t('backtest.result.performance')}</button>
        {#if signals}
          <button class:active={view === 'scatter3d'} aria-pressed={view === 'scatter3d'} onclick={() => (view = 'scatter3d')}>{$t('journal.analytics.tab.scatter3d')}</button>
          <button class:active={view === 'streaks3d'} aria-pressed={view === 'streaks3d'} onclick={() => (view = 'streaks3d')}>{$t('journal.analytics.tab.streaks')}</button>
        {/if}
      </nav>
      {#if view === 'chart'}
        <PaperChart {session} {instruments} {book} bind:assetId={chartAssetId} />
      {:else if view !== 'overview' && !analysisTrades.length}
        <div class="card empty"><Icon name="history" size={25} /><b>{label('noHistory')}</b><span>{label('noHistoryHint')}</span></div>
      {:else if view === 'stats'}
        <section class="card analytics"><TradeStats s={closedStats} trades={analysisTrades} /></section>
      {:else if view === 'perf'}
        <section class="card analytics curve-box"><TradeCurve trades={analysisTrades} /></section>
      {:else if view === 'scatter3d'}
        <Scatter3DPanel points={plotPoints} currency="" metricIds={PLOT_METRICS}
          colorModes={instruments.length > 1 ? ['result', 'side', 'ticker'] : ['result', 'side']}
          storageKey="otw.backtest.paper.scatter3d.v1" />
      {:else if view === 'streaks3d'}
        <StreaksPanel points={plotPoints} currency="" units={['money']} storageKey="otw.backtest.paper.streaks3d.v1" />
      {:else}
      <div class="metrics">
        <article class="metric principal"><span>{label('realized')}</span><strong class={tone(book.realized)}>{signed(book.realized)}</strong><small>{label('closedCount', { n: book.closed.length })}</small></article>
        <article class="metric"><span>{label('unrealized')}</span><strong class={tone(book.unrealized)}>{signed(book.unrealized)}</strong><small>{label('grossEstimate')}</small></article>
        <article class="metric"><span>{label('exposure')}</span><strong>{num(book.exposure)}</strong><small>{label('positionCount', { n: book.open.length })}</small></article>
        <article class="metric"><span>{label('winRate')}</span><strong>{num(book.winRate, 1)}{finite(book.winRate) ? '%' : ''}</strong><small>{label('closedOnly')}</small></article>
      </div>
      <div class="overview">
        <section class="card performance">
          <div class="section-head"><div><h3>{label('pnlCurve')}</h3><p>{label('bookScope')}</p></div><span class="count">{label('netFees')}</span></div>
          {#if book.curve.length}<QChart build={chartOptions} height={215} />{:else}<div class="empty chart-empty"><Icon name="trending-up" size={28} /><b>{label('noHistory')}</b><span>{label('noHistoryHint')}</span></div>{/if}
          <div class="chart-footer"><span>{label('fees')} <b>{num(book.fees)}</b></span><span>{label('capital')} <b>{num(session.settings?.starting_capital)}</b></span></div>
        </section>
        <section class="card markets">
          <div class="section-head"><div><h3>{label('prices')}</h3><p>{label('pricesHint')}</p></div><Icon name="radio" size={17} /></div>
          <div class="market-list">
            {#each instruments as instrument (instrument.id)}
              {@const quote = quoteFor(instrument)}
              {@const status = quoteState(instrument)}
              <div class="market">
                <div><b>{instrument.ticker}</b><small>{instrument.provider} · {instrument.timeframe}</small></div>
                <div class="price"><b>{num(quote.price, 4)}</b><small class:positive={status === 'live'} title={feedStates[instrument.id]?.message ?? ''}>{label(`quote.${status}`)}</small></div>
                <time title={when(quote.at)}>{when(quote.at)}</time>
                {#if feedStates[instrument.id]?.message}<small class="feed-error">{feedStates[instrument.id].message}</small>{/if}
              </div>
            {:else}<div class="empty"><span>{label('noPrices')}</span></div>{/each}
          </div>
        </section>
      </div>
      <section class="card book">
        <nav class="tabs" aria-label={label('book')}>
          {#each ['positions', 'orders', 'history', 'activity'] as item}
            <button class:active={tab === item} aria-pressed={tab === item} onclick={() => (tab = item)}>{label(item)}<span class="count">{item === 'positions' ? book.open.length : item === 'orders' ? book.pending.length : item === 'history' ? book.closed.length : data.events?.length ?? 0}</span></button>
          {/each}
        </nav>
        {#if tab === 'positions'}
          {#if book.open.length}
            <!-- Scrollable tables must remain keyboard accessible. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <div class="table-scroll" tabindex="0" role="region" aria-label={label('positions')}><table><thead><tr>{#each ['instrument', 'side', 'quantity', 'entry', 'mark', 'unrealized', 'stopLoss', 'target'] as col}<th>{label(col)}</th>{/each}</tr></thead><tbody>
              {#each book.open as p}<tr><td><b>{p.ticker}</b><small>{when(p.entry_ts)}</small></td><td><span class="side" class:positive={p.direction === 'long'}>{label(p.direction)}</span></td><td>{num(p.qty, 4)}</td><td>{num(p.avg_price, 4)}</td><td title={when(p.quote?.at)}>{num(p.mark, 4)}<small>{label(p.quote?.source === 'live' ? 'latestQuote' : 'quote.stored')}</small></td><td class={tone(p.unrealized)}>{signed(p.unrealized)}</td><td>{num(p.stop, 4)}</td><td>{num(p.take_profit ?? p.exit_limit, 4)}</td></tr>{/each}
            </tbody></table></div>
          {:else}<div class="empty"><Icon name="layers" size={25} /><b>{label('noPositions')}</b><span>{label('noPositionsHint')}</span></div>{/if}
        {:else if tab === 'orders'}
          {#if book.pending.length}
            <!-- Scrollable tables must remain keyboard accessible. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <div class="table-scroll" tabindex="0" role="region" aria-label={label('orders')}><table><thead><tr>{#each ['instrument', 'side', 'orderType', 'quantity', 'amount', 'orderPrice', 'validity', 'signal'] as col}<th>{label(col)}</th>{/each}</tr></thead><tbody>
              {#each book.pending as o}<tr><td><b>{o.ticker}</b><small>{$t(`backtest.paper.orderKind.${o.kind}`)}</small></td><td>{label(o.direction)}</td><td>{label(o.order === 'market' ? 'marketOrder' : 'limitOrder')}</td><td>{num(o.qty, 4)}</td><td>{num(o.amount)}</td><td>{o.order === 'market' ? label('nextPrice') : num(o.price, 4)}{#if o.provisional}<small>{label('provisional')}</small>{/if}</td><td>{label('barsLeft', { n: o.bars_left })}</td><td>{when(o.signal_ts)}</td></tr>{/each}
            </tbody></table></div>
          {:else}<div class="empty"><Icon name="clipboard-list" size={25} /><b>{label('noOrders')}</b><span>{label('noOrdersHint')}</span></div>{/if}
        {:else if tab === 'history'}
          {#if history.length}
            <!-- Scrollable tables must remain keyboard accessible. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <div class="table-scroll" tabindex="0" role="region" aria-label={label('history')}><table><thead><tr>{#each ['instrument', 'side', 'quantity', 'entry', 'exit', 'realized', 'fees', 'closed'] as col}<th>{label(col)}</th>{/each}</tr></thead><tbody>
              {#each history as trade}<tr><td><b>{trade.ticker}</b><small>{trade.exit_reason ?? '—'}</small></td><td>{label(trade.direction)}</td><td>{num(trade.qty, 4)}</td><td>{num(trade.entry_price, 4)}</td><td>{num(trade.exit_price, 4)}</td><td class={tone(trade.pnl)}>{signed(trade.pnl)}</td><td>{num(trade.fees)}</td><td>{when(trade.exit_ts)}</td></tr>{/each}
            </tbody></table></div>
            <div class="pagination"><Button size="sm" disabled={page === 0} onclick={() => page--}>{label('previous')}</Button><span>{label('page', { n: Math.min(page + 1, pages), total: pages })}</span><Button size="sm" disabled={page >= pages - 1} onclick={() => page++}>{label('nextPage')}</Button></div>
          {:else}<div class="empty"><Icon name="history" size={25} /><b>{label('noHistory')}</b><span>{label('noHistoryHint')}</span></div>{/if}
        {:else}
          <div class="activity">
            {#each data.events ?? [] as event (event.id)}<article><span class="event-icon" class:negative={event.kind === 'error'}><Icon name={event.kind === 'error' ? 'alert-triangle' : event.kind === 'close' ? 'check-circle' : event.kind === 'open' ? 'arrow-right' : 'zap'} size={15} /></span><div><b>{event.title}</b><p>{event.message}</p></div><time>{when(event.at)}</time></article>
            {:else}<div class="empty"><Icon name="zap" size={25} /><b>{label('noActivity')}</b></div>{/each}
          </div>
        {/if}
      </section>
      {/if}
      <footer class="footnote"><Icon name="info" size={13} /><span>{label('valuationHint')}</span></footer>
    {/if}
  </div>


<style>
  .dashboard { box-sizing: border-box; width: 100%; height: 100%; overflow-y: auto; padding: 22px 28px; display: flex; flex-direction: column; gap: 18px; color: var(--text); font-size: var(--text-sm); min-width: 0; }
  .page-bar { display: flex; align-items: center; gap: 16px; color: var(--muted); font-size: var(--text-xs); }
  .back { text-decoration: none; flex: none; }
  .view-tabs { display: flex; gap: 22px; overflow-x: auto; border-bottom: 1px solid var(--border); flex: none; }
  .view-tabs button { white-space: nowrap; display: inline-flex; align-items: center; gap: 7px; padding: 0 2px 12px; border: 0; border-bottom: 2px solid transparent; background: none; color: var(--muted); font: inherit; cursor: pointer; }
  .view-tabs button.active { color: var(--accent); border-bottom-color: var(--accent); }
  .dashboard > :global(*) { flex-shrink: 0; }
  .hero { display: flex; justify-content: space-between; gap: 20px; align-items: center; }
  .identity { min-width: 0; }
  .eyebrow { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: var(--text-xs); }
  h2 { font-size: 24px; line-height: 1.3; letter-spacing: -.025em; margin: 8px 0 10px; overflow-wrap: anywhere; }
  h3 { font-size: var(--text-sm); font-weight: 600; margin: 0; }
  .status-line { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; color: var(--muted); font-size: var(--text-xs); }
  .status { display: inline-flex; gap: 6px; align-items: center; padding: 4px 9px; border: 1px solid var(--border-control); border-radius: 20px; color: var(--muted); }
  .status.watching, .status.running { color: var(--green); border-color: color-mix(in srgb, var(--green) 30%, transparent); background: color-mix(in srgb, var(--green) 6%, transparent); }
  .status.pending { color: var(--amber); }.status.error { color: var(--red); }
  .dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: currentColor; flex: none; }
  .healthy { color: var(--green); }
  .controls { display: flex; flex-direction: column; gap: 9px; align-items: flex-end; }
  .buttons { display: flex; gap: 7px; flex-wrap: wrap; }
  small { display: block; color: var(--muted); font-size: var(--text-xs); font-weight: 400; line-height: 1.5; }
  .controls small { max-width: 370px; text-align: right; }
  .timing { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 24px; padding: 11px 0; border-block: 1px solid var(--border); color: var(--muted); font-size: var(--text-xs); }
  .timing > span { display: flex; align-items: center; gap: 7px; }.timing b { color: var(--text); font-weight: 500; }.sync { margin-left: auto; }
  .icon-button { border: 0; background: transparent; color: var(--muted); cursor: pointer; display: inline-flex; padding: 5px; }
  .metrics { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; }
  .metric { border: 1px solid var(--border-control); border-radius: var(--radius-lg); padding: 17px 20px; display: flex; flex-direction: column; gap: 9px; background: var(--surface); }
  .metric.principal { border-top: 2px solid var(--accent); padding-top: 16px; }
  .metric > span { color: var(--muted); font-size: var(--text-xs); }.metric strong { font-size: clamp(20px, 2.2vw, 30px); font-weight: 550; letter-spacing: -.035em; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .positive { color: var(--green); }.negative { color: var(--red); }
  .overview { display: grid; grid-template-columns: minmax(0, 1.8fr) minmax(260px, 1fr); gap: 14px; }
  .card { border: 1px solid var(--border-control); border-radius: var(--radius-lg); background: var(--surface); overflow: hidden; min-width: 0; }
  .section-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 16px 18px 8px; }.section-head p { margin: 5px 0 0; color: var(--muted); font-size: var(--text-xs); }
  .count { border-radius: var(--radius); background: var(--surface-2); color: var(--muted); padding: 3px 6px; font-size: var(--text-xs); white-space: nowrap; font-variant-numeric: tabular-nums; }
  .chart-footer { display: flex; gap: 20px; flex-wrap: wrap; padding: 12px 18px; border-top: 1px solid var(--border); font-size: var(--text-xs); color: var(--muted); }.chart-footer b { margin-left: 6px; color: var(--text); font-weight: 500; }
  .market-list { max-height: 274px; overflow-y: auto; padding: 0 18px; }.market { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px 12px; padding: 14px 0; border-bottom: 1px solid var(--border); }.market:last-child { border: 0; }.market b { font-weight: 550; overflow-wrap: anywhere; }.price { text-align: right; font-variant-numeric: tabular-nums; }.market time { grid-column: 1 / -1; color: var(--muted); font-size: 10px; }.feed-error { grid-column: 1 / -1; color: var(--amber); }
  .tabs { display: flex; gap: 10px; padding: 0 16px; border-bottom: 1px solid var(--border); overflow-x: auto; }.tabs button { display: flex; align-items: center; gap: 7px; padding: 14px 3px 12px; background: transparent; border: 0; border-bottom: 2px solid transparent; color: var(--muted); font: inherit; white-space: nowrap; cursor: pointer; }.tabs button.active { color: var(--accent); border-bottom-color: var(--accent); }
  .table-scroll { max-height: 340px; overflow: auto; }table { border-collapse: collapse; width: 100%; font-size: var(--text-xs); white-space: nowrap; font-variant-numeric: tabular-nums; }th { color: var(--muted); font-weight: 500; background: var(--surface-2); position: sticky; top: 0; z-index: 1; }th, td { text-align: right; padding: 12px 16px; border-bottom: 1px solid var(--border); }th:first-child, td:first-child, th:nth-child(2), td:nth-child(2) { text-align: left; }td b { font-weight: 550; }td small { font-size: 10px; margin-top: 3px; }tbody tr:last-child td { border-bottom: 0; }tbody tr:hover { background: var(--surface-2); }.side { display: inline-block; padding: 3px 7px; border-radius: var(--radius); background: var(--surface-2); }
  .empty { min-height: 155px; display: flex; flex-direction: column; gap: 10px; justify-content: center; align-items: center; text-align: center; padding: 24px; color: var(--muted); }.empty b { color: var(--text); font-size: var(--text-sm); font-weight: 500; }.empty span { font-size: var(--text-xs); }.chart-empty { height: 215px; box-sizing: border-box; }
  .activity { max-height: 340px; overflow: auto; }.activity article { display: flex; align-items: flex-start; gap: 12px; padding: 15px 18px; border-bottom: 1px solid var(--border); }.activity b { font-weight: 500; }.activity p { color: var(--muted); font-size: var(--text-xs); white-space: pre-wrap; overflow-wrap: anywhere; line-height: 1.6; margin: 5px 0 0; }.activity article > div { flex: 1; min-width: 0; }.activity time { color: var(--muted); font-size: var(--text-xs); flex-shrink: 0; }.event-icon { color: var(--accent); padding-top: 1px; }
  .analytics { padding: var(--space-4); }.curve-box { height: 460px; box-sizing: border-box; }
  .pagination { display: flex; justify-content: flex-end; align-items: center; gap: 12px; padding: 10px 16px; font-size: var(--text-xs); color: var(--muted); border-top: 1px solid var(--border); }
  .notice { display: flex; align-items: center; gap: 10px; padding: 12px; border-radius: var(--radius); background: color-mix(in srgb, var(--red) 7%, transparent); color: var(--red); }.notice span { flex: 1; overflow-wrap: anywhere; }.footnote { display: flex; align-items: flex-start; gap: 7px; font-size: var(--text-xs); color: var(--muted); line-height: 1.6; }.loading { padding: 100px 20px; text-align: center; color: var(--muted); }
  button:focus-visible, .table-scroll:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }button:disabled { opacity: .5; cursor: default; }
  @media (max-width: 900px) { .hero { align-items: flex-start; flex-direction: column; }.controls { align-items: flex-start; }.controls small { text-align: left; max-width: none; }.overview { grid-template-columns: 1fr; }.market-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: 20px; }.sync { margin-left: 0; }.metrics { gap: 8px; }.metric { padding: 14px; }.metric.principal { padding-top: 13px; } }
  @media (max-width: 560px) { .dashboard { padding: 16px; } .metrics { grid-template-columns: repeat(2, minmax(0, 1fr)); }.market-list { grid-template-columns: 1fr; }.section-head { align-items: flex-start; }.section-head > .count { white-space: normal; }.activity article { flex-wrap: wrap; }.activity time { margin-left: 27px; }.timing > span { flex-wrap: wrap; }h2 { font-size: 21px; } }
  @media (max-width: 767px) {
    .view-tabs,
    .tabs {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 0 8px;
      overflow: visible;
    }
    .view-tabs button,
    .tabs button {
      min-width: 0;
      min-height: 44px;
      padding: 8px 4px;
      white-space: normal;
      overflow-wrap: anywhere;
    }
  }
</style>
