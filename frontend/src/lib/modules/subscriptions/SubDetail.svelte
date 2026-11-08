<script>
  // Detail modal body: the facts of one subscription, then its price history.
  import { fmtDate } from '$lib/format.js';
  import { nextBillingDate } from './calc.js';
  import { daysUntil } from './fmt.svelte.js';
  import PriceHistory from './PriceHistory.svelte';
  import { t } from '$lib/i18n';

  let { sub } = $props();

  const next = $derived(nextBillingDate(sub));
  const na = $derived($t('subscriptions.page.na'));
  const due = $derived.by(() => {
    const n = daysUntil(next);
    if (n === null) return '';
    return n === 0 ? $t('subscriptions.page.dueToday') : $t('subscriptions.page.inDays', { count: n });
  });
</script>

<dl class="facts">
  <div><dt>{$t('subscriptions.page.nextBilling')}</dt><dd>{next ? `${fmtDate(next)} (${due})` : na}</dd></div>
  <div><dt>{$t('subscriptions.form.myShare')}</dt><dd>{Math.round(sub.my_share_pct ?? 100)} %</dd></div>
  {#if sub.payment_method}<div><dt>{$t('subscriptions.form.paymentMethod')}</dt><dd>{sub.payment_method}</dd></div>{/if}
  {#if sub.card_expiry}<div><dt>{$t('subscriptions.form.cardExpiry')}</dt><dd>{sub.card_expiry}</dd></div>{/if}
  {#if sub.trial_end}<div><dt>{$t('subscriptions.form.trialEnd')}</dt><dd>{fmtDate(sub.trial_end)}</dd></div>{/if}
  {#if sub.ended_at}<div><dt>{$t('subscriptions.form.endedAt')}</dt><dd>{fmtDate(sub.ended_at)}</dd></div>{/if}
  <div>
    <dt>{$t('subscriptions.worth.score')}</dt>
    <dd>{sub.last_score ? `${sub.last_score} / 5 (${fmtDate(sub.last_rated_at)})` : $t('subscriptions.page.notRated')}</dd>
  </div>
  {#if sub.remind_days !== null && sub.remind_days !== undefined}
    <div><dt>{$t('subscriptions.form.remind')}</dt><dd>{$t('subscriptions.page.remindDaysValue', { days: sub.remind_days })}</dd></div>
  {/if}
</dl>
<PriceHistory {sub} />

<style>
  .facts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: var(--space-3) var(--space-4);
    margin: 0 0 var(--space-4);
    padding-bottom: var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
  }
  .facts div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  dt {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  dd {
    margin: 0;
    font-size: 13px;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--text);
    overflow-wrap: anywhere;
  }
</style>
