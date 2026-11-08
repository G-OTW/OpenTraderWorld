<script>
  // A subscription's price over time: a step line from the first recorded price to today
  // (each price holds until the next change), the change since the first price, and the
  // list of changes. Prices are the full plan price in their own currency.
  import { subsApi } from './api.js';
  import { money } from './fmt.svelte.js';
  import { parseDay, startOfDay } from './calc.js';
  import { fmtDate, fmtMoney, fmtCompactValue } from '$lib/format.js';
  import ChartFrame from '$lib/ui/charts/ChartFrame.svelte';
  import TimeSeries from '$lib/ui/charts/TimeSeries.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import { t } from '$lib/i18n';

  let { sub } = $props();

  let prices = $state(null);
  let error = $state('');

  $effect(() => {
    const id = sub?.id;
    if (!id) return;
    prices = null;
    error = '';
    subsApi
      .prices(id)
      .then((p) => (prices = p))
      .catch((e) => (error = e.message));
  });

  const HEIGHT = 180;
  // One currency per line: a plan that changed currency starts a new run at the switch.
  const ccy = $derived(prices?.length ? prices[prices.length - 1].currency : sub?.currency ?? 'USD');
  const points = $derived.by(() => {
    if (!prices?.length) return [];
    const rows = prices.filter((p) => p.currency === ccy);
    const pts = rows.map((p) => [parseDay(p.effective_from), p.price]);
    const last = pts[pts.length - 1];
    const today = startOfDay();
    // Hold the latest price until today so the last step has a length.
    if (last && last[0] < today) pts.push([today, last[1]]);
    return pts;
  });

  const creep = $derived.by(() => {
    if (!prices || prices.length < 2) return null;
    const first = prices[0];
    const last = prices[prices.length - 1];
    if (first.currency !== last.currency || first.price <= 0) return null;
    return ((last.price - first.price) / first.price) * 100;
  });
</script>

<section class="history">
  <ChartFrame
    title={$t('subscriptions.detail.priceHistory')}
    subtitle={$t('subscriptions.detail.historySubtitle', { cur: ccy })}
    height={HEIGHT}
    loading={prices === null && !error}
    unavailable={error
      ? { reason: error, icon: 'alert-triangle' }
      : prices && prices.length === 0
        ? { reason: $t('subscriptions.detail.noHistory') }
        : null}
  >
    {#snippet actions()}
      {#if creep !== null}
        <span class="creep">
          <Delta value={creep} invert digits={1} />
          <span class="since">{$t('subscriptions.detail.sinceFirst')}</span>
        </span>
      {/if}
    {/snippet}
    <TimeSeries
      series={[{ key: 'price', label: $t('subscriptions.form.price'), points, step: true }]}
      height={HEIGHT}
      change="pct"
      format={(v) => fmtMoney(v, ccy)}
      axisFormat={(v) => fmtCompactValue(v, ccy)}
      ariaLabel={$t('subscriptions.detail.priceHistory')}
    />
  </ChartFrame>
  {#if prices?.length}
    <table class="tbl changes">
      <thead>
        <tr>
          <th>{$t('subscriptions.detail.from')}</th>
          <th class="num">{$t('subscriptions.form.price')}</th>
        </tr>
      </thead>
      <tbody>
        {#each [...prices].reverse() as p (p.effective_from + p.currency)}
          <tr>
            <td>{fmtDate(p.effective_from)}</td>
            <td class="num">{money(p.price, p.currency)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .history {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .creep {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .since {
    font-size: 12px;
    color: var(--dim);
  }
  .changes td {
    height: 32px;
  }
  .changes tbody tr:last-child td {
    border-bottom: none;
  }
</style>
