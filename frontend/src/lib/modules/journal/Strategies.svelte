<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Strategy settings: manage strategies (name + signal names) and per-category capital
  // (beginning stack + refills of fresh capital).
  import { journalApi, fmtMoney, CURRENCIES } from './api.js';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  // Capital events, by what they do to the stack: money in, money out, or the
  // opening balance. Semantic tones — the page never names a color.
  const KIND_TONE = {
    initial: 'accent',
    refill: 'success',
    withdrawal: 'danger'
  };

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
      <Button variant="primary" icon="plus" onclick={addStrategy}>{$t('journal.strategies.add')}</Button>
    </div>
    {#if strategies.length === 0}
      <EmptyState icon="target" description={$t('journal.strategies.empty')} compact />
    {:else}
      <ul class="card-list">
        {#each strategies as s (s.id)}
          <li>
            <div>
              <span class="strong">{s.name}</span>
              {#if s.signals?.length}
                <span class="signals">{s.signals.join(' · ')}</span>
              {/if}
            </div>
            <button class="icon danger-hover" onclick={() => delStrategy(s)} aria-label={$t('journal.fees.schedules.delete')} title={$t('journal.fees.schedules.delete')}><Icon name="trash" size={14} /></button>
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
          {#each categories as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
        </select>
      </label>
      <div class="total">
        <span class="muted">{$t('journal.strategies.capital.invested')}</span>
        {#if totalsByCurrency.length === 0}
          <strong class="num">—</strong>
        {:else}
          {#each totalsByCurrency as [cur, amt] (cur)}
            <!-- A net total can go negative (withdrawals exceed deposits). It's a
                 balance, not a result, so it keeps its plain formatting — but the
                 figures are tabular so several currencies stack in alignment. -->
            <strong class="num">{fmtMoney(amt, cur)}</strong>
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
        {#each CURRENCIES as c (c.id)}<option value={c.id}>{c.id}</option>{/each}
      </select>
      <input placeholder={$t('journal.strategies.capital.notePlaceholder')} bind:value={capNote} />
      <Button variant="primary" icon="plus" onclick={addCapital}>{$t('journal.strategies.add')}</Button>
    </div>

    {#if events.length === 0}
      <EmptyState icon="coins" description={$t('journal.strategies.capital.empty')} compact />
    {:else}
      <ul class="card-list">
        {#each events as ev (ev.id)}
          <li>
            <div class="ev">
              <!-- Money in vs money out: a semantic tone, not a decoration. -->
              <Badge tone={KIND_TONE[ev.kind]}>{ev.kind}</Badge>
              <span class="strong num">{fmtMoney(ev.amount, ev.currency)}</span>
              {#if ev.note}<span class="muted">— {ev.note}</span>{/if}
              <span class="muted date">{new Date(ev.occurred_at).toLocaleDateString()}</span>
            </div>
            <button class="icon danger-hover" onclick={() => delCapital(ev)} aria-label={$t('journal.fees.schedules.delete')} title={$t('journal.fees.schedules.delete')}><Icon name="trash" size={14} /></button>
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
  /* .card / .card h3 / .hint / .card-list come from theme/components.css. */
  .hint {
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
    font-family: var(--mono);
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
  }

  /* One capital event: badge, amount, optional note, date. */
  .ev {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .signals {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-left: var(--space-2);
  }
  .date {
    margin-left: auto;
    white-space: nowrap;
    font-family: var(--mono);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-xs);
  }
</style>
