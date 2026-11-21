<script>
  // Settings, Brokers: background trading on one account (`/api/brokers/{id}/background-trading`).
  // Shown with the trading grant: it says whether orders armed on this account may be sent with
  // nobody watching, since when, and keeps every signature made on it readable as it was signed.
  //
  // Turning it on opens the signature (BackgroundTradingModal). Turning it off asks nothing on
  // the server, but it disarms everything armed on the account, so the card confirms first.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import BackgroundTradingModal from './BackgroundTradingModal.svelte';
  import { brokersApi } from './api.js';
  import { t } from '$lib/i18n';

  let { account, onchanged } = $props();

  let history = $state([]);
  let error = $state('');
  let busy = $state(false);
  let signOpen = $state(false);
  let offOpen = $state(false);
  let viewing = $state(null);
  let viewOpen = $state(false);

  const accountId = $derived(account?.id ?? null);
  const since = $derived(account?.background_trading_since ?? null);
  const inForce = $derived(account?.background_trading_ack ?? null);

  async function load(id) {
    try {
      history = (await brokersApi.backgroundTrading(id)).history ?? [];
      error = '';
    } catch (e) {
      error = e.message;
    }
  }
  $effect(() => {
    // Re-read when the signature in force changes, not on every list refresh.
    void inForce;
    if (accountId) load(accountId);
  });

  async function turnOff() {
    busy = true;
    error = '';
    try {
      await brokersApi.revokeBackgroundTrading(account.id);
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  function view(record) {
    viewing = record;
    viewOpen = true;
  }

  const when = (iso) => new Date(iso).toLocaleString();
</script>

<div class="bg box">
  <div class="head">
    <span class="title">{$t('brokers.background.title')}</span>
    <span class="state" class:on={!!since}>
      {since ? $t('brokers.background.on', { time: when(since) }) : $t('brokers.background.off')}
    </span>
  </div>
  <p class="hint">{$t('brokers.background.hint')}</p>
  {#if history.length}
    <ul class="history" aria-label={$t('brokers.background.history')}>
      {#each history as h (h.id)}
        <li>
          <button class="link" onclick={() => view(h)}>
            <Icon name="file-text" size={11} />
            {when(h.signed_at)}
          </button>
          <span class="muted">{$t(`brokers.background.env.${h.environment}`)}</span>
          <span class="muted">{h.second_factor ? $t('brokers.background.factorYes') : $t('brokers.background.factorNo')}</span>
          {#if h.id === inForce}<span class="tag">{$t('brokers.background.inForce')}</span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
  <div class="actions">
    {#if since}
      <Button onclick={() => (offOpen = true)} loading={busy} disabled={busy}>{$t('brokers.background.turnOff')}</Button>
    {:else}
      <Button variant="danger" icon="lock" onclick={() => (signOpen = true)}>{$t('brokers.background.turnOn')}</Button>
    {/if}
  </div>
  <ErrorText {error} />
</div>

<BackgroundTradingModal bind:open={signOpen} {account} onsigned={() => onchanged?.()} />
<BackgroundTradingModal bind:open={viewOpen} {account} record={viewing} />

<ConfirmModal
  bind:open={offOpen}
  title={$t('brokers.background.turnOff')}
  message={$t('brokers.background.turnOffMessage', { name: account?.name ?? '' })}
  confirmLabel={$t('brokers.background.turnOff')}
  cancelLabel={$t('common.cancel')}
  onconfirm={turnOff}
/>

<style>
  .bg {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .title {
    font-weight: var(--fw-medium);
  }
  .state {
    margin-left: auto;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .state.on {
    color: var(--amber);
  }
  .hint {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .history {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--text-sm);
  }
  .history li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .muted {
    color: var(--muted);
  }
  .tag {
    font-size: var(--text-xs);
    color: var(--green);
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
</style>
