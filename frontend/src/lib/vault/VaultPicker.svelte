<script>
  // Reusable secret input. Secrets are never pasted in place: they live in the
  // centralized vault (Settings → Vault) and are referenced here by item.
  //   vaultItemId  — vault item reference (the only way to set a secret)
  // "New vault" opens the shared VaultModal inline; the gear link jumps to the
  // vault page for full management.
  import { onMount } from 'svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
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

  // One flat list with a `header` row per vault — the Dropdown's stand-in for <optgroup>.
  const options = $derived([
    { value: '', label: hasSecret ? $t('vault.picker.keepCurrent') : $t('vault.picker.choose') },
    ...vaults.flatMap((v) =>
      (v.items ?? []).length
        ? [
            { header: true, label: v.name },
            ...v.items.map((it) => ({ value: it.id, label: `${v.name}.${it.name}` }))
          ]
        : []
    )
  ]);
</script>

<div class="picker">
  <div class="row">
    <Dropdown
      value={vaultItemId ?? ''}
      {options}
      onpick={pick}
      ariaLabel={$t('vault.picker.choose')}
    />
    <button type="button" class="btn" onclick={() => (modalOpen = true)}>
      <Icon name="plus" size={12} /> {$t('vault.picker.newVault')}
    </button>
    <a class="icon" href="/settings#vault" title={$t('vault.picker.manage')}>
      <Icon name="settings" size={14} />
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
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  /* Picker takes the row; the two actions keep their intrinsic width. The button and
     the link ride the global .btn / .icon layer so they match the controls around them. */
  .row > :global(.dd) {
    flex: 1;
    min-width: 0;
  }
  .btn {
    white-space: nowrap;
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
    margin: 0;
  }
</style>
