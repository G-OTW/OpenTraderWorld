<script>
  // Create/edit one vault in a modal: name, items (write-only values), optional
  // vault-wide request limit. All changes are staged locally and applied on Save,
  // so cancelling never leaves a half-edited vault behind.
  //
  // Reused by Settings → Vault and by VaultPicker (inline "new vault" from any
  // secret field). `onsaved(vault)` fires with the fresh server state.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { vaultApi } from '$lib/vault/api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), vault = null, onsaved } = $props();

  const PERIODS = ['minute', 'hour', 'day', 'week', 'month'];

  let name = $state('');
  /** Existing items: {id, name, in_use, newValue, removed}. */
  let items = $state([]);
  /** Staged new items: {name, value}. */
  let added = $state([]);
  let newName = $state('');
  let newValue = $state('');
  let quotaEnabled = $state(false);
  let quotaMax = $state('');
  let quotaPeriod = $state('day');
  let error = $state('');
  let saving = $state(false);

  // Re-seed the form each time the modal opens (also when switching target vault).
  $effect(() => {
    if (!open) return;
    name = vault?.name ?? '';
    items = (vault?.items ?? []).map((i) => ({ ...i, newValue: '', removed: false }));
    added = [];
    newName = '';
    newValue = '';
    quotaEnabled = !!vault?.quota;
    quotaMax = vault?.quota?.max_requests ?? '';
    quotaPeriod = vault?.quota?.period ?? 'day';
    error = '';
  });

  function stageAdd() {
    const n = newName.trim();
    if (!n || !newValue) return;
    const clash = (k) => k.toLowerCase() === n.toLowerCase();
    if (items.some((i) => !i.removed && clash(i.name)) || added.some((a) => clash(a.name))) {
      error = $t('vault.err.dupItem');
      return;
    }
    added = [...added, { name: n, value: newValue }];
    newName = '';
    newValue = '';
    error = '';
  }

  async function save() {
    if (!name.trim()) {
      error = $t('vault.err.nameRequired');
      return;
    }
    // A filled but un-added item row is almost certainly meant to be saved.
    if (newName.trim() && newValue) stageAdd();
    saving = true;
    error = '';
    try {
      let id = vault?.id;
      if (!id) {
        id = (await vaultApi.create(name.trim())).vault.id;
      } else if (name.trim() !== vault.name) {
        await vaultApi.rename(id, name.trim());
      }
      for (const it of items.filter((i) => i.removed)) {
        await vaultApi.removeItem(it.id);
      }
      for (const it of items.filter((i) => !i.removed && i.newValue)) {
        await vaultApi.setItem(id, it.name, it.newValue);
      }
      for (const a of added) {
        await vaultApi.setItem(id, a.name, a.value);
      }
      if (quotaEnabled) {
        const max = quotaMax === '' ? null : Number(quotaMax);
        await vaultApi.setQuota(id, max, quotaPeriod);
      } else if (vault?.quota) {
        await vaultApi.removeQuota(id);
      }
      const fresh = (await vaultApi.list()).find((v) => v.id === id) ?? null;
      open = false;
      onsaved?.(fresh);
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal bind:open title={vault ? $t('vault.editTitle') : $t('vault.newTitle')} size="md">
  <div class="form">
    <label class="field">
      <span>{$t('vault.name')}</span>
      <input type="text" bind:value={name} placeholder={$t('vault.namePlaceholder')} maxlength="80" />
    </label>

    <div>
      <div class="subhead">{$t('vault.items')}</div>
      <p class="muted small">{$t('vault.itemsHint')}</p>
      <div class="itemlist">
        {#each items as it (it.id)}
          <div class="itemrow" class:removed={it.removed}>
            <span class="iname mono">{it.name}</span>
            {#if it.removed}
              <span class="muted small">{$t('vault.willRemove')}</span>
            {:else}
              <input
                type="password"
                class="ival"
                bind:value={it.newValue}
                placeholder={$t('vault.valueSet')}
                autocomplete="new-password"
              />
            {/if}
            <button
              class="ghost danger"
              title={it.in_use > 0 ? $t('vault.inUseNoDelete') : $t('common.remove')}
              disabled={it.in_use > 0 && !it.removed}
              onclick={() => (it.removed = !it.removed)}
            >
              <Icon name={it.removed ? 'rotate-ccw' : 'trash'} size={12} />
            </button>
          </div>
        {/each}
        {#each added as a, i (a.name)}
          <div class="itemrow">
            <span class="iname mono">{a.name}</span>
            <span class="muted small grow">{$t('vault.pending')}</span>
            <button class="ghost danger" onclick={() => (added = added.filter((_, j) => j !== i))}>
              <Icon name="trash" size={12} />
            </button>
          </div>
        {/each}
        <div class="itemrow addrow">
          <input
            type="text"
            class="iname"
            bind:value={newName}
            placeholder={$t('vault.itemNamePlaceholder')}
            maxlength="80"
          />
          <input
            type="password"
            class="ival"
            bind:value={newValue}
            placeholder={$t('vault.itemValuePlaceholder')}
            autocomplete="new-password"
            onkeydown={(e) => e.key === 'Enter' && stageAdd()}
          />
          <button class="ghost" disabled={!newName.trim() || !newValue} onclick={stageAdd}>
            <Icon name="plus" size={12} /> {$t('vault.addItem')}
          </button>
        </div>
      </div>
    </div>

    <div>
      <label class="check">
        <input type="checkbox" bind:checked={quotaEnabled} />
        {$t('vault.limitToggle')}
      </label>
      {#if quotaEnabled}
        <div class="quota">
          <input
            type="number"
            min="1"
            bind:value={quotaMax}
            placeholder={$t('vault.limitUnlimited')}
          />
          <span class="muted small">{$t('vault.limitPer')}</span>
          <select bind:value={quotaPeriod}>
            {#each PERIODS as p (p)}
              <option value={p}>{$t(`common.period.${p}`)}</option>
            {/each}
          </select>
        </div>
      {/if}
      <p class="muted small">{$t('vault.limitHint')}</p>
    </div>

    <ErrorText {error} />
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={saving} onclick={save}>
      {saving ? $t('common.saving') : $t('common.save')}
    </button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-base);
  }
  input,
  select {
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    padding: 7px 10px;
    font-size: var(--text-base);
  }
  .subhead {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  .mono {
    font-family: var(--mono);
  }
  .itemlist {
    border: 0.5px solid var(--border);
    border-radius: 0;
    margin-top: var(--space-2);
  }
  .itemrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 6px 8px;
    border-bottom: 0.5px solid var(--border);
  }
  .itemrow:last-child {
    border-bottom: none;
  }
  .itemrow.removed .iname {
    text-decoration: line-through;
    color: var(--dim);
  }
  .iname {
    flex: 0 0 32%;
    min-width: 0;
    font-size: var(--text-sm);
  }
  input.iname,
  input.ival {
    padding: 5px 8px;
    font-size: var(--text-sm);
  }
  .ival,
  .grow {
    flex: 1;
    min-width: 0;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .quota {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .quota input {
    width: 120px;
  }
  button.ghost {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 0.5px solid var(--border-control);
    color: var(--muted);
    border-radius: 0;
    padding: 4px 10px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  button.ghost:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  button.ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.ghost.danger:hover:not(:disabled) {
    color: var(--red-ink);
    border-color: var(--red);
  }
  button.primary {
    background: var(--accent);
    color: var(--accent-contrast);
    border: none;
    border-radius: 0;
    padding: 7px 14px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  button.primary:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
