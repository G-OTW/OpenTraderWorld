<script>
  // Settings → Vault: centralized API keys & secrets. One vault per external service
  // (e.g. "Binance") holding named items (apikey, secretkey, …). Values are write-only;
  // modules plug items by reference. Requests/rate limits are tracked per vault.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import VaultModal from '$lib/vault/VaultModal.svelte';
  import { vaultApi } from '$lib/vault/api.js';
  import { t } from '$lib/i18n';

  let vaults = $state([]);
  let loading = $state(true);
  let error = $state('');

  let modalOpen = $state(false);
  let editing = $state(null);

  // Two-step delete (first click arms, second deletes).
  let confirmDelete = $state('');

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      vaults = await vaultApi.list();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function openCreate() {
    editing = null;
    modalOpen = true;
  }

  function openEdit(v) {
    editing = v;
    modalOpen = true;
  }

  async function remove(v) {
    if (confirmDelete !== v.id) {
      confirmDelete = v.id;
      return;
    }
    confirmDelete = '';
    try {
      await vaultApi.remove(v.id);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  const inUse = (v) => (v.items ?? []).reduce((n, i) => n + (i.in_use ?? 0), 0);

  function usage(v) {
    if (!v.quota) return $t('vault.noLimit');
    const max = v.quota.max_requests;
    return `${v.quota.used} / ${max ?? '∞'} · ${$t(`common.period.${v.quota.period}`)}`;
  }
</script>

<div class="section">
  <div class="head">
    <h2>{$t('vault.title')}</h2>
    <button class="primary" onclick={openCreate}>
      <Icon name="plus" size={13} /> {$t('vault.newVault')}
    </button>
  </div>
  <p class="muted small">{$t('vault.subtitle')}</p>
  <p class="muted small">{$t('vault.trackingNote')}</p>

  <ErrorText {error} />

  {#if loading}
    <div class="tablewrap" aria-busy="true">
      <table>
        <thead>
          <tr>
            <th>{$t('vault.colName')}</th>
            <th>{$t('vault.colItems')}</th>
            <th>{$t('vault.colUsage')}</th>
            <th>{$t('vault.colInUse')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each Array.from({ length: 3 }, (_, i) => i) as i (i)}
            <tr>
              <td><Skeleton height="0.85rem" width="60%" /></td>
              <td><Skeleton height="0.85rem" width="85%" /></td>
              <td><Skeleton height="0.85rem" width="55%" /></td>
              <td><Skeleton height="0.85rem" width="40%" /></td>
              <td><Skeleton height="0.85rem" width="45%" /></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if !vaults.length}
    <EmptyState icon="lock" compact title={$t('vault.empty')} description={$t('vault.emptyHint')} />
  {:else}
    <div class="tablewrap">
      <table>
        <thead>
          <tr>
            <th>{$t('vault.colName')}</th>
            <th>{$t('vault.colItems')}</th>
            <th>{$t('vault.colUsage')}</th>
            <th>{$t('vault.colInUse')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each vaults as v (v.id)}
            <tr>
              <td class="strong">{v.name}</td>
              <td>
                {#if (v.items ?? []).length}
                  <span class="chips">
                    {#each v.items as it (it.id)}
                      <span class="chip mono" title={it.in_use > 0 ? $t('vault.chipInUse', { n: it.in_use }) : ''}>
                        {v.name}.{it.name}{#if it.in_use > 0}&nbsp;·&nbsp;{it.in_use}{/if}
                      </span>
                    {/each}
                  </span>
                {:else}
                  <span class="muted">{$t('vault.noItems')}</span>
                {/if}
              </td>
              <td class="mono">{usage(v)}</td>
              <td>{inUse(v)}</td>
              <td class="actions">
                <button class="ghost" onclick={() => openEdit(v)}>
                  <Icon name="pencil" size={12} /> {$t('vault.edit')}
                </button>
                <button
                  class="ghost danger"
                  class:armed={confirmDelete === v.id}
                  disabled={inUse(v) > 0}
                  title={inUse(v) > 0 ? $t('vault.inUseNoDelete') : ''}
                  onclick={() => remove(v)}
                >
                  <Icon name="trash" size={12} />
                  {confirmDelete === v.id ? $t('vault.deleteConfirm') : $t('vault.delete')}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <ul class="notes muted small">
    <li>{$t('vault.note1')}</li>
    <li>{$t('vault.note2')}</li>
    <li>{$t('vault.note3')}</li>
  </ul>
</div>

<VaultModal bind:open={modalOpen} vault={editing} onsaved={reload} />

<style>
  .section h2 {
    margin: 0;
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: 11.5px;
  }
  .tablewrap {
    overflow-x: auto;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    margin: var(--space-3) 0;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th,
  td {
    text-align: left;
    padding: 8px 10px;
    border-bottom: var(--hairline) solid var(--border);
    white-space: nowrap;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  th {
    color: var(--dim);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--fw-medium);
    background: var(--surface-2);
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .mono {
    font-family: var(--mono);
  }
  .chips {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    border: var(--hairline) solid var(--border);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 1px 7px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .actions {
    text-align: right;
  }
  button.ghost {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: var(--hairline) solid var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 4px 10px;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  button.ghost:hover:not(:disabled) {
    color: var(--text);
  }
  button.ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.ghost.danger:hover:not(:disabled),
  button.ghost.armed {
    color: var(--red-ink);
    border-color: var(--red);
  }
  button.primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    color: var(--text);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: 7px 14px;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
    cursor: pointer;
  }
  button.primary:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .notes {
    margin: var(--space-2) 0 0;
    padding-left: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
</style>
