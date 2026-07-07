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
  let displayCurrency = $state(settings.display_currency ?? 'USD');
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
        {#each CURRENCIES as c}<option value={c.id}>{c.label}</option>{/each}
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
          {#each FEE_AMOUNT_KINDS as k}<option value={k.id}>{k.label}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{$t('journal.fees.schedules.charged')}</span>
        <select bind:value={per}>
          {#each FEE_PER as p}<option value={p.id}>{p.label}</option>{/each}
        </select>
      </label>
      <label class="field" class:dim={amountKind === 'pct'}>
        <span>{$t('journal.fees.schedules.currency')}</span>
        <select bind:value={currency} disabled={amountKind === 'pct'}>
          {#each CURRENCIES as c}<option value={c.id}>{c.id}</option>{/each}
        </select>
      </label>
      <button class="primary" onclick={addSchedule}>{$t('journal.fees.schedules.add')}</button>
    </div>

    {#if schedules.length === 0}
      <p class="muted">{$t('journal.fees.schedules.empty')}</p>
    {:else}
      <ul class="list">
        {#each schedules as s (s.id)}
          <li>
            <div>
              <span class="strong">{s.name}</span>
              <span class="muted desc">{describe(s)}</span>
            </div>
            <button class="icon" onclick={() => delSchedule(s)} title={$t('journal.fees.schedules.delete')}><Icon name="trash" size={14} /></button>
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
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }
  .card h3 {
    font-size: 0.95rem;
    font-weight: 600;
  }
  .hint {
    color: var(--muted);
    font-size: 0.82rem;
    margin: var(--space-2) 0 var(--space-4);
  }
  .add-grid {
    display: grid;
    grid-template-columns: 1.6fr 0.9fr 1fr 1fr 0.8fr auto;
    gap: var(--space-2);
    align-items: end;
    margin-bottom: var(--space-4);
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
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .list li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 0.85rem;
  }
  .strong {
    font-weight: 600;
  }
  .desc {
    margin-left: var(--space-2);
  }
  .icon:hover {
    color: var(--text);
  }
  .muted {
    color: var(--muted);
    font-size: 0.82rem;
  }
</style>
