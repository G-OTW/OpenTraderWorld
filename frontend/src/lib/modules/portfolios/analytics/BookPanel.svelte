<script>
  // `book`: what the portfolio is, right now. The one panel that works on day one: no
  // bars, no connector, no history.
  import { t } from '$lib/i18n';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Segmented from '$lib/ui/charts/Segmented.svelte';
  import Donut from '$lib/ui/charts/Donut.svelte';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { identityColor, otherColor } from '$lib/ui/charts/palette.js';
  import { fmtMoney, fmtSignedMoney, fmtPct } from '../api.js';
  import { fmtPct as fmtPlainPct } from '$lib/format.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';
  import Panel from './Panel.svelte';

  let { block = null, currency = 'USD', onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  const c = $derived(chartColors());
  // Deployed share, so the invested/cash split reads as one bar rather than two figures the
  // user has to divide in their head. A ledger with no deposit has no cash account, so
  // there is no split to draw: the bar would claim 100% deployed, which is a measurement
  // and not the absence of one.
  const tracked = $derived(d?.cash_tracked !== false);
  const money = (v) => fmtMoney(v, currency);
  const split = $derived(
    d && tracked && d.net_worth
      ? [
          { key: 'invested', label: $t('portfolios.analytics.book.invested'), value: d.invested, color: identityColor(0, c) },
          { key: 'cash', label: $t('portfolios.analytics.book.cash'), value: d.cash_total, color: otherColor(c) }
        ]
      : []
  );
  const buckets = $derived(
    (d?.allocation ?? []).map((a) => ({ key: a.bucket, label: $t(`portfolios.class.${a.bucket}`), value: a.value }))
  );
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="pa-stack">
    <div class="pa-stats">
      <StatCard label={$t('portfolios.analytics.book.netWorth')} value={money(d.net_worth)} />
      <StatCard label={$t('portfolios.analytics.book.invested')} value={money(d.invested)} />
      {#if tracked}
        <StatCard label={$t('portfolios.analytics.book.cash')} value={money(d.cash_total)} hint={fmtPlainPct(d.cash_pct)} />
      {/if}
      <StatCard label={$t('portfolios.analytics.book.unrealized')} value={fmtSignedMoney(d.unrealized, currency)} />
      <StatCard label={$t('portfolios.analytics.book.realized')} value={fmtSignedMoney(d.realized, currency)} />
      <StatCard label={$t('portfolios.analytics.book.income')} value={money(d.income.total)} />
    </div>

    {#if d.unpriced > 0}
      <p class="pa-warn">{$t('portfolios.analytics.book.unpriced', { n: d.unpriced })}</p>
    {/if}

    <div class="pa-cols">
      <Panel title={$t('portfolios.analytics.book.splitTitle')} subtitle={tracked ? '' : $t('portfolios.headline.cashUntracked')}>
        {#if split.length}
          <Segmented items={split} size="tall" format={money} ariaLabel={$t('portfolios.analytics.book.splitAria')} />
        {:else}
          <!-- Not a warning about a wrong number, a note about a number that does not exist
               yet: book a deposit and the cash line starts saying something. -->
          <p class="pa-note">{$t('portfolios.analytics.book.noCash')}</p>
        {/if}
        {#if tracked && d.cash?.length > 1}
          <!-- Per currency, in that currency: converting them into one figure hides that half
               the cash is sitting in a currency the user did not mean to hold. -->
          <table class="pa-table ccys">
            <tbody>
              {#each d.cash as cc (cc.currency)}
                <tr>
                  <td>{cc.currency}</td>
                  <td class="num" class:pa-down={cc.amount < 0}>{fmtMoney(cc.amount, cc.currency)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </Panel>
      {#if buckets.length}
        <Panel title={$t('portfolios.alloc.title')} subtitle={$t('portfolios.alloc.byClass')}>
          <Donut
            items={buckets}
            max={5}
            legend="side"
            totalLabel={$t('portfolios.alloc.total')}
            otherLabel={$t('portfolios.alloc.other')}
            format={(v) => fmtMoney(v, currency, 0)}
          />
        </Panel>
      {/if}
    </div>

    <Sample sample={block.sample} />
  </div>
{/if}

<style>
  .ccys {
    margin-top: var(--space-3);
  }
</style>
