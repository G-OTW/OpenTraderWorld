<script>
  // The versioning controls of one item (an editor page or database, or a saved strategy):
  // a "Save version" button while versioning is on for it, and a history modal that lists
  // its versions, opens one for preview, deletes one, and switches versioning for this item.
  // Rendered only when the global switch for the area is on (the parent decides).
  //
  // props:
  //   api                 docVersions | strategyVersions (versioning/api.js)
  //   itemId, itemName    what is versioned
  //   scope               'doc' | 'strategy' (wording)
  //   versioned           per-item switch, as the parent knows it
  //   activeId            id of the version being previewed, marked in the list
  //   beforesave()        optional, awaited before a snapshot (flush pending edits)
  //   onversionedchange(on), onpreview(versionMeta), onerror(message),
  //   oncount(n) whenever the number of versions is known or changes
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ChoiceModal from '$lib/ui/ChoiceModal.svelte';
  import SaveVersionModal from './SaveVersionModal.svelte';
  import { fmtVersionDate } from './state.svelte.js';
  import { t } from '$lib/i18n';

  let {
    api,
    itemId,
    itemName = '',
    scope = 'doc',
    versioned = false,
    activeId = null,
    beforesave = null,
    onversionedchange = () => {},
    onpreview = () => {},
    onerror = () => {},
    oncount = () => {}
  } = $props();

  let versions = $state([]);
  let currentId = $state(null); // latest version, while the item still matches it
  let query = $state('');
  // Search runs over the note and the date as shown.
  let shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return versions;
    return versions.filter((v) =>
      `${v.note ?? ''} ${fmtVersionDate(v.created_at)}`.toLowerCase().includes(q)
    );
  });
  let menuOpen = $state(false);
  let saveOpen = $state(false);
  let offOpen = $state(false);
  let pendingDelete = $state(null);
  let deleteOpen = $state(false);

  // Reported from here rather than at each mutation, so no path forgets it.
  let loaded = $state(false);
  $effect(() => {
    if (loaded) oncount(versions.length);
  });

  /** Re-read the list (after a restore, which adds a backup version). */
  export async function reload() {
    const id = itemId;
    try {
      const r = await api.listWithCurrent(id);
      if (id === itemId) {
        versions = r.versions;
        currentId = r.current;
        loaded = true;
      }
    } catch (e) {
      onerror(e.message);
    }
  }

  $effect(() => {
    void itemId;
    loaded = false;
    versions = [];
    currentId = null;
    menuOpen = false;
    reload();
  });

  async function save(note) {
    await beforesave?.();
    const v = await api.create(itemId, note);
    versions = [v, ...versions];
    currentId = v.id;
  }

  async function switchOn() {
    try {
      await api.toggle(itemId, true);
      onversionedchange(true);
    } catch (e) {
      onerror(e.message);
    }
  }

  async function switchOff(choice) {
    try {
      await api.toggle(itemId, false, choice === 'purge');
      if (choice === 'purge') {
        versions = [];
        currentId = null;
      }
      onversionedchange(false);
    } catch (e) {
      onerror(e.message);
    }
  }

  function askDelete(v) {
    pendingDelete = v;
    deleteOpen = true;
  }

  async function confirmDelete() {
    const v = pendingDelete;
    pendingDelete = null;
    if (!v) return;
    try {
      await api.remove(itemId, v.id);
      versions = versions.filter((x) => x.id !== v.id);
      if (currentId === v.id) currentId = null;
    } catch (e) {
      onerror(e.message);
    }
  }

  // The current version is what is on screen already: nothing to preview or restore.
  function preview(v) {
    menuOpen = false;
    if (v.id !== currentId) onpreview(v);
  }
</script>

<div class="vctl">
  {#if versioned}
    <button class="vbtn" onclick={() => (saveOpen = true)} title={$t('versioning.save.title')}>
      <Icon name="save" size={13} /> {$t('versioning.save.button')}
    </button>
  {/if}
  <button
    class="vbtn"
    class:on={menuOpen}
    onclick={() => {
      query = '';
      menuOpen = true;
      reload();
    }}
    title={$t('versioning.menu.title')}
    aria-label={$t('versioning.menu.title')}
  >
    <Icon name="history" size={13} />{#if versions.length}<span class="count">{versions.length}</span>{/if}
  </button>
</div>

<Modal bind:open={menuOpen} title={$t('versioning.menu.title')} size="md">
  <div class="head">
    <span class="name">{itemName}</span>
    <label class="sw" class:on={versioned}>
      <input
        type="checkbox"
        checked={versioned}
        onchange={(e) => {
          e.currentTarget.checked = versioned;
          if (versioned) offOpen = true;
          else switchOn();
        }}
      />
      <span>{versioned ? $t('versioning.menu.on') : $t('versioning.menu.off')}</span>
    </label>
  </div>
  {#if !versioned}
    <p class="hint">{$t(`versioning.menu.offHint.${scope}`)}</p>
  {/if}
  {#if versions.length}
    <div class="search">
      <Icon name="search" size={14} />
      <!-- svelte-ignore a11y_autofocus -->
      <input type="search" bind:value={query} autofocus placeholder={$t('versioning.menu.search')} aria-label={$t('versioning.menu.search')} />
    </div>
    <div class="list">
      {#each shown as v (v.id)}
        <div class="row" class:active={v.id === activeId}>
          <button class="main" onclick={() => preview(v)} title={v.id === currentId ? '' : $t('versioning.menu.open')}>
            <span class="when">
              {fmtVersionDate(v.created_at)}
              {#if v.id === currentId}<span class="current-tag">{$t('versioning.menu.current')}</span>{/if}
            </span>
            {#if v.note}<span class="note" title={v.note}>{v.note}</span>{/if}
          </button>
          <button class="del" onclick={() => askDelete(v)} title={$t('versioning.menu.delete')} aria-label={$t('versioning.menu.delete')}>
            <Icon name="trash" size={14} />
          </button>
        </div>
      {:else}
        <p class="hint">{$t('common.noMatches')}</p>
      {/each}
    </div>
  {:else if versioned}
    <p class="hint">{$t('versioning.menu.empty')}</p>
  {/if}
</Modal>

<SaveVersionModal bind:open={saveOpen} name={itemName} onsave={save} />

<ChoiceModal
  bind:open={offOpen}
  title={$t('versioning.off.title')}
  message={$t(`versioning.off.message.${scope}`, { n: versions.length })}
  cancelLabel={$t('common.cancel')}
  choices={[
    { value: 'keep', label: $t('versioning.off.keep'), variant: 'btn' },
    { value: 'purge', label: $t('versioning.off.purge'), variant: 'danger' }
  ]}
  onpick={switchOff}
/>

<ConfirmModal
  bind:open={deleteOpen}
  title={$t('versioning.menu.delete')}
  message={$t('versioning.menu.deleteConfirm', { date: fmtVersionDate(pendingDelete?.created_at) })}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .vctl {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }
  .vbtn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    padding: 4px 6px;
    cursor: pointer;
    font-size: var(--text-sm);
    line-height: 1;
    white-space: nowrap;
  }
  .vbtn:hover,
  .vbtn.on {
    color: var(--text);
    background: var(--surface-2);
  }
  .count {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: var(--text-base);
  }
  .sw {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
    font-size: var(--text-base);
    color: var(--muted);
    cursor: pointer;
  }
  .sw.on {
    color: var(--green-ink);
  }
  .hint {
    margin: 0;
    padding: var(--space-4) 0;
    font-size: var(--text-base);
    color: var(--muted);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
    padding: 0 var(--space-3);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
  }
  .search:focus-within {
    border-color: var(--accent);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    padding: var(--space-2) 0;
    font-size: var(--text-base);
    color: var(--text);
    outline: none;
    box-shadow: none;
  }
  .list {
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
  }
  .list .hint {
    padding: var(--space-4);
  }
  .row {
    display: flex;
    align-items: center;
    border-bottom: var(--hairline) solid var(--border);
  }
  .row:last-child {
    border-bottom: none;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.active {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-1);
    background: transparent;
    border: none;
    color: var(--text);
    text-align: left;
    padding: var(--space-3) var(--space-4);
    cursor: pointer;
    font-size: var(--text-base);
  }
  .when {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-weight: var(--fw-medium);
  }
  .current-tag {
    padding: 2px 8px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  /* Two lines at most; the full note is in the tooltip. */
  .note {
    color: var(--muted);
    font-size: var(--text-base);
    line-height: 1.4;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-width: 100%;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
  .del {
    align-self: stretch;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0 var(--space-4);
    display: inline-flex;
    align-items: center;
    opacity: 0;
  }
  .row:hover .del,
  .del:focus-visible {
    opacity: 1;
  }
  .del:hover {
    color: var(--red);
  }
</style>
