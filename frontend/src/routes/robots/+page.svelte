<script>
  // Trading Robots: the fleet. Cards on a phone, a table on a desktop, filtered by state and
  // by account. Every figure comes from one read (`GET /api/robots/fleet`), refreshed while
  // the page is visible; the countdowns tick locally between two reads.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import RobotCard from '$lib/modules/robots/RobotCard.svelte';
  import FleetTable from '$lib/modules/robots/FleetTable.svelte';
  import EquityChart from '$lib/modules/journal/EquityChart.svelte';
  import DatePicker from '$lib/ui/DatePicker.svelte';
  import { robotsApi } from '$lib/modules/robots/api.js';
  import { RANK_PERIODS, countdown, filterFleet, fleetAccounts, fleetEnvironments, fleetExposure, fleetPnl, fleetRanking, robotState, stateTone } from '$lib/modules/robots/fleet.js';
  import { t } from '$lib/i18n';

  let data = $state(null);
  let error = $state('');
  let now = $state(Date.now());
  let status = $state('');
  let account = $state('');
  let acking = $state(false);
  // The fleet P&L block: its own environment and date filters, apart from the table's.
  let env = $state('');
  let from = $state('');
  let to = $state('');

  const STATES = ['active', 'held', 'stopped', 'interrupted', 'degraded', 'blocked', 'unreachable', 'error'];
  const robots = $derived(data?.robots ?? []);
  const shown = $derived(filterFleet(robots, { status, account }));
  // The state of each robot, the word the filter reads (`running` counts as active).
  const stateOf = (r) => {
    const s = robotState(r);
    return s === 'running' ? 'active' : s;
  };
  const counts = $derived.by(() => {
    const c = Object.fromEntries(STATES.map((s) => [s, 0]));
    for (const r of robots) if (stateOf(r) in c) c[stateOf(r)] += 1;
    return c;
  });
  const chips = $derived(STATES.filter((s) => counts[s] > 0));
  const accountOptions = $derived([
    { value: '', label: $t('robots.fleet.allAccounts') },
    { value: 'simulated', label: $t('robots.env.simulated') },
    ...fleetAccounts(robots).map((a) => ({ value: a.id, label: a.name }))
  ]);
  const totals = $derived.by(() => {
    const sum = (k) => robots.reduce((n, r) => n + (Number.isFinite(r[k]) ? r[k] : 0), 0);
    return { equity: sum('equity'), today: sum('today_pnl'), open: sum('open'), inMarket: robots.filter((r) => r.open > 0).length };
  });
  // Equity split by robot: one segment per robot holding a positive equity, in its state tone.
  const split = $derived.by(() => {
    const parts = robots.map((r) => ({ id: r.id, name: r.name, v: Number.isFinite(r.equity) ? Math.max(0, r.equity) : 0, tone: stateTone(stateOf(r)) }));
    const total = parts.reduce((n, p) => n + p.v, 0);
    return total > 0 ? parts.filter((p) => p.v > 0).map((p) => ({ ...p, pct: (p.v / total) * 100 })) : [];
  });
  // What the user has to look at: an error, a guard holding, events not yet read.
  const attention = $derived(robots.filter((r) => r.last_error || r.guard_holds || r.unseen > 0));
  const upcoming = $derived(
    robots
      .filter((r) => Number.isFinite(Date.parse(r.next_run_at ?? '')))
      .sort((a, b) => Date.parse(a.next_run_at) - Date.parse(b.next_run_at))
      .slice(0, 8)
  );
  const envOptions = $derived([
    { value: '', label: $t('robots.fleet.allEnvironments') },
    ...fleetEnvironments(robots).map((e) => ({ value: e, label: $t(`robots.env.${e}`) }))
  ]);
  const pnl = $derived(fleetPnl(robots, { env, from, to }, now - (now % 60000)));
  // The chart's currency: the one every robot shown names, else none (the figures stay bare).
  const CCY = $derived.by(() => {
    const set = new Set(robots.filter((r) => !env || r.environment === env).map((r) => r.currency || ''));
    return set.size === 1 ? [...set][0] : '';
  });
  // Ranking by the P&L closed in a period; exposure by asset type of the open book.
  let period = $state('today');
  const ranking = $derived(fleetRanking(robots, period, now - (now % 60000)));
  const rankMax = $derived(Math.max(0, ...ranking.map((r) => Math.abs(r.pnl))));
  const exposure = $derived(fleetExposure(robots));
  const assetLabel = (k) => {
    const key = `robots.fleet.asset.${k}`;
    const v = $t(key);
    return v === key ? k : v;
  };
  const pct = (v) => `${v > 0 ? '+' : ''}${v.toLocaleString(undefined, { maximumFractionDigits: 2 })}%`;
  const money = (v) => v.toLocaleString(undefined, { maximumFractionDigits: 2 });
  const signed = (v) => `${v > 0 ? '+' : ''}${money(v)}`;

  async function load() {
    try {
      data = await robotsApi.fleet();
      error = '';
    } catch (e) {
      error = e.message;
    }
  }
  async function ack() {
    acking = true;
    try {
      await robotsApi.markSeen(null);
      await load();
    } catch (e) {
      error = e.message;
    } finally {
      acking = false;
    }
  }

  onMount(() => {
    load();
    const clock = setInterval(() => (now = Date.now()), 1000);
    const poll = setInterval(() => {
      if (!document.hidden) load();
    }, 10000);
    return () => {
      clearInterval(clock);
      clearInterval(poll);
    };
  });
</script>

<RequireModule module="robots">
  <div class="page" data-robots-fleet>
    <PageHeader title={$t('robots.title')} subtitle={$t('robots.fleet.subtitle', { n: robots.length })}>
      {#snippet actions()}
        {#if data?.overview?.unseen}
          <Button loading={acking} onclick={ack}>{$t('robots.ackAll', { n: data.overview.unseen })}</Button>
        {/if}
        <Button variant="primary" icon="plus" onclick={() => goto('/robots/new')}>{$t('robots.fleet.new')}</Button>
      {/snippet}
    </PageHeader>

    {#if error}
      <div class="notice" role="alert"><Icon name="alert-triangle" size={14} /><span>{error}</span></div>
    {/if}

    {#if data && robots.length}
      <div class="stats">
        <div class="tile">
          <span class="label">{$t('robots.fleet.robots')}</span>
          <span class="value">{robots.length}</span>
          <span class="hint">{$t('robots.fleet.activeCount', { n: data.overview?.active ?? 0 })}</span>
          <span class="dots" aria-hidden="true">
            {#each robots as r (r.id)}<i class:on={r.status === 'active'} title={r.name}></i>{/each}
          </span>
        </div>
        <div class="tile">
          <span class="label">{$t('robots.fleet.equity')}</span>
          <span class="value">{money(totals.equity)}</span>
          <span class="split" aria-hidden="true">
            {#each split as p (p.id)}<i class={p.tone} style:width={`${p.pct}%`} title={`${p.name} ${money(p.v)}`}></i>{/each}
          </span>
        </div>
        <div class="tile">
          <span class="label">{$t('robots.fleet.today')}</span>
          <span class="value" class:pos={totals.today > 0} class:neg={totals.today < 0}>{signed(totals.today)}</span>
          {#if data.overview?.unseen}<span class="hint">{$t('robots.fleet.unseen')} <span class="mono">{data.overview.unseen}</span></span>{/if}
        </div>
        <div class="tile">
          <span class="label">{$t('robots.fleet.open')}</span>
          <span class="value">{totals.open}</span>
          <span class="hint"><span class="mono">{totals.inMarket}</span> / <span class="mono">{robots.length}</span> {$t('robots.fleet.robots').toLowerCase()}</span>
        </div>
      </div>

      <section class="perf">
        <div class="perf-bar">
          <h3>{$t('robots.fleet.performance')}</h3>
          <div class="env"><Select options={envOptions} bind:value={env} /></div>
          <div class="date"><DatePicker bind:value={from} max={to} placeholder={$t('robots.fleet.from')} ariaLabel={$t('robots.fleet.from')} /></div>
          <div class="date"><DatePicker bind:value={to} min={from} placeholder={$t('robots.fleet.to')} ariaLabel={$t('robots.fleet.to')} /></div>
        </div>
        <div class="perf-figs">
          <div class="fig">
            <span class="label">{$t('robots.fleet.pnl')}</span>
            <span class="value" class:pos={pnl.total > 0} class:neg={pnl.total < 0}>{signed(pnl.total)}</span>
          </div>
          <div class="fig">
            <span class="label">{$t('robots.fleet.realized')}</span>
            <span class="value" class:pos={pnl.realized > 0} class:neg={pnl.realized < 0}>{signed(pnl.realized)}</span>
          </div>
          <div class="fig">
            <span class="label">{$t('robots.fleet.unrealized')}</span>
            <span class="value" class:pos={pnl.unrealized > 0} class:neg={pnl.unrealized < 0}>{pnl.unrealized === null ? '-' : signed(pnl.unrealized)}</span>
          </div>
        </div>
        <div class="curve"><EquityChart points={pnl.points} currency={CCY || 'USD'} /></div>
      </section>

      <div class="panels">
        <section class="panel">
          <h3>
            <Icon name="trending-up" size={13} />{$t('robots.fleet.ranking')}
            <span class="periods" role="group" aria-label={$t('robots.fleet.rankingHint')}>
              {#each RANK_PERIODS as p (p)}
                <button type="button" class:on={period === p} aria-pressed={period === p} onclick={() => (period = p)}>{$t(`robots.fleet.period.${p}`)}</button>
              {/each}
            </span>
          </h3>
          {#if ranking.some((r) => r.trades > 0)}
            <ol class="rank">
              {#each ranking as r, i (r.id)}
                <li>
                  <span class="mono pos-n">{i + 1}</span>
                  <a href={`/robots/${r.id}`}>{r.name}</a>
                  <span class="bar" aria-hidden="true">
                    {#if rankMax > 0 && r.pnl !== 0}<i class:up={r.pnl > 0} class:down={r.pnl < 0} style:width={`${(Math.abs(r.pnl) / rankMax) * 50}%`}></i>{/if}
                  </span>
                  <span class="mono amt" class:pos={r.pnl > 0} class:neg={r.pnl < 0}>{signed(r.pnl)}{r.currency ? ` ${r.currency}` : ''}</span>
                  <span class="mono pct" class:pos={r.pct > 0} class:neg={r.pct < 0}>{r.pct === null ? '-' : pct(r.pct)}</span>
                </li>
              {/each}
            </ol>
          {:else}
            <p class="calm">{$t('robots.fleet.noClosed')}</p>
          {/if}
        </section>
        <section class="panel">
          <h3><Icon name="pie-chart" size={13} />{$t('robots.fleet.exposure')}<span class="sub">{$t('robots.fleet.exposureHint')}</span></h3>
          {#if exposure.length}
            <div class="mix" aria-hidden="true">
              {#each exposure as e, i (e.kind)}<i style:width={`${e.pct}%`} style:background={`var(--chart-${(i % 8) + 1})`} title={`${assetLabel(e.kind)} ${e.pct.toFixed(1)}%`}></i>{/each}
            </div>
            <ul>
              {#each exposure as e, i (e.kind)}
                <li>
                  <i class="swatch" style:background={`var(--chart-${(i % 8) + 1})`}></i>
                  <span class="kind">{assetLabel(e.kind)}</span>
                  <span class="why">{e.robots} {$t('robots.fleet.robots').toLowerCase()}</span>
                  <span class="mono amt">{money(e.value)}</span>
                  <span class="mono pct">{e.pct.toLocaleString(undefined, { maximumFractionDigits: 1 })}%</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="calm">{$t('robots.fleet.noExposure')}</p>
          {/if}
        </section>
      </div>

      <div class="filters">
        <div class="chips" role="group" aria-label={$t('robots.fleet.state')}>
          <button type="button" class:on={status === ''} aria-pressed={status === ''} onclick={() => (status = '')}>
            {$t('robots.fleet.allStates')}<span class="n">{robots.length}</span>
          </button>
          {#each chips as s (s)}
            <button type="button" class:on={status === s} aria-pressed={status === s} onclick={() => (status = status === s ? '' : s)}>
              <i class="dot {stateTone(s)}"></i>{$t(`robots.state.${s}`)}<span class="n">{counts[s]}</span>
            </button>
          {/each}
        </div>
        <div class="account"><Select options={accountOptions} bind:value={account} /></div>
        <span class="count">{shown.length} / {robots.length}</span>
      </div>

      {#if shown.length}
        <div class="cards">
          {#each shown as robot (robot.id)}<RobotCard {robot} {now} />{/each}
        </div>
        <div class="table">
          <FleetTable robots={shown} {now} />
        </div>
      {:else}
        <EmptyState icon="search" title={$t('robots.fleet.noMatch')} compact />
      {/if}

      <div class="panels">
        <section class="panel">
          <h3><Icon name="alert-triangle" size={13} />{$t('robots.fleet.attention')}{#if attention.length}<span class="n">{attention.length}</span>{/if}</h3>
          {#if attention.length}
            <ul>
              {#each attention as r (r.id)}
                <li>
                  <a href={`/robots/${r.id}`}>{r.name}</a>
                  <span class="why" class:bad={!!r.last_error} class:warn={!r.last_error && !!r.guard_holds}>
                    {#if r.last_error}{r.last_error}{:else if r.guard_holds}{r.guard_holds}{:else}{$t('robots.fleet.unseen')}{/if}
                  </span>
                  {#if r.unseen > 0}<span class="mono tag">{r.unseen}</span>{/if}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="calm">{$t('robots.fleet.allCalm')}</p>
          {/if}
        </section>
        <section class="panel">
          <h3><Icon name="clock" size={13} />{$t('robots.fleet.upcoming')}</h3>
          {#if upcoming.length}
            <ul>
              {#each upcoming as r (r.id)}
                <li>
                  <i class="dot {stateTone(stateOf(r))}"></i>
                  <a href={`/robots/${r.id}`}>{r.name}</a>
                  <span class="why">{(r.tickers ?? []).join(', ')}</span>
                  <span class="mono when">{countdown(r.next_run_at, now) === 'due' ? $t('robots.due') : countdown(r.next_run_at, now)}</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="calm">{$t('robots.fleet.noUpcoming')}</p>
          {/if}
        </section>
      </div>
    {:else if data}
      <EmptyState icon="bot" title={$t('robots.fleet.empty')} description={$t('robots.fleet.emptyHint')}>
        {#snippet action()}
          <Button variant="primary" icon="plus" onclick={() => goto('/robots/new')}>{$t('robots.fleet.new')}</Button>
        {/snippet}
      </EmptyState>
    {/if}
  </div>
</RequireModule>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  .mono {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--dim);
    flex: none;
  }
  .ok {
    background: var(--green);
  }
  .warn {
    background: var(--amber);
  }
  .bad {
    background: var(--red);
  }

  /* Key figures: the 10px label, the 28px mono value, and under it what the fleet is made
     of (one mark per robot in its state tone, the equity split by robot). */
  .stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
    padding: var(--pad-metric);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
  }
  .label {
    color: var(--dim);
    font-size: var(--fs-metric-label);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .value {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xl);
    font-weight: var(--fw-normal);
    line-height: var(--lh-tight);
    letter-spacing: -0.02em;
    overflow-wrap: anywhere;
  }
  .hint {
    color: var(--dim);
    font-size: var(--fs-desc);
    line-height: var(--lh-tight);
  }
  .dots {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    margin-top: var(--space-2);
  }
  .dots i {
    width: 8px;
    height: 8px;
    background: var(--dim);
  }
  .dots i.on {
    background: var(--green);
    box-shadow: 0 0 6px color-mix(in srgb, var(--green) 70%, transparent);
  }
  .split {
    display: flex;
    gap: 2px;
    height: 6px;
    margin-top: var(--space-3);
    background: var(--surface-2);
  }
  .split i {
    display: block;
    min-width: 2px;
    background: var(--dim);
  }

  /* Fleet P&L: a toolbar (environment, date range), three figures, the summed equity curve. */
  .perf {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
  }
  .perf-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .perf-bar h3 {
    margin: 0 auto 0 0;
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
  }
  .env {
    width: 240px;
  }
  .date {
    width: 180px;
  }
  .perf-figs {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .fig {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .curve {
    height: 280px;
  }

  /* One toolbar row: the states as counted chips on the left, the account on the right,
     the match count in mono, a hairline rule under the row. */
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding-bottom: var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .chips button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    padding: 0 var(--space-3);
    border: var(--hairline) solid transparent;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
  }
  .chips button:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .chips button.on {
    color: var(--text);
    background: var(--surface-2);
    border-color: var(--border-control);
  }
  .chips .n {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--dim);
  }
  .chips button.on .n {
    color: var(--text);
  }
  .account {
    margin-left: auto;
    min-width: 200px;
  }
  .count {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-body);
    color: var(--dim);
  }
  .table {
    border: var(--hairline) solid var(--border);
    background: var(--surface);
  }
  .cards {
    display: none;
    flex-direction: column;
    gap: var(--space-3);
  }

  /* Under the table: what needs a look, and what fires next. */
  .panels {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .panel {
    min-width: 0;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
  }
  .panel h3 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-3) var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
    color: var(--dim);
    font-size: var(--fs-metric-label);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .panel h3 .n {
    font-family: var(--mono);
    color: var(--text);
  }
  .panel ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .panel li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--row-h);
    padding: var(--space-1) var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--text-sm);
  }
  .panel li:last-child {
    border-bottom: 0;
  }
  .panel a {
    color: var(--text);
    font-weight: var(--fw-medium);
    text-decoration: none;
    white-space: nowrap;
  }
  .panel a:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .why {
    flex: 1;
    min-width: 0;
    color: var(--dim);
    font-size: var(--fs-desc);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .why.bad {
    color: var(--red);
    background: none;
  }
  .why.warn {
    color: var(--amber);
    background: none;
  }
  .tag {
    padding: 1px var(--space-2);
    background: var(--surface-2);
    font-size: var(--text-xs);
  }
  .when {
    color: var(--text);
    font-size: var(--text-xs);
  }
  .calm {
    margin: 0;
    padding: var(--space-4);
    color: var(--dim);
    font-size: var(--fs-desc);
    line-height: var(--lh-base);
  }

  /* Ranking: the period as a segmented control in the header, one row per robot with a
     diverging bar around the middle (gains right, losses left), the amount and the return. */
  .panel h3 .sub {
    margin-left: auto;
    color: var(--dim);
    font-size: var(--text-xs);
    text-transform: none;
    letter-spacing: 0;
    font-weight: var(--fw-normal);
  }
  .periods {
    display: inline-flex;
    gap: 2px;
    margin-left: auto;
  }
  .periods button {
    height: 22px;
    padding: 0 var(--space-2);
    border: var(--hairline) solid transparent;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-xs);
    text-transform: none;
    letter-spacing: 0;
    cursor: pointer;
  }
  .periods button:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .periods button.on {
    color: var(--text);
    background: var(--surface-2);
    border-color: var(--border-control);
  }
  .rank {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 360px;
    overflow-y: auto;
  }
  .rank li {
    display: grid;
    grid-template-columns: 20px minmax(0, 1.2fr) minmax(60px, 1fr) auto 64px;
  }
  .rank a {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pos-n {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .bar {
    position: relative;
    height: 6px;
    background: var(--surface-2);
  }
  .bar::after {
    content: '';
    position: absolute;
    left: 50%;
    top: -2px;
    bottom: -2px;
    width: 1px;
    background: var(--border);
  }
  .bar i {
    position: absolute;
    top: 0;
    bottom: 0;
  }
  .bar i.up {
    left: 50%;
    background: var(--green);
  }
  .bar i.down {
    right: 50%;
    background: var(--red);
  }
  .amt {
    text-align: right;
    white-space: nowrap;
  }
  .pct {
    text-align: right;
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .pct.pos {
    color: var(--green);
  }
  .pct.neg {
    color: var(--red);
  }

  /* Exposure: one stacked bar of the open book by asset type, then a row per type. */
  .mix {
    display: flex;
    gap: 2px;
    height: 10px;
    margin: var(--space-3) var(--space-4);
    background: var(--surface-2);
  }
  .mix i {
    display: block;
    min-width: 2px;
  }
  .swatch {
    width: 8px;
    height: 8px;
    flex: none;
  }
  .kind {
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }

  /* Notice: a tint, a hairline in the hue, ink in --text; the icon carries the meaning. */
  .notice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: color-mix(in srgb, var(--red) 10%, transparent);
    border: var(--hairline) solid color-mix(in srgb, var(--red) 45%, transparent);
    color: var(--text);
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  .notice :global(svg) {
    flex: none;
    color: var(--red);
  }
  @media (max-width: 767px) {
    .page {
      min-width: 0;
      overflow-x: clip;
      padding: var(--space-4) var(--space-4) calc(72px + env(safe-area-inset-bottom));
    }
    .stats,
    .panels {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .panels {
      grid-template-columns: minmax(0, 1fr);
    }
    .account {
      margin-left: 0;
      flex: 1 1 100%;
    }
    .cards {
      display: flex;
    }
    .table {
      display: none;
    }
  }
</style>
