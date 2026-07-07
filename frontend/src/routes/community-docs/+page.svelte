<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Community Docs. A library of community-authored docs synced from the website and kept
  // available offline.
  //
  // Left pane: the user's favorited docs (persistent — only the user removes them). Its search
  // filters favorites only; its refresh button reloads all docs from the website feed
  // without ever removing favorites.
  //
  // Center pane: a browser with three states — (1) category cards, (2) the docs in a chosen
  // category, (3) the selected doc's reader with a back button. Its search matches categories
  // AND docs at once, with results grouped by category.
  //
  // Bodies are trusted (curated/sanitized at the source), so they render with {@html}.
  import { onMount } from 'svelte';
  import { communityDocsApi, groupByCategory } from '$lib/modules/community-docs/api.js';
  import { t } from '$lib/i18n';

  let docs = $state([]); // all docs (summaries)
  let favorites = $state([]); // favorited summaries, recency order
  let loading = $state(true);
  let refreshing = $state(false);

  // Center pane state machine: 'categories' | 'category' | 'doc'.
  let view = $state('categories');
  let activeCategory = $state(null);
  let centerSearch = $state('');

  // Reader.
  let current = $state(null); // full doc (with body)
  let loadingDoc = $state(false);

  // Left pane.
  let favSearch = $state('');

  onMount(async () => {
    await load();
    // Fresh install: the local library is empty, so pull the website feed once
    // automatically instead of waiting for a manual refresh. Best-effort — an
    // offline instance still gets the (empty) page rather than an error.
    if (docs.length === 0) await refresh().catch(() => {});
    loading = false;
  });

  async function load() {
    [docs, favorites] = await Promise.all([communityDocsApi.list(), communityDocsApi.favorites()]);
  }

  async function refresh() {
    refreshing = true;
    try {
      // Reloads docs from upstream; favorites are untouched server-side. We re-pull both so
      // the panes reflect any new/updated docs.
      await communityDocsApi.refresh();
      await load();
      if (current) current = await communityDocsApi.get(current.slug).catch(() => null);
    } finally {
      refreshing = false;
    }
  }

  // ---- Left pane: favorites grouped by category, filtered by favSearch (favorites only) ----
  let collapsed = $state(new Set()); // category names the user has collapsed

  // A doc's categories as a non-empty list; empty falls back to a single "Uncategorized".
  const uncategorizedLabel = $derived($t('communityDocs.uncategorized'));
  const docCats = (d) => (d.categories?.length ? d.categories : [uncategorizedLabel]);
  // For sorting/search: the doc's categories joined into one searchable string.
  const catText = (d) => docCats(d).join(' ');

  const favGroups = $derived.by(() => {
    const q = favSearch.trim().toLowerCase();
    const list = q
      ? favorites.filter(
          (d) =>
            d.title.toLowerCase().includes(q) ||
            (d.summary || '').toLowerCase().includes(q) ||
            catText(d).toLowerCase().includes(q)
        )
      : favorites;
    // Group by category, alphabetical by category then title for a stable rail.
    return groupByCategory(
      [...list].sort(
        (a, b) => catText(a).localeCompare(catText(b)) || a.title.localeCompare(b.title)
      )
    );
  });

  function toggleGroup(cat) {
    const next = new Set(collapsed);
    if (next.has(cat)) next.delete(cat);
    else next.add(cat);
    collapsed = next;
  }

  // ---- Center pane search: matches categories + docs, grouped by category ----
  const centerGroups = $derived.by(() => {
    const q = centerSearch.trim().toLowerCase();
    if (!q) return [];
    const matched = docs.filter((d) => {
      const cat = catText(d).toLowerCase();
      return (
        cat.includes(q) ||
        d.title.toLowerCase().includes(q) ||
        (d.summary || '').toLowerCase().includes(q)
      );
    });
    return groupByCategory(matched);
  });

  // Category cards for the default browse view: [{ category, count }].
  const categoryCards = $derived.by(() =>
    groupByCategory(docs).map((g) => ({ category: g.category, count: g.docs.length }))
  );

  const categoryDocs = $derived.by(() =>
    activeCategory ? docs.filter((d) => docCats(d).includes(activeCategory)) : []
  );

  function openCategory(cat) {
    activeCategory = cat;
    view = 'category';
  }

  async function openDoc(slug) {
    loadingDoc = true;
    view = 'doc';
    try {
      current = await communityDocsApi.get(slug);
    } finally {
      loadingDoc = false;
    }
  }

  // Back button target: from a doc, go to its category listing if we arrived via one,
  // else back to categories; from a category listing, go to categories.
  function goBack() {
    if (view === 'doc') {
      view = activeCategory ? 'category' : 'categories';
      current = null;
    } else if (view === 'category') {
      view = 'categories';
      activeCategory = null;
    }
  }

  async function toggleFavorite(doc) {
    const next = !doc.favorited;
    await communityDocsApi.setFavorite(doc.slug, next);
    if (current && current.slug === doc.slug) current = { ...current, favorited: next };
    // Refresh both panes' favorite state cheaply.
    docs = docs.map((d) => (d.slug === doc.slug ? { ...d, favorited: next } : d));
    if (next) {
      const summary = docs.find((d) => d.slug === doc.slug);
      if (summary && !favorites.some((f) => f.slug === doc.slug))
        favorites = [{ ...summary, favorited: true }, ...favorites];
    } else {
      favorites = favorites.filter((f) => f.slug !== doc.slug);
    }
  }

  const fmtDate = (iso) => {
    try {
      return new Date(iso).toLocaleDateString();
    } catch {
      return '';
    }
  };
</script>

<div class="page">
  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <div class="layout">
      <!-- LEFT PANE: favorites (persistent) -->
      <aside class="fav-pane">
        <div class="fav-head">
          <input
            class="search sm"
            type="search"
            placeholder={$t('communityDocs.favorites.searchPlaceholder')}
            bind:value={favSearch}
          />
          <button
            class="icon-btn"
            title={$t('communityDocs.favorites.reloadTitle')}
            onclick={refresh}
            disabled={refreshing}
            aria-label={$t('communityDocs.favorites.refreshAriaLabel')}
          >
            <span class:spin={refreshing}>⟳</span>
          </button>
        </div>
        <div class="fav-list">
          {#if !favGroups.length}
            <p class="muted small">
              {favSearch ? $t('communityDocs.favorites.noneMatch') : $t('communityDocs.favorites.emptyHint')}
            </p>
          {/if}
          {#each favGroups as g (g.category)}
            <div class="fav-group">
              <button
                class="fav-cat-row"
                aria-expanded={!collapsed.has(g.category)}
                onclick={() => toggleGroup(g.category)}
              >
                <span class="chev" class:open={!collapsed.has(g.category)}><Icon name="chevron-right" size={13} /></span>
                <span class="fav-cat-name">{g.category}</span>
                <span class="fav-cat-count">{g.docs.length}</span>
              </button>
              {#if !collapsed.has(g.category)}
                {#each g.docs as d (d.slug)}
                  <button
                    class="fav"
                    class:active={current?.slug === d.slug}
                    onclick={() => openDoc(d.slug)}
                  >
                    {d.title}
                  </button>
                {/each}
              {/if}
            </div>
          {/each}
        </div>
      </aside>

      <!-- CENTER PANE: browse -->
      <section class="center">
        {#if view === 'doc'}
          <div class="center-top">
            <button class="back" onclick={goBack}>{$t('communityDocs.nav.back')}</button>
          </div>
          {#if loadingDoc}
            <p class="muted">{$t('common.loading')}</p>
          {:else if !current}
            <p class="muted">{$t('communityDocs.reader.notFound')}</p>
          {:else}
            <article class="article">
              <div class="article-head">
                <div class="article-head-row">
                  {#each docCats(current) as cat (cat)}
                    <span class="tag">{cat}</span>
                  {/each}
                  <button
                    class="star"
                    class:on={current.favorited}
                    title={current.favorited ? $t('communityDocs.reader.removeFavorite') : $t('communityDocs.reader.addFavorite')}
                    aria-pressed={current.favorited}
                    onclick={() => toggleFavorite(current)}
                  >
                    <Icon name="star" size={13} /> {current.favorited ? $t('communityDocs.reader.favorited') : $t('communityDocs.reader.favorite')}
                  </button>
                </div>
                <h1>{current.title}</h1>
                {#if current.summary}<p class="lead">{current.summary}</p>{/if}
                <p class="meta">
                  {$t('communityDocs.reader.synced', { date: fmtDate(current.synced_at) })}
                  {#if current.source_url}
                    · <a href={current.source_url} target="_blank" rel="noopener">{$t('communityDocs.reader.source')}</a>
                  {/if}
                </p>
              </div>
              <!-- eslint-disable-next-line svelte/no-at-html-tags -->
              <div class="content">{@html current.body}</div>
            </article>
          {/if}
        {:else}
          <div class="browse-head">
            <input
              class="search lg"
              type="search"
              placeholder={$t('communityDocs.browse.searchPlaceholder')}
              bind:value={centerSearch}
            />
          </div>

          {#if centerSearch.trim()}
            <!-- Search results: grouped by category -->
            {#if !centerGroups.length}
              <p class="muted">{$t('communityDocs.browse.noMatches', { query: centerSearch })}</p>
            {:else}
              {#each centerGroups as g (g.category)}
                <div class="result-group">
                  <h2 class="group-title">{g.category}</h2>
                  {#each g.docs as d (d.slug)}
                    <button class="doc-row" onclick={() => openDoc(d.slug)}>
                      <span class="doc-title">{d.title}</span>
                      {#if d.summary}<span class="doc-summary">{d.summary}</span>{/if}
                    </button>
                  {/each}
                </div>
              {/each}
            {/if}
          {:else if view === 'category'}
            <!-- Docs in the chosen category -->
            <div class="center-top">
              <button class="back" onclick={goBack}>{$t('communityDocs.nav.back')}</button>
              <h2 class="cat-heading">{activeCategory}</h2>
            </div>
            {#if !categoryDocs.length}
              <p class="muted">{$t('communityDocs.category.noDocs')}</p>
            {:else}
              <div class="doc-grid">
                {#each categoryDocs as d (d.slug)}
                  <button class="doc-card" onclick={() => openDoc(d.slug)}>
                    <span class="doc-title">{d.title}</span>
                    {#if d.summary}<span class="doc-summary">{d.summary}</span>{/if}
                  </button>
                {/each}
              </div>
            {/if}
          {:else}
            <!-- Default: category cards -->
            {#if !categoryCards.length}
              <p class="muted">{$t('communityDocs.category.noDocsYet')}</p>
            {:else}
              <div class="cat-grid">
                {#each categoryCards as c (c.category)}
                  <button class="cat-card" onclick={() => openCategory(c.category)}>
                    <span class="cat-name">{c.category}</span>
                    <span class="cat-count">{$t('communityDocs.category.docCount', { count: c.count, doc: c.count === 1 ? $t('communityDocs.category.doc') : $t('communityDocs.category.docs') })}</span>
                  </button>
                {/each}
              </div>
            {/if}
          {/if}
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .page {
    padding: var(--space-4);
    height: 100%;
  }
  .layout {
    display: flex;
    gap: var(--space-4);
    height: 100%;
    min-height: 0;
  }

  /* Left pane */
  .fav-pane {
    width: 260px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border-right: 1px solid var(--border);
    padding-right: var(--space-4);
    min-height: 0;
  }
  .fav-head {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .fav-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
    min-height: 0;
  }
  .fav-group {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin-bottom: var(--space-2);
  }
  .fav-cat-row {
    display: flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    padding: 6px 6px;
    cursor: pointer;
    text-align: left;
    width: 100%;
  }
  .fav-cat-row:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .chev {
    display: inline-block;
    transition: transform 0.12s ease;
    font-size: 0.9rem;
    line-height: 1;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .fav-cat-name {
    flex: 1;
    font-size: 0.74rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fav-cat-count {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .fav {
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--text);
    padding: 6px 10px 6px 22px;
    cursor: pointer;
    text-align: left;
    font-size: 0.86rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fav:hover {
    background: var(--surface-2);
  }
  .fav.active {
    background: var(--surface-2);
    border-color: var(--border);
  }

  /* Center pane */
  .center {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    overflow-y: auto;
  }
  .browse-head {
    display: flex;
    justify-content: center;
  }
  .center-top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .cat-heading {
    margin: 0;
    font-size: 1.2rem;
    color: var(--text);
  }
  .back {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 6px 12px;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .back:hover {
    background: var(--surface);
  }

  /* Search inputs */
  .search {
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 7px 10px;
    font: inherit;
  }
  .search.sm {
    font-size: 0.85rem;
    flex: 1;
    min-width: 0;
  }
  .search.lg {
    font-size: 1rem;
    padding: 12px 16px;
    width: 100%;
    max-width: 620px;
  }

  .icon-btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    width: 34px;
    height: 34px;
    flex-shrink: 0;
    cursor: pointer;
    font-size: 1.05rem;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .icon-btn:hover:not(:disabled) {
    background: var(--surface);
  }
  .icon-btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .icon-btn .spin {
    display: inline-block;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Category cards */
  .cat-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-3);
  }
  .cat-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg, var(--radius));
    color: var(--text);
    padding: var(--space-4);
    cursor: pointer;
    text-align: left;
  }
  .cat-card:hover {
    border-color: var(--accent);
  }
  .cat-name {
    font-size: 1rem;
    font-weight: 600;
  }
  .cat-count {
    font-size: 0.78rem;
    color: var(--muted);
  }

  /* Doc grid within a category */
  .doc-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--space-3);
  }
  .doc-card,
  .doc-row {
    display: flex;
    flex-direction: column;
    gap: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-3);
    cursor: pointer;
    text-align: left;
  }
  .doc-card:hover,
  .doc-row:hover {
    border-color: var(--accent);
  }
  .doc-row {
    background: transparent;
    border-color: transparent;
  }
  .doc-row:hover {
    background: var(--surface-2);
    border-color: var(--border);
  }

  /* Search results grouping */
  .result-group {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: var(--space-3);
  }
  .group-title {
    margin: 0 0 4px;
    font-size: 0.72rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .doc-title {
    font-size: 0.9rem;
  }
  .doc-summary {
    font-size: 0.76rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  /* Reader */
  .article {
    max-width: 760px;
  }
  .article-head {
    margin-bottom: var(--space-4);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  .article-head-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .tag {
    display: inline-block;
    background: var(--surface-2);
    color: var(--muted);
    border-radius: var(--radius);
    padding: 2px 8px;
    font-size: 0.72rem;
  }
  .star {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: 5px 10px;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .star:hover {
    color: var(--text);
  }
  .star.on {
    color: var(--amber);
    border-color: var(--amber);
  }
  .article-head h1 {
    margin: var(--space-2) 0 0;
    font-size: 1.5rem;
    color: var(--text);
  }
  .lead {
    margin: var(--space-2) 0 0;
    color: var(--muted);
    font-size: 0.95rem;
  }
  .meta {
    margin: var(--space-2) 0 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .meta a {
    color: var(--accent);
    text-decoration: none;
  }
  .meta a:hover {
    text-decoration: underline;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.82rem;
  }

  /* Rendered doc body. */
  .content {
    color: var(--text);
    font-size: 0.92rem;
    line-height: 1.65;
  }
  .content :global(h2) {
    font-size: 1.15rem;
    margin: var(--space-6) 0 var(--space-2);
    color: var(--text);
  }
  .content :global(h3) {
    font-size: 1rem;
    margin: var(--space-4) 0 var(--space-1);
    color: var(--text);
  }
  .content :global(p) {
    margin: var(--space-2) 0;
  }
  .content :global(ul),
  .content :global(ol) {
    margin: var(--space-2) 0;
    padding-left: var(--space-6);
  }
  .content :global(li) {
    margin: 4px 0;
  }
  .content :global(a) {
    color: var(--accent);
  }
  .content :global(strong) {
    color: var(--text);
  }
  .content :global(table) {
    border-collapse: collapse;
    margin: var(--space-3) 0;
    font-size: 0.86rem;
  }
  .content :global(th),
  .content :global(td) {
    border: 1px solid var(--border);
    padding: 6px 12px;
    text-align: left;
  }
  .content :global(th) {
    background: var(--surface-2);
    color: var(--muted);
    font-weight: 600;
  }
  .content :global(svg) {
    margin: var(--space-3) 0;
    color: var(--muted);
  }
  .star.on :global(svg) {
    fill: currentColor;
  }
</style>
