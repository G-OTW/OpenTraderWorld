<script>
  // Settings, Brokers: live trading on one account (`POST /api/brokers/{id}/live`). Only an
  // account that trades real money needs it; testnet, demo and paper accounts never do.
  //
  // Turning it on needs the broker's order path rated E4 (or this account named for the live
  // canary) and the account's limits set. The trading lock is optional: when it is on, the API
  // client asks for the authenticator code and runs the request again (see
  // $lib/reauth.svelte.js); when it is off, a banner says so and blocks nothing. Turning it off
  // asks nothing.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { brokersApi } from './api.js';
  import { settingsApi } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';

  let { account, onchanged } = $props();

  let limits = $state(null);
  let lockOn = $state(true);
  let busy = $state(false);
  let error = $state('');

  $effect(() => {
    const id = account?.id;
    if (!id) return;
    brokersApi
      .risk()
      .then((r) => (limits = r.limits?.[id] ?? {}))
      .catch((e) => (error = e.message));
  });

  $effect(() => {
    settingsApi
      .tradingLock()
      .then((r) => (lockOn = !!r.enabled))
      .catch(() => {});
  });

  const level = $derived(account?.order_caps?.level ?? 'E0');
  const on = $derived(!!account?.live_since);
  /** The limits a live account must have, each with whether it is set. */
  const required = $derived(
    limits == null
      ? []
      : [
          { key: 'size', set: limits.max_order_qty != null || limits.max_order_value != null },
          { key: 'position', set: limits.max_position_qty != null || limits.max_position_value != null },
          { key: 'loss', set: limits.max_loss != null }
        ]
  );

  async function setLive(enabled) {
    busy = true;
    error = '';
    try {
      await brokersApi.setLive(account.id, enabled);
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<div class="live box">
  <div class="head">
    <span class="title">{$t('brokers.live.title')}</span>
    <span class="state" class:on>{on ? $t('brokers.live.on', { time: new Date(account.live_since).toLocaleString() }) : $t('brokers.live.off')}</span>
  </div>
  <p class="hint">{$t('brokers.live.hint')}</p>
  <dl class="facts">
    <div><dt>{$t('brokers.live.level')}</dt><dd class="mono" class:ok={level === 'E4'}>{level}</dd></div>
    <div><dt>{$t('brokers.live.disconnect')}</dt><dd>{account?.order_caps?.cancel_on_disconnect ? $t('brokers.live.disconnectYes') : $t('brokers.live.disconnectNo')}</dd></div>
    {#each required as r (r.key)}
      <div>
        <dt>{$t(`brokers.live.limit.${r.key}`)}</dt>
        <dd class:ok={r.set} class:bad={!r.set}><Icon name={r.set ? 'check' : 'alert-triangle'} size={11} /> {r.set ? $t('brokers.set') : $t('brokers.notSet')}</dd>
      </div>
    {/each}
  </dl>
  {#if !lockOn}
    <p class="lock-off"><Icon name="info" size={12} /> {$t('brokers.live.lockOff')}</p>
  {/if}
  {#if account?.live_canary}
    <p class="canary"><Icon name="alert-triangle" size={12} /> {$t('brokers.live.canary')}</p>
  {/if}
  <div class="actions">
    {#if on}
      <Button onclick={() => setLive(false)} loading={busy} disabled={busy}>{$t('brokers.live.turnOff')}</Button>
    {:else}
      <Button variant="danger" icon="lock" onclick={() => setLive(true)} loading={busy} disabled={busy}>{$t('brokers.live.turnOn')}</Button>
    {/if}
  </div>
  <ErrorText {error} />
</div>

<style>
  .live {
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
    color: var(--red);
  }
  .hint {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    margin: 0;
  }
  .facts dt {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .facts dd {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    font-size: var(--text-sm);
  }
  .facts dd.ok {
    color: var(--green);
  }
  .facts dd.bad {
    color: var(--amber);
  }
  .mono {
    font-family: var(--mono);
  }
  .canary {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    font-size: var(--text-sm);
    color: var(--amber);
  }
  .lock-off {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    margin: 0;
    padding: var(--space-2);
    border-radius: var(--radius);
    background: var(--surface);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .lock-off :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
</style>
