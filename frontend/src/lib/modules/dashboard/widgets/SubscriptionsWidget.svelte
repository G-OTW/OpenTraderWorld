<script>
  // Subscriptions widget: what the recurring spend costs per month, then what renews next.
  // Totals come from the module breakdown (display currency, FX-converted); each renewal
  // row shows the amount in the sub's OWN currency, because that is what gets charged.
  // Config: { limit } — how many upcoming renewals to list (default 5).
  import { subsApi, fmtMoney, nextBillingDate } from '$lib/modules/subscriptions/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();
  const limit = $derived(Math.max(1, Math.min(20, item.config?.limit ?? 5)));

  let bd = $state(null);
  let subs = $state(null);
  let err = $state('');

  async function load() {
    err = '';
    try {
      // months_back/fwd 0: the widget only reads the totals, not the chart columns.
      [subs, bd] = await Promise.all([
        subsApi.list({ active_only: true }),
        subsApi.breakdown({ months_back: 0, months_fwd: 0 })
      ]);
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const DAY = 86400000;

  // Active subs with a start anchor, soonest renewal first. A sub with no start date has
  // no computable next date, so it is listed under the totals but not as a renewal.
  const upcoming = $derived.by(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    return (subs ?? [])
      .map((s) => ({ sub: s, due: nextBillingDate(s) }))
      .filter((r) => r.due)
      .map((r) => ({ ...r, days: Math.round((r.due - today) / DAY) }))
      .sort((a, b) => a.days - b.days)
      .slice(0, limit);
  });

  function dueLabel(days, due) {
    if (days <= 0) return $t('dashboard.widgets.subs.today');
    if (days === 1) return $t('dashboard.widgets.subs.tomorrow');
    if (days <= 30) return $t('dashboard.widgets.subs.inDays', { days });
    return due.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }

  const cur = $derived(bd?.display_currency ?? 'USD');
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.subs.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if bd === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else}
  <div class="top">
    <div class="total">
      <strong>{fmtMoney(bd.monthly_total, cur)}</strong>
      <span class="per">{$t('dashboard.widgets.subs.perMonth')}</span>
    </div>
    <div class="meta">
      {$t('dashboard.widgets.subs.meta', {
        yearly: fmtMoney(bd.yearly_total, cur),
        count: bd.active_count ?? 0
      })}
    </div>
    {#if bd.unconverted > 0}
      <div class="warn">{$t('dashboard.widgets.subs.unconverted', { count: bd.unconverted, cur })}</div>
    {/if}
  </div>

  {#if upcoming.length === 0}
    <p class="hint">{$t('dashboard.widgets.subs.noRenewals')}</p>
  {:else}
    <div class="rows">
      {#each upcoming as r (r.sub.id)}
        <div class="row" class:soon={r.days <= 3}>
          <span class="name" title={r.sub.name}>{r.sub.name}</span>
          <span class="due">{dueLabel(r.days, r.due)}</span>
          <span class="amt">{fmtMoney(r.sub.price, r.sub.currency)}</span>
        </div>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .hint {
    color: var(--dim);
  }
  .sk {
    padding: var(--space-1) 0;
  }
  /* Totals stay pinned while the renewal list scrolls under them (the shell body is the
     scroller), so the headline number never scrolls out of a compact widget. */
  .top {
    position: sticky;
    top: 0;
    background: var(--bg);
    padding-bottom: var(--space-2);
    border-bottom: 0.5px solid var(--border);
    margin-bottom: var(--space-2);
  }
  .total {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .total strong {
    font-size: 1.5rem;
    font-weight: var(--fw-normal);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .per {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .meta {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .warn {
    font-size: var(--text-xs);
    color: var(--amber);
    margin-top: 2px;
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: var(--space-3);
    align-items: baseline;
    padding: 3px 0;
    font-size: var(--text-sm);
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .due {
    font-size: var(--text-xs);
    color: var(--dim);
    white-space: nowrap;
  }
  /* Renewing within 3 days is the only thing worth a color here. */
  .row.soon .due {
    color: var(--amber);
  }
  .amt {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
</style>
