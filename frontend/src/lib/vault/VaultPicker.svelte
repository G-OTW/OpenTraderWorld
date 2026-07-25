<script>
  // Reusable secret input. Secrets are never pasted in place: they live in the
  // centralized vault (Settings → Vault) and are referenced here by item.
  //   vaultItemId  — vault item reference (the only way to set a secret)
  // "New vault" opens the shared VaultModal inline; the gear link jumps to the
  // vault page for full management.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import VaultModal from '$lib/vault/VaultModal.svelte';
  import { vaultApi } from '$lib/vault/api.js';
  import { t } from '$lib/i18n';

  let {
    // No fallback: callers bind lazily-created map entries that may be undefined, and
    // `bind:` to a prop with a fallback throws props_invalid_value. Normalized below.
    vaultItemId = $bindable(),
    /** True when a value is already stored server-side (shows the "set" hint). */
    hasSecret = false
  } = $props();

  let vaults = $state([]);
  let loaded = $state(false);
  let modalOpen = $state(false);

  onMount(reload);

  async function reload() {
    try {
      vaults = await vaultApi.list();
    } catch {
      vaults = [];
    } finally {
      loaded = true;
    }
  }

  function pick(id) {
    vaultItemId = id || null;
  }

  function onVaultSaved(fresh) {
    reload().then(() => {
      // A vault with exactly one item was almost certainly created to be used here.
      if (fresh?.items?.length === 1 && !vaultItemId) pick(fresh.items[0].id);
    });
  }

  const hasItems = $derived(vaults.some((v) => (v.items ?? []).length));
</script>

<div class="picker">
  <div class="row">
    <select value={vaultItemId ?? ''} onchange={(e) => pick(e.currentTarget.value)}>
      <option value="">{hasSecret ? $t('vault.picker.keepCurrent') : $t('vault.picker.choose')}</option>
      {#each vaults as v (v.id)}
        {#if (v.items ?? []).length}
          <optgroup label={v.name}>
            {#each v.items as it (it.id)}
              <option value={it.id}>{v.name}.{it.name}</option>
            {/each}
          </optgroup>
        {/if}
      {/each}
    </select>
    <button type="button" class="ghost" onclick={() => (modalOpen = true)}>
      <Icon name="plus" size={12} /> {$t('vault.picker.newVault')}
    </button>
    <a class="ghost link" href="/settings#vault" title={$t('vault.picker.manage')}>
      <Icon name="settings" size={12} />
    </a>
  </div>
  {#if loaded && !hasItems}
    <p class="muted small">{$t('vault.picker.empty')}</p>
  {/if}
</div>

<VaultModal bind:open={modalOpen} vault={null} onsaved={onVaultSaved} />

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  select {
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    padding: 7px 10px;
    font-size: var(--text-base);
    width: 100%;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .row select {
    flex: 1;
    min-width: 0;
  }
  .ghost {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 0.5px solid var(--border-control);
    color: var(--muted);
    border-radius: 0;
    padding: 5px 9px;
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
    text-decoration: none;
  }
  .ghost:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
    margin: 0;
  }
</style>
