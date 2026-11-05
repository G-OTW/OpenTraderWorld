<script>
  // A paper account's money: what it is worth, cash in and out, a reset, and the orders that
  // stopped working (a rejection says why here). Orders and positions themselves are the
  // chart's, like any broker account's.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import AccountSummary from './AccountSummary.svelte';
  import { brokersApi } from './api.js';
  import { poll } from './poll.js';
  import { t } from '$lib/i18n';

  let { account, onchanged } = $props();

  let view = $state(null);
  let error = $state('');
  let amount = $state('');
  let busy = $state(false);
  let resetOpen = $state(false);

  async function load() {
    try {
      view = await brokersApi.paper(account.id);
      error = '';
    } catch (e) {
      error = e.message;
    }
  }
  // Kept fresh while shown: a fill or a mark moves the money without anyone touching this card.
  $effect(() => {
    account.id;
    view = null;
    return poll(load);
  });

  async function move(kind) {
    busy = true;
    error = '';
    try {
      const n = Number(amount);
      await brokersApi.paperCash(account.id, kind, kind === 'reset' && !(n > 0) ? null : n);
      amount = '';
      await load();
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  const money = (v) =>
    v == null || !Number.isFinite(v)
      ? '–'
      : v.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const s = $derived(view?.summary ?? null);
  const startBalance = $derived(`${account.config?.start_balance ?? ''}`);
</script>

<div class="paper box">
  <span class="sname">{$t('brokers.paper.title')}</span>
  {#if s}
    <AccountSummary
      summary={{
        currency: s.currency,
        equity: s.equity,
        cash: s.cash,
        unrealized: s.unrealized,
        realized: s.realized,
        fees: s.fees,
        margin_used: s.margin_used,
        available: s.buying_power,
        gross_position_value: s.exposure
      }}
    />
    <p class="hint">{$t('brokers.paper.funded')}: {money(s.funded)} {s.currency}</p>
    {#if s.unpriced?.length}
      <p class="note">
        <Icon name="alert-triangle" size={12} />
        {$t('brokers.paper.unpriced', { list: s.unpriced.join(', '), currency: s.currency })}
      </p>
    {/if}
  {/if}
  <div class="cash">
    <input
      type="number"
      min="0"
      step="any"
      bind:value={amount}
      placeholder={s?.currency ?? ''}
      aria-label={$t('brokers.paper.amount')}
    />
    <Button onclick={() => move('deposit')} disabled={busy || !(Number(amount) > 0)}>
      {$t('brokers.paper.deposit')}
    </Button>
    <Button onclick={() => move('withdraw')} disabled={busy || !(Number(amount) > 0)}>
      {$t('brokers.paper.withdraw')}
    </Button>
    <Button variant="danger" icon="rotate-ccw" onclick={() => (resetOpen = true)} disabled={busy}>
      {$t('brokers.paper.reset')}
    </Button>
  </div>
  <p class="hint">{$t('brokers.paper.hint')}</p>

  {#if view?.history?.length}
    <details class="history">
      <summary>{$t('brokers.paper.history', { n: view.history.length })}</summary>
      <ul>
        {#each view.history as o (o.id)}
          <li class:bad={o.status === 'rejected'}>
            <span class="when">{new Date(o.placed_at).toLocaleString()}</span>
            <span>{o.side} {o.qty} {o.ticker}</span>
            <span class="muted">{o.order_type}{o.limit_price ? ` ${o.limit_price}` : ''}{o.stop_price ? ` / ${o.stop_price}` : ''}</span>
            <span class="status">{$t(`brokers.paper.status.${o.status}`)}</span>
            {#if o.reason}<span class="muted">{o.reason}</span>{/if}
          </li>
        {/each}
      </ul>
    </details>
  {/if}
  <ErrorText {error} />
</div>

<ConfirmModal
  bind:open={resetOpen}
  title={$t('brokers.paper.reset')}
  message={$t('brokers.paper.resetMessage', {
    name: account.name,
    amount: Number(amount) > 0 ? amount : startBalance,
    currency: s?.currency ?? ''
  })}
  confirmLabel={$t('brokers.paper.reset')}
  danger
  onconfirm={() => move('reset')}
/>

<style>
  .paper {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .sname {
    font-size: var(--text-base);
    color: var(--muted);
  }
  .cash {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
  }
  .cash input {
    width: 140px;
  }
  .hint,
  .note {
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: 1.4;
    max-width: 66ch;
    margin: 0;
  }
  .note {
    display: flex;
    gap: var(--space-1);
    align-items: baseline;
    color: var(--amber);
  }
  .history summary {
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .history ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-2);
    max-height: 240px;
    overflow: auto;
  }
  .history li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    font-size: var(--text-xs);
    font-family: var(--mono);
  }
  .history li.bad .status {
    color: var(--red);
  }
  .when,
  .muted {
    color: var(--muted);
  }
</style>
