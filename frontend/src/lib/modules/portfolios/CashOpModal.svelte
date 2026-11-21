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

  const CASH_KINDS = OP_KINDS.filter((k) => !k.trade && !k.corporate);
  // Corporate actions: units or cost basis change, no cash moves.
  const CORP_KINDS = OP_KINDS.filter((k) => k.corporate);
  let kind = $state('deposit');
  let assetId = $state('');
  let opDate = $state(dateKey());
  let amount = $state('');
  let fee = $state('');
  let ccy = $state(currency);
  let note = $state('');
  let ratioNum = $state('');
  let ratioDen = $state('');
  let childId = $state('');
  let childQty = $state('');
  let costShare = $state('');
  let saving = $state(false);
  let error = $state(null);

  // Income belongs to a holding when the user knows which; the rest is portfolio-level.
  const canName = $derived(opKind(kind).income === true);
  const sign = $derived(opKind(kind).sign);
  const corporate = $derived(opKind(kind).corporate === true);
  const isSplit = $derived(kind === 'split' || kind === 'reverse_split');
  const valid = $derived(
    corporate
      ? !!assetId &&
          (isSplit
            ? Number(ratioNum) > 0 && Number(ratioDen) > 0
            : !!childId && childId !== assetId && Number(childQty) > 0 && Number(costShare) >= 0 && Number(costShare) <= 100 && costShare !== '')
      : Number(amount) > 0
  );

  async function save() {
    if (!valid || saving) return;
    saving = true;
    error = null;
    try {
      const body = corporate
        ? isSplit
          ? { side: kind, op_date: opDate, asset_id: assetId, ratio_num: Number(ratioNum), ratio_den: Number(ratioDen), note: note.trim() }
          : {
              side: kind,
              op_date: opDate,
              asset_id: assetId,
              child_asset_id: childId,
              child_qty: Number(childQty),
              cost_share_pct: Number(costShare),
              note: note.trim()
            }
        : {
            side: kind,
            op_date: opDate,
            amount: Number(amount),
            fee: Number(fee) || 0,
            note: note.trim(),
            currency: ccy
          };
      if (!corporate && canName && assetId) body.asset_id = assetId;
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
  <div class="form">
  <div class="kinds">
    {#each CASH_KINDS as k (k.id)}
      <button class="kind" class:active={kind === k.id} onclick={() => (kind = k.id)}>
        <span class="sign" class:in={k.sign > 0} class:out={k.sign < 0}>{k.sign > 0 ? '+' : '−'}</span>
        {$t(`portfolios.kind.${k.id}`)}
      </button>
    {/each}
  </div>
  <p class="group">{$t('portfolios.corp.group')}</p>
  <div class="kinds">
    {#each CORP_KINDS as k (k.id)}
      <button class="kind" class:active={kind === k.id} onclick={() => (kind = k.id)}>
        {$t(`portfolios.kind.${k.id}`)}
      </button>
    {/each}
  </div>

  <Field label={$t('portfolios.cash.date')}>
    <input type="date" bind:value={opDate} />
  </Field>

  {#if corporate}
    <Field label={$t('portfolios.corp.asset')}>
      <Select
        bind:value={assetId}
        options={[{ value: '', label: $t('portfolios.corp.pickAsset') }].concat(
          assets.map((a) => ({ value: a.id, label: a.symbol }))
        )}
      />
    </Field>
    {#if isSplit}
      <div class="row even">
        <Field label={$t('portfolios.corp.ratioNum')}>
          <input type="number" step="any" min="0" bind:value={ratioNum} placeholder="2" />
        </Field>
        <Field label={$t('portfolios.corp.ratioDen')}>
          <input type="number" step="any" min="0" bind:value={ratioDen} placeholder="1" />
        </Field>
      </div>
      <p class="effect">{$t(kind === 'split' ? 'portfolios.corp.splitHint' : 'portfolios.corp.reverseHint')}</p>
    {:else}
      <Field label={$t('portfolios.corp.child')} hint={$t('portfolios.corp.childHint')}>
        <Select
          bind:value={childId}
          options={[{ value: '', label: $t('portfolios.corp.pickAsset') }].concat(
            assets.filter((a) => a.id !== assetId).map((a) => ({ value: a.id, label: a.symbol }))
          )}
        />
      </Field>
      <div class="row even">
        <Field label={$t('portfolios.corp.childQty')}>
          <input type="number" step="any" min="0" bind:value={childQty} />
        </Field>
        <Field label={$t('portfolios.corp.costShare')}>
          <input type="number" step="any" min="0" max="100" bind:value={costShare} placeholder="0 - 100" />
        </Field>
      </div>
      <p class="effect">{$t('portfolios.corp.spinHint')}</p>
    {/if}
  {:else}
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

  {/if}

  <Field label={$t('portfolios.cash.note')}>
    <input type="text" bind:value={note} />
  </Field>

  <ErrorText {error} />

  <!-- What the row will do to the balance, spelled out before it is written: a fee and a
       withdrawal both take money out and are one click apart. -->
  {#if !corporate}
    <p class="effect" class:in={sign > 0} class:out={sign < 0}>
      {$t(sign > 0 ? 'portfolios.cash.willAdd' : 'portfolios.cash.willRemove', {
        amount: amount || '0',
        currency: ccy
      })}
    </p>
  {/if}

  </div>

  {#snippet footer()}
    <button class="btn ghost" onclick={onclose}>{$t('common.cancel')}</button>
    <button class="btn primary" onclick={save} disabled={!valid || saving}>
      {saving ? $t('common.saving') : $t('portfolios.cash.book')}
    </button>
  {/snippet}
</Modal>

<style>
  /* One rhythm for the whole form: a 12px gap between fields, no per-field margins. */
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .form .kinds {
    margin-bottom: 0;
  }
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
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
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
  .row.even {
    grid-template-columns: 1fr 1fr;
  }
  .group {
    margin: 0 0 var(--space-2);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--muted);
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
  @media (max-width: 767px) {
    .kind {
      min-height: 44px;
    }
    .row {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
