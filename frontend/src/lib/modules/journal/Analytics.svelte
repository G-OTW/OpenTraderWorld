<script>
  // The analytics screen: what the raw breakdown cannot say. Seven questions, seven tabs.
  //
  //   overview      — is the edge real, and how much risk buys it (R, streaks, ratios)
  //   distributions — when and how big does it work (hold time, hour, weekday, size, PnL)
  //   behavior      — what the trader does around the edge, and what it costs
  //   market        — what the candles say (MAE/MFE, stops, targets, regime)
  //   exposure      — what is still open right now, and how correlated it is
  //   scatter       — does one variable move with another, one dot per trade
  //   groups        — which strategy / symbol / tag actually pays
  //   compare       — is this period better than the last
  //
  // Everything here is derived from logged trades. R-multiples need the trade's own
  // planned stop, so a journal logged without stops still gets every other measure.
  import { journalApi, fmtMoney, fmtSignedMoney, fmtPct, fmtNum, ASSET_CLASSES } from './api.js';
  import FilterBar from './FilterBar.svelte';
  import DistChart from './DistChart.svelte';
  import { pnlBuckets, autoStep } from './pnldist.js';
  import ScatterPanel from './ScatterPanel.svelte';
  import BehaviorPanel from './BehaviorPanel.svelte';
  import MarketPanel from './MarketPanel.svelte';
  import MarketBar from './MarketBar.svelte';
  import ExposurePanel from './ExposurePanel.svelte';
  import GroupTable from './GroupTable.svelte';
  import ComparePanel from './ComparePanel.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let {
    categoryId = '',
    strategies = [],
    tags = [],
    suggestions = { tickers: [], exchanges: [], signals: [] },
    displayCurrency = 'USD'
  } = $props();

  let filter = $state({});
  // `$state.raw`: the payload is replaced wholesale, never mutated, and it carries one
  // row per closed trade. Deep-proxying that array costs a proxy per trade the moment
  // the scatter reads it, for no reactivity anyone uses.
  let an = $state.raw(null);
  let loading = $state(true);
  let tab = $state('overview');

  $effect(() => {
    const f = filter;
    loading = true;
    journalApi
      .analytics(f)
      .then((d) => {
        an = d;
      })
      .finally(() => {
        loading = false;
      });
  });

  /** Re-fetch after the market-data pass wrote new measurements. */
  function reload() {
    journalApi.analytics(filter).then((d) => {
      an = d;
    });
  }

  // The scatter plots one row per trade, and a measured trade carries market axes too.
  // Merging by id here keeps `points` the single per-trade array: the axis catalogue in
  // `scatter.js` reads `p.mae_r` on a measured trade and `null` on every other one.
  const plotPoints = $derived.by(() => {
    const pts = an?.points ?? [];
    const mk = an?.market?.points;
    if (!mk || mk.length === 0) return pts;
    const by = new Map(mk.map((p) => [p.id, p]));
    return pts.map((p) => (by.has(p.id) ? { ...p, ...by.get(p.id) } : p));
  });

  const money = (n) => fmtMoney(n, displayCurrency);
  const signedMoney = (n) => (n == null ? '—' : fmtSignedMoney(n, displayCurrency));
  const r = (v) => (v == null ? '—' : `${v > 0 ? '+' : ''}${fmtNum(v)}R`);
  const pct = (v) => (v == null ? '—' : fmtPct(v));

  // ── Overview tiles ──
  // Ordered as a reading path: what a unit of risk returns, how the account survives it,
  // then the behavioural shape (streaks, days).
  const tiles = $derived.by(() => {
    if (!an) return [];
    const s = an.streaks;
    return [
      {
        label: $t('journal.analytics.stat.avgR'),
        value: r(an.r.avg_r),
        tone: an.r.avg_r,
        sub: $t('journal.analytics.stat.avgRSub', { count: an.r.with_stop })
      },
      {
        label: $t('journal.analytics.stat.totalR'),
        value: r(an.r.total_r),
        tone: an.r.total_r,
        sub: $t('journal.analytics.stat.totalRSub', {
          win: r(an.r.avg_win_r),
          loss: r(an.r.avg_loss_r)
        })
      },
      {
        label: $t('journal.analytics.stat.riskPerTrade'),
        value: pct(an.r.avg_risk_pct),
        sub: $t('journal.analytics.stat.riskPerTradeSub', { count: an.r.without_stop })
      },
      {
        label: $t('journal.analytics.stat.sharpe'),
        value: fmtNum(an.daily.sharpe),
        tone: an.daily.sharpe,
        sub: $t('journal.analytics.stat.sharpeSub', { sortino: fmtNum(an.daily.sortino) })
      },
      {
        label: $t('journal.analytics.stat.maxDrawdown'),
        value: pct(an.daily.max_drawdown),
        tone: -1,
        sub: $t('journal.analytics.stat.maxDrawdownSub', { days: an.daily.max_drawdown_days })
      },
      {
        label: $t('journal.analytics.stat.tradingDays'),
        value: String(an.daily.days),
        sub: $t('journal.analytics.stat.tradingDaysSub', {
          win: an.daily.win_days,
          loss: an.daily.loss_days
        })
      },
      {
        label: $t('journal.analytics.stat.avgDay'),
        value: signedMoney(an.daily.avg_day),
        tone: an.daily.avg_day,
        sub: $t('journal.analytics.stat.avgDaySub', {
          best: signedMoney(an.daily.best_day),
          worst: signedMoney(an.daily.worst_day)
        })
      },
      {
        label: $t('journal.analytics.stat.streak'),
        value:
          s.current === 0
            ? '—'
            : $t(s.current > 0 ? 'journal.analytics.stat.streakWins' : 'journal.analytics.stat.streakLosses', {
                count: Math.abs(s.current)
              }),
        tone: s.current,
        sub: $t('journal.analytics.stat.streakSub', { best: s.best_win, worst: s.worst_loss })
      }
    ];
  });

  function toneClass(v) {
    if (v === undefined || v === null) return '';
    return v > 0 ? 'pos' : v < 0 ? 'neg' : '';
  }
  const isEmptyVal = (v) => v === '—' || v == null || v === '';

  // ── Distribution labels ──
  const R_LABEL = {
    lt3: '< −3R',
    m3m2: '−3…−2R',
    m2m1: '−2…−1R',
    m1z: '−1…0R',
    z1: '0…1R',
    r1r2: '1…2R',
    r2r3: '2…3R',
    gt3: '> 3R'
  };
  const rLabel = (b) => R_LABEL[b.key] ?? b.key;
  const holdLabel = (b) => $t(`journal.analytics.hold.${b.key}`);
  const hourLabel = (b) => `${b.key}h`;
  // Weekday names come from the browser: the journal follows the viewer's locale here,
  // exactly like the calendar heatmap does.
  const WEEKDAYS = Array.from({ length: 7 }, (_, i) =>
    // 2024-01-01 was a Monday, so +i walks Monday → Sunday.
    new Date(2024, 0, 1 + i).toLocaleDateString(undefined, { weekday: 'short' })
  );
  const weekdayLabel = (b) => WEEKDAYS[Number(b.key)] ?? b.key;
  const sizeLabel = (b) =>
    b.hi == null ? `≥ ${money(b.lo)}` : `${money(b.lo)}–${money(b.hi)}`;

  // ── PnL per trade ──
  // The bucket width is the reader's: the server sends the raw per-trade PnL, so a
  // scale change is a re-derive, not a request. 0 = the automatic graduation.
  const SCALES = [0, 1, 5, 10, 50, 100, 500, 1000];
  let scale = $state(0);
  // `points` is the per-trade payload the scatter plots; the histogram only needs the
  // PnL column, sorted (the bucketing reads quantiles off it).
  const pnlValues = $derived((an?.points ?? []).map((p) => p.net).sort((a, b) => a - b));
  const pnlStep = $derived(scale > 0 ? scale : autoStep(pnlValues));
  const pnlBars = $derived(pnlBuckets(pnlValues, pnlStep));
  // A long histogram cannot label every bar without the ticks colliding.
  const pnlTickEvery = $derived(Math.max(1, Math.ceil(pnlBars.length / 18)));

  // Axis number: no currency symbol (the card header carries it) and short enough to
  // sit under an 18 px column.
  function axisNum(v) {
    const a = Math.abs(v);
    const sign = v < 0 ? '−' : '';
    if (a >= 1e6) return `${sign}${+(a / 1e6).toFixed(1)}M`;
    if (a >= 1e3) return `${sign}${+(a / 1e3).toFixed(1)}k`;
    return `${sign}${+a.toFixed(a > 0 && a < 1 ? 2 : 0)}`;
  }
  const pnlLabel = (b) =>
    b.lo == null
      ? `≤ ${axisNum(b.hi)}`
      : b.hi == null
        ? `≥ ${axisNum(b.lo)}`
        : axisNum(b.lo);

  const assetLabel = (row) =>
    ASSET_CLASSES.find((a) => a.id === row.key)?.label ?? row.name;
  const sideLabel = (row) => $t(`journal.side.${row.key}`) ?? row.name;

  const TABS = $derived([
    { id: 'overview', label: $t('journal.analytics.tab.overview') },
    { id: 'distributions', label: $t('journal.analytics.tab.distributions') },
    { id: 'behavior', label: $t('journal.analytics.tab.behavior') },
    { id: 'market', label: $t('journal.analytics.tab.market') },
    { id: 'exposure', label: $t('journal.analytics.tab.exposure') },
    { id: 'scatter', label: $t('journal.analytics.tab.scatter') },
    { id: 'groups', label: $t('journal.analytics.tab.groups') },
    { id: 'compare', label: $t('journal.analytics.tab.compare') }
  ]);
</script>

<div class="analytics">
  <FilterBar
    {categoryId}
    {strategies}
    {tags}
    {suggestions}
    storageKey="otw.journal.analytics.filters.v1"
    idPrefix="an"
    bind:value={filter}
  />

  <MarketBar {filter} onmeasured={reload} />

  <Tabs tabs={TABS} bind:value={tab} ariaLabel={$t('journal.analytics.tabsLabel')} />

  {#if loading && !an}
    <Skeleton height="420px" />
  {:else if an && an.closed_count === 0 && tab !== 'compare' && tab !== 'exposure'}
    <EmptyState
      icon="bar-chart"
      title={$t('journal.analytics.empty.title')}
      description={$t('journal.analytics.empty.description')}
    />
  {:else if an}
    {#if an.unconverted_trades > 0}
      <div class="warn">
        <Icon name="alert-triangle" size={13} />
        {$t('journal.analytics.unconverted', {
          count: an.unconverted_trades,
          currency: displayCurrency
        })}
      </div>
    {/if}

    {#if tab === 'overview'}
      <section class="grid">
        {#each tiles as s (s.label)}
          <div class="stat">
            <span class="stat-label">{s.label}</span>
            <span class="stat-value num {toneClass(s.tone)}" class:is-empty={isEmptyVal(s.value)}>
              {s.value}
            </span>
            {#if s.sub}<span class="stat-sub">{s.sub}</span>{/if}
          </div>
        {/each}
      </section>

      <section class="card">
        <h3>
          {$t('journal.analytics.rHistogram.title')}
          <span class="cur">{$t('journal.analytics.rHistogram.hint')}</span>
        </h3>
        {#if an.r.with_stop === 0}
          <EmptyState
            compact
            icon="target"
            title={$t('journal.analytics.rHistogram.noStops')}
            description={$t('journal.analytics.rHistogram.noStopsHint')}
          />
        {:else}
          <DistChart buckets={an.r.buckets} currency={displayCurrency} label={rLabel} />
        {/if}
      </section>

      <section class="card mistakes">
        <h3>{$t('journal.analytics.mistakes.title')}</h3>
        {#if an.mistakes.tagged === 0}
          <EmptyState
            compact
            icon="tag"
            title={$t('journal.analytics.mistakes.none')}
            description={$t('journal.analytics.mistakes.noneHint')}
          />
        {:else}
          <p class="sentence">
            {$t('journal.analytics.mistakes.sentence', {
              tagged: an.mistakes.tagged,
              closed: an.closed_count,
              cost: money(Math.abs(an.mistakes.cost ?? 0)),
              tagged_exp: signedMoney(an.mistakes.tagged_expectancy),
              clean_exp: signedMoney(an.mistakes.clean_expectancy)
            })}
          </p>
          <div class="mgrid">
            <div class="stat">
              <span class="stat-label">{$t('journal.analytics.mistakes.cost')}</span>
              <span class="stat-value num" class:neg={(an.mistakes.cost ?? 0) > 0} class:pos={(an.mistakes.cost ?? 0) < 0}>
                {money(Math.abs(an.mistakes.cost ?? 0))}
              </span>
            </div>
            <div class="stat">
              <span class="stat-label">{$t('journal.analytics.mistakes.adherence')}</span>
              <span class="stat-value num">{pct(an.mistakes.adherence)}</span>
            </div>
            <div class="stat">
              <span class="stat-label">{$t('journal.analytics.mistakes.taggedNet')}</span>
              <span class="stat-value num {toneClass(an.mistakes.tagged_net)}">
                {signedMoney(an.mistakes.tagged_net)}
              </span>
            </div>
            <div class="stat">
              <span class="stat-label">{$t('journal.analytics.mistakes.cleanNet')}</span>
              <span class="stat-value num {toneClass(an.mistakes.clean_net)}">
                {signedMoney(an.mistakes.clean_net)}
              </span>
            </div>
          </div>
          <GroupTable
            rows={an.groups.tag.filter((g) => g.kind === 'mistake')}
            currency={displayCurrency}
            emptyTitle={$t('journal.analytics.mistakes.none')}
          />
        {/if}
      </section>
    {:else if tab === 'distributions'}
      <section class="card">
        <h3>
          {$t('journal.analytics.dist.hold')}
          <span class="cur">{$t('journal.analytics.dist.holdHint')}</span>
        </h3>
        <DistChart buckets={an.distributions.hold} currency={displayCurrency} label={holdLabel} />
      </section>
      <section class="card">
        <h3>
          {$t('journal.analytics.dist.hour')}
          <span class="cur">{$t('journal.analytics.dist.hourHint')}</span>
        </h3>
        <DistChart
          buckets={an.distributions.hour}
          currency={displayCurrency}
          label={hourLabel}
          compact
        />
      </section>
      <section class="card">
        <h3>{$t('journal.analytics.dist.weekday')}</h3>
        <DistChart
          buckets={an.distributions.weekday}
          currency={displayCurrency}
          label={weekdayLabel}
        />
      </section>
      <section class="card">
        <h3>
          {$t('journal.analytics.dist.size')}
          <span class="cur">{$t('journal.analytics.dist.sizeHint')}</span>
        </h3>
        <DistChart buckets={an.distributions.size} currency={displayCurrency} label={sizeLabel} />
      </section>
      <section class="card">
        <h3 class="row">
          <span>
            {$t('journal.analytics.dist.pnl')}
            <span class="cur">
              {$t('journal.analytics.dist.pnlHint', { currency: displayCurrency })}
            </span>
          </span>
          <span class="scale" role="group" aria-label={$t('journal.analytics.dist.scale')}>
            <span class="scale-label">{$t('journal.analytics.dist.scale')}</span>
            {#each SCALES as s (s)}
              <button
                type="button"
                class="chip"
                class:on={scale === s}
                onclick={() => (scale = s)}
              >
                {s === 0 ? $t('journal.analytics.dist.scaleAuto') : axisNum(s)}
              </button>
            {/each}
          </span>
        </h3>
        <DistChart
          buckets={pnlBars}
          currency={displayCurrency}
          label={pnlLabel}
          metric="trades"
          tickEvery={pnlTickEvery}
          compact={pnlBars.length > 24}
        />
      </section>
    {:else if tab === 'behavior'}
      <BehaviorPanel behavior={an.behavior} points={an.points} currency={displayCurrency} />
    {:else if tab === 'market'}
      <MarketPanel market={an.market} currency={displayCurrency} />
    {:else if tab === 'exposure'}
      <ExposurePanel {filter} {displayCurrency} />
    {:else if tab === 'scatter'}
      <ScatterPanel points={plotPoints} currency={displayCurrency} />
    {:else if tab === 'groups'}
      <section class="card">
        <h3>{$t('journal.analytics.group.byStrategy')}</h3>
        <GroupTable rows={an.groups.strategy} currency={displayCurrency} />
      </section>
      <section class="card">
        <h3>{$t('journal.analytics.group.byTicker')}</h3>
        <GroupTable rows={an.groups.ticker} currency={displayCurrency} limit={10} />
      </section>
      <section class="card">
        <h3>{$t('journal.analytics.group.byTag')}</h3>
        <GroupTable
          rows={an.groups.tag}
          currency={displayCurrency}
          emptyTitle={$t('journal.analytics.group.noTags')}
        />
      </section>
      <div class="two">
        <section class="card">
          <h3>{$t('journal.analytics.group.byAssetClass')}</h3>
          <GroupTable rows={an.groups.asset_class} currency={displayCurrency} nameOf={assetLabel} />
        </section>
        <section class="card">
          <h3>{$t('journal.analytics.group.bySide')}</h3>
          <GroupTable rows={an.groups.side} currency={displayCurrency} nameOf={sideLabel} />
        </section>
      </div>
    {:else if tab === 'compare'}
      <ComparePanel {filter} {displayCurrency} />
    {/if}
  {/if}
</div>

<style>
  .analytics {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-4);
  }
  .card h3 {
    font-size: 12.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.03em;
    margin-bottom: var(--space-4);
  }
  .cur {
    color: var(--dim);
    font-weight: var(--fw-normal);
    font-size: 11px;
  }
  .card h3.row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  /* Graduation picker: one frame per control, so the chips carry their own hairline
     and nothing wraps them. */
  .scale {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .scale-label {
    color: var(--dim);
    font-weight: var(--fw-normal);
    font-size: 11px;
    margin-right: var(--space-1);
  }
  .chip {
    background: transparent;
    border: 0.5px solid var(--border);
    color: var(--muted);
    font-family: var(--mono);
    font-size: 10.5px;
    line-height: 1;
    padding: 3px 6px;
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text);
    border-color: var(--accent);
  }
  .chip.on {
    color: var(--text);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: var(--space-6);
  }
  /* Continuous filet grid, same shape as the breakdown's stat grid. */
  .grid,
  .mgrid {
    display: grid;
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
  }
  .grid {
    grid-template-columns: repeat(4, 1fr);
  }
  .mgrid {
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    margin-bottom: var(--space-4);
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--bg);
    padding: var(--pad-metric);
  }
  .stat-label {
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--fw-normal);
    color: var(--dim);
  }
  .stat-value {
    font-family: var(--mono);
    font-size: var(--fs-metric-value);
    font-weight: var(--fw-normal);
    color: var(--text);
    line-height: var(--lh-tight);
  }
  /* The tile's second line: the operands behind the headline figure. */
  .stat-sub {
    font-size: 10.5px;
    color: var(--faint);
    line-height: var(--lh-tight);
  }
  .stat-value.pos {
    color: var(--green);
  }
  .stat-value.neg {
    color: var(--red);
  }
  .stat-value.is-empty {
    color: var(--faint);
  }
  .sentence {
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: var(--lh-base);
    margin-bottom: var(--space-4);
  }
  .warn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    border: 0.5px solid color-mix(in srgb, var(--amber) 45%, transparent);
    color: var(--text);
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-sm);
  }
  @media (max-width: 860px) {
    .grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
