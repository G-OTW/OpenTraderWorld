<script>
  // Next 12 calendar months of subscription outflows, one accent bar per month in the
  // display currency, the monthly average as a reference hairline. Same numbers MyWealth
  // reads. The tooltip splits a month by billing currency when several are involved.
  import { subsApi } from './api.js';
  import { money } from './fmt.svelte.js';
  import { fmtMoney, fmtCompactValue, fmtMonth } from '$lib/format.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import ChartFrame from '$lib/ui/charts/ChartFrame.svelte';
  import Bars from '$lib/ui/charts/Bars.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // `version` changes when the page reloads its data, so the forecast follows edits.
  // `actions` is the caller's toggle row, rendered in the frame header.
  let { version = 0, actions = undefined, height = 260 } = $props();

  let fc = $state(null);
  let error = $state('');

  $effect(() => {
    void version;
    subsApi
      .forecast()
      .then((f) => {
        fc = f;
        error = '';
      })
      .catch((e) => (error = e.message));
  });

  const c = $derived(chartColors());
  const ccy = $derived(fc?.currency ?? 'USD');
  const months = $derived(fc?.months ?? []);
  const empty = $derived(!!fc && months.every((m) => !m.total));
  const unavailable = $derived(
    error
      ? { reason: error, icon: 'alert-triangle' }
      : empty
        ? { reason: $t('subscriptions.forecast.empty') }
        : null
  );
  const series = $derived([
    { key: 'billed', label: $t('subscriptions.page.billed'), values: months.map((m) => m.total), color: c.accent }
  ]);
  const lines = $derived(
    fc
      ? [
          {
            key: 'avg',
            label: $t('subscriptions.forecast.avg'),
            values: months.map(() => fc.monthly_avg),
            color: c.dim,
            width: 1
          }
        ]
      : []
  );
  // Foreign billing currencies of a month, in their own currency.
  const tipExtra = (i) =>
    Object.entries(months[i]?.by_currency ?? {})
      .filter(([k]) => k !== ccy)
      .map(([k, v]) => ({ label: k, value: money(v, k) }));
</script>

<ChartFrame
  title={$t('subscriptions.forecast.title')}
  subtitle={fc ? $t('subscriptions.forecast.subtitle', { cur: ccy, avg: money(fc.monthly_avg, ccy) }) : ''}
  {actions}
  {height}
  loading={!fc && !error}
  {unavailable}
>
  <Bars
    categories={months.map((m) => fmtMonth(m.month + '-01'))}
    {series}
    {lines}
    {height}
    format={(v) => fmtMoney(v, ccy)}
    axisFormat={(v) => fmtCompactValue(v, ccy)}
    {tipExtra}
    ariaLabel={$t('subscriptions.forecast.title')}
  />
</ChartFrame>
{#if fc?.unconverted > 0}
  <p class="warn"><Icon name="alert-triangle" size={13} /> {$t('subscriptions.page.unconvertedWarning', { count: fc.unconverted, cur: ccy })}</p>
{/if}

<style>
  .warn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-2);
    font-size: 12px;
    color: var(--amber-ink);
  }
</style>
