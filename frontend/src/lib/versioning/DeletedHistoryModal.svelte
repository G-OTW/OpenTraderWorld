<script>
  // Histories left behind by deleted items (documents or strategies deleted with "keep
  // versions"). Pick an item, then restore one of its versions (the item comes back) or
  // delete versions one by one or all at once.
  //
  // props: open (bindable), api (docVersions | strategyVersions), scope ('doc' | 'strategy'),
  //        onchange() after anything changed, onrestored(itemId)
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtVersionDate } from './state.svelte.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), api, scope = 'doc', onchange = () => {}, onrestored = () => {} } = $props();

  let items = $state([]);
  let selected = $state(null);
  let versions = $state([]);
  let error = $state('');
  let busy = $state(false);
  let purgeOpen = $state(false);

  $effect(() => {
    if (open) load();
  });

  async function load() {
    error = '';
    try {
      items = await api.deleted();
      if (!items.some((i) => i.id === selected?.id)) pick(items[0] ?? null);
    } catch (e) {
      error = e.message;
    }
  }

  async function pick(item) {
    selected = item;
    versions = [];
    if (!item) return;
    try {
      versions = await api.list(item.id);
    } catch (e) {
      error = e.message;
    }
  }

  async function restore(v) {
    busy = true;
    error = '';
    try {
      await api.restore(
        selected.id,
        v.id,
        $t('versioning.restoredNote', { date: fmtVersionDate(v.created_at) })
      );
      const id = selected.id;
      selected = null;
      onchange();
      onrestored(id);
      open = false;
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  async function removeVersion(v) {
    error = '';
    try {
      await api.remove(selected.id, v.id);
      versions = versions.filter((x) => x.id !== v.id);
      onchange();
      if (!versions.length) await load();
    } catch (e) {
      error = e.message;
    }
  }

  async function purge() {
    error = '';
    try {
      await api.purge(selected.id);
      selected = null;
      onchange();
      await load();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<Modal bind:open title={$t(`versioning.deleted.title.${scope}`)} size="lg">
  {#if !items.length}
    <p class="empty">{$t('versioning.deleted.empty')}</p>
  {:else}
    <div class="split">
      <div class="items">
        {#each items as it (it.id)}
          <button class="item" class:on={selected?.id === it.id} onclick={() => pick(it)}>
            <span class="nm">{it.title || $t('editor.docTree.untitled')}</span>
            <span class="meta">{$t('versioning.deleted.count', { n: it.versions })} · {fmtVersionDate(it.last_at)}</span>
          </button>
        {/each}
      </div>
      <div class="versions">
        {#if selected}
          {#each versions as v (v.id)}
            <div class="row">
              <div class="txt">
                <span class="when">{fmtVersionDate(v.created_at)}</span>
                {#if v.note}<span class="note">{v.note}</span>{/if}
              </div>
              <button class="btn sm" disabled={busy} onclick={() => restore(v)}>
                <Icon name="rotate-ccw" size={12} /> {$t('versioning.restore')}
              </button>
              <button class="del" onclick={() => removeVersion(v)} title={$t('versioning.menu.delete')} aria-label={$t('versioning.menu.delete')}>
                <Icon name="x" size={12} />
              </button>
            </div>
          {/each}
          <div class="foot">
            <button class="danger sm" onclick={() => (purgeOpen = true)}>{$t('versioning.deleted.purge')}</button>
          </div>
        {/if}
      </div>
    </div>
  {/if}
  <ErrorText {error} />
</Modal>

<ConfirmModal
  bind:open={purgeOpen}
  title={$t('versioning.deleted.purge')}
  message={$t('versioning.deleted.purgeConfirm', { name: selected?.title ?? '' })}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={purge}
/>

<style>
  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    margin: 0;
  }
  .split {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr);
    gap: var(--space-4);
    min-height: 260px;
  }
  .items {
    display: flex;
    flex-direction: column;
    border-right: var(--hairline) solid var(--border);
    padding-right: var(--space-3);
  }
  .item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .item:hover,
  .item.on {
    background: var(--surface-2);
  }
  .nm {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta,
  .note {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .versions {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .txt {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-sm);
  }
  .del {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-1);
    display: inline-flex;
  }
  .del:hover {
    color: var(--red);
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    margin-top: var(--space-3);
  }
</style>
