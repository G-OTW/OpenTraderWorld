<script>
  // Subscription widgets — the cards of the Subscriptions mockup, each a `variant`.
  // Totals come from the module breakdown (display currency, FX-converted); a renewal row
  // shows the amount in the sub's OWN currency, because that is what gets charged.
  //
  // Config: { variant, limit, months }. `display: 'renewals'` from older saved widgets
  // still selects the renewals list.
  import { subsApi, fmtMoney, nextBillingDate } from '$lib/modules/subscriptions/api.js';
  import { fmtSignedPct } from '$lib/format';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Donut from './parts/Donut.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import Stat from './parts/Stat.svelte';
  import Buckets from './parts/Buckets.svelte';

  let { item, editing } = $props();
  const limit = $derived(Math.max(1, Math.min(20, item.config?.limit ?? 5)));
  const variant = $derived(item.config?.variant ?? (item.config?.display === 'renewals' ? 'renewals' : 'overview'));
  const display = $derived(variant === 'renewals' ? 'renewals' : 'overview');
  // The chart cards need real columns; the pure-total cards do not pay for them.
  const chartWindow = $derived(
    ['monthly', 'yearly', 'forecast', 'months'].includes(variant)
      ? { months_back: item.config?.monthsBack ?? 6, months_fwd: item.config?.monthsFwd ?? 6 }
      : { months_back: 0, months_fwd: 0 }
  );

  let bd = $state(null);
  let subs = $state(null);
  let err = $state('');

  $effect(() => {
    if (editing) return;
    const w = chartWindow;
    let alive = true;
    err = '';
    Promise.all([subsApi.list(), subsApi.breakdown(w)])
      .then(([s, b]) => { if (alive) { subs = s; bd = b; } })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const DAY = 86400000;
  const cur = $derived(bd?.display_currency ?? 'USD');
  const money = (v) => (v == null ? '—' : fmtMoney(v, cur));

  const active = $derived((subs ?? []).filter((s) => s.active));
  const inactive = $derived((subs ?? []).length - active.length);

  // Active subs with a start anchor, soonest renewal first. A sub with no start date has
  // no computable next date, so it is listed under the totals but not as a renewal.
  const upcoming = $derived.by(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    return active
      .map((s) => ({ sub: s, due: nextBillingDate(s) }))
      .filter((r) => r.due)
      .map((r) => ({ ...r, days: Math.round((r.due - today) / DAY) }))
      .sort((a, b) => a.days - b.days)
      .slice(0, limit);
  });

  // Most expensive, compared on the monthly-equivalent the breakdown already ranks by.
  const priciest = $derived(
    [...active].sort((a, b) => b.price - a.price).slice(0, limit)
  );

  const months = $derived(bd?.months ?? []);
  const monthValues = $derived(months.map((m) => m.amount));
  const monthNames = $derived(
    months.map((m) =>
      new Date(`${m.month}T00:00:00`).toLocaleDateString(undefined, { month: 'short', year: '2-digit' })
    )
  );
  // Change against the previous column of the same series: the mockup's "vs last month".
  const momPct = $derived.by(() => {
    if (monthValues.length < 2) return null;
    const prev = monthValues.at(-2);
    return prev ? ((monthValues.at(-1) - prev) / prev) * 100 : null;
  });

  const categories = $derived(
    (bd?.categories ?? []).map((c) => ({
      label: c.category || $t('dashboard.widgets.subs.uncategorized'),
      value: c.amount
    }))
  );

  function dueLabel(days, due) {
    if (days <= 0) return $t('dashboard.widgets.subs.today');
    if (days === 1) return $t('dashboard.widgets.subs.tomorrow');
    if (days <= 30) return $t('dashboard.widgets.subs.inDays', { days });
    return due.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }
</script>

<WidgetState
  {editing}
  error={err}
  loading={bd === null || subs === null}
  preview={$t('dashboard.widgets.subs.preview')}
  rows={3}
>
  {#if variant === 'monthly' || variant === 'yearly' || variant === 'forecast'}
    {@const value = variant === 'yearly' ? bd.yearly_total
      : variant === 'forecast' ? bd.next_month_total : bd.monthly_total}
    {@const note = variant === 'yearly' ? $t('dashboard.widgets.subs.vsLastYear')
      : variant === 'forecast' ? $t('dashboard.widgets.subs.vsThisMonth')
      : $t('dashboard.widgets.subs.vsLastMonth')}
    <div class="split">
      <Stat
        value={money(value)}
        delta={momPct == null ? '' : fmtSignedPct(momPct, 1)}
        deltaTone={momPct == null ? '' : momPct > 0 ? 'neg' : 'pos'}
        note={momPct == null ? '' : note}
      />
      {#if monthValues.length > 1}
        <div class="barbox">
          <MiniBars values={monthValues} labels={monthNames} tone="accent" height={46}
            valueFormat={money} label={$t('dashboard.widgets.subs.perMonth')} />
        </div>
      {/if}
    </div>
  {:else if variant === 'active'}
    <div class="w-body">
      <Buckets
        align="left"
        cells={[
          { value: active.length, label: $t('subscriptions.page.activeSubs') },
          { value: inactive, label: $t('dashboard.widgets.subs.inactive') }
        ]}
      />
      <div class="w-bar pos" role="progressbar" aria-valuemin="0" aria-valuemax="100"
        aria-valuenow={subs.length ? Math.round((active.length / subs.length) * 100) : 0}>
        <span style:width={`${subs.length ? (active.length / subs.length) * 100 : 0}%`}></span>
      </div>
    </div>
  {:else if variant === 'category'}
    <Donut
      segments={categories}
      center={money(bd.monthly_total)}
      centerLabel={$t('dashboard.widgets.subs.perMonth')}
      valueFormat={money}
    />
  {:else if variant === 'months'}
    <div class="w-body">
      <div class="head">
        <span class="w-eyebrow">{$t('dashboard.widgets.subs.perMonth')}</span>
        <span class="w-num">{money(bd.monthly_total)}</span>
      </div>
      <div class="w-fill chartbox">
        <MiniBars values={monthValues} labels={monthNames} tone="accent" height={0}
          valueFormat={money} label={$t('dashboard.widgets.subs.perMonth')} />
      </div>
      <ul class="axis">
        {#each months as m (m.month)}
          <li>{new Date(`${m.month}T00:00:00`).toLocaleDateString(undefined, { month: 'short' })}</li>
        {/each}
      </ul>
    </div>
  {:else if variant === 'expensive'}
    <ul class="w-list">
      {#each priciest as s, i (s.id)}
        <li class="w-row">
          <span class="rank">{i + 1}</span>
          <span class="w-name grow">{s.name}</span>
          <span class="w-num">{fmtMoney(s.price, s.currency)}</span>
        </li>
      {/each}
    </ul>
  {:else}
    <div class="w-body">
      <!-- Totals stay pinned while the renewal list scrolls under them (the shell body is
           the scroller), so the headline number never scrolls out of a compact widget. -->
      <div class="w-sticky top">
        <div class="w-metric">
          <div class="w-metric-value">{fmtMoney(bd.monthly_total, cur)}</div>
          <div class="w-metric-delta">
            <span class="w-eyebrow">{$t('dashboard.widgets.subs.perMonth')}</span>
          </div>
        </div>
        <div class="w-metrics">
          <div>
            <span class="w-eyebrow">{$t('subscriptions.page.yearly')}</span>
            <span class="figure">{fmtMoney(bd.yearly_total, cur)}</span>
          </div>
          <div>
            <span class="w-eyebrow">{$t('subscriptions.page.activeSubs')}</span>
            <span class="figure">{bd.active_count ?? 0}</span>
          </div>
        </div>
        {#if bd.unconverted > 0}
          <div class="w-sub w-warn">
            {$t('dashboard.widgets.subs.unconverted', { count: bd.unconverted, cur })}
          </div>
        {/if}
      </div>

      {#if display === 'overview'}
        <p class="w-state">{$t('dashboard.widgets.subs.upcoming', { count: upcoming.length })}</p>
      {:else if upcoming.length === 0}
        <p class="w-state">{$t('dashboard.widgets.subs.noRenewals')}</p>
      {:else}
        <ul class="w-list">
          {#each upcoming as r (r.sub.id)}
            <li class="w-row">
              <span class="w-name" title={r.sub.name}>{r.sub.name}</span>
              <span class="due" class:soon={r.days <= 3}>{dueLabel(r.days, r.due)}</span>
              <span class="w-num">{fmtMoney(r.sub.price, r.sub.currency)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</WidgetState>

<style>
  .top {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    /* Cancel the body gap so the sticky band owns its own rhythm. */
    margin-bottom: 0;
  }
  .figure {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .due {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--dim);
    white-space: nowrap;
  }
  /* Renewing within 3 days is the only thing here worth a colour. */
  .due.soon {
    color: var(--amber-ink);
  }
  .split {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-width: 0;
  }
  .barbox {
    flex: 1;
    max-width: 52%;
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  .chartbox {
    display: flex;
    align-items: stretch;
  }
  .chartbox :global(> *) {
    width: 100%;
    height: 100%;
  }
  /* One tick per column, sharing the bars' flex rhythm so labels stay under their bar. */
  .axis {
    display: flex;
    list-style: none;
    margin: 0;
    padding: 0;
    gap: 2px;
    flex-shrink: 0;
  }
  .axis li {
    flex: 1;
    min-width: 0;
    text-align: center;
    font-size: 9px;
    color: var(--faint);
    overflow: hidden;
  }
  .rank {
    width: 14px;
    color: var(--faint);
    font-family: var(--mono);
    font-size: var(--text-xs);
    flex-shrink: 0;
  }
  .grow {
    flex: 1;
  }
</style>
