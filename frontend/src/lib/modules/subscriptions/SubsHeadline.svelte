<script>
  // Header block: what the running subscriptions cost a month (monthly equivalent, at the
  // user's share), its change against last month, and three tiles: the yearly total, what
  // bills in the next 30 days, and what cancelling has saved. Budget and FX warnings sit
  // under it as one-line notes.
  import StatCard from '$lib/ui/StatCard.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { money } from './fmt.svelte.js';
  import { upcomingBillings, isEnded } from './calc.js';
  import { t } from '$lib/i18n';

  // `toDisplay(amount, currency)` converts to `cur`, or returns null with no rate.
  let { bd = null, subs = [], cur = 'USD', budgets = {}, toDisplay = (a) => a } = $props();

  const na = $derived($t('subscriptions.page.na'));
  const now = new Date();
  const thisMonth = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-01`;

  // Change of the monthly-equivalent cost against last month (the cost-creep series).
  const change = $derived.by(() => {
    const ms = bd?.months ?? [];
    const i = ms.findIndex((m) => m.month === thisMonth);
    if (i < 1) return null;
    const a = ms[i - 1].monthly_equiv;
    const b = ms[i].monthly_equiv;
    if (!(a > 0) || b == null) return null;
    return ((b - a) / a) * 100;
  });

  const next30 = $derived.by(() => {
    let sum = 0;
    let missing = 0;
    const rows = upcomingBillings(subs, 30);
    for (const r of rows) {
      const v = toDisplay(r.amount, r.currency);
      if (v === null || v === undefined) missing++;
      else sum += v;
    }
    return { sum, count: rows.length, missing };
  });

  // The breakdown counts the cancelled rows it saw; when the list holds cancelled rows the
  // breakdown did not count, its saving is not measured and reads as a word, never 0.
  const saved = $derived.by(() => {
    const listed = subs.filter((s) => isEnded(s)).length;
    const counted = bd?.ended_count ?? 0;
    return {
      count: Math.max(listed, counted),
      known: counted > 0 || listed === 0,
      value: counted > 0 ? bd.saved_since_cancelled : 0
    };
  });

  const overBudget = $derived(
    (bd?.categories ?? []).filter((c) => {
      const cap = budgets?.[c.category];
      return cap > 0 && c.amount > cap;
    })
  );
</script>

<section class="headline">
  <div class="hero">
    <span class="eyebrow">{$t('subscriptions.page.thisMonth')}</span>
    <strong class="figure">{money(bd?.monthly_total, cur, na)}</strong>
    <span class="change">
      <Delta value={change} invert size="md" empty={na} />
      <span class="vs">{$t('subscriptions.page.vsLastMonth')}</span>
    </span>
    <span class="caption">{$t('subscriptions.page.heroCaption', { count: bd?.active_count ?? 0, cur })}</span>
  </div>
  <div class="tiles">
    <StatCard
      label={$t('subscriptions.page.yearly')}
      value={money(bd?.yearly_total, cur, na)}
      hint={$t('subscriptions.page.yearlyHint')}
    />
    <StatCard
      label={$t('subscriptions.page.next30')}
      value={money(next30.sum, cur, na)}
      hint={$t('subscriptions.page.next30Hint', { count: next30.count, amount: money(bd?.next_month_total, cur, na) })}
    />
    <StatCard
      label={$t('subscriptions.page.savedSinceCancelled')}
      value={saved.known ? money(saved.value, cur, na) : na}
      hint={saved.count > 0
        ? $t('subscriptions.page.cancelledCount', { count: saved.count })
        : $t('subscriptions.page.noneCancelled')}
    />
  </div>
  {#if overBudget.length || bd?.unconverted > 0 || next30.missing > 0}
    <ul class="notes">
      {#each overBudget as c (c.category)}
        <li>
          <Icon name="alert-triangle" size={13} />
          {$t('subscriptions.budgets.over', { category: c.category, amount: money(c.amount, cur), cap: money(budgets[c.category], cur) })}
        </li>
      {/each}
      {#if bd?.unconverted > 0}
        <li><Icon name="alert-triangle" size={13} /> {$t('subscriptions.page.unconvertedWarning', { count: bd.unconverted, cur })}</li>
      {:else if next30.missing > 0}
        <li><Icon name="alert-triangle" size={13} /> {$t('subscriptions.page.unconvertedWarning', { count: next30.missing, cur })}</li>
      {/if}
    </ul>
  {/if}
</section>

<style>
  .headline {
    display: grid;
    grid-template-columns: minmax(240px, 1fr) minmax(0, 2fr);
    gap: var(--space-4);
    align-items: stretch;
  }
  .hero {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: var(--space-1);
    min-width: 0;
  }
  .eyebrow {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  /* The one number the page leads with: sans, proportional figures at this size. */
  .figure {
    font-family: var(--font);
    font-size: 28px;
    font-weight: 600;
    line-height: 1.15;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .change {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .vs,
  .caption {
    font-size: 12px;
    color: var(--dim);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-3);
    min-width: 0;
  }
  .tiles :global(.statcard) {
    background: var(--surface);
    border-radius: var(--radius);
    justify-content: center;
  }
  .notes {
    grid-column: 1 / -1;
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .notes li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 12px;
    color: var(--amber-ink);
  }
  @media (max-width: 1099px) {
    .headline {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  @media (max-width: 640px) {
    .tiles {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
