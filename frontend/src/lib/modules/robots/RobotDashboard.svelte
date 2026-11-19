<script>
  // A robot's page: its state always in sight (status dot, environment, guard badge, next tick,
  // last sync), then eight tabs. Every figure is the server's (`GET /api/robots/{id}/dashboard`,
  // its `robot` block counts the robot's own size); the live hub moves the marks between two
  // reads. A refusal is shown as the server said it.
  import { onMount, untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import RobotChart from './RobotChart.svelte';
  import DataGaps from '$lib/modules/backtest/DataGaps.svelte';
  import Execution from './Execution.svelte';
  import Positions from './Positions.svelte';
  import Pending from './Pending.svelte';
  import GuardTimeline from './GuardTimeline.svelte';
  import RobotBadges from './RobotBadges.svelte';
  import { onBroker } from './execution.js';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Table from '$lib/ui/Table.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import EquityChart from '$lib/modules/journal/EquityChart.svelte';
  import DistChart from '$lib/modules/journal/DistChart.svelte';
  import CalendarView from '$lib/modules/journal/CalendarView.svelte';
  import TradeStats from '$lib/analysis/TradeStats.svelte';
  import TradeCurve from '$lib/analysis/TradeCurve.svelte';
  import { stats as closedTradeStats } from '$lib/analysis/trades.js';
  import Scatter3DPanel from '$lib/modules/journal/Scatter3DPanel.svelte';
  import StreaksPanel from '$lib/modules/journal/StreaksPanel.svelte';
  import { toAnalysisTrades, toPlotPoints, PLOT_METRICS } from '$lib/modules/backtest/api.js';
  import { robotsApi } from './api.js';
  import { finite, projectBook, sessionState } from './dashboard.js';
  import { age, countdown } from './fleet.js';
  import { t } from '$lib/i18n';
  import { liveStream } from '$lib/live/stream.js';
  import InsightsButton from '$lib/insights/InsightsButton.svelte';

  let { sessionId, onedit = () => {}, onchanged = () => {} } = $props();
  const prefix = 'robots.dashboard.';
  const label = (key, params) => $t(prefix + key, params);
  const num = (value, digits = 2) => (finite(value) ? value.toLocaleString(undefined, { maximumFractionDigits: digits, minimumFractionDigits: digits }) : '-');
  const signed = (value) => (finite(value) ? `${value > 0 ? '+' : ''}${num(value)}` : '-');
  const when = (value) => (value && Number.isFinite(Date.parse(value)) ? new Date(value).toLocaleString() : '-');
  const tone = (value) => (!finite(value) || value === 0 ? '' : value > 0 ? 'positive' : 'negative');
  const historyColumns = $derived([
    { key: 'instrument', label: label('instrument') },
    { key: 'side', label: label('side') },
    { key: 'quantity', label: label('quantity'), numeric: true },
    { key: 'entry', label: label('entry'), numeric: true },
    { key: 'exit', label: label('exit'), numeric: true },
    { key: 'realized', label: label('realized'), numeric: true },
    { key: 'fees', label: label('fees'), numeric: true },
    { key: 'closed', label: label('closed'), align: 'right' }
  ]);

  const TABS = ['overview', 'positions', 'pending', 'trades', 'stats', 'guards', 'log', 'settings'];
  const start = typeof location === 'undefined' ? null : new URLSearchParams(location.search).get('tab');
  let tab = $state(TABS.includes(start) ? start : 'overview');
  let tradesView = $state('stats');
  let data = $state(null);
  let quotes = $state({});
  let feedStates = $state({});
  let error = $state('');
  let actionError = $state('');
  let busy = $state('');
  let refreshing = $state(false);
  let now = $state(Date.now());
  let visible = $state(true);
  let streamEpoch = $state(0);
  let chartAssetId = $state('');
  let page = $state(0);
  let deleteOpen = $state(false);
  let refreezeOpen = $state(false);
  let controller;
  let alive = true;
  let inFlight = null;

  const session = $derived(data?.session);
  const robot = $derived(data?.robot);
  const book = $derived(projectBook(data, quotes));
  const state = $derived(busy === 'run' ? 'running' : sessionState(session));
  const instruments = $derived(data?.instruments ?? []);
  const streamKey = $derived(JSON.stringify(instruments.filter((i) => i.stream).map((i) => ({
    id: i.id, provider: i.provider, asset_type: i.asset_type, ticker: i.ticker,
    timeframe: i.stream_timeframe, connector_id: i.connector_id
  }))));
  // Live marks by ticker, for the robot's positions.
  const tickerQuotes = $derived(Object.fromEntries(instruments.filter((i) => quotes[i.id]).map((i) => [i.ticker, quotes[i.id]])));
  const multiplier = $derived(finite(session?.settings?.instrument?.multiplier) && session.settings.instrument.multiplier > 0 ? session.settings.instrument.multiplier : 1);
  // Unrealized on the live marks where the hub streams, the stored ones elsewhere.
  const unrealized = $derived.by(() => {
    const list = robot?.positions ?? [];
    if (!list.length) return 0;
    let sum = 0;
    for (const p of list) {
      const live = tickerQuotes[p.ticker];
      const sign = p.direction === 'short' ? -1 : 1;
      const u = live && finite(p.avg_price) ? (live.price - p.avg_price) * p.qty * multiplier * sign : p.unrealized;
      if (!finite(u)) return null;
      sum += u;
    }
    return sum;
  });
  const equity = $derived(finite(robot?.start_equity) && finite(robot?.realized) ? robot.start_equity + robot.realized + (unrealized ?? 0) : null);
  const badgeRobot = $derived(session && robot ? {
    status: session.status, running: session.running || busy === 'run', last_error: session.last_error,
    exec_state: session.exec_state, environment: robot.environment, armed: !!session.armed_at,
    guard: robot.guards?.badge, guard_holds: robot.guards?.holds
  } : null);
  const next = $derived(countdown(session?.next_run_at, now));
  const synced = $derived(age(onBroker(session) ? session?.exec_synced_at : session?.last_run_at, now));
  const analysisTrades = $derived(toAnalysisTrades(book.closed));
  const closedStats = $derived(closedTradeStats(analysisTrades));
  const plotPoints = $derived(toPlotPoints(book.closed));
  const signals = $derived((session?.settings?.kind ?? 'signals') === 'signals');
  const pages = $derived(Math.max(1, Math.ceil(book.closed.length / 25)));
  const history = $derived([...book.closed].reverse().slice(Math.min(page, pages - 1) * 25, (Math.min(page, pages - 1) + 1) * 25));
  const currency = $derived(robot?.capital?.currency || '');
  const tabs = $derived(TABS.map((id) => ({
    id,
    label: label(`tab.${id}`) + (id === 'positions' ? ` (${robot?.positions?.length ?? 0})` : id === 'pending' ? ` (${robot?.pending?.length ?? 0})` : id === 'trades' ? ` (${book.closed.length})` : '')
  })));
  const stats = $derived(robot?.stats ?? {});

  async function refresh() {
    if (inFlight) return inFlight;
    refreshing = true;
    controller = new AbortController();
    inFlight = (async () => {
      try {
        const next = await robotsApi.dashboard(sessionId, controller.signal);
        if (!alive) return;
        data = next;
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
    const clock = setInterval(() => (now = Date.now()), 1000);
    const timer = setInterval(() => { if (!document.hidden && !busy) refresh(); }, 5000);
    const visibility = () => { visible = !document.hidden; if (visible) refresh(); };
    document.addEventListener('visibilitychange', visibility);
    return () => {
      alive = false;
      controller?.abort();
      clearInterval(clock);
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
    const sources = [];
    for (let offset = 0; offset < items.length; offset += 12) {
      const batch = items.slice(offset, offset + 12);
      const stopped = new Set();
      const receive = (payload, event) => {
        try {
          const item = batch[payload?.i];
          if (!item) return;
          if (event.type === 'status') {
            feedStates = { ...feedStates, [item.id]: payload };
            if (payload.state === 'stopped') stopped.add(item.id);
            if (stopped.size === batch.length) source?.close();
          } else if (finite(payload.c)) {
            quotes = { ...quotes, [item.id]: { price: payload.c, at: new Date(Date.now() - Math.max(0, payload.lag_ms ?? 0)).toISOString(), source: 'live' } };
          }
        } catch { /* A malformed stream frame must not break the dashboard. */ }
      };
      // Supervised: reopened with backoff when dropped, refused or silent (lib/live/stream.js).
      const source = liveStream({
        url: () => robotsApi.streamsUrl(batch),
        events: { snapshot: receive, bar: receive, status: receive },
        onstate: (st) => {
          if (st !== 'reconnecting') return;
          const message = $t('histviz.live.serverLostShort');
          const retrying = batch.filter((i) => !stopped.has(i.id)).map((i) => [i.id, { state: 'retrying', message }]);
          feedStates = { ...feedStates, ...Object.fromEntries(retrying) };
        }
      });
      sources.push(source);
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
        const result = await robotsApi.runNow(sessionId);
        if (result.error) throw new Error(result.error);
      } else if (kind === 'toggle') {
        await robotsApi.setStatus(sessionId, session.status === 'active' ? 'paused' : 'active');
      } else if (kind === 'resume') {
        await robotsApi.resume(sessionId);
      } else if (kind === 'refreeze') {
        await robotsApi.refreeze(sessionId);
      } else if (kind === 'delete') {
        await robotsApi.remove(sessionId);
        goto('/robots');
        return;
      }
      await refresh();
      onchanged();
    } catch (e) { if (alive) actionError = e.message; }
    finally { if (alive) busy = ''; }
  }
</script>

<div class="dashboard" data-paper-dashboard data-robot-dashboard>
  <div class="page-bar"><a class="back btn sm" href="/robots"><Icon name="arrow-left" size={14} />{label('back')}</a><span>{$t('robots.title')}</span></div>
  {#if error}
    <div class="notice error" role="alert"><Icon name="alert-triangle" /> <span>{label(data ? 'refreshFailed' : 'loadFailed')} {error}</span><Button size="sm" onclick={refreshNow} loading={refreshing}>{label('retry')}</Button></div>
  {/if}
  {#if !data}
    <div class="loading" aria-live="polite"><Icon name="bot" size={28} /><p>{label(error ? 'loadFailed' : 'loading')}</p></div>
  {:else}
    <header class="hero">
      <div class="identity">
        <div class="eyebrow">
          {#if onBroker(session)}<Icon name="briefcase" size={12} /> <span class="broker-eyebrow">{label('onBroker', { account: session.execution?.account ?? '' })}</span>{:else}<Icon name="flask" size={12} /> {label('paperOnly')}{/if}
          {#if robot?.strategy?.name}<span class="sep">·</span> {robot.strategy.name}{#if robot.strategy.version !== null && robot.strategy.version !== undefined} <span class="ver">v{robot.strategy.version}</span>{/if}{/if}
        </div>
        <!-- Title and state on one line: the badges read as part of the name, not a row of their own. -->
        <div class="title-row">
          <h2>{session.name}</h2>
          {#if badgeRobot}<RobotBadges robot={badgeRobot} />{/if}
        </div>
        <div class="status-line"><span>{label(`stateHint.${state}`)}</span></div>
      </div>
      <div class="controls">
        <div class="buttons">
          <InsightsButton source={{ kind: 'paper', session: session.id }} size="sm" />
          <Button size="sm" icon="refresh-cw" disabled={!!busy || session.running} loading={busy === 'run'} onclick={() => act('run')}>{$t('robots.runNow')}</Button>
          <Button size="sm" icon={session.status === 'active' ? 'stop' : 'play'} variant={session.status === 'active' ? 'danger' : 'primary'} disabled={!!busy || session.running} loading={busy === 'toggle'} onclick={() => act('toggle')}>{label(session.status === 'active' ? 'stop' : 'start')}</Button>
        </div>
        <small>{label('stopHint')}</small>
      </div>
    </header>
    {#if actionError || session.last_error}<div class="notice error" role="alert"><Icon name="alert-triangle" /><span>{actionError || session.last_error}</span></div>{/if}
    {#if robot?.guards?.holds}<div class="notice warn" role="status"><Icon name="shield" size={14} /><span>{robot.guards.holds}</span><Button size="sm" loading={busy === 'resume'} onclick={() => act('resume')}>{$t('robots.guard.resume')}</Button></div>{/if}
    <DataGaps gaps={session.stats?.data_gaps ?? []} excluded={session.stats?.excluded_trades ?? []} />
    <div class="timing">
      <span><Icon name="clock" size={12} /> {label('next')} <b data-next-tick>{next ? (next === 'due' ? $t('robots.due') : $t('robots.fleet.nextIn', { in: next })) : '-'}</b></span>
      <span>{label('last')} <b>{when(session.last_run_at)}</b></span>
      <span>{$t('robots.fleet.synced')} <b>{synced ?? '-'}</b></span>
      <span class="sync"><span class="dot" class:healthy={!error}></span>{label(error ? 'disconnected' : 'autoRefresh')}</span>
      <button class="icon-button" disabled={refreshing || !!busy} onclick={refreshNow} aria-label={label('refresh')} title={label('refresh')}><Icon name="refresh-cw" size={12} /></button>
    </div>

    <Tabs {tabs} bind:value={tab} ariaLabel={session.name} />

    {#if tab === 'overview'}
      <div class="metrics">
        <StatCard label={$t('robots.fleet.equity')} value={num(equity)} hint={currency} />
        <StatCard label={label('realized')} value={signed(robot?.realized)} hint={label('closedCount', { n: stats.trades ?? 0 })} />
        <StatCard label={label('unrealized')} value={signed(unrealized)} hint={label('positionCount', { n: robot?.positions?.length ?? 0 })} />
        <StatCard label={$t('robots.fleet.today')} value={signed(robot?.today?.realized)} hint={$t('robots.fleet.ordersToday', { n: robot?.today?.orders_sent ?? 0 })} />
        <StatCard label={label('winRate')} value={finite(stats.win_rate) ? `${num(stats.win_rate, 1)}%` : '-'} hint={label('closedOnly')} />
        <StatCard label={$t('robots.stats.profitFactor')} value={num(stats.profit_factor)} />
        <StatCard label={$t('robots.stats.maxDrawdown')} value={finite(stats.max_drawdown_pct) ? `${num(stats.max_drawdown_pct, 1)}%` : '-'} hint={num(stats.max_drawdown)} />
        <StatCard label={$t('robots.stats.expectancy')} value={signed(stats.expectancy)} hint={label('fees') + ' ' + num(stats.fees)} />
      </div>
      {#if onBroker(session)}<Execution {sessionId} onchanged={refresh} />{/if}
      <div class="overview">
        <section class="panel performance">
          <EquityChart points={robot?.curve ?? []} currency={currency || 'USD'} />
        </section>
        <section class="panel markets">
          <div class="section-head"><div><h3>{label('prices')}</h3><p>{label('pricesHint')}</p></div><Icon name="radio" size={17} /></div>
          <div class="market-list">
            {#each instruments as instrument (instrument.id)}
              {@const quote = quoteFor(instrument)}
              {@const status = quoteState(instrument)}
              <div class="market">
                <div><b>{instrument.ticker}</b><small>{instrument.provider} · {instrument.timeframe}</small></div>
                <div class="price"><b class="figure">{num(quote.price, 4)}</b><small class:positive={status === 'live'} title={feedStates[instrument.id]?.message ?? ''}>{label(`quote.${status}`)}</small></div>
                <time title={when(quote.at)}>{when(quote.at)}</time>
                {#if feedStates[instrument.id]?.message}<small class="feed-error">{feedStates[instrument.id].message}</small>{/if}
              </div>
            {:else}<div class="no-prices"><span>{label('noPrices')}</span></div>{/each}
          </div>
        </section>
      </div>
      <div class="overview two">
        <section class="panel pad">
          <h3>{$t('robots.breakdown.instrument')}</h3>
          <DistChart buckets={robot?.breakdown?.instrument ?? []} currency={currency || 'USD'} />
        </section>
        <section class="panel pad">
          <h3>{$t('robots.breakdown.direction')}</h3>
          <DistChart buckets={robot?.breakdown?.direction ?? []} currency={currency || 'USD'} label={(b) => label(b.key)} />
        </section>
      </div>
      <section class="panel pad">
        <CalendarView days={robot?.calendar ?? []} monthKey="otw.robots.calendar.month.v1" displayCurrency={currency || 'USD'} />
      </section>
      <!-- No frame here: RobotChart draws its own, a second one would be a border inside a border. -->
      <section class="chart-slot">
        <RobotChart {session} {instruments} {book} bind:assetId={chartAssetId} />
      </section>
    {:else if tab === 'positions'}
      <section class="panel"><Positions positions={robot?.positions ?? []} quotes={tickerQuotes} {multiplier} /></section>
    {:else if tab === 'pending'}
      <section class="panel"><Pending pending={robot?.pending ?? []} /></section>
    {:else if tab === 'stats'}
      <div class="sub" role="group" aria-label={label('tab.stats')}>
        {#each ['stats', 'perf', ...(signals ? ['scatter3d', 'streaks3d'] : [])] as v (v)}
          <button class:on={tradesView === v} aria-pressed={tradesView === v} onclick={() => (tradesView = v)}>{$t(`robots.tradesView.${v}`)}</button>
        {/each}
      </div>
      {#if !analysisTrades.length}
        <EmptyState icon="history" title={label('noHistory')} description={label('noHistoryHint')} compact />
      {:else if tradesView === 'stats'}
        <section class="panel analytics"><TradeStats s={closedStats} trades={analysisTrades} /></section>
      {:else if tradesView === 'perf'}
        <section class="panel analytics curve-box"><TradeCurve trades={analysisTrades} /></section>
      {:else if tradesView === 'scatter3d'}
        <Scatter3DPanel points={plotPoints} {currency} metricIds={PLOT_METRICS}
          colorModes={instruments.length > 1 ? ['result', 'side', 'ticker'] : ['result', 'side']}
          storageKey="otw.robots.scatter3d.v1" />
      {:else if tradesView === 'streaks3d'}
        <StreaksPanel points={plotPoints} {currency} units={['money']} storageKey="otw.robots.streaks3d.v1" />
      {/if}
    {:else if tab === 'trades'}
      <section class="panel book">
        {#if history.length}
          <!-- Scrollable tables must remain keyboard accessible. -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <div class="table-scroll" tabindex="0" role="region" aria-label={label('history')}>
            <Table columns={historyColumns} rows={history}>
              {#snippet cell(trade, col)}
                {#if col.key === 'instrument'}<b>{trade.ticker}</b><small>{trade.exit_reason ?? '-'}</small>
                {:else if col.key === 'side'}{label(trade.direction)}
                {:else if col.key === 'quantity'}{num(trade.qty, 4)}{#if trade.scale && trade.scale !== '1'}<small>x{trade.scale}</small>{/if}
                {:else if col.key === 'entry'}{num(trade.entry_price, 4)}
                {:else if col.key === 'exit'}{num(trade.exit_price, 4)}
                {:else if col.key === 'realized'}<span class={tone(trade.pnl)}>{signed(trade.pnl)}</span>
                {:else if col.key === 'fees'}{num(trade.fees)}
                {:else}<span class="mono">{when(trade.exit_ts)}</span>{/if}
              {/snippet}
            </Table>
          </div>
          <div class="pagination"><Button size="sm" disabled={page === 0} onclick={() => page--}>{label('previous')}</Button><span>{label('page', { n: Math.min(page + 1, pages), total: pages })}</span><Button size="sm" disabled={page >= pages - 1} onclick={() => page++}>{label('nextPage')}</Button></div>
        {:else}<EmptyState icon="history" title={label('noHistory')} description={label('noHistoryHint')} compact />{/if}
        {#if book.skipped.length}
          <div class="skipped">
            <h4>{$t('robots.notTaken', { n: book.skipped.length })}</h4>
            {#each book.skipped.slice(-10).reverse() as s, i (i)}<p><b>{s.ticker}</b> {label(s.direction)} {when(s.entry_ts)}: {s.note}</p>{/each}
          </div>
        {/if}
      </section>
    {:else if tab === 'guards'}
      <section class="panel"><GuardTimeline guards={robot?.guards} busy={busy === 'resume'} onresume={() => act('resume')} /></section>
    {:else if tab === 'log'}
      <section class="panel">
        <div class="activity">
          {#each data.events ?? [] as event (event.id)}<article><span class="event-icon" class:negative={event.kind === 'error' || event.kind === 'refused'}><Icon name={event.kind === 'error' || event.kind === 'refused' ? 'alert-triangle' : event.kind === 'close' ? 'check-circle' : event.kind === 'open' ? 'arrow-right' : event.kind === 'guard' ? 'shield' : 'zap'} size={15} /></span><div><b>{event.title}</b>{#if event.kind === 'refused'}<p class="reason" title={event.message}>{event.message}</p>{:else}<p>{event.message}</p>{/if}</div><time>{when(event.at)}</time></article>
          {:else}<EmptyState icon="zap" title={label('noActivity')} compact />{/each}
        </div>
      </section>
    {:else}
      <section class="panel pad settings">
        <dl>
          <dt>{$t('robots.wizard.step.strategy')}</dt>
          <dd>{robot?.strategy?.name || $t('robots.wizard.fromRun')}{#if robot?.strategy?.version !== null && robot?.strategy?.version !== undefined} · v{robot.strategy.version}{/if}</dd>
          <dt>{$t('robots.wizard.step.account')}</dt>
          <dd>{onBroker(session) ? $t('robots.exec.summaryBroker', { account: session.execution?.account ?? '' }) : $t('robots.exec.summarySimulate')}</dd>
          <dt>{$t('robots.wizard.step.schedule')}</dt>
          <dd>{$t(`automator.sched.kind.${session.kind}`)}{#if session.kind === 'interval'} · {$t('robots.everyMinutes', { n: session.every_minutes ?? 0 })}{/if} · {session.timezone}</dd>
          <dt>{$t('robots.priority')}</dt>
          <dd>{session.priority}</dd>
          <dt>{$t('robots.wizard.step.capital')}</dt>
          <dd>{Object.keys(session.capital ?? {}).length ? Object.entries(session.capital).map(([k, v]) => `${$t(`robots.capital.short.${k}`)} ${v}`).join(' · ') : $t('robots.capital.noCap')}</dd>
          <dt>{$t('robots.wizard.step.guards')}</dt>
          <dd>{#if robot?.guards?.described?.length}{robot.guards.described.join('; ')}{:else}{$t('robots.guard.none')}{/if}</dd>
        </dl>
        <div class="buttons">
          <Button icon="settings" disabled={!!busy} onclick={() => onedit(session, instruments.map((i) => i.ticker))}>{label('settings')}</Button>
          {#if session.strategy_id}<Button disabled={!!busy} loading={busy === 'refreeze'} onclick={() => (refreezeOpen = true)}>{$t('robots.refreeze')}</Button>{/if}
          <Button variant="danger" disabled={!!busy} onclick={() => (deleteOpen = true)}>{$t('robots.deleteTitle')}</Button>
        </div>
        <p class="hint">{$t('robots.refreezeHint')}</p>
      </section>
    {/if}
    <footer class="footnote"><Icon name="info" size={13} /><span>{label(onBroker(session) ? 'valuationHintBroker' : 'valuationHint')}</span></footer>
  {/if}
</div>

<ConfirmModal bind:open={deleteOpen} title={$t('robots.deleteTitle')} message={$t('robots.deleteConfirm', { name: session?.name ?? '' })} confirmLabel={$t('robots.deleteTitle')} cancelLabel={$t('common.cancel')} danger onconfirm={() => act('delete')} />
<ConfirmModal bind:open={refreezeOpen} title={$t('robots.refreeze')} message={$t('robots.refreezeConfirm')} confirmLabel={$t('robots.refreeze')} cancelLabel={$t('common.cancel')} onconfirm={() => act('refreeze')} />

<style>
  .dashboard { box-sizing: border-box; width: 100%; height: 100%; overflow-y: auto; padding: var(--space-6); display: flex; flex-direction: column; gap: var(--space-4); color: var(--text); font-size: var(--text-sm); min-width: 0; }
  .page-bar { display: flex; align-items: center; gap: var(--space-4); color: var(--dim); font-size: var(--fs-body); }
  .back { text-decoration: none; flex: none; }
  .dashboard > :global(*) { flex-shrink: 0; }

  /* Identity block: eyebrow, title with its state, one hint line. Controls on the right. */
  .hero { display: flex; justify-content: space-between; gap: var(--space-6); align-items: flex-start; }
  .identity { min-width: 0; display: flex; flex-direction: column; gap: var(--space-2); }
  .eyebrow { display: flex; align-items: center; gap: var(--space-2); color: var(--dim); font-size: var(--fs-metric-label); text-transform: uppercase; letter-spacing: 0.06em; flex-wrap: wrap; }
  .broker-eyebrow { color: var(--dim); font-weight: var(--fw-medium); }
  .sep { color: var(--faint); }
  .ver { font-family: var(--mono); text-transform: none; letter-spacing: 0; }
  .title-row { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2) var(--space-4); min-width: 0; }
  h2 { font-size: var(--fs-page-title); font-weight: var(--fw-medium); line-height: var(--lh-tight); letter-spacing: -0.025em; margin: 0; overflow-wrap: anywhere; }
  h3 { font-size: var(--fs-item-title); font-weight: var(--fw-medium); letter-spacing: 0.02em; margin: 0 0 var(--space-3); }
  h4 { font-size: var(--fs-metric-label); font-weight: var(--fw-medium); text-transform: uppercase; letter-spacing: 0.06em; margin: 0 0 var(--space-2); color: var(--dim); }
  .status-line { color: var(--dim); font-size: var(--fs-desc); }
  .dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: currentColor; flex: none; }
  .healthy { color: var(--green); }
  .controls { display: flex; flex-direction: column; gap: var(--space-2); align-items: flex-end; flex: none; }
  .buttons { display: flex; gap: var(--space-2); flex-wrap: wrap; }
  small { display: block; color: var(--dim); font-size: var(--fs-desc); font-weight: var(--fw-normal); line-height: var(--lh-base); }
  .controls small { max-width: 370px; text-align: right; }

  /* One metadata strip between two hairlines: labels small caps, values mono. */
  .timing { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2) var(--space-6); min-height: var(--row-h); padding: var(--space-1) 0; border-block: var(--hairline) solid var(--border); color: var(--dim); font-size: var(--fs-metric-label); text-transform: uppercase; letter-spacing: 0.06em; }
  .timing > span { display: flex; align-items: center; gap: var(--space-2); }
  .timing b { color: var(--text); font-family: var(--mono); font-size: var(--fs-body); font-weight: var(--fw-normal); text-transform: none; letter-spacing: 0; }
  .sync { margin-left: auto; }
  .icon-button { border: 0; background: transparent; color: var(--dim); cursor: pointer; display: inline-flex; padding: var(--space-1); }
  .icon-button:hover:not(:disabled) { color: var(--text); }

  .metrics { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--space-3); }
  .positive { color: var(--green); }
  .negative { color: var(--red); }
  .overview { display: grid; grid-template-columns: minmax(0, 1.8fr) minmax(260px, 1fr); gap: var(--space-3); }
  .overview.two { grid-template-columns: repeat(2, minmax(0, 1fr)); }

  /* Level-1 panel: square, one hairline, no lift. `panel`, not `card`: the global .card
     layer would add its radius and shadow. */
  .panel { border: var(--hairline) solid var(--border); background: var(--surface); overflow: hidden; min-width: 0; }
  .panel.pad { padding: var(--space-4); }
  .chart-slot { min-height: 420px; min-width: 0; }
  .performance { padding: var(--space-3); }

  .section-head { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--space-3); padding: var(--space-4) var(--space-4) var(--space-2); color: var(--dim); }
  .section-head h3 { margin: 0; color: var(--text); }
  .section-head p { margin: var(--space-1) 0 0; color: var(--dim); font-size: var(--fs-desc); }
  .market-list { max-height: 274px; overflow-y: auto; padding: 0 var(--space-4); }
  .market { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: var(--space-1) var(--space-3); min-height: var(--row-h); padding: var(--space-2) 0; border-bottom: var(--hairline) solid var(--border); }
  .market:last-child { border: 0; }
  .market b { font-weight: var(--fw-medium); overflow-wrap: anywhere; }
  .market b.figure { font-family: var(--mono); font-weight: var(--fw-normal); }
  .price { text-align: right; font-variant-numeric: tabular-nums; }
  .market time { grid-column: 1 / -1; color: var(--dim); font-family: var(--mono); font-size: var(--fs-metric-label); }
  .feed-error { grid-column: 1 / -1; color: var(--amber); }

  /* View switch: one frame, borderless segments, the pressed one filled. */
  .sub { display: inline-flex; flex-wrap: wrap; align-self: flex-start; border: var(--hairline) solid var(--border-control); }
  .sub button { border: 0; background: transparent; color: var(--muted); padding: var(--space-1) var(--space-3); font: inherit; font-size: var(--fs-body); cursor: pointer; }
  .sub button:hover { color: var(--text); }
  .sub button.on { background: var(--surface-2); color: var(--text); }

  /* History table: Table.svelte inside a 440px scroll region. The region owns the scroll so the
     sticky header sticks to it; Table's own wrapper must not scroll. */
  .table-scroll { max-height: 440px; overflow: auto; }
  .table-scroll :global(.wrap) { overflow: visible; }
  .table-scroll b { font-weight: var(--fw-medium); }
  .table-scroll small { font-size: var(--fs-metric-label); }
  .mono { font-family: var(--mono); }

  .no-prices { min-height: 155px; display: flex; flex-direction: column; gap: var(--space-2); justify-content: center; align-items: center; text-align: center; padding: var(--space-6); color: var(--dim); font-size: var(--fs-desc); }

  /* Activity log: neutral icons, red only for an error or a refusal; times in mono. */
  .activity { max-height: 540px; overflow: auto; }
  .activity article { display: flex; align-items: flex-start; gap: var(--space-3); padding: var(--space-3) var(--space-4); border-bottom: var(--hairline) solid var(--border); }
  .activity b { font-weight: var(--fw-medium); }
  .activity p { color: var(--dim); font-size: var(--fs-desc); white-space: pre-wrap; overflow-wrap: anywhere; line-height: var(--lh-base); margin: var(--space-1) 0 0; }
  .activity article > div { flex: 1; min-width: 0; }
  /* A refusal: one line in the log, the exact reason on hover. */
  .activity p.reason { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; cursor: help; }
  .activity time { color: var(--dim); font-family: var(--mono); font-size: var(--fs-desc); flex-shrink: 0; }
  .event-icon { color: var(--dim); padding-top: 1px; }
  .event-icon.negative { color: var(--red); }

  .analytics { padding: var(--space-4); }
  .curve-box { height: 460px; box-sizing: border-box; }
  .pagination { display: flex; justify-content: flex-end; align-items: center; gap: var(--space-3); padding: var(--space-2) var(--space-4); font-size: var(--fs-desc); color: var(--dim); border-top: var(--hairline) solid var(--border); }
  .pagination > span { font-family: var(--mono); }
  .skipped { padding: var(--space-3) var(--space-4); border-top: var(--hairline) solid var(--border); }
  .skipped p { margin: 0 0 var(--space-1); font-size: var(--fs-desc); color: var(--dim); overflow-wrap: anywhere; }
  .skipped b { font-weight: var(--fw-medium); color: var(--text); }

  /* Settings: labels as metric labels, values in body ink. */
  .settings dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); align-items: baseline; gap: var(--space-3) var(--space-6); margin: 0 0 var(--space-4); }
  .settings dt { color: var(--dim); font-size: var(--fs-metric-label); text-transform: uppercase; letter-spacing: 0.06em; }
  .settings dd { margin: 0; font-size: var(--fs-body); overflow-wrap: anywhere; }
  .hint { margin: var(--space-3) 0 0; color: var(--dim); font-size: var(--fs-desc); }

  /* Notices: a tint, a hairline in the hue, ink in --text; the icon carries the meaning. */
  .notice { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-3) var(--space-4); background: color-mix(in srgb, var(--red) 10%, transparent); border: var(--hairline) solid color-mix(in srgb, var(--red) 45%, transparent); color: var(--text); line-height: var(--lh-base); }
  .notice span { flex: 1; overflow-wrap: anywhere; }
  .notice > :global(svg) { flex: none; color: var(--red); }
  .notice.warn { background: color-mix(in srgb, var(--amber) 14%, transparent); border-color: color-mix(in srgb, var(--amber) 45%, transparent); }
  .notice.warn > :global(svg) { color: var(--amber); }

  .footnote { display: flex; align-items: flex-start; gap: var(--space-2); font-size: var(--fs-desc); color: var(--dim); line-height: var(--lh-base); }
  .loading { padding: var(--space-8) var(--space-4); text-align: center; color: var(--dim); }
  button:focus-visible, .table-scroll:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  button:disabled { opacity: .5; cursor: default; }
  @media (max-width: 900px) {
    .hero { flex-direction: column; }
    .controls { align-items: flex-start; }
    .controls small { text-align: left; max-width: none; }
    .overview, .overview.two { grid-template-columns: 1fr; }
    .sync { margin-left: 0; }
    .metrics { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-2); }
  }
  @media (max-width: 560px) {
    .dashboard { padding: var(--space-4); }
    .activity article { flex-wrap: wrap; }
    .activity time { margin-left: 27px; }
    .timing > span { flex-wrap: wrap; }
  }
  @media (max-width: 767px) {
    .analytics :global(.dist) { grid-template-columns: minmax(0, 1fr); }
    .analytics :global(.donut-row) { flex-wrap: wrap; }
  }
</style>
