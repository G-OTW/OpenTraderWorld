<script>
  // A channel config field that accepts either a plain value or a vault item.
  //
  // The credential of a channel is its sealed secret, but a destination can be worth
  // hiding too: a Telegram chat id names a person. In vault mode the field stores an item
  // id under `<key>_vault_item` and the server resolves it on the way to dispatch, so the
  // value itself never travels back to the browser.
  import Icon from '$lib/ui/Icon.svelte';
  import VaultPicker from '$lib/vault/VaultPicker.svelte';
  import { t } from '$lib/i18n';

  // Neither bound prop carries a fallback: a caller binds config entries that may not
  // exist yet (a channel stored before these fields, or a form seeded one tick later),
  // and `bind:` to a prop with a fallback is a fatal render error, not an empty field.
  let { value = $bindable(), vaultItem = $bindable(), placeholder = '', type = 'text' } = $props();

  // Sticky: a field switched to vault mode has nothing to read the mode back from until
  // an item is picked.
  let wantVault = $state(!!vaultItem);
  const asVault = $derived(wantVault || !!vaultItem);
  const picked = $derived(vaultItem ?? null);
  const text = $derived(value ?? '');

  function toVault() {
    wantVault = true;
    value = '';
  }
  function toPlain() {
    wantVault = false;
    vaultItem = null;
  }
</script>

<div class="row">
  <div class="val">
    {#if asVault}
      <VaultPicker bind:vaultItemId={vaultItem} hasSecret={!!picked} />
    {:else}
      <input {type} value={text} oninput={(e) => (value = e.currentTarget.value)} {placeholder} />
    {/if}
  </div>
  <div class="switch" role="group" aria-label={$t('notifch.valueKind')}>
    <button
      type="button"
      class:on={!asVault}
      onclick={toPlain}
      title={$t('notifch.plainValue')}
      aria-pressed={!asVault}
    >
      <Icon name="type" size={12} />
    </button>
    <button
      type="button"
      class:on={asVault}
      onclick={toVault}
      title={$t('notifch.vaultValue')}
      aria-pressed={asVault}
    >
      <Icon name="key" size={12} />
    </button>
  </div>
</div>

<style>
  .row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }
  .val {
    flex: 1;
    min-width: 0;
  }
  .val input {
    width: 100%;
  }
  /* One frame around the pair, none around each, and exactly as tall as the field. */
  .switch {
    display: flex;
    flex: none;
    height: var(--control-h);
    border: 0.5px solid var(--border);
  }
  .switch button {
    display: inline-flex;
    align-items: center;
    padding: 0 var(--space-2);
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .switch button.on {
    background: var(--surface-2);
    color: var(--accent);
  }
</style>
