<script>
  // Book a ledger row that is not a trade: a deposit, a withdrawal, income, a fee, a tax.
  //
  // One form for all of them, because they differ in exactly two ways: the sign they have on
  // cash, and whether naming a line makes sense. A dividend can name the holding that paid
  // it; a bank interest row names nothing, and forcing one would be asking a question with
  // no answer.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Field from '$lib/ui/Field.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { dateKey } from '$lib/format.js';
  import { OP_KINDS, opKind, CURRENCIES, portfoliosApi } from './api.js';

  let { portfolioId, assets = [], currency = 'USD', onclose, onsaved } = $props();

  const CASH_KINDS = OP_KINDS.filter((k) => !k.trade);
  let kind = $state('deposit');
  let assetId = $state('');
  let opDate = $state(dateKey());
  let amount = $state('');
  let fee = $state('');
  let ccy = $state(currency);
  let note = $state('');
  let saving = $state(false);
  let error = $state(null);

  // Income belongs to a holding when the user knows which; the rest is portfolio-level.
  const canName = $derived(opKind(kind).income === true);
  const sign = $derived(opKind(kind).sign);
  const valid = $derived(Number(amount) > 0);

  async function save() {
    if (!valid || saving) return;
    saving = true;
    error = null;
    try {
      const body = {
        side: kind,
        op_date: opDate,
        amount: Number(amount),
        fee: Number(fee) || 0,
        note: note.trim(),
        currency: ccy
      };
      if (canName && assetId) body.asset_id = assetId;
      await portfoliosApi.addPortfolioOperation(portfolioId, body);
      onsaved?.();
      onclose?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal open title={$t('portfolios.cash.title')} size="sm" {onclose}>
  <div class="kinds">
    {#each CASH_KINDS as k (k.id)}
      <button class="kind" class:active={kind === k.id} onclick={() => (kind = k.id)}>
        <span class="sign" class:in={k.sign > 0} class:out={k.sign < 0}>{k.sign > 0 ? '+' : '−'}</span>
        {$t(`portfolios.kind.${k.id}`)}
      </button>
    {/each}
  </div>

  <Field label={$t('portfolios.cash.date')}>
    <input type="date" bind:value={opDate} />
  </Field>

  <div class="row">
    <Field label={$t('portfolios.cash.amount')}>
      <input type="number" step="any" min="0" bind:value={amount} placeholder="0.00" />
    </Field>
    <Field label={$t('portfolios.cash.currency')}>
      <Select bind:value={ccy} options={CURRENCIES.map((c) => ({ value: c, label: c }))} />
    </Field>
  </div>

  <Field label={$t('portfolios.cash.fee')} hint={$t('portfolios.cash.feeHint')}>
    <input type="number" step="any" min="0" bind:value={fee} placeholder="0.00" />
  </Field>

  {#if canName}
    <Field label={$t('portfolios.cash.asset')} hint={$t('portfolios.cash.assetHint')}>
      <Select
        bind:value={assetId}
        options={[{ value: '', label: $t('portfolios.cash.noAsset') }].concat(
          assets.map((a) => ({ value: a.id, label: a.symbol }))
        )}
      />
    </Field>
  {/if}

  <Field label={$t('portfolios.cash.note')}>
    <input type="text" bind:value={note} />
  </Field>

  <ErrorText {error} />

  <!-- What the row will do to the balance, spelled out before it is written: a fee and a
       withdrawal both take money out and are one click apart. -->
  <p class="effect" class:in={sign > 0} class:out={sign < 0}>
    {$t(sign > 0 ? 'portfolios.cash.willAdd' : 'portfolios.cash.willRemove', {
      amount: amount || '0',
      currency: ccy
    })}
  </p>

  {#snippet footer()}
    <button class="btn ghost" onclick={onclose}>{$t('common.cancel')}</button>
    <button class="btn primary" onclick={save} disabled={!valid || saving}>
      {saving ? $t('common.saving') : $t('portfolios.cash.book')}
    </button>
  {/snippet}
</Modal>

<style>
  .kinds {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  .kind {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .kind.active {
    border-color: var(--accent);
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .sign.in {
    color: var(--green);
  }
  .sign.out {
    color: var(--red);
  }
  .row {
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: var(--space-3);
  }
  .effect {
    margin: var(--space-3) 0 0;
    font-size: 12px;
    color: var(--muted);
  }
  .effect.in {
    color: var(--green);
  }
  .effect.out {
    color: var(--red);
  }
</style>
