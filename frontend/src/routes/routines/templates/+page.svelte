<script>
  // Templates — the authoring half of the Routines module.
  //
  // Two panes: categories on the left, the selected category's templates on the right.
  // A trading day is structured by phase (pre-market, in session, post-mortem, week
  // review…), so the category list is the page's spine — picking one narrows the right
  // pane instead of scrolling through every template at once. Each category carries its
  // template count as a badge, so the shape of the library is readable without clicking.
  //
  // "All" stays available as the first entry, and it is the only view that groups: it
  // prints a category heading before each run so a flat read is still organised.
  //
  // Categories are managed inline here rather than in a settings screen — they exist only
  // to organise templates, so they belong next to them.
  import { onMount } from 'svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import TemplateEditor from '$lib/modules/routines/TemplateEditor.svelte';
  import CategoryManager from '$lib/ui/CategoryManager.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import {
    traderApi,
    describeSchedule,
    describeWindow,
    nextOccurrences
  } from '$lib/modules/routines/api.js';
  import { t, locale } from '$lib/i18n';

  const NAV_LINKS = [
    { href: '/routines', icon: 'check-square', key: 'routines.nav.board' },
    { href: '/routines/templates', icon: 'clipboard-list', key: 'routines.nav.templates' }
  ];

  let rows = $state([]); // [{ routine, items }]
  let categories = $state([]);
  let loading = $state(true);
  let error = $state('');

  let showEditor = $state(false);
  let editingId = $state(null);
  let presetCategoryId = $state(null);
  let showCategories = $state(false);

  // 'all' | a category id | 'none' (uncategorised)
  let selected = $state('all');
  let search = $state('');
  let expanded = $state(new Set()); // template ids whose checklist is unfolded

  onMount(load);

  async function load() {
    loading = true;
    error = '';
    try {
      const r = await traderApi.listRoutines();
      rows = r.routines;
      categories = r.categories;
      // A category deleted elsewhere must not leave the page filtered to nothing.
      if (selected !== 'all' && selected !== 'none' && !categories.some((c) => c.id === selected)) {
        selected = 'all';
      }
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  // Search spans everything a template is made of — its name, its description and its
  // steps — so "max loss" finds the template containing that step, not just one named it.
  // It applies before the category split, so the sidebar counts reflect what a search
  // actually leaves behind rather than advertising rows the right pane won't show.
  const searched = $derived.by(() => {
    const q = search.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter(({ routine, items }) =>
      [routine.name, routine.description, ...items.map((i) => i.label)]
        .filter(Boolean)
        .join(' ')
        .toLowerCase()
        .includes(q)
    );
  });

  const countFor = (id) =>
    searched.filter(({ routine }) =>
      id === 'none' ? !routine.category_id : routine.category_id === id
    ).length;

  const looseCount = $derived(countFor('none'));

  // The sidebar: All, every category, then Uncategorised when anything landed there.
  const sideItems = $derived.by(() => {
    const out = [
      { id: 'all', name: $t('routines.templates.allCategories'), color: '', count: searched.length }
    ];
    for (const c of categories) {
      out.push({ id: c.id, name: c.name, color: c.color, count: countFor(c.id) });
    }
    if (looseCount > 0) {
      out.push({
        id: 'none',
        name: $t('routines.templates.uncategorised'),
        color: '',
        count: looseCount
      });
    }
    return out;
  });

  const catById = $derived(new Map(categories.map((c) => [c.id, c])));

  // The right pane. On "All" it is grouped with a heading per category; on a single
  // category it is one flat run, since the heading would just repeat the sidebar.
  const panes = $derived.by(() => {
    if (selected === 'all') {
      const out = [];
      for (const c of categories) {
        const list = searched.filter(({ routine }) => routine.category_id === c.id);
        if (list.length > 0) out.push({ category: c, rows: list });
      }
      const loose = searched.filter(({ routine }) => !routine.category_id);
      if (loose.length > 0) out.push({ category: null, rows: loose });
      return out;
    }
    const list = searched.filter(({ routine }) =>
      selected === 'none' ? !routine.category_id : routine.category_id === selected
    );
    return [{ category: catById.get(selected) ?? null, rows: list, flat: true }];
  });

  const visibleCount = $derived(panes.reduce((n, p) => n + p.rows.length, 0));
  const selectedName = $derived(
    selected === 'all'
      ? $t('routines.templates.allCategories')
      : selected === 'none'
        ? $t('routines.templates.uncategorised')
        : (catById.get(selected)?.name ?? '')
  );

  function openNew(categoryId = null) {
    editingId = null;
    // Creating from inside a category pre-selects it — the obvious intent.
    presetCategoryId =
      categoryId ?? (selected !== 'all' && selected !== 'none' ? selected : null);
    showEditor = true;
  }
  function openEdit(id) {
    editingId = id;
    presetCategoryId = null;
    showEditor = true;
  }

  function toggleExpand(id) {
    // A Set in a $state rune must be reassigned to trigger reactivity.
    const next = new Set(expanded);
    next.has(id) ? next.delete(id) : next.add(id);
    expanded = next;
  }

  async function toggleActive(routine) {
    try {
      await traderApi.updateRoutine(routine.id, { active: !routine.active });
      await load();
    } catch (e) {
      error = e.message;
    }
  }

  async function duplicate(routine) {
    try {
      await traderApi.duplicateRoutine(routine.id, `${routine.name} (copy)`);
      await load();
    } catch (e) {
      error = e.message;
    }
  }

  // Deleting a template takes its tick history with it.
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function del(routine) {
    pendingDelete = routine;
    confirmOpen = true;
  }
  async function confirmDelete() {
    const r = pendingDelete;
    pendingDelete = null;
    if (!r) return;
    try {
      await traderApi.deleteRoutine(r.id);
      await load();
    } catch (e) {
      error = e.message;
    }
  }

  function fmtNext(iso) {
    return new Date(`${iso}T00:00:00`).toLocaleDateString($locale, {
      weekday: 'short',
      day: 'numeric',
      month: 'short'
    });
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="routines.nav.label" />

  <PageHeader
    title={$t('routines.templates.title')}
    subtitle={$t('routines.templates.subtitle', { n: rows.length })}
  >
    {#snippet actions()}
      <button class="btn" onclick={() => (showCategories = true)}>
        <Icon name="tag" size={13} /> {$t('routines.templates.manageCategories')}
      </button>
      <button class="btn primary" onclick={() => openNew()}>
        <Icon name="plus" size={13} /> {$t('routines.templates.newTemplate')}
      </button>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} copyable />

  {#if loading}
    <Skeleton rows={5} height="3rem" gap="var(--space-3)" />
  {:else if rows.length === 0}
    <EmptyState
      icon="clipboard-list"
      title={$t('routines.templates.emptyTitle')}
      description={$t('routines.templates.emptyBody')}
    >
      {#snippet action()}
        <button class="btn primary" onclick={() => openNew()}>
          {$t('routines.templates.newTemplate')}
        </button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="layout">
      <!-- ── Left: categories ── -->
      <aside class="side">
        <input
          class="search"
          type="search"
          placeholder={$t('routines.templates.searchPlaceholder')}
          bind:value={search}
        />

        <nav class="catlist" aria-label={$t('routines.templates.manageCategories')}>
          {#each sideItems as c (c.id)}
            <button
              class="cat"
              class:on={selected === c.id}
              class:muted={c.count === 0}
              aria-current={selected === c.id ? 'true' : undefined}
              onclick={() => (selected = c.id)}
            >
              <span
                class="dot"
                style:background={c.color || 'var(--muted)'}
                class:all={c.id === 'all'}
                aria-hidden="true"
              ></span>
              <span class="cname">{c.name}</span>
              <span class="count">{c.count}</span>
            </button>
          {/each}
        </nav>

        <button class="sidemanage" onclick={() => (showCategories = true)}>
          <Icon name="settings" size={12} /> {$t('routines.templates.manageCategories')}
        </button>
      </aside>

      <!-- ── Right: templates in the selected category ── -->
      <div class="main">
        <div class="mainhead">
          <h2>{selectedName}</h2>
          <span class="mcount">{$t('routines.templates.steps2', { n: visibleCount })}</span>
        </div>

        {#if visibleCount === 0}
          <p class="nores">
            {#if search}
              {$t('routines.templates.noMatches', { q: search })}
            {:else}
              {$t('routines.templates.categoryEmpty')}
            {/if}
          </p>
        {/if}

        {#each panes as pane (pane.category?.id ?? 'none')}
          {#if pane.rows.length > 0}
            <section class="pane">
              {#if !pane.flat}
                <h3 class="phead">
                  <span
                    class="dot"
                    style:background={pane.category?.color || 'var(--muted)'}
                    aria-hidden="true"
                  ></span>
                  {pane.category?.name ?? $t('routines.templates.uncategorised')}
                  <span class="count">{pane.rows.length}</span>
                </h3>
              {/if}

              {#each pane.rows as { routine, items } (routine.id)}
                {@const next = nextOccurrences(routine, new Date(), 3)}
                <article
                  class="card"
                  class:paused={!routine.active}
                  style:--cat={pane.category?.color || 'var(--border)'}
                >
                  <div class="chead">
                    <button
                      class="title"
                      onclick={() => toggleExpand(routine.id)}
                      aria-expanded={expanded.has(routine.id)}
                      aria-controls="steps-{routine.id}"
                    >
                      <Icon
                        name={expanded.has(routine.id) ? 'chevron-down' : 'chevron-right'}
                        size={13}
                      />
                      <span class="tname">{routine.name}</span>
                      <span class="steps-n">{$t('routines.templates.steps', { n: items.length })}</span>
                      {#if !routine.active}
                        <span class="pill">{$t('routines.templates.paused')}</span>
                      {/if}
                    </button>

                    <div class="cactions">
                      <button
                        class="icon"
                        title={routine.active ? $t('routines.templates.pause') : $t('routines.templates.resume')}
                        aria-label={routine.active ? $t('routines.templates.pause') : $t('routines.templates.resume')}
                        onclick={() => toggleActive(routine)}
                      >
                        <Icon name={routine.active ? 'eye' : 'eye-off'} size={14} />
                      </button>
                      <button class="icon" title={$t('routines.templates.duplicate')} aria-label={$t('routines.templates.duplicate')} onclick={() => duplicate(routine)}>
                        <Icon name="copy" size={14} />
                      </button>
                      <button class="icon" title={$t('common.edit')} aria-label={$t('common.edit')} onclick={() => openEdit(routine.id)}>
                        <Icon name="pencil" size={14} />
                      </button>
                      <button class="icon del" title={$t('common.delete')} aria-label={$t('common.delete')} onclick={() => del(routine)}>
                        <Icon name="trash" size={14} />
                      </button>
                    </div>
                  </div>

                  {#if routine.description}
                    <p class="desc">{routine.description}</p>
                  {/if}

                  <div class="meta">
                    <span class="sched">
                      <Icon name="repeat" size={12} />
                      {describeSchedule(routine, $t)}
                    </span>
                    {#if describeWindow(routine, $t)}
                      <span>{describeWindow(routine, $t)}</span>
                    {/if}
                    {#if routine.active && next.length > 0}
                      <span class="next">
                        {$t('routines.templates.next')}
                        {#each next as d (d)}<b>{fmtNext(d)}</b>{/each}
                      </span>
                    {/if}
                  </div>

                  {#if expanded.has(routine.id)}
                    <div class="body" id="steps-{routine.id}">
                      {#if routine.notes}
                        <!-- Sanitised server-side at write time (ammonia allowlist). -->
                        <div class="notes">{@html routine.notes}</div>
                      {/if}
                      <ul class="steplist">
                        {#each items as it, n (it.id)}
                          <li>
                            <span class="sn">{n + 1}</span>
                            <div class="sbody">
                              <div class="sline">
                                <span class="stext">{it.label}</span>
                                {#if it.url}
                                  <a href={it.url} target="_blank" rel="noopener noreferrer nofollow">
                                    <Icon name="external-link" size={11} />
                                    {it.link_label || $t('routines.templates.openLink')}
                                  </a>
                                {/if}
                              </div>
                              {#if it.note}
                                <div class="snote">{@html it.note}</div>
                              {/if}
                            </div>
                          </li>
                        {/each}
                      </ul>
                    </div>
                  {/if}
                </article>
              {/each}
            </section>
          {/if}
        {/each}
      </div>
    </div>
  {/if}
</div>

<TemplateEditor
  bind:open={showEditor}
  routineId={editingId}
  {categories}
  {presetCategoryId}
  onsaved={load}
/>

<CategoryManager bind:open={showCategories} {categories} api={traderApi} onsaved={load} />

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('routines.editor.deleteTemplate')}
  message={$t('routines.editor.confirmDelete')}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .page {
    height: 100%;
    padding: var(--space-6);
    overflow-y: auto;
  }

  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }

  /* ── Two-pane layout ── */
  .layout {
    display: grid;
    grid-template-columns: 216px minmax(0, 1fr);
    gap: var(--space-6);
    align-items: start;
  }
  /* Below ~900px the sidebar stops being a sidebar and becomes a row above the list —
     a 216px rail plus cards does not fit a phone. */
  @media (max-width: 900px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-4);
    }
  }

  /* ── Left rail ── */
  .side {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    position: sticky;
    top: 0;
  }
  @media (max-width: 900px) {
    .side {
      position: static;
    }
  }

  /* Same plain search field the other modules use (resources, prompt-store, mportfolios):
     one bordered input, no wrapper and no icon. */
  .search {
    background: var(--surface-2, var(--surface));
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 7px 10px;
    font: inherit;
    font-size: var(--text-base);
    width: 100%;
    min-width: 0;
  }

  .catlist {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  @media (max-width: 900px) {
    .catlist {
      flex-direction: row;
      flex-wrap: wrap;
      gap: var(--space-1);
    }
  }

  .cat {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-1) var(--space-2);
    background: transparent;
    /* The selected state is a left bar; keeping it transparent on the others stops the
       row from shifting when selection moves. */
    border: none;
    border-left: 2px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }
  .cat:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .cat.on {
    color: var(--text);
    background: var(--surface-2);
    border-left-color: var(--accent);
    font-weight: var(--fw-medium);
  }
  .cat.muted:not(.on) {
    opacity: 0.55;
  }
  @media (max-width: 900px) {
    .cat {
      width: auto;
      border-left: none;
      border: 0.5px solid var(--border);
    }
    .cat.on {
      border-color: var(--accent);
    }
  }

  .cname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  /* "All" has no colour of its own — a ring reads as "every colour" without picking one. */
  .dot.all {
    background: transparent !important;
    border: 1.5px solid var(--muted);
  }

  /* Deliberately not `.badge`: that global class carries a border and an uppercase
     treatment meant for status pills. A count is just a number. */
  .count {
    flex: none;
    min-width: 16px;
    text-align: right;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .cat.on .count {
    color: var(--text);
  }

  .sidemanage {
    align-self: flex-start;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 0 var(--space-2);
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .sidemanage:hover {
    color: var(--text);
  }

  /* ── Right pane ── */
  .main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .mainhead {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    padding-bottom: var(--space-2);
    border-bottom: 0.5px solid var(--border);
  }
  .mainhead h2 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .mcount {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .nores {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }

  .pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .phead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--muted);
    margin-top: var(--space-2);
  }

  /* ── Card ── */
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    /* The category colour is the card's left edge, so a card stays identifiable once it
       has scrolled away from its heading. */
    border-left: 2px solid var(--cat);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .card.paused {
    opacity: 0.62;
  }

  .chead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .title {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    padding: 0;
    color: var(--text);
    font-size: var(--text-base);
    text-align: left;
    cursor: pointer;
  }
  .tname {
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .steps-n {
    font-size: var(--text-xs);
    color: var(--muted);
    flex: none;
  }
  .pill {
    font-size: var(--text-xs);
    color: var(--amber);
    border: 0.5px solid var(--amber);
    border-radius: var(--radius);
    padding: 0 var(--space-1);
    flex: none;
  }

  .cactions {
    display: flex;
    gap: 2px;
    flex: none;
  }
  .icon {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 4px;
    border-radius: var(--radius);
    display: inline-flex;
  }
  .icon:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .icon.del:hover {
    color: var(--red);
  }

  .desc {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .sched {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--text);
  }
  .next {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .next b {
    font-weight: 400;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0 var(--space-1);
    white-space: nowrap;
  }

  /* ── Unfolded body ── */
  .body {
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .notes {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .notes :global(p) {
    margin: 0 0 var(--space-1);
  }
  .notes :global(ul),
  .notes :global(ol) {
    margin: 0;
    padding-left: var(--space-4);
  }
  .notes :global(a) {
    color: var(--accent);
  }

  /* Steps read as a numbered sequence, not a browser <ol>: the index is a fixed-width
     dim column so the labels align on one edge however many steps there are, and a note
     hangs under its own step rather than under the whole list. */
  .steplist {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .steplist li {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    font-size: var(--text-sm);
  }
  .steplist li + li {
    border-top: 0.5px solid var(--border);
  }
  .sn {
    flex: none;
    width: 16px;
    text-align: right;
    color: var(--faint, var(--muted));
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    line-height: 1.6;
  }
  .sbody {
    min-width: 0;
    flex: 1;
  }
  .sline {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .stext {
    color: var(--text);
  }
  .steplist a {
    color: var(--accent);
    font-size: var(--text-xs);
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .steplist a:hover {
    text-decoration: underline;
  }
  .snote {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-top: var(--space-1);
  }
  .snote :global(p) {
    margin: 0;
  }
  .snote :global(ul),
  .snote :global(ol) {
    margin: 0;
    padding-left: var(--space-4);
  }
</style>
