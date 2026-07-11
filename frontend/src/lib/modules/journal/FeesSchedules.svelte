<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Fees & currency settings: reusable fee schedules (applied to a trade as a shortcut)
  // and the journal-wide display currency for the breakdown.
  import {
    journalApi,
    fmtMoney,
    CURRENCIES,
    FEE_AMOUNT_KINDS,
    FEE_PER
  } from './api.js';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { schedules = [], settings = { display_currency: 'USD' }, onchanged = () => {} } = $props();

  // Modal confirm (replaces native confirm()).
  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});
  function askConfirm(message, onyes) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  // ── New fee schedule ──
  let name = $state('');
  let amount = $state(null);
  let amountKind = $state('fixed');
  let per = $state('trade');
  let currency = $state('USD');

  async function addSchedule() {
    const n = name.trim();
    if (!n) return;
    await journalApi.addFeeSchedule({
      name: n,
      amount: Number(amount) || 0,
      amount_kind: amountKind,
      per,
      currency
    });
    name = '';
    amount = null;
    amountKind = 'fixed';
    per = 'trade';
    currency = 'USD';
    onchanged();
  }

  function delSchedule(s) {
    askConfirm($t('journal.fees.confirmDelete', { name: s.name }), async () => {
      await journalApi.deleteFeeSchedule(s.id);
      onchanged();
    });
  }

  function describe(s) {
    const perLabel = FEE_PER.find((p) => p.id === s.per)?.label ?? s.per;
    const amt = s.amount_kind === 'pct' ? `${s.amount}%` : fmtMoney(s.amount, s.currency);
    return `${amt} · ${perLabel.toLowerCase()}`;
  }

  // ── Display currency ──
  // Local state rather than $derived, because the select updates optimistically before
  // the round-trip lands. The $effect re-syncs it when the parent hands down a fresh
  // `settings` (after onchanged, or when another tab saved). Reading the prop inside the
  // effect — not in the initializer — is what makes it track later values.
  let displayCurrency = $state('USD');
  $effect(() => {
    displayCurrency = settings.display_currency ?? 'USD';
  });

  async function saveDisplayCurrency(e) {
    const next = e.target.value;
    displayCurrency = next;
    await journalApi.updateSettings({ display_currency: next });
    onchanged();
  }
</script>

<div class="settings">
  <section class="card">
    <h3>{$t('journal.fees.displayCurrency.title')}</h3>
    <p class="hint">{$t('journal.fees.displayCurrency.hint')}</p>
    <label class="field inline">
      <span>{$t('journal.fees.displayCurrency.label')}</span>
      <select value={displayCurrency} onchange={saveDisplayCurrency}>
        {#each CURRENCIES as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
      </select>
    </label>
  </section>

  <section class="card">
    <h3>{$t('journal.fees.schedules.title')}</h3>
    <p class="hint">{$t('journal.fees.schedules.hint')}</p>

    <div class="add-grid">
      <label class="field">
        <span>{$t('journal.fees.schedules.name')}</span>
        <input placeholder={$t('journal.fees.schedules.namePlaceholder')} bind:value={name} />
      </label>
      <label class="field">
        <span>{$t('journal.fees.schedules.amount')}</span>
        <input type="number" step="any" placeholder="0" bind:value={amount} />
      </label>
      <label class="field">
        <span>{$t('journal.fees.schedules.kind')}</span>
        <select bind:value={amountKind}>
          {#each FEE_AMOUNT_KINDS as k (k.id)}<option value={k.id}>{k.label}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{$t('journal.fees.schedules.charged')}</span>
        <select bind:value={per}>
          {#each FEE_PER as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
        </select>
      </label>
      <label class="field" class:dim={amountKind === 'pct'}>
        <span>{$t('journal.fees.schedules.currency')}</span>
        <select bind:value={currency} disabled={amountKind === 'pct'}>
          {#each CURRENCIES as c (c.id)}<option value={c.id}>{c.id}</option>{/each}
        </select>
      </label>
      <Button variant="primary" icon="plus" onclick={addSchedule}>{$t('journal.fees.schedules.add')}</Button>
    </div>

    {#if schedules.length === 0}
      <EmptyState icon="receipt" description={$t('journal.fees.schedules.empty')} compact />
    {:else}
      <ul class="card-list">
        {#each schedules as s (s.id)}
          <li>
            <div>
              <span class="strong">{s.name}</span>
              <span class="muted desc">{describe(s)}</span>
            </div>
            <button class="icon danger-hover" onclick={() => delSchedule(s)} aria-label={$t('journal.fees.schedules.delete')} title={$t('journal.fees.schedules.delete')}><Icon name="trash" size={14} /></button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('journal.fees.deleteModal.title')}
  message={confirmMessage}
  confirmLabel={$t('journal.fees.deleteModal.confirm')}
  danger
  onconfirm={onConfirmYes}
/>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    max-width: 760px;
  }
  /* .card / .card h3 / .hint / .card-list come from theme/components.css. */
  .hint {
    margin: var(--space-2) 0 var(--space-4);
  }
  /* Six fixed tracks overflow below ~900px; let them wrap instead. */
  .add-grid {
    display: grid;
    grid-template-columns: 1.6fr 0.9fr 1fr 1fr 0.8fr auto;
    gap: var(--space-2);
    align-items: end;
    margin-bottom: var(--space-4);
  }
  @media (max-width: 900px) {
    .add-grid {
      grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    }
  }
  .field.inline {
    flex-direction: row;
    align-items: center;
    gap: var(--space-3);
  }
  .field.dim {
    opacity: 0.5;
  }
  input,
  select {
    width: 100%;
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .desc {
    margin-left: var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-xs);
  }
</style>
