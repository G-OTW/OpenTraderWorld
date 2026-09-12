<script>
  // Journal analytics widgets — the cards of the Journal mockup, each one a `variant` of
  // the same component. Nothing here is new backend: the module already serves
  // `breakdown`, `analytics`, `exposure` and `calendar`, and each variant fetches only
  // the one it needs, so a dashboard carrying three journal cards makes three cheap
  // calls rather than one heavy one.
  //
  // Config: { variant, categoryId, since, until, limit }.
  import { journalApi } from '$lib/modules/journal/api.js';
  import { insightText, SEVERITY_ICON } from '$lib/modules/journal/insights.js';
  import {
    fmtMoney, fmtSignedMoney, fmtNum, fmtPct, fmtSignedPct, fmtFixed, fmtTime, fmtCompactValue
  } from '$lib/format';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import Spark from './parts/Spark.svelte';
  import TrendChart from './parts/TrendChart.svelte';
  import Gauge from './parts/Gauge.svelte';
  import Donut from './parts/Donut.svelte';
  import RankBars from './parts/RankBars.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import Buckets from './parts/Buckets.svelte';
  import Heatmap from './parts/Heatmap.svelte';
  import { dateLabel } from './parts/axis.js';
  import { livePulse, LIVE } from '../live.svelte.js';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'pnl');
  const limit = $derived(item.config?.limit ?? 6);
  const group = $derived(item.config?.group ?? 'strategy');
  const filter = $derived({
    category_id: item.config?.categoryId ?? '',
    since: item.config?.since ?? '',
    until: item.config?.until ?? ''
  });

  // Which endpoint each card needs. One source per variant keeps the widget honest about
  // its cost, and a dashboard's journal cards never fan out into the same heavy call.
  const SOURCE = {
    pnl: 'breakdown', winrate: 'breakdown', pf: 'breakdown', equity: 'breakdown',
    expectancy: 'breakdown', bestworst: 'breakdown',
    streak: 'analytics', groups: 'analytics', bygroup: 'analytics',
    behavior: 'analytics', recent: 'analytics',
    positions: 'exposure', exposure: 'exposure', correlation: 'exposure',
    heatmap: 'calendar'
  };
  const source = $derived(SOURCE[variant] ?? 'breakdown');

  const live = livePulse(LIVE.quotes);

  let data = $state.raw(null);
  let err = $state('');

  // Blank to the loading state only when the question changes; a periodic refresh of the
  // same question keeps the numbers on screen until the new ones land.
  let asked = '';
  $effect(() => {
    live.n;
    if (editing) return;
    const f = filter;
    const src = source;
    let alive = true;
    err = '';
    const q = `${src}|${JSON.stringify(f)}`;
    if (q !== asked) {
      asked = q;
      data = null;
    }
    const call =
      src === 'analytics' ? journalApi.analytics(f)
      : src === 'exposure' ? journalApi.exposure(f)
      : src === 'calendar' ? journalApi.calendar(f)
      : journalApi.breakdown(f);
    call
      .then((d) => { if (alive) data = d; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // The book the card is reading. The shell names the metric ("Equity curve"); the card
  // names the account it was pointed at, the way the portfolio cards name their book.
  const NAMED = new Set(['equity', 'heatmap']);
  let categories = $state(null);
  $effect(() => {
    if (editing || !NAMED.has(variant) || categories !== null) return;
    let alive = true;
    journalApi
      .listCategories()
      .then((r) => { if (alive) categories = r; })
      .catch(() => {});
    return () => { alive = false; };
  });
  const accountName = $derived(
    filter.category_id
      ? (categories ?? []).find((c) => String(c.id) === String(filter.category_id))?.name ?? ''
      : $t('journal.page.categories.all')
  );

  const ccy = $derived(data?.display_currency ?? 'USD');
  const money = (v) => (v == null ? '—' : fmtMoney(v, ccy));
  const signed = (v) => (v == null ? '—' : fmtSignedMoney(v, ccy));
  const num = (v) => (v == null ? '—' : fmtNum(v));
  const tone = (v) => (v == null ? '' : v > 0 ? 'pos' : v < 0 ? 'neg' : '');

  // ── breakdown-derived ──
  // Gross win / gross loss are not served as such; they are the exact product of the
  // averages and the counts the breakdown does serve.
  const grossWin = $derived(
    data?.avg_win != null ? data.avg_win * (data.win_count ?? 0) : null
  );
  const grossLoss = $derived(
    data?.avg_loss != null ? Math.abs(data.avg_loss) * (data.loss_count ?? 0) : null
  );
  const curve = $derived((data?.equity_curve ?? []).map((p) => p.cum_pnl));
  const curveLabels = $derived((data?.equity_curve ?? []).map((p) => dateLabel(p.at)));
  // The dated form of the same curve, for the full-card chart.
  const curvePoints = $derived((data?.equity_curve ?? []).map((p) => ({ at: p.at, value: p.cum_pnl })));

  // ── analytics-derived ──
  const groupRows = $derived(data?.groups?.[group] ?? []);
  const recent = $derived([...(data?.points ?? [])].sort((a, b) => b.at - a.at).slice(0, limit));

  const INSIGHT_FMT = {
    fragileEdge: { net: signed, without: signed },
    concentrated: { share: (v) => fmtPct(v), winners: (v) => String(Math.round(v)) },
    sizeEscalation: { escalation: (v) => fmtSignedPct(v), trades: (v) => String(Math.round(v)), net: signed },
    revenge: { trades: (v) => String(Math.round(v)), net: signed, expectancy: signed, normal: signed },
    concentratedTicker: { share: (v) => fmtPct(v), risk: money },
    concentratedClass: { share: (v) => fmtPct(v) },
    thinBook: { effective: num, positions: String },
    correlatedPair: { corr: num },
    stacked: { stacking: num, correlated: money, independent: money },
    diversified: { avg: num }
  };
  const insightPrefix = $derived(source === 'exposure' ? 'journal.exposure.warning' : 'journal.behavior.insight');
  const insights = $derived(
    source === 'exposure' ? (data?.warnings ?? []) : (data?.behavior?.insights ?? [])
  );
  const sentence = (i) => insightText(i, $t, insightPrefix, INSIGHT_FMT[i.key] ?? {}, num);

  // ── calendar-derived ──
  const heatDays = $derived(new Map((data?.days ?? []).map((d) => [d.date, d.net_pnl])));
  const heatTotal = $derived((data?.days ?? []).reduce((s, d) => s + (d.net_pnl ?? 0), 0));

  const isEmpty = $derived.by(() => {
    if (!data) return false;
    if (source === 'analytics') return (data.closed_count ?? 0) === 0 && (data.open_count ?? 0) === 0;
    if (source === 'exposure') return (data.open_count ?? 0) === 0;
    if (source === 'calendar') return (data.days ?? []).length === 0;
    return (data.trade_count ?? 0) === 0;
  });
</script>

<WidgetState
  {editing}
  error={err}
  loading={data === null}
  empty={isEmpty}
  preview={$t('dashboard.widgets.journalStats.preview')}
  emptyText={$t('dashboard.widgets.journalStats.empty')}
  rows={4}
>
  {#if variant === 'pnl'}
    <div class="split">
      <Stat
        value={signed(data.realized_pnl)}
        tone={tone(data.realized_pnl)}
        delta={data.return_pct == null ? '' : fmtSignedPct(data.return_pct)}
        deltaTone={tone(data.return_pct)}
      />
      {#if curve.length > 1}
        <div class="sparkbox">
          <Spark values={curve} labels={curveLabels} height={44} valueFormat={signed}
            label={$t('dashboard.widgets.journalStats.cumulative')} />
        </div>
      {/if}
    </div>
  {:else if variant === 'winrate'}
    <div class="split">
      <Stat
        value={data.win_rate == null ? '—' : fmtPct(data.win_rate, 1)}
        note={$t('dashboard.widgets.journalStats.winsLosses', { wins: data.win_count, losses: data.loss_count })}
      />
      <Gauge
        percent={data.win_rate ?? 0}
        size={62}
        thickness={10}
        tone="pos"
        label={$t('dashboard.widgets.journalStats.winsLosses', { wins: data.win_count, losses: data.loss_count })}
      />
    </div>
  {:else if variant === 'pf'}
    <div class="split">
      <div class="pfleft">
        <Stat value={data.profit_factor == null ? '—' : fmtFixed(data.profit_factor, 2)} size="lg" />
        <dl class="gross">
          <div><dt>{$t('dashboard.widgets.journalStats.grossWin')}</dt><dd class="w-pos">{money(grossWin)}</dd></div>
          <div><dt>{$t('dashboard.widgets.journalStats.grossLoss')}</dt><dd class="w-neg">{money(grossLoss)}</dd></div>
        </dl>
      </div>
      <div class="sparkbox">
        <MiniBars
          values={[grossWin ?? 0, -(grossLoss ?? 0)]}
          labels={[$t('dashboard.widgets.journalStats.grossWin'), $t('dashboard.widgets.journalStats.grossLoss')]}
          tone="signed"
          height={44}
          valueFormat={signed}
          label={$t('dashboard.widgets.journalStats.pnl')}
        />
      </div>
    </div>
  {:else if variant === 'equity'}
    <TrendChart
      points={curvePoints}
      valueLabel={accountName || $t('dashboard.widgets.journalStats.cumulative')}
      format={signed}
      axisFormat={(v) => fmtCompactValue(v, ccy)}
      changeFormat={signed}
      defaultRange="3m"
      label={$t('dashboard.widgets.journalStats.equity')}
    />
  {:else if variant === 'expectancy'}
    <div class="w-body">
      <Stat value={signed(data.expectancy)} tone={tone(data.expectancy)} size="lg" />
      <Buckets
        cells={[
          { value: money(data.avg_win), label: $t('dashboard.widgets.journalStats.avgWin'), tone: 'pos' },
          { value: money(data.avg_loss), label: $t('dashboard.widgets.journalStats.avgLoss'), tone: 'neg' }
        ]}
      />
    </div>
  {:else if variant === 'bestworst'}
    <Buckets
      cells={[
        { value: signed(data.best_trade), label: $t('dashboard.widgets.journalStats.bestTrade'), tone: 'pos' },
        { value: signed(data.worst_trade), label: $t('dashboard.widgets.journalStats.worstTrade'), tone: 'neg' }
      ]}
    />
  {:else if variant === 'streak'}
    <Buckets
      cells={[
        { value: data.streaks.best_win, label: $t('dashboard.widgets.journalStats.winStreak'), tone: 'pos',
          sub: signed(data.streaks.best_win_net) },
        { value: data.streaks.worst_loss, label: $t('dashboard.widgets.journalStats.lossStreak'), tone: 'neg',
          sub: signed(data.streaks.worst_loss_net) }
      ]}
    />
  {:else if variant === 'groups'}
    <Donut
      segments={groupRows.slice(0, 6).map((r) => ({ label: r.name || '—', value: r.trades }))}
      centerLabel={$t('dashboard.widgets.journalStats.trades')}
    />
  {:else if variant === 'bygroup'}
    <RankBars
      rows={groupRows.slice(0, limit).map((r) => ({ label: r.name || '—', value: r.net, display: signed(r.net) }))}
      signed
    />
  {:else if variant === 'behavior' || variant === 'correlation'}
    {#if insights.length === 0}
      <p class="w-state">{$t('dashboard.widgets.journalStats.noInsights')}</p>
    {:else}
      <ul class="w-list">
        {#each (variant === 'correlation' ? insights.slice(0, 1) : insights.slice(0, limit)) as i (i.key)}
          <li class="w-row ins {i.severity}">
            <Icon name={SEVERITY_ICON[i.severity] ?? 'lightbulb'} size={15} />
            <span class="w-clamp">{sentence(i)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if variant === 'positions'}
    <table class="w-tbl">
      <thead>
        <tr>
          <th>{$t('dashboard.widgets.journalStats.symbol')}</th>
          <th class="num">{$t('dashboard.widgets.journalStats.qty')}</th>
          <th class="num">{$t('dashboard.widgets.journalStats.entry')}</th>
          <th class="num">{$t('dashboard.widgets.journalStats.price')}</th>
          <th class="num">{$t('dashboard.widgets.journalStats.unrealized')}</th>
        </tr>
      </thead>
      <tbody>
        {#each data.positions.slice(0, limit) as p (p.id)}
          <tr>
            <td class="w-name">{p.ticker}</td>
            <td class="num">{fmtNum(p.open_qty, 0)}</td>
            <td class="num">{p.avg_entry == null ? '—' : fmtNum(p.avg_entry)}</td>
            <td class="num">{p.last_price == null ? '—' : fmtNum(p.last_price)}</td>
            <td class="num {tone(p.unrealized)}">{signed(p.unrealized)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else if variant === 'exposure'}
    <Buckets
      cells={[
        { value: data.herfindahl == null ? '—' : fmtFixed(data.herfindahl, 2), label: 'HHI' },
        { value: data.effective_positions == null ? '—' : fmtFixed(data.effective_positions, 1),
          label: $t('dashboard.widgets.journalStats.effectivePositions') }
      ]}
    />
  {:else if variant === 'recent'}
    <table class="w-tbl">
      <thead>
        <tr>
          <th>{$t('dashboard.widgets.journalStats.time')}</th>
          <th>{$t('dashboard.widgets.journalStats.symbol')}</th>
          <th>{$t('dashboard.widgets.journalStats.side')}</th>
          <th class="num">{$t('dashboard.widgets.journalStats.qty')}</th>
          <th class="num">{$t('dashboard.widgets.journalStats.pnl')}</th>
        </tr>
      </thead>
      <tbody>
        {#each recent as p (p.id)}
          <tr>
            <td class="w-sub">{fmtTime(p.at)}</td>
            <td class="w-name">{p.ticker}</td>
            <td><span class="w-pill" class:pos={p.side === 'long'} class:neg={p.side === 'short'}>
              {$t(`journal.side.${p.side}`)}</span></td>
            <td class="num">{fmtNum(p.qty, 0)}</td>
            <td class="num {tone(p.net)}">{signed(p.net)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else if variant === 'heatmap'}
    <div class="w-body">
      <div class="eqhead">
        <span class="w-eyebrow w-name">{accountName || $t('dashboard.widgets.journalStats.dailyPnl')}</span>
        <span class="w-num {tone(heatTotal)}">{signed(heatTotal)}</span>
      </div>
      <Heatmap days={heatDays} weeks={item.config?.weeks ?? 53} valueFormat={signed}
        label={$t('dashboard.widgets.journalStats.dailyPnl')} />
    </div>
  {/if}
</WidgetState>

<style>
  .split {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-width: 0;
  }
  .split > :global(:first-child) {
    min-width: 0;
  }
  .sparkbox {
    flex: 1;
    max-width: 55%;
    min-width: 0;
  }
  .pfleft {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .gross {
    display: flex;
    gap: var(--space-4);
    margin: 0;
  }
  .gross div {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .gross dt {
    font-size: 10px;
    color: var(--dim);
  }
  .gross dd {
    margin: 0;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xs);
  }
  .eqhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  .ins {
    align-items: flex-start;
  }
  .ins :global(svg) {
    margin-top: 2px;
    flex-shrink: 0;
    color: var(--dim);
  }
  .ins.warn :global(svg) {
    color: var(--amber);
  }
  .ins.good :global(svg) {
    color: var(--green);
  }
</style>
