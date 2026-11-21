<script>
  // Promote to portfolio: add a watched symbol to a chosen Portfolio Tracker portfolio with a
  // first buy prefilled at the current price. Goes through the Portfolio Tracker's own
  // endpoints (add asset, then book the buy), so its rules and validation apply unchanged.
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Field from '$lib/ui/Field.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { goto } from '$app/navigation';
  import { portfoliosApi, CURRENCIES } from '$lib/modules/portfolios/api.js';
  import { watchlistsApi } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), item = null, ondone = () => {} } = $props();

  let portfolios = $state([]);
  let portfolioId = $state('');
  let qty = $state('');
  let price = $state('');
  let date = $state('');
  let currency = $state('USD');
  let busy = $state(false);
  let error = $state('');
  let done = $state(false);

  const today = () => new Date().toISOString().slice(0, 10);

  $effect(() => {
    if (!open || !item) return;
    error = '';
    done = false;
    qty = '';
    date = today();
    const p = item.quote?.price_usd;
    price = typeof p === 'number' && isFinite(p) ? String(p) : '';
    // Yahoo quotes a listing in its own currency; CoinGecko and the default path quote USD.
    currency = item.quote?.currency && CURRENCIES.includes(item.quote.currency) ? item.quote.currency : 'USD';
    watchlistsApi
      .portfolios()
      .then((list) => {
        portfolios = list ?? [];
        if (!portfolios.some((x) => x.id === portfolioId)) portfolioId = portfolios[0]?.id ?? '';
      })
      .catch((e) => (error = e.message));
  });

  const valid = $derived(!!portfolioId && Number(qty) > 0 && Number(price) > 0 && !!date);

  async function save() {
    if (!valid || busy) return;
    busy = true;
    error = '';
    try {
      const asset = await portfoliosApi.addAsset(portfolioId, {
        asset_class: item.asset_class,
        provider: item.provider,
        provider_id: item.provider_id,
        symbol: item.symbol,
        name: item.name,
        currency
      });
      await portfoliosApi.addOperation(asset.id, {
        side: 'buy',
        quantity: Number(qty),
        price: Number(price),
        op_date: date,
        currency
      });
      done = true;
      ondone();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open size="sm" title={$t('watchlists.promote.title', { symbol: item?.symbol ?? '' })}>
  <div class="form">
    {#if portfolios.length === 0}
      <p class="muted">{$t('watchlists.promote.noPortfolio')}</p>
    {:else}
      <div class="field">
        <span class="lbl">{$t('watchlists.promote.portfolio')}</span>
        <Dropdown
          bind:value={portfolioId}
          ariaLabel={$t('watchlists.promote.portfolio')}
          options={portfolios.map((p) => ({ value: p.id, label: p.name }))}
        />
      </div>
      <div class="pair">
        <Field id="wl-promote-qty" label={$t('watchlists.promote.qty')}>
          <input id="wl-promote-qty" class="num" type="number" step="any" min="0" bind:value={qty} />
        </Field>
        <Field id="wl-promote-price" label={$t('watchlists.promote.price')}>
          <input id="wl-promote-price" class="num" type="number" step="any" min="0" bind:value={price} />
        </Field>
        <Field id="wl-promote-date" label={$t('watchlists.promote.date')}>
          <input id="wl-promote-date" type="date" max={today()} bind:value={date} />
        </Field>
        <div class="field">
          <span class="lbl">{$t('watchlists.promote.currency')}</span>
          <Dropdown
            bind:value={currency}
            ariaLabel={$t('watchlists.promote.currency')}
            options={CURRENCIES.map((c) => ({ value: c, label: c }))}
          />
        </div>
      </div>
      {#if done}<p class="ok"><Icon name="check-circle" size={13} /> {$t('watchlists.promote.done')}</p>{/if}
    {/if}
    <ErrorText {error} copyable />
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{done ? $t('common.close') : $t('common.cancel')}</Button>
    {#if !done}
      <Button variant="primary" onclick={save} loading={busy} disabled={!valid}>{$t('watchlists.promote.add')}</Button>
    {:else}
      <Button variant="primary" icon="briefcase" onclick={() => ((open = false), goto(`/portfolios?id=${portfolioId}`))}>{$t('watchlists.promote.open')}</Button>
    {/if}
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  /* Same label as the shared Field. */
  .lbl {
    font-size: 12px;
    font-weight: 500;
    color: var(--muted);
    line-height: var(--lh-tight);
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .pair :global(input) {
    width: 100%;
    min-width: 0;
  }
  .num {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--muted);
    font-size: 13px;
    margin: 0;
  }
  .ok {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--green);
    font-size: 13px;
    margin: 0;
  }
  @media (max-width: 520px) {
    .pair {
      grid-template-columns: 1fr;
    }
  }
</style>
