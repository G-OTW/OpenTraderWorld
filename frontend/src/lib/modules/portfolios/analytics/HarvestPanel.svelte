<script>
  // `harvest`: lots under water, worst first, with the Tax Calculator's wash window. A lot
  // bought inside the window before today is flagged: selling it now would defer the loss.
  import { t } from '$lib/i18n';
  import { goto } from '$app/navigation';
  import StatCard from '$lib/ui/StatCard.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Unavailable from './Unavailable.svelte';
  import Panel from './Panel.svelte';
  import { portfoliosApi, fmtMoney, fmtSignedMoney, fmtNum, fmtDate } from '../api.js';

  let { portfolioId, currency = 'USD', onfix = null } = $props();
  // The Tax Calculator reads one number from the link; it is in this portfolio's currency.
  let data = $state(null);
  let error = $state(null);

  $effect(() => {
    if (!portfolioId) return;
    portfoliosApi
      .harvest(portfolioId)
      .then((r) => (data = r))
      .catch((e) => (error = e.message));
  });

  // The Tax Calculator opens with the realized loss prefilled, never with a sale booked.
  function sendToTax() {
    const realized = (-Math.abs(data.harvestable ?? 0)).toFixed(2);
    goto(`/taxcalc?realized_capital_gains=${realized}`);
  }
</script>

<ErrorText {error} />
{#if !data}
  <div class="pa-stack">
    <div class="pa-stats">
      {#each Array(3) as _, i (i)}<Skeleton height="64px" />{/each}
    </div>
    <Skeleton height="240px" />
  </div>
{:else if data.status !== 'ok'}
  <Unavailable missing={data.missing ?? []} {onfix} />
{:else}
  <div class="pa-stack">
    <div class="pa-stats">
      <StatCard label={$t('portfolios.harvest.total')} value={fmtMoney(data.harvestable, currency)} hint={$t('portfolios.harvest.totalHint')} />
      <StatCard label={$t('portfolios.harvest.count')} value={String(data.candidates.length)} />
      <StatCard
        label={$t('portfolios.harvest.wash')}
        value={data.wash_window ? `${fmtDate(data.wash_window.from)} → ${fmtDate(data.wash_window.to)}` : $t('portfolios.harvest.noWash')}
      />
    </div>

    <Panel
      title={$t('portfolios.harvest.count')}
      subtitle={`${$t('portfolios.lots.method', { method: $t(`portfolios.lots.methods.${data.rules.method}`) })} ${
        data.rules.defaulted ? $t('portfolios.lots.defaulted') : $t('portfolios.lots.fromProfile', { name: data.rules.profile_name })
      }`}
      height={data.candidates.length === 0 ? 160 : null}
      unavailable={data.candidates.length === 0 ? { reason: $t('portfolios.harvest.none'), icon: 'check-circle' } : null}
      flush
    >
      {#snippet actions()}
        {#if data.candidates.length}
          <Button size="sm" icon="arrow-right" onclick={sendToTax}>{$t('portfolios.harvest.send')}</Button>
        {/if}
      {/snippet}
      <div class="pa-scroll">
        <table class="pa-table">
          <thead>
            <tr>
              <th>{$t('portfolios.detail.colSymbol')}</th>
              <th>{$t('portfolios.lots.opened')}</th>
              <th class="r">{$t('portfolios.detail.colQty')}</th>
              <th class="r">{$t('portfolios.lots.cost')}</th>
              <th class="r">{$t('portfolios.detail.colValue')}</th>
              <th class="r">{$t('portfolios.harvest.loss')}</th>
              <th>{$t('portfolios.lots.term')}</th>
              <th>{$t('portfolios.harvest.wash')}</th>
            </tr>
          </thead>
          <tbody>
            {#each data.candidates as c, i (i)}
              <tr>
                <td class="sym">{c.symbol}</td>
                <td>{fmtDate(c.opened)}</td>
                <td class="r">{fmtNum(c.qty)}</td>
                <td class="r">{fmtMoney(c.cost, currency)}</td>
                <td class="r">{fmtMoney(c.value, currency)}</td>
                <td class="r pa-down">{fmtSignedMoney(c.unrealized, currency)}</td>
                <td>{c.term ? $t(`portfolios.lots.terms.${c.term === 'short' ? 'short' : 'long'}`) : $t('portfolios.lots.noTerm')}</td>
                <td>
                  {#if c.wash_flag}
                    <Badge tone="warn" icon="alert-triangle">{$t('portfolios.harvest.washFlag', { date: fmtDate(c.last_buy) })}</Badge>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </Panel>

    {#if data.unpriced?.length}
      <p class="pa-warn">{$t('portfolios.harvest.unpriced', { list: data.unpriced.join(', ') })}</p>
    {/if}
  </div>
{/if}

<style>
  .sym {
    font-weight: 600;
  }
</style>
