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
  // AND docs at once, with results grouped by category. Docs render as cards or as a dense
  // list (`display`, persisted per-browser); categories are always cards.
  //
  // Bodies render with {@html}. They are ammonia-sanitised server-side on every write path
  // (community_docs_api::clean_doc) — the upstream site is not treated as trusted, so a
  // compromised feed cannot ship script into this origin.
  import { onMount } from 'svelte';
  import { communityDocsApi, groupByCategory } from '$lib/modules/community-docs/api.js';
  import Skeleton from '$lib/ui/Skeleton.svelte';
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

  // Doc display mode — 'card' | 'list'. Persisted per-browser; categories ignore it.
  let display = $state('card');
  const PREFS_KEY = 'otw.communityDocs.prefs.v1';
  let prefsLoaded = false;

  $effect(() => {
    const snapshot = { display };
    if (!prefsLoaded) return;
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(snapshot));
    } catch {
      /* quota / unavailable — non-fatal */
    }
  });

  onMount(async () => {
    try {
      const p = JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
      if (p.display === 'card' || p.display === 'list') display = p.display;
    } catch {
      /* corrupt prefs — ignore, use defaults */
    }
    prefsLoaded = true;
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

  // Category cards for the default browse view. `sample` is the first few doc titles —
  // a card that names what is inside beats a card that only counts it.
  const CAT_PREVIEW = 3;
  const categoryCards = $derived.by(() =>
    groupByCategory(docs).map((g) => ({
      category: g.category,
      count: g.docs.length,
      sample: g.docs.slice(0, CAT_PREVIEW).map((d) => d.title)
    }))
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

<!-- One doc list, two skins: cards (grid, 3-line summary) or rows (dense, one line).
     Used by the category listing and by every search-result group. -->
{#snippet docItems(list)}
  {#if display === 'card'}
    <div class="doc-grid">
      {#each list as d (d.slug)}
        <button class="doc-card" onclick={() => openDoc(d.slug)}>
          <span class="doc-head">
            <span class="doc-title">{d.title}</span>
            {#if d.favorited}<span class="doc-star"><Icon name="star" size={12} /></span>{/if}
          </span>
          {#if d.summary}<span class="doc-summary">{d.summary}</span>{/if}
          <span class="doc-foot">
            <span class="doc-cats">{docCats(d).join(' · ')}</span>
            <span class="doc-date">{fmtDate(d.synced_at)}</span>
          </span>
        </button>
      {/each}
    </div>
  {:else}
    <ul class="doc-list">
      {#each list as d (d.slug)}
        <li>
          <button class="doc-row" onclick={() => openDoc(d.slug)}>
            <span class="row-main">
              <span class="doc-title">{d.title}</span>
              {#if d.favorited}<span class="doc-star"><Icon name="star" size={12} /></span>{/if}
              {#if d.summary}<span class="doc-summary">{d.summary}</span>{/if}
            </span>
            <span class="row-meta">
              <span class="doc-cats">{docCats(d).join(' · ')}</span>
              <span class="doc-date">{fmtDate(d.synced_at)}</span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
{/snippet}

<div class="page">
  {#if loading}
    <div class="sk-page" aria-busy="true">
      <Skeleton rows={6} height="2.4rem" gap="var(--space-3)" />
    </div>
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
            <div class="seg" role="group" aria-label={$t('communityDocs.browse.display')}>
              <button
                class:active={display === 'card'}
                aria-pressed={display === 'card'}
                title={$t('communityDocs.browse.cards')}
                onclick={() => (display = 'card')}><Icon name="grid" size={13} /></button
              >
              <button
                class:active={display === 'list'}
                aria-pressed={display === 'list'}
                title={$t('communityDocs.browse.list')}
                onclick={() => (display = 'list')}><Icon name="list" size={13} /></button
              >
            </div>
          </div>

          {#if centerSearch.trim()}
            <!-- Search results: grouped by category -->
            {#if !centerGroups.length}
              <p class="muted">{$t('communityDocs.browse.noMatches', { query: centerSearch })}</p>
            {:else}
              {#each centerGroups as g (g.category)}
                <div class="result-group">
                  <h2 class="group-title">
                    <span>{g.category}</span><em>{g.docs.length}</em>
                  </h2>
                  {@render docItems(g.docs)}
                </div>
              {/each}
            {/if}
          {:else if view === 'category'}
            <!-- Docs in the chosen category -->
            <div class="center-top">
              <button class="back" onclick={goBack}>{$t('communityDocs.nav.back')}</button>
              <h2 class="cat-heading">{activeCategory}</h2>
              <span class="cat-heading-count">
                {$t('communityDocs.category.docCount', { count: categoryDocs.length, doc: categoryDocs.length === 1 ? $t('communityDocs.category.doc') : $t('communityDocs.category.docs') })}
              </span>
            </div>
            {#if !categoryDocs.length}
              <p class="muted">{$t('communityDocs.category.noDocs')}</p>
            {:else}
              {@render docItems(categoryDocs)}
            {/if}
          {:else}
            <!-- Default: the categories -->
            {#if !categoryCards.length}
              <p class="muted">{$t('communityDocs.category.noDocsYet')}</p>
            {:else if display === 'card'}
              <div class="cat-grid">
                {#each categoryCards as c (c.category)}
                  <button class="cat-card" onclick={() => openCategory(c.category)}>
                    <span class="cat-top">
                      <span class="cat-name">{c.category}</span>
                      <span class="cat-go"><Icon name="arrow-right" size={14} /></span>
                    </span>
                    <span class="cat-preview">
                      {#each c.sample as title (title)}
                        <span class="cat-doc">{title}</span>
                      {/each}
                      {#if c.count > c.sample.length}
                        <span class="cat-more">
                          {$t('communityDocs.category.more', { count: c.count - c.sample.length })}
                        </span>
                      {/if}
                    </span>
                    <span class="cat-foot">
                      <em>{c.count}</em>
                      {c.count === 1 ? $t('communityDocs.category.doc') : $t('communityDocs.category.docs')}
                    </span>
                  </button>
                {/each}
              </div>
            {:else}
              <ul class="cat-list">
                {#each categoryCards as c (c.category)}
                  <li>
                    <button class="cat-row" onclick={() => openCategory(c.category)}>
                      <span class="row-main">
                        <span class="cat-name">{c.category}</span>
                        <span class="cat-doc">{c.sample.join(' · ')}</span>
                      </span>
                      <span class="row-meta">
                        <span class="cat-foot">
                          <em>{c.count}</em>
                          {c.count === 1 ? $t('communityDocs.category.doc') : $t('communityDocs.category.docs')}
                        </span>
                        <span class="cat-go"><Icon name="arrow-right" size={14} /></span>
                      </span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          {/if}
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .sk-page {
    padding: var(--space-4);
  }
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
  /* Stretch, not center: the refresh button takes the search field's height so the
     two line up top and bottom. */
  .fav-head {
    display: flex;
    gap: var(--space-2);
    align-items: stretch;
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
    font-size: var(--text-base);
    line-height: 1;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .fav-cat-name {
    flex: 1;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fav-cat-count {
    font-size: var(--text-xs);
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
    font-size: var(--text-base);
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
    align-items: center;
    gap: var(--space-2);
  }
  .center-top {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }
  .cat-heading {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .cat-heading-count {
    font-size: var(--text-xs);
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  /* Card / list toggle — same segmented control as the other library pages. */
  .seg {
    display: inline-flex;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    flex-shrink: 0;
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--surface);
    border: none;
    color: var(--dim);
    padding: 10px 12px;
    cursor: pointer;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.active {
    background: var(--surface-2);
    color: var(--text);
  }
  .back {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 6px 12px;
    font: inherit;
    font-size: var(--text-base);
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
    font-size: var(--text-base);
    flex: 1;
    min-width: 0;
  }
  .search.lg {
    font-size: var(--text-md);
    padding: 12px 16px;
    width: 100%;
    max-width: 620px;
  }

  .icon-btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    width: 36px;
    flex-shrink: 0;
    cursor: pointer;
    font-size: var(--text-md);
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

  /* Category cards — a shelf: what the category holds, then how much of it.
     The left rule is the only color spent here, and only on hover. */
  .cat-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--space-3);
  }
  .cat-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-left: var(--active-rule) solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-4);
    min-height: 168px;
    cursor: pointer;
    text-align: left;
    transition:
      background-color var(--dur-fast) var(--ease),
      border-left-color var(--dur-fast) var(--ease);
  }
  .cat-card:hover {
    background: var(--surface-2);
    border-left-color: var(--accent);
  }
  .cat-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .cat-name {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The arrow only shows the card is a door — it stays quiet until hover. */
  .cat-go {
    display: inline-flex;
    color: var(--faint);
    flex-shrink: 0;
    transition:
      color var(--dur-fast) var(--ease),
      transform var(--dur-fast) var(--ease);
  }
  .cat-card:hover .cat-go {
    color: var(--accent);
    transform: translateX(2px);
  }
  .cat-preview {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }
  .cat-doc {
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: var(--lh-base);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cat-more {
    font-size: var(--text-xs);
    color: var(--faint);
  }
  .cat-foot {
    display: flex;
    align-items: baseline;
    gap: 5px;
    padding-top: var(--space-2);
    border-top: var(--hairline) solid var(--border);
    font-size: var(--text-xs);
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .cat-foot em {
    font-family: var(--mono);
    font-style: normal;
    font-size: var(--text-sm);
    color: var(--text);
  }

  /* Category list — the dense skin of the same shelf: name, sample titles, count. */
  .cat-list {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: var(--hairline) solid var(--border);
  }
  .cat-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
    width: 100%;
    background: transparent;
    border: none;
    border-bottom: var(--hairline) solid var(--border);
    border-left: var(--active-rule) solid transparent;
    color: var(--text);
    padding: 12px var(--space-3);
    cursor: pointer;
    text-align: left;
    transition:
      background-color var(--dur-fast) var(--ease),
      border-left-color var(--dur-fast) var(--ease);
  }
  .cat-row:hover {
    background: var(--surface-2);
    border-left-color: var(--accent);
  }
  .cat-row .cat-name {
    flex-shrink: 0;
  }
  /* In a row the sample titles are a trailing clause; the count rule loses its filet. */
  .cat-row .cat-doc {
    font-size: var(--text-xs);
    color: var(--faint);
  }
  .cat-row .cat-foot {
    padding-top: 0;
    border-top: none;
  }
  .cat-row .cat-go {
    align-self: center;
  }
  .cat-row:hover .cat-go {
    color: var(--accent);
    transform: translateX(2px);
  }

  /* Doc cards — title, summary, then a meta rule (categories · sync date). */
  .doc-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: var(--space-3);
  }
  .doc-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-left: var(--active-rule) solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-4);
    min-height: 150px;
    cursor: pointer;
    text-align: left;
    transition:
      background-color var(--dur-fast) var(--ease),
      border-left-color var(--dur-fast) var(--ease);
  }
  .doc-card:hover {
    background: var(--surface-2);
    border-left-color: var(--accent);
  }
  .doc-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .doc-star {
    display: inline-flex;
    color: var(--amber);
    flex-shrink: 0;
  }
  .doc-star :global(svg) {
    fill: currentColor;
  }
  .doc-foot {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    margin-top: auto;
    padding-top: var(--space-2);
    border-top: var(--hairline) solid var(--border);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .doc-cats {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .doc-date {
    font-family: var(--mono);
    flex-shrink: 0;
    color: var(--faint);
  }

  /* Doc list — the dense skin: one row per doc, title then summary on the same line. */
  .doc-list {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: var(--hairline) solid var(--border);
  }
  .doc-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
    width: 100%;
    background: transparent;
    border: none;
    border-bottom: var(--hairline) solid var(--border);
    border-left: var(--active-rule) solid transparent;
    color: var(--text);
    padding: 10px var(--space-3);
    cursor: pointer;
    text-align: left;
    transition:
      background-color var(--dur-fast) var(--ease),
      border-left-color var(--dur-fast) var(--ease);
  }
  .doc-row:hover {
    background: var(--surface-2);
    border-left-color: var(--accent);
  }
  .row-main {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
  }
  .row-meta {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    flex-shrink: 0;
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .row-meta .doc-cats {
    max-width: 200px;
  }

  /* Search results grouping */
  .result-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-bottom: var(--space-6);
  }
  .group-title {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .group-title em {
    font-family: var(--mono);
    font-style: normal;
    color: var(--faint);
  }
  .doc-title {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
    color: var(--text);
  }
  .doc-card .doc-title {
    font-size: var(--text-md);
  }
  .doc-summary {
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: var(--lh-base);
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
  }
  /* In a row the summary is a trailing clause, never a second line. */
  .row-main .doc-summary {
    display: block;
    white-space: nowrap;
    font-size: var(--text-xs);
    min-width: 0;
  }
  .row-main .doc-title {
    flex-shrink: 0;
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
    font-size: var(--text-xs);
  }
  .star {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: 5px 10px;
    font: inherit;
    font-size: var(--text-sm);
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
    font-size: var(--text-lg);
    color: var(--text);
  }
  .lead {
    margin: var(--space-2) 0 0;
    color: var(--muted);
    font-size: var(--text-md);
  }
  .meta {
    margin: var(--space-2) 0 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .meta a {
    color: var(--text);
    text-decoration: none;
  }
  .meta a:hover {
    text-decoration: underline;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }

  /* Rendered doc body. */
  .content {
    color: var(--text);
    font-size: var(--text-base);
    line-height: 1.65;
  }
  .content :global(h2) {
    font-size: var(--text-md);
    margin: var(--space-6) 0 var(--space-2);
    color: var(--text);
  }
  .content :global(h3) {
    font-size: var(--text-md);
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
    color: var(--text);
  }
  .content :global(strong) {
    color: var(--text);
  }
  .content :global(table) {
    border-collapse: collapse;
    margin: var(--space-3) 0;
    font-size: var(--text-base);
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
    font-weight: var(--fw-medium);
  }
  .content :global(svg) {
    margin: var(--space-3) 0;
    color: var(--muted);
  }
  .star.on :global(svg) {
    fill: currentColor;
  }
</style>
