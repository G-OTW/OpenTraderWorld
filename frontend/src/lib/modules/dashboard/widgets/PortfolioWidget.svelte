<script>
  // Portfolio widgets — the cards of the Portfolio mockup, each a `variant` of one
  // component. Two sources only: `list()` for anything about portfolios as a set, and
  // `detail(id)` for anything inside one book. The Portfolios module itself is untouched.
  //
  // Config: { variant, portfolio_id, limit }. Without a portfolio_id the first one wins,
  // which is what a single-portfolio user expects.
  import { portfoliosApi, fmtMoney, fmtPct, gainPct } from '$lib/modules/portfolios/api.js';
  import { fmtSignedMoney, fmtSignedPct, fmtCompactMoney, fmtCompactValue, fmtNum } from '$lib/format';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import Spark from './parts/Spark.svelte';
  import TrendChart from './parts/TrendChart.svelte';
  import Donut from './parts/Donut.svelte';
  import RankBars from './parts/RankBars.svelte';
  import { livePulse, LIVE } from '../live.svelte.js';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'summary');
  const wanted = $derived(item.config?.portfolio_id || null);
  const limit = $derived(item.config?.limit ?? 6);

  // Cards about one book need the ledger walk; the rest ride on the cheap list call.
  const needsBook = $derived(
    ['cash', 'income', 'evolution', 'allocation', 'positions', 'movers', 'best', 'worst'].includes(variant)
  );

  const live = livePulse(LIVE.quotes);

  let list = $state(null);
  let book = $state.raw(null);
  let err = $state('');

  $effect(() => {
    live.n;
    if (editing) return;
    let alive = true;
    err = '';
    portfoliosApi
      .list()
      .then((r) => { if (alive) list = r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const p = $derived(
    wanted ? (list ?? []).find((x) => x.id === wanted) ?? null : (list ?? [])[0] ?? null
  );

  $effect(() => {
    live.n;
    if (editing || !needsBook || !p) return;
    let alive = true;
    const id = p.id;
    portfoliosApi
      .detail(id)
      .then((d) => { if (alive) book = d; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const ccy = $derived(p?.currency ?? 'USD');
  const money = (v) => (v == null ? '—' : fmtMoney(v, ccy));
  const signed = (v) => (v == null ? '—' : fmtSignedMoney(v, ccy));
  const tone = (v) => (v == null ? '' : v > 0 ? 'pos' : v < 0 ? 'neg' : '');

  const gain = $derived(p ? gainPct(p.market_value, p.cost_basis) : null);
  const spark = $derived(p?.sparkline ?? []);
  // The list call ships bare values (no dates), so this series draws without a range rail.
  const series = $derived(spark.map((v) => ({ value: v })));

  // ── book-derived ──
  const assets = $derived(book?.assets ?? []);
  const snapshots = $derived(book?.snapshots ?? []);
  const invested = $derived(book?.summary?.market_value ?? 0);
  const cash = $derived(book?.summary?.cash_total ?? 0);
  const income = $derived(book?.book?.income ?? null);

  const byClass = $derived.by(() => {
    const sums = new Map();
    for (const a of assets) {
      const v = a.market_value ?? 0;
      if (!v) continue;
      sums.set(a.asset_class, (sums.get(a.asset_class) ?? 0) + v);
    }
    const rows = [...sums]
      .map(([key, value]) => ({ label: $t(`portfolios.class.${key}`), value }))
      .sort((a, b) => b.value - a.value);
    if (cash > 0) rows.push({ label: $t('dashboard.widgets.portfolio.cash'), value: cash });
    return rows;
  });

  // Ranked by the position's own gain: the module stores no intraday series, so "today's
  // mover" is not derivable here and open PnL % is the honest ranking.
  const ranked = $derived(
    assets
      .filter((a) => a.quantity && a.cost_basis)
      .map((a) => ({ ...a, pct: gainPct(a.market_value ?? 0, a.cost_basis) }))
      .filter((a) => a.pct != null)
      .sort((a, b) => b.pct - a.pct)
  );
  const best = $derived(ranked[0] ?? null);
  const worst = $derived(ranked.at(-1) ?? null);

  const compare = $derived(
    (list ?? [])
      .map((x) => ({
        label: x.name,
        value: x.market_value,
        display: fmtCompactMoney(x.market_value) ?? fmtMoney(x.market_value, x.currency),
        pct: gainPct(x.market_value, x.cost_basis)
      }))
      .sort((a, b) => b.value - a.value)
      .slice(0, limit)
  );

  const loading = $derived(list === null || (needsBook && p !== null && book === null));
</script>

<WidgetState
  {editing}
  error={err}
  loading={loading}
  empty={!p}
  preview={$t('dashboard.widgets.portfolio.preview')}
  emptyText={$t('dashboard.widgets.portfolio.empty')}
  rows={3}
>
  {#if variant === 'value'}
    {#if series.length > 1}
      <TrendChart
        points={series}
        valueLabel={p.name}
        format={money}
        axisFormat={(v) => fmtCompactValue(v, ccy)}
        changeFormat={signed}
        label={$t('dashboard.widgets.portfolio.value')}
      />
    {:else}
      <div class="w-body">
        <Stat
          value={money(p.net_worth ?? p.market_value)}
          delta={gain == null ? '' : fmtSignedPct(gain)}
          deltaTone={tone(gain)}
          note={signed(p.unrealized)}
        />
      </div>
    {/if}
  {:else if variant === 'pnl'}
    <div class="w-body">
      <Stat
        value={gain == null ? '—' : fmtSignedPct(gain)}
        tone={tone(gain)}
        note={signed(p.unrealized)}
      />
      {#if spark.length > 1}
        <Spark values={spark} height={46} tone={tone(gain) || 'accent'} valueFormat={money}
          label={$t('dashboard.widgets.portfolio.value')} />
      {/if}
    </div>
  {:else if variant === 'cash'}
    <Donut
      segments={[
        { label: $t('dashboard.widgets.portfolio.invested'), value: invested, color: 'var(--chart-1)' },
        { label: $t('dashboard.widgets.portfolio.cash'), value: cash, color: 'var(--chart-4)' }
      ]}
      center={invested + cash > 0 ? `${Math.round((invested / (invested + cash)) * 100)}%` : '—'}
      centerLabel={$t('dashboard.widgets.portfolio.invested')}
      showPct={false}
      valueFormat={money}
    />
  {:else if variant === 'income'}
    <div class="w-body">
      <Stat
        value={money(income?.total)}
        note={$t('portfolios.analytics.costs.incomeTitleAll')}
      />
      <ul class="w-list">
        <li class="w-row">
          <span class="w-name grow">{$t('portfolios.kind.dividend')}</span>
          <span class="w-num">{money(income?.dividends)}</span>
        </li>
        <li class="w-row">
          <span class="w-name grow">{$t('portfolios.kind.interest')}</span>
          <span class="w-num">{money(income?.interest)}</span>
        </li>
        <li class="w-row">
          <span class="w-name grow">{$t('portfolios.kind.coupon')}</span>
          <span class="w-num">{money(income?.coupons)}</span>
        </li>
      </ul>
    </div>
  {:else if variant === 'evolution'}
    <TrendChart
      points={snapshots.map((s) => ({ at: s.snap_date, value: s.market_value }))}
      valueLabel={p.name}
      format={money}
      axisFormat={(v) => fmtCompactValue(v, ccy)}
      changeFormat={signed}
      defaultRange="1m"
      label={$t('dashboard.widgets.portfolio.evolution')}
    />
  {:else if variant === 'allocation'}
    <Donut
      segments={byClass}
      center={fmtCompactMoney(invested + cash) ?? money(invested + cash)}
      centerLabel={$t('dashboard.widgets.portfolio.totalValue')}
      valueFormat={money}
      showPct={true}
      legend={true}
    />
  {:else if variant === 'movers'}
    <ul class="w-list">
      {#each ranked.slice(0, limit) as a (a.id)}
        <li class="w-row">
          <span class="w-name sym">{a.symbol}</span>
          <span class="w-name grow">{a.name}</span>
          <span class="w-num {tone(a.pct)}">{fmtSignedPct(a.pct)}</span>
        </li>
      {/each}
    </ul>
  {:else if variant === 'best' || variant === 'worst'}
    {@const a = variant === 'best' ? best : worst}
    {#if a}
      <div class="w-body">
        <div class="posid">
          <span class="w-name sym">{a.symbol}</span>
          <span class="w-sub w-name">{a.name}</span>
        </div>
        <Stat value={signed(a.unrealized)} tone={tone(a.unrealized)} size="sm"
          delta={fmtSignedPct(a.pct)} deltaTone={tone(a.pct)} />
      </div>
    {:else}
      <p class="w-state">{$t('dashboard.widgets.portfolio.empty')}</p>
    {/if}
  {:else if variant === 'compare'}
    <RankBars rows={compare} />
  {:else if variant === 'positions'}
    <table class="w-tbl">
      <thead>
        <tr>
          <th>{$t('dashboard.widgets.portfolio.symbol')}</th>
          <th class="num">{$t('dashboard.widgets.portfolio.qty')}</th>
          <th class="num">{$t('dashboard.widgets.portfolio.value')}</th>
          <th class="num">{$t('dashboard.widgets.portfolio.pnlPct')}</th>
        </tr>
      </thead>
      <tbody>
        {#each ranked.slice(0, limit) as a (a.id)}
          <tr>
            <td class="w-name">{a.symbol}</td>
            <td class="num">{fmtNum(a.quantity, 2)}</td>
            <td class="num">{money(a.market_value)}</td>
            <td class="num {tone(a.pct)}">{fmtSignedPct(a.pct)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <!-- summary: the original thumbnail -->
    <div class="w-body">
      <div class="head">
        <div class="w-metric">
          <div class="w-eyebrow w-name">{p.name}</div>
          <div class="w-metric-value">{fmtMoney(p.market_value, p.currency)}</div>
          <div class="w-metric-delta {(p.unrealized ?? 0) < 0 ? 'w-neg' : 'w-pos'}">
            {fmtMoney(p.unrealized, p.currency)}
            <span class="w-metric-note">{$t('portfolios.detail.unrealized')}</span>
          </div>
        </div>
        {#if gain != null}
          <span class="w-pill" class:pos={gain >= 0} class:neg={gain < 0}>{fmtPct(gain)}</span>
        {/if}
      </div>

      <div class="w-metrics">
        <div>
          <span class="w-eyebrow">{$t('portfolios.detail.costBasis')}</span>
          <span class="w-num cell">{fmtMoney(p.cost_basis, p.currency)}</span>
        </div>
        <div>
          <span class="w-eyebrow">{$t('portfolios.detail.assets')}</span>
          <span class="w-num cell">{p.asset_count ?? 0}</span>
        </div>
      </div>
    </div>
  {/if}
</WidgetState>

<style>
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    flex-shrink: 0;
  }
  /* Left-aligned under its label: these are read as a pair, not as a column of
     figures needing decimal alignment. */
  .cell {
    text-align: left;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .sym {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    flex-shrink: 0;
  }
  .grow {
    flex: 1;
    color: var(--dim);
  }
  .posid {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
</style>
