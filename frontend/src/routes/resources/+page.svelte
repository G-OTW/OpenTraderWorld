<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Resources module. A bookmarks library grouped by master categories. A left rail
  // selects "All" or a single category; the toolbar holds search, a card/list display
  // toggle, and an A–Z sort toggle. All view/filter state is persisted per-browser.
  import { onMount } from 'svelte';
  import { resourcesApi, linkHost } from '$lib/modules/resources/api.js';
  import ResourceForm from '$lib/modules/resources/ResourceForm.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';

  let categories = $state([]);
  let resources = $state([]);
  let loading = $state(true);

  let catFilter = $state('all'); // 'all' or a category id
  let search = $state('');
  let display = $state('card'); // 'card' | 'list'
  let sortDir = $state('asc'); // 'asc' (A–Z) | 'desc' (Z–A)

  let showForm = $state(false);
  let editing = $state(null);

  let showCatForm = $state(false);
  let editingCat = $state(null);
  let catName = $state('');

  // ── UI preferences (persisted in localStorage, per-browser) ──
  // View/filter state survives a refresh; resources/categories live in the DB.
  const PREFS_KEY = 'otw.resources.prefs.v1';
  let prefsLoaded = false; // gate the save effect until after we restore

  function loadPrefs() {
    try {
      const p = JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
      if (typeof p.catFilter === 'string') catFilter = p.catFilter;
      if (typeof p.search === 'string') search = p.search;
      if (p.display === 'card' || p.display === 'list') display = p.display;
      if (p.sortDir === 'asc' || p.sortDir === 'desc') sortDir = p.sortDir;
    } catch {
      /* corrupt prefs — ignore, use defaults */
    }
  }

  $effect(() => {
    const snapshot = { catFilter, search, display, sortDir };
    if (!prefsLoaded) return;
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(snapshot));
    } catch {
      /* quota / unavailable — non-fatal */
    }
  });

  onMount(async () => {
    loadPrefs();
    await reload();
    // Restored category may have been deleted since; fall back to All.
    if (catFilter !== 'all' && !categories.some((c) => c.id === catFilter)) catFilter = 'all';
    prefsLoaded = true;
    loading = false;
  });

  async function reload() {
    [categories, resources] = await Promise.all([
      resourcesApi.listCategories(),
      resourcesApi.list()
    ]);
  }

  const catName_ = (id) => categories.find((c) => c.id === id)?.name ?? '';

  const filtered = $derived.by(() => {
    const q = search.trim().toLowerCase();
    let out = resources.filter((r) => {
      if (catFilter !== 'all' && r.category_id !== catFilter) return false;
      if (!q) return true;
      return (
        r.name.toLowerCase().includes(q) ||
        (r.description || '').toLowerCase().includes(q) ||
        catName_(r.category_id).toLowerCase().includes(q)
      );
    });
    out = [...out].sort((a, b) => a.name.localeCompare(b.name));
    if (sortDir === 'desc') out.reverse();
    return out;
  });

  const countFor = (id) => resources.filter((r) => r.category_id === id).length;

  // ── Resources ──
  function openAdd() {
    if (!categories.length) {
      openAddCat();
      return;
    }
    editing = null;
    showForm = true;
  }
  function openEdit(r) {
    editing = r;
    showForm = true;
  }
  async function submitResource(input) {
    if (editing) await resourcesApi.update(editing.id, input);
    else await resourcesApi.add(input);
    showForm = false;
    editing = null;
    await reload();
  }
  async function removeResource(r) {
    if (!confirm($t('resources.page.confirmDeleteResource', { name: r.name }))) return;
    await resourcesApi.remove(r.id);
    await reload();
  }

  // ── Categories ──
  function openAddCat() {
    editingCat = null;
    catName = '';
    showCatForm = true;
  }
  function openEditCat(c) {
    editingCat = c;
    catName = c.name;
    showCatForm = true;
  }
  async function submitCat() {
    const name = catName.trim();
    if (!name) return;
    if (editingCat) await resourcesApi.updateCategory(editingCat.id, { name });
    else await resourcesApi.addCategory({ name });
    showCatForm = false;
    editingCat = null;
    await reload();
  }
  async function removeCat(c) {
    if (!confirm($t('resources.page.confirmDeleteCategory', { name: c.name }))) return;
    await resourcesApi.removeCategory(c.id);
    if (catFilter === c.id) catFilter = 'all';
    await reload();
  }
</script>

<div class="page">
  <header class="top">
    <h1>{$t('resources.page.title')}</h1>
    <div class="tools">
      <input class="search" type="search" placeholder={$t('resources.page.searchPlaceholder')} bind:value={search} />
      <div class="seg" role="group" aria-label={$t('resources.page.display')}>
        <button class:active={display === 'card'} onclick={() => (display = 'card')}><Icon name="grid" size={13} /> {$t('resources.page.cards')}</button>
        <button class:active={display === 'list'} onclick={() => (display = 'list')}><Icon name="list" size={13} /> {$t('resources.page.list')}</button>
      </div>
      <button
        class="ghost"
        onclick={() => (sortDir = sortDir === 'asc' ? 'desc' : 'asc')}
        title={$t('resources.page.toggleSort')}
      >
        {sortDir === 'asc' ? $t('resources.page.sortAsc') : $t('resources.page.sortDesc')}
      </button>
      <button class="primary" onclick={openAdd}>{$t('resources.page.addResource')}</button>
    </div>
  </header>

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <div class="body">
      <nav class="rail">
        <button class="cat" class:active={catFilter === 'all'} onclick={() => (catFilter = 'all')}>
          <span>{$t('resources.page.all')}</span><em>{resources.length}</em>
        </button>
        <button class="add-cat" onclick={openAddCat}>{$t('resources.page.addCategory')}</button>
        {#each categories as c}
          <div class="cat-row" class:active={catFilter === c.id}>
            <button class="cat" onclick={() => (catFilter = c.id)}>
              <span>{c.name}</span><em>{countFor(c.id)}</em>
            </button>
            <div class="cat-actions">
              <button class="icon" title={$t('resources.page.editCategory')} onclick={() => openEditCat(c)}><Icon name="pencil" size={14} /></button>
              <button class="icon" title={$t('resources.page.deleteCategory')} onclick={() => removeCat(c)}><Icon name="trash" size={14} /></button>
            </div>
          </div>
        {/each}
      </nav>

      <section class="content">
        {#if !filtered.length}
          <p class="muted">{search ? $t('resources.page.noneMatchSearch') : $t('resources.page.noneYet')}</p>
        {:else if display === 'card'}
          <div class="cards">
            {#each filtered as r (r.id)}
              <article class="card">
                <div class="card-head">
                  <h3>
                    {#if r.link}<a href={r.link} target="_blank" rel="noopener">{r.name}</a>
                    {:else}{r.name}{/if}
                  </h3>
                  <div class="row-actions">
                    <button class="icon" title={$t('resources.page.editResource')} onclick={() => openEdit(r)}><Icon name="pencil" size={14} /></button>
                    <button class="icon" title={$t('resources.page.deleteResource')} onclick={() => removeResource(r)}><Icon name="trash" size={14} /></button>
                  </div>
                </div>
                <span class="tag">{catName_(r.category_id)}</span>
                {#if r.description}<p class="desc">{r.description}</p>{/if}
                {#if r.link}<a class="host" href={r.link} target="_blank" rel="noopener">{linkHost(r.link)}</a>{/if}
              </article>
            {/each}
          </div>
        {:else}
          <ul class="list">
            {#each filtered as r (r.id)}
              <li>
                <div class="li-main">
                  <span class="li-name">
                    {#if r.link}<a href={r.link} target="_blank" rel="noopener">{r.name}</a>
                    {:else}{r.name}{/if}
                  </span>
                  <span class="tag">{catName_(r.category_id)}</span>
                  {#if r.description}<span class="desc">{r.description}</span>{/if}
                </div>
                <div class="row-actions">
                  {#if r.link}<a class="host" href={r.link} target="_blank" rel="noopener">{linkHost(r.link)}</a>{/if}
                  <button class="icon" title={$t('resources.page.editResource')} onclick={() => openEdit(r)}><Icon name="pencil" size={14} /></button>
                  <button class="icon" title={$t('resources.page.deleteResource')} onclick={() => removeResource(r)}><Icon name="trash" size={14} /></button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    </div>
  {/if}
</div>

<Modal bind:open={showForm} title={editing ? $t('resources.page.editResourceTitle') : $t('resources.page.addResourceTitle')} size="md">
  <ResourceForm
    initial={editing}
    {categories}
    defaultCategoryId={catFilter === 'all' ? null : catFilter}
    onsubmit={submitResource}
    oncancel={() => (showForm = false)}
  />
</Modal>

<Modal bind:open={showCatForm} title={editingCat ? $t('resources.page.editCategoryTitle') : $t('resources.page.addCategoryTitle')} size="sm">
  <form
    class="cat-form"
    onsubmit={(e) => {
      e.preventDefault();
      submitCat();
    }}
  >
    <label class="field">
      <span>{$t('resources.form.name')}</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={catName} autofocus placeholder={$t('resources.page.categoryNamePlaceholder')} />
    </label>
    <div class="actions">
      <button type="button" class="ghost" onclick={() => (showCatForm = false)}>{$t('common.cancel')}</button>
      <button type="submit" class="primary">{editingCat ? $t('common.save') : $t('resources.page.addCategory')}</button>
    </div>
  </form>
</Modal>

<style>
  .page {
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    height: 100%;
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
    font-size: 1.3rem;
    color: var(--text);
  }
  .tools {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .search {
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 7px 10px;
    font: inherit;
    font-size: 0.85rem;
    min-width: 180px;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg button {
    background: var(--surface);
    border: none;
    color: var(--muted);
    padding: 7px 10px;
    cursor: pointer;
    font-size: 0.8rem;
  }
  .seg button.active {
    background: var(--surface-2);
    color: var(--text);
  }
  .body {
    display: flex;
    gap: var(--space-4);
    flex: 1;
    min-height: 0;
  }
  .rail {
    width: 220px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    align-content: start;
    overflow-y: auto;
  }
  .rail > .cat,
  .rail > .add-cat,
  .cat-row {
    flex: 0 0 auto;
  }
  .cat-row {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
  }
  .cat-row.active,
  .cat-row:hover {
    background: var(--surface-2);
  }
  .cat {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    background: transparent;
    border: none;
    color: var(--text);
    padding: 8px 10px;
    border-radius: var(--radius);
    cursor: pointer;
    font-size: 0.88rem;
    text-align: left;
  }
  .cat.active {
    background: var(--surface-2);
  }
  /* Standalone "All" button (not wrapped in a .cat-row) keeps its own hover. */
  .rail > .cat:hover {
    background: var(--surface-2);
  }
  .cat em {
    font-style: normal;
    color: var(--muted);
    font-size: 0.78rem;
  }
  /* On hover, swap the count badge out for the edit/delete icons. */
  .cat-row:hover .cat em {
    display: none;
  }
  .cat-actions {
    display: none;
    gap: 2px;
    padding-right: 4px;
  }
  .cat-row:hover .cat-actions {
    display: flex;
  }
  .add-cat {
    margin: var(--space-1) 0 var(--space-2);
    background: transparent;
    border: 1px dashed var(--border);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 8px;
    cursor: pointer;
    font-size: 0.82rem;
  }
  .content {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--space-3);
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg, var(--radius));
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .card-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .card h3 {
    margin: 0;
    font-size: 0.95rem;
    color: var(--text);
  }
  .card h3 a,
  .li-name a {
    color: var(--text);
    text-decoration: none;
  }
  .card h3 a:hover,
  .li-name a:hover {
    color: var(--accent);
  }
  .tag {
    align-self: flex-start;
    background: var(--surface-2);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 2px 8px;
    font-size: 0.72rem;
  }
  .desc {
    margin: 0;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .host {
    color: var(--accent);
    font-size: 0.78rem;
    text-decoration: none;
  }
  .host:hover {
    text-decoration: underline;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .list li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 10px 8px;
    border-bottom: 1px solid var(--border);
  }
  .li-main {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    flex-wrap: wrap;
  }
  .li-name {
    font-size: 0.9rem;
    color: var(--text);
  }
  .li-main .desc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 360px;
  }
  .row-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }
  .muted {
    color: var(--muted);
  }
  .cat-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
