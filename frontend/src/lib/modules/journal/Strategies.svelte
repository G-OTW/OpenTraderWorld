<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Strategy settings: manage strategies (name + signal names) and per-category capital
  // (beginning stack + refills of fresh capital).
  import { journalApi, fmtMoney, CURRENCIES } from './api.js';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { t } from '$lib/i18n';

  let { categories = [], strategies = [], oncategoriesChanged = () => {}, onstrategiesChanged = () => {} } =
    $props();

  // Modal confirm (replaces native confirm()).
  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});
  function askConfirm(message, onyes) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  // ── Strategies ──
  let newStratName = $state('');
  let newStratSignals = $state(''); // comma-separated

  async function addStrategy() {
    const name = newStratName.trim();
    if (!name) return;
    const signals = newStratSignals
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
    await journalApi.addStrategy({ name, signals });
    newStratName = '';
    newStratSignals = '';
    onstrategiesChanged();
  }

  function delStrategy(s) {
    askConfirm($t('journal.strategies.confirmDelete', { name: s.name }), async () => {
      await journalApi.deleteStrategy(s.id);
      onstrategiesChanged();
    });
  }

  // ── Capital per category ──
  let capCategory = $state('');
  let events = $state([]);
  let capKind = $state('refill');
  let capAmount = $state(null);
  let capCurrency = $state('USD');
  let capNote = $state('');

  $effect(() => {
    if (!capCategory && categories.length) capCategory = categories[0].id;
  });

  $effect(() => {
    const id = capCategory;
    if (!id) {
      events = [];
      return;
    }
    journalApi.listCapital(id).then((e) => {
      events = e;
    });
  });

  // Invested capital grouped by currency (no FX conversion — values stay as entered).
  const totalsByCurrency = $derived.by(() => {
    const m = new Map();
    for (const e of events) {
      const signed = e.kind === 'withdrawal' ? -e.amount : e.amount;
      m.set(e.currency, (m.get(e.currency) ?? 0) + signed);
    }
    return [...m.entries()];
  });

  async function addCapital() {
    const amount = Number(capAmount);
    if (!amount || !capCategory) return;
    await journalApi.addCapital(capCategory, {
      kind: capKind,
      amount,
      currency: capCurrency,
      note: capNote || null
    });
    capAmount = null;
    capNote = '';
    events = await journalApi.listCapital(capCategory);
  }

  async function delCapital(ev) {
    await journalApi.deleteCapital(ev.id);
    events = await journalApi.listCapital(capCategory);
  }
</script>

<div class="settings">
  <section class="card">
    <h3>{$t('journal.strategies.title')}</h3>
    <p class="hint">{$t('journal.strategies.hint')}</p>
    <div class="add-row">
      <input placeholder={$t('journal.strategies.namePlaceholder')} bind:value={newStratName} />
      <input placeholder={$t('journal.strategies.signalsPlaceholder')} bind:value={newStratSignals} />
      <button class="primary" onclick={addStrategy}>{$t('journal.strategies.add')}</button>
    </div>
    {#if strategies.length === 0}
      <p class="muted">{$t('journal.strategies.empty')}</p>
    {:else}
      <ul class="list">
        {#each strategies as s (s.id)}
          <li>
            <div>
              <span class="strong">{s.name}</span>
              {#if s.signals?.length}
                <span class="signals">{s.signals.join(' · ')}</span>
              {/if}
            </div>
            <button class="icon" onclick={() => delStrategy(s)} title={$t('journal.fees.schedules.delete')}><Icon name="trash" size={14} /></button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="card">
    <h3>{$t('journal.strategies.capital.title')}</h3>
    <p class="hint">{$t('journal.strategies.capital.hint')}</p>
    <div class="cap-head">
      <label class="field">
        <span>{$t('journal.strategies.capital.category')}</span>
        <select bind:value={capCategory}>
          {#each categories as c}<option value={c.id}>{c.name}</option>{/each}
        </select>
      </label>
      <div class="total">
        <span class="muted">{$t('journal.strategies.capital.invested')}</span>
        {#if totalsByCurrency.length === 0}
          <strong>—</strong>
        {:else}
          {#each totalsByCurrency as [cur, amt]}
            <strong>{fmtMoney(amt, cur)}</strong>
          {/each}
        {/if}
      </div>
    </div>

    <div class="add-row">
      <select bind:value={capKind}>
        <option value="initial">{$t('journal.strategies.capital.kind.initial')}</option>
        <option value="refill">{$t('journal.strategies.capital.kind.refill')}</option>
        <option value="withdrawal">{$t('journal.strategies.capital.kind.withdrawal')}</option>
      </select>
      <input type="number" step="any" placeholder={$t('journal.strategies.capital.amount')} bind:value={capAmount} />
      <select bind:value={capCurrency} title={$t('journal.strategies.capital.currency')}>
        {#each CURRENCIES as c}<option value={c.id}>{c.id}</option>{/each}
      </select>
      <input placeholder={$t('journal.strategies.capital.notePlaceholder')} bind:value={capNote} />
      <button class="primary" onclick={addCapital}>{$t('journal.strategies.add')}</button>
    </div>

    {#if events.length === 0}
      <p class="muted">{$t('journal.strategies.capital.empty')}</p>
    {:else}
      <ul class="list">
        {#each events as ev (ev.id)}
          <li>
            <div>
              <span class="kind {ev.kind}">{ev.kind}</span>
              <span class="strong">{fmtMoney(ev.amount, ev.currency)}</span>
              {#if ev.note}<span class="muted"> — {ev.note}</span>{/if}
              <span class="muted date">{new Date(ev.occurred_at).toLocaleDateString()}</span>
            </div>
            <button class="icon" onclick={() => delCapital(ev)} title={$t('journal.fees.schedules.delete')}><Icon name="trash" size={14} /></button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('journal.strategies.confirmTitle')}
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
  .add-row {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
    margin-bottom: var(--space-4);
  }
  .add-row input,
  .add-row select {
    flex: 1;
    min-width: 120px;
  }
  .cap-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
    margin-bottom: var(--space-4);
  }
  .total {
    text-align: right;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .total strong {
    font-size: 1.15rem;
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
  .signals {
    color: var(--muted);
    font-size: 0.8rem;
    margin-left: var(--space-2);
  }
  .kind {
    text-transform: uppercase;
    font-size: 0.65rem;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 999px;
    margin-right: var(--space-2);
    background: var(--surface-2, var(--bg));
    color: var(--muted);
  }
  .kind.initial {
    color: var(--accent);
  }
  .kind.refill {
    color: var(--green);
  }
  .kind.withdrawal {
    color: var(--red);
  }
  .date {
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
