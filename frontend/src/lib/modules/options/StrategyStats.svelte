<script>
  // What the strategy costs and can make: net premium, extremes, breakevens, probability of
  // profit at expiry, the margin it ties up, the aggregated greeks, and what the numbers leave
  // out: early assignment, and costs, which no connector publishes.
  import Icon from '$lib/ui/Icon.svelte';
  import InfoNote from './InfoNote.svelte';
  import { price, greek, pct } from './columns.js';
  import { t } from '$lib/i18n';

  let { ev = null } = $props();

  const extreme = (v) => (v == null ? $t('options.stats.unlimited') : price(v));
</script>

{#if ev}
  <div class="qp-cards">
    <div class="qp-card">
      <span class="k">{$t('options.stats.net')}</span>
      <span class="v" class:qp-bad={ev.net_premium < 0} class:qp-good={ev.net_premium > 0}>{price(ev.net_premium)}</span>
      <span class="s">{ev.net_premium < 0 ? $t('options.stats.debit') : ev.net_premium > 0 ? $t('options.stats.credit') : ''}</span>
    </div>
    <div class="qp-card">
      <span class="k">{$t('options.stats.maxProfit')}</span>
      <span class="v qp-good">{extreme(ev.max_profit)}</span>
    </div>
    <div class="qp-card">
      <span class="k">{$t('options.stats.maxLoss')}</span>
      <span class="v qp-bad">{extreme(ev.max_loss)}</span>
    </div>
    <div class="qp-card">
      <span class="k">{$t('options.stats.breakevens')}</span>
      <span class="v">{ev.breakevens?.length ? ev.breakevens.map((b) => price(b)).join(' / ') : $t('options.stats.none')}</span>
    </div>
    <div class="qp-card">
      <span class="k">{$t('options.stats.pop')}</span>
      <span class="v">{pct(ev.pop, 1)}</span>
      <span class="s">{$t(ev.pop_basis === 'smile' ? 'options.stats.popSmile' : 'options.stats.popAtm')}</span>
    </div>
    <div class="qp-card">
      <span class="k">{$t('options.stats.buyingPower')}</span>
      <span class="v">{price(ev.margin?.buying_power)}</span>
      <span class="s">{$t('options.stats.marginHint', { r: price(ev.margin?.requirement) })}</span>
    </div>
  </div>
  <div class="qp-cards">
    {#each ['delta', 'gamma', 'vega', 'theta', 'rho'] as g (g)}
      <div class="qp-card">
        <span class="k raw">{$t(`options.col.${g}`)}</span>
        <span class="v">{greek(ev.greeks?.[g])}</span>
      </div>
    {/each}
  </div>
  {#each ev.warnings?.filter((w) => w.code === 'early_assignment') ?? [] as w (w.message)}
    <p class="warn" role="status"><Icon name="alert-triangle" size={14} /> <span>{w.message}</span></p>
  {/each}
  <ul class="qp-hint notes">
    {#each ev.warnings?.filter((w) => w.code !== 'early_assignment') ?? [] as w (w.message)}<li>{w.message} <InfoNote note={$t('options.stats.assignmentNote')} /></li>{/each}
    <li>{$t('options.stats.popCaveat')}</li>
    <li>{$t('options.stats.costs')}</li>
  </ul>
{/if}

<style>
  .warn {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    color: var(--amber);
    font-size: var(--fs-desc);
  }
  .notes {
    margin: 0;
    padding-left: var(--space-4);
  }
  /* Phone: the figures read two by two instead of one per line. */
  @media (max-width: 767px) {
    .qp-cards {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
