<script>
  // PromptStore module. A grid of prompt vignettes (name, tags, last-saved). A top
  // search bar filters by name/tag/body; quick filters isolate thumbs-up / thumbs-down
  // prompts. Each vignette carries thumb voting and edit / duplicate / delete quick
  // actions. The "+" (or Edit) opens a modal editor with tag chips and — when editing
  // an existing prompt — its version history with rollback. View/filter state is
  // persisted per-browser.
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import PromptForm from '$lib/modules/prompt-store/PromptForm.svelte';
  import VersionHistory from '$lib/modules/prompt-store/VersionHistory.svelte';
  import { promptsApi, fmtDate } from '$lib/modules/prompt-store/api.js';
  import { t } from '$lib/i18n';
  import { onMount } from 'svelte';

  // Max tag chips shown on a vignette before collapsing into a "+N" badge.
  const TAG_LIMIT = 3;

  let prompts = $state([]);
  let allTags = $state([]);
  let loading = $state(true);

  let search = $state('');
  let vote = $state('all'); // 'all' | 'up' | 'down'
  let tagFilter = $state('all'); // 'all' or a tag string

  let showForm = $state(false);
  let editing = $state(null);

  let showHistory = $state(false);

  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  // ── Persisted view prefs ──
  const PREFS_KEY = 'otw.prompt-store.prefs.v1';
  let prefsLoaded = false;

  function loadPrefs() {
    try {
      const p = JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
      if (typeof p.search === 'string') search = p.search;
      if (p.vote === 'all' || p.vote === 'up' || p.vote === 'down') vote = p.vote;
      if (typeof p.tagFilter === 'string') tagFilter = p.tagFilter;
    } catch {
      /* corrupt — ignore */
    }
  }
  $effect(() => {
    const snapshot = { search, vote, tagFilter };
    if (!prefsLoaded) return;
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(snapshot));
    } catch {
      /* non-fatal */
    }
  });

  onMount(async () => {
    loadPrefs();
    await reload();
    if (tagFilter !== 'all' && !allTags.includes(tagFilter)) tagFilter = 'all';
    prefsLoaded = true;
    loading = false;
  });

  async function reload() {
    [prompts, allTags] = await Promise.all([promptsApi.list(), promptsApi.tags()]);
  }

  const filtered = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return prompts.filter((p) => {
      if (vote === 'up' && p.vote !== 1) return false;
      if (vote === 'down' && p.vote !== -1) return false;
      if (tagFilter !== 'all' && !p.tags.includes(tagFilter)) return false;
      if (!q) return true;
      return (
        p.name.toLowerCase().includes(q) ||
        p.tags.some((tg) => tg.toLowerCase().includes(q)) ||
        p.body.toLowerCase().includes(q)
      );
    });
  });

  const upCount = $derived(prompts.filter((p) => p.vote === 1).length);
  const downCount = $derived(prompts.filter((p) => p.vote === -1).length);

  // ── Actions ──
  function openAdd() {
    editing = null;
    showForm = true;
  }
  function openEdit(p) {
    editing = p;
    showForm = true;
  }
  async function submitPrompt(input) {
    if (editing) await promptsApi.update(editing.id, input);
    else await promptsApi.add(input);
    showForm = false;
    editing = null;
    await reload();
  }
  // Open the version-history modal (on top of the form) for the prompt being edited.
  function openHistory() {
    showHistory = true;
  }
  // After a rollback: refresh the grid and re-point `editing` at the updated prompt so
  // the still-open form shows the restored content.
  async function afterRollback(updated) {
    showHistory = false;
    await reload();
    if (updated) editing = prompts.find((p) => p.id === updated.id) ?? updated;
  }
  async function toggleVote(p, dir) {
    const next = p.vote === dir ? 0 : dir; // clicking the active thumb clears it
    await promptsApi.setVote(p.id, next);
    p.vote = next; // optimistic; list order unaffected by vote
  }
  async function duplicate(p) {
    await promptsApi.duplicate(p.id);
    await reload();
  }
  function askDelete(p) {
    pendingDelete = p;
    confirmOpen = true;
  }
  async function confirmDelete() {
    if (!pendingDelete) return;
    await promptsApi.remove(pendingDelete.id);
    pendingDelete = null;
    await reload();
  }

  async function copyBody(p) {
    try {
      await navigator.clipboard.writeText(p.body);
    } catch {
      /* clipboard unavailable — silent */
    }
  }
</script>

<div class="page">
  <header class="top">
    <h1>{$t('promptStore.title')}</h1>
    <div class="tools">
      <input class="search" type="search" placeholder={$t('promptStore.searchPlaceholder')} bind:value={search} />
      <div class="seg" role="group" aria-label={$t('promptStore.filter.label')}>
        <button class:active={vote === 'all'} onclick={() => (vote = 'all')}>{$t('promptStore.filter.all')}</button>
        <button class:active={vote === 'up'} onclick={() => (vote = 'up')} title={$t('promptStore.filter.up')}>
          <Icon name="thumbs-up" size={13} /> {upCount}
        </button>
        <button class:active={vote === 'down'} onclick={() => (vote = 'down')} title={$t('promptStore.filter.down')}>
          <Icon name="thumbs-down" size={13} /> {downCount}
        </button>
      </div>
      <button class="primary" onclick={openAdd}><Icon name="plus" size={14} /> {$t('promptStore.addPrompt')}</button>
    </div>
  </header>

  {#if allTags.length}
    <nav class="tag-rail" aria-label={$t('promptStore.tags')}>
      <button class="pill" class:active={tagFilter === 'all'} onclick={() => (tagFilter = 'all')}>
        {$t('promptStore.filter.allTags')}
      </button>
      {#each allTags as tg}
        <button class="pill" class:active={tagFilter === tg} onclick={() => (tagFilter = tagFilter === tg ? 'all' : tg)}>
          <Icon name="tag" size={11} /> {tg}
        </button>
      {/each}
    </nav>
  {/if}

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if !filtered.length}
    <div class="empty">
      <Icon name="message-square" size={32} />
      <p class="muted">
        {prompts.length ? $t('promptStore.noneMatch') : $t('promptStore.empty')}
      </p>
      {#if !prompts.length}
        <button class="primary" onclick={openAdd}><Icon name="plus" size={14} /> {$t('promptStore.addPrompt')}</button>
      {/if}
    </div>
  {:else}
    <div class="grid">
      {#each filtered as p (p.id)}
        <article class="card" class:up={p.vote === 1} class:down={p.vote === -1}>
          <div class="card-head">
            <h3 title={p.name}>{p.name}</h3>
            <div class="row-actions">
              <button class="icon" title={$t('promptStore.edit')} onclick={() => openEdit(p)}><Icon name="pencil" size={14} /></button>
              <button class="icon" title={$t('promptStore.duplicate')} onclick={() => duplicate(p)}><Icon name="copy-plus" size={14} /></button>
              <button class="icon danger" title={$t('promptStore.delete')} onclick={() => askDelete(p)}><Icon name="trash" size={14} /></button>
            </div>
          </div>

          <button class="preview" onclick={() => openEdit(p)} title={$t('promptStore.openToEdit')}>
            {p.body || $t('promptStore.emptyBody')}
          </button>

          {#if p.tags.length}
            <div class="tags">
              {#each p.tags.slice(0, TAG_LIMIT) as tg}
                <button class="tag" onclick={() => (tagFilter = tg)}>{tg}</button>
              {/each}
              {#if p.tags.length > TAG_LIMIT}
                <span class="tag more" title={p.tags.slice(TAG_LIMIT).join(', ')}>
                  {$t('promptStore.moreTags', { n: p.tags.length - TAG_LIMIT })}
                </span>
              {/if}
            </div>
          {/if}

          <div class="card-foot">
            <div class="votes">
              <button class="vote" class:on={p.vote === 1} onclick={() => toggleVote(p, 1)} title={$t('promptStore.thumbUp')} aria-pressed={p.vote === 1}>
                <Icon name="thumbs-up" size={14} />
              </button>
              <button class="vote" class:on={p.vote === -1} onclick={() => toggleVote(p, -1)} title={$t('promptStore.thumbDown')} aria-pressed={p.vote === -1}>
                <Icon name="thumbs-down" size={14} />
              </button>
              <button class="vote" onclick={() => copyBody(p)} title={$t('promptStore.copy')}>
                <Icon name="copy" size={14} />
              </button>
            </div>
            <span class="saved" title={$t('promptStore.versionN', { n: p.version })}>
              {$t('promptStore.savedOn', { date: fmtDate(p.updated_at) })}
            </span>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>

<Modal bind:open={showForm} title={editing ? $t('promptStore.editTitle') : $t('promptStore.addTitle')} size="lg">
  {#if showForm}
    <!-- Key on id+version so a rollback (which bumps version) remounts the form with the restored content. -->
    {#key `${editing?.id ?? 'new'}:${editing?.version ?? 0}`}
      <PromptForm
        initial={editing}
        {allTags}
        onsubmit={submitPrompt}
        onhistory={openHistory}
        oncancel={() => (showForm = false)}
      />
    {/key}
  {/if}
</Modal>

<Modal bind:open={showHistory} title={editing ? $t('promptStore.history.titleFor', { name: editing.name }) : $t('promptStore.history.title')} size="md">
  {#if showHistory && editing}
    <VersionHistory prompt={editing} onrolledback={afterRollback} />
  {/if}
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('promptStore.deleteTitle')}
  message={pendingDelete ? $t('promptStore.confirmDelete', { name: pendingDelete.name }) : ''}
  confirmLabel={$t('promptStore.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .page {
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: 100%;
    min-height: 0;
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  h1 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
    color: var(--text);
  }
  .tools {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .search {
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    padding: 7px 10px;
    font: inherit;
    font-size: var(--text-base);
    min-width: 200px;
  }
  .search::placeholder {
    color: var(--dim);
  }
  .seg {
    display: inline-flex;
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--surface);
    border: none;
    color: var(--muted);
    padding: 7px 10px;
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .seg button.active {
    background: var(--surface-2);
    color: var(--text);
  }
  .primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  /* Filter tags as a continuous hairline band — no floating rounded pills. */
  .tag-rail {
    display: flex;
    gap: 0;
    overflow-x: auto;
    overflow-y: hidden;
    flex-shrink: 0;
    scrollbar-width: thin;
    border: 0.5px solid var(--border);
    background: var(--border);
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--bg);
    border: none;
    border-left: 1.5px solid transparent;
    color: var(--muted);
    border-radius: var(--radius);
    padding: 5px 12px;
    margin-left: 0.5px;
    cursor: pointer;
    font-size: var(--text-xs);
    white-space: nowrap;
    flex-shrink: 0;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .pill:first-child {
    margin-left: 0;
  }
  .pill:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .pill.active {
    background: var(--surface-2);
    border-left-color: var(--accent);
    color: var(--text);
  }
  /* Continuous grid: cells on a --border ground, split by 0.5px filets. */
  .grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
    overflow-y: auto;
    align-content: start;
  }
  .card {
    background: var(--bg);
    border-left: 1.5px solid transparent;
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 0;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .card:hover {
    background: var(--surface-2);
  }
  .card.up {
    border-left-color: var(--green);
  }
  .card.down {
    border-left-color: var(--red);
  }

  @media (max-width: 900px) {
    .grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }
  @media (max-width: 600px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
  .card-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .card h3 {
    margin: 0;
    font-size: 0.84rem;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    width: 26px;
    height: 26px;
    border-radius: var(--radius);
  }
  .icon:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .icon.danger:hover {
    color: var(--red);
  }
  .preview {
    text-align: left;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: 8px;
    cursor: pointer;
    font-family: var(--mono);
    font-size: var(--text-xs);
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
    white-space: pre-wrap;
    word-break: break-word;
    min-height: 3.5em;
  }
  .preview:hover {
    color: var(--text);
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .tag {
    background: var(--surface-2);
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px 8px;
    font-size: 0.7rem;
    cursor: pointer;
  }
  .tag:hover {
    color: var(--text);
  }
  .tag.more {
    cursor: default;
  }
  .tag.more:hover {
    color: var(--muted);
  }
  .card-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    margin-top: auto;
    padding-top: 2px;
  }
  .votes {
    display: flex;
    gap: 2px;
  }
  .vote {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    width: 26px;
    height: 26px;
    border-radius: var(--radius);
  }
  .vote:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .vote.on {
    color: var(--accent);
  }
  .saved {
    color: var(--dim);
    font-family: var(--mono);
    font-size: var(--text-xs);
    flex-shrink: 0;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
  }
</style>
