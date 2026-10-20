<script>
  // A one-time note that simulated results carry no guarantee. Shown before the first backtest
  // run and the first paper session of each app version, then remembered in this browser.
  // Only "Understood" goes on; closing the dialog cancels the action and asks again next time.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';

  const KEY = 'otw.simNotice';

  let open = $state(false);
  let kind = $state('backtest');
  let pending = null;
  let version = null;

  function seen() {
    try {
      return JSON.parse(localStorage.getItem(KEY)) ?? {};
    } catch {
      return {};
    }
  }

  function settle(ok) {
    pending?.(ok);
    pending = null;
  }

  /** Resolves true once the note for `kind` ('backtest' | 'paper') is read on this version,
   *  false when the user closes it instead. */
  export async function ensure(next) {
    version ??= await settingsApi.version().catch(() => 'unknown');
    if (seen()[next] === version) return true;
    settle(false);
    kind = next;
    open = true;
    return new Promise((resolve) => (pending = resolve));
  }

  function acknowledge() {
    try {
      localStorage.setItem(KEY, JSON.stringify({ ...seen(), [kind]: version }));
    } catch {
      /* shown again next time */
    }
    open = false;
    settle(true);
  }
</script>

<Modal bind:open title={$t('backtest.notice.title')} onclose={() => settle(false)}>
  <div class="notice">
    <span class="badge"><Icon name="info" size={16} /></span>
    <p>{$t(`backtest.notice.${kind}`)}</p>
  </div>

  {#snippet footer()}
    <button class="primary" onclick={acknowledge}>{$t('backtest.notice.ok')}</button>
  {/snippet}
</Modal>

<style>
  .notice {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }
  .badge {
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  p {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.6;
  }
</style>
