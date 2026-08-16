<script>
  // Templates — the authoring half of the Mindset module.
  //
  // Two panes: categories on the left with counts, their check-in templates on the right —
  // the same shape as the Routines library, so the two modules are learned once. A card
  // states its phase and prompt mix, and unfolds into the prompts themselves.
  import { onMount } from 'svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import CategoryManager from '$lib/ui/CategoryManager.svelte';
  import TemplateEditor from '$lib/modules/mindset/TemplateEditor.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { mindsetApi, NAV_LINKS, PHASES, KINDS } from '$lib/modules/mindset/nav.js';
  import { t } from '$lib/i18n';

  let rows = $state([]); // [{ template, prompts }]
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
  let expanded = $state(new Set());

  onMount(load);

  async function load() {
    loading = true;
    error = '';
    try {
      const r = await mindsetApi.listTemplates();
      rows = r.templates;
      categories = r.categories;
      if (selected !== 'all' && selected !== 'none' && !categories.some((c) => c.id === selected)) {
        selected = 'all';
      }
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  // Search spans the template and its prompts, so a phrase finds the check-in that asks it.
  const searched = $derived.by(() => {
    const q = search.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter(({ template, prompts }) =>
      [template.name, template.description, ...prompts.map((p) => p.label)]
        .filter(Boolean)
        .join(' ')
        .toLowerCase()
        .includes(q)
    );
  });

  const countFor = (id) =>
    searched.filter(({ template }) =>
      id === 'none' ? !template.category_id : template.category_id === id
    ).length;

  const looseCount = $derived(countFor('none'));

  const sideItems = $derived.by(() => {
    const out = [
      { id: 'all', name: $t('mindset.templates.allCategories'), color: '', count: searched.length }
    ];
    for (const c of categories) {
      out.push({ id: c.id, name: c.name, color: c.color, count: countFor(c.id) });
    }
    if (looseCount > 0) {
      out.push({
        id: 'none',
        name: $t('mindset.templates.uncategorised'),
        color: '',
        count: looseCount
      });
    }
    return out;
  });

  const catById = $derived(new Map(categories.map((c) => [c.id, c])));

  const panes = $derived.by(() => {
    if (selected === 'all') {
      const out = [];
      for (const c of categories) {
        const list = searched.filter(({ template }) => template.category_id === c.id);
        if (list.length > 0) out.push({ category: c, rows: list });
      }
      const loose = searched.filter(({ template }) => !template.category_id);
      if (loose.length > 0) out.push({ category: null, rows: loose });
      return out;
    }
    const list = searched.filter(({ template }) =>
      selected === 'none' ? !template.category_id : template.category_id === selected
    );
    return [{ category: catById.get(selected) ?? null, rows: list, flat: true }];
  });

  const visibleCount = $derived(panes.reduce((n, p) => n + p.rows.length, 0));
  const selectedName = $derived(
    selected === 'all'
      ? $t('mindset.templates.allCategories')
      : selected === 'none'
        ? $t('mindset.templates.uncategorised')
        : (catById.get(selected)?.name ?? '')
  );

  function openNew(categoryId = null) {
    editingId = null;
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
    const next = new Set(expanded);
    next.has(id) ? next.delete(id) : next.add(id);
    expanded = next;
  }

  async function run(fn) {
    try {
      await fn();
      await load();
    } catch (e) {
      error = e.message;
    }
  }

  const toggleActive = (t) => run(() => mindsetApi.updateTemplate(t.id, { active: !t.active }));
  const duplicate = (t) => run(() => mindsetApi.duplicateTemplate(t.id, `${t.name} (copy)`));

  // Deleting a template goes through ConfirmModal: past answers survive, but the prompts
  // that framed them don't, so old entries stop rendering.
  let confirmOpen = $state(false);
  let pending = $state(null);

  function ask(template) {
    pending = template;
    confirmOpen = true;
  }

  async function onConfirm() {
    const tpl = pending;
    pending = null;
    if (tpl) await run(() => mindsetApi.deleteTemplate(tpl.id));
  }

  /// "3 scales · 2 tags · 2 texts" — the shape of a check-in at a glance.
  function promptMix(prompts) {
    return KINDS.map((k) => ({ k, n: prompts.filter((p) => p.kind === k.key).length }))
      .filter(({ n }) => n > 0)
      .map(({ k, n }) => `${n} ${$t(`mindset.kind.${k.key}`).toLowerCase()}`)
      .join(' · ');
  }

  const phaseOf = (key) => PHASES.find((p) => p.key === key) ?? PHASES[0];
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="mindset.nav.label" />

  <PageHeader
    title={$t('mindset.templates.title')}
    subtitle={$t('mindset.templates.subtitle', { n: rows.length })}
  >
    {#snippet actions()}
      <button class="btn" onclick={() => (showCategories = true)}>
        <Icon name="tag" size={13} /> {$t('mindset.templates.manageCategories')}
      </button>
      <button class="btn primary" onclick={() => openNew()}>
        <Icon name="plus" size={13} /> {$t('mindset.templates.newTemplate')}
      </button>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} copyable />

  {#if loading}
    <Skeleton rows={5} height="3rem" gap="var(--space-3)" />
  {:else if rows.length === 0}
    <EmptyState
      icon="clipboard-list"
      title={$t('mindset.templates.emptyTitle')}
      description={$t('mindset.templates.emptyBody')}
    >
      {#snippet action()}
        <button class="btn primary" onclick={() => openNew()}>
          {$t('mindset.templates.newTemplate')}
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
          placeholder={$t('mindset.templates.searchPlaceholder')}
          bind:value={search}
        />

        <nav class="catlist" aria-label={$t('mindset.templates.manageCategories')}>
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
                class:all={c.id === 'all'}
                style:background={c.color || 'var(--muted)'}
                aria-hidden="true"
              ></span>
              <span class="cname">{c.name}</span>
              <span class="count">{c.count}</span>
            </button>
          {/each}
        </nav>

        <div class="sidefoot">
          <button class="link" onclick={() => (showCategories = true)}>
            <Icon name="settings" size={12} /> {$t('mindset.templates.manageCategories')}
          </button>
        </div>
      </aside>

      <!-- ── Right: templates ── -->
      <div class="main">
        <div class="mainhead">
          <h2>{selectedName}</h2>
          <span class="mcount">{$t('mindset.templates.subtitle', { n: visibleCount })}</span>
        </div>

        {#if visibleCount === 0}
          <p class="nores">
            {#if search}
              {$t('mindset.templates.noMatches', { q: search })}
            {:else}
              {$t('mindset.templates.categoryEmpty')}
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
                  {pane.category?.name ?? $t('mindset.templates.uncategorised')}
                  <span class="count">{pane.rows.length}</span>
                </h3>
              {/if}

              {#each pane.rows as { template, prompts } (template.id)}
                <article
                  class="card"
                  class:paused={!template.active}
                  style:--cat={pane.category?.color || 'var(--border)'}
                >
                  <div class="chead">
                    <button
                      class="title"
                      onclick={() => toggleExpand(template.id)}
                      aria-expanded={expanded.has(template.id)}
                      aria-controls="p-{template.id}"
                    >
                      <Icon
                        name={expanded.has(template.id) ? 'chevron-down' : 'chevron-right'}
                        size={13}
                      />
                      <span class="tname">{template.name}</span>
                      <span class="pcount">
                        {$t('mindset.templates.prompts', { n: prompts.length })}
                      </span>
                      {#if !template.active}
                        <span class="pill">{$t('mindset.templates.paused')}</span>
                      {/if}
                    </button>

                    <div class="cactions">
                      <button
                        class="icon"
                        title={template.active ? $t('mindset.templates.pause') : $t('mindset.templates.resume')}
                        aria-label={template.active ? $t('mindset.templates.pause') : $t('mindset.templates.resume')}
                        onclick={() => toggleActive(template)}
                      >
                        <Icon name={template.active ? 'eye' : 'eye-off'} size={14} />
                      </button>
                      <button class="icon" title={$t('mindset.templates.duplicate')} aria-label={$t('mindset.templates.duplicate')} onclick={() => duplicate(template)}>
                        <Icon name="copy" size={14} />
                      </button>
                      <button class="icon" title={$t('common.edit')} aria-label={$t('common.edit')} onclick={() => openEdit(template.id)}>
                        <Icon name="pencil" size={14} />
                      </button>
                      <button class="icon del" title={$t('common.delete')} aria-label={$t('common.delete')} onclick={() => ask(template)}>
                        <Icon name="trash" size={14} />
                      </button>
                    </div>
                  </div>

                  {#if template.description}
                    <p class="desc">{template.description}</p>
                  {/if}

                  <div class="meta">
                    <span class="phase">
                      {phaseOf(template.phase).icon}
                      {$t(`mindset.phase.${template.phase}`)}
                    </span>
                    {#if prompts.length > 0}<span>{promptMix(prompts)}</span>{/if}
                  </div>

                  {#if expanded.has(template.id)}
                    <div class="body" id="p-{template.id}">
                      {#if template.notes}
                        <!-- Sanitised server-side at write time (ammonia allowlist). -->
                        <div class="notes">{@html template.notes}</div>
                      {/if}
                      <ul class="plist">
                        {#each prompts as p, n (p.id)}
                          <li>
                            <span class="pn">{n + 1}</span>
                            <div class="pbody">
                              <span class="ptext">{p.label}</span>
                              <span class="pkind">{$t(`mindset.kind.${p.kind}`)}</span>
                              {#if p.hint}<div class="phint">{p.hint}</div>{/if}
                              {#if p.config?.options?.length}
                                <div class="popts">{p.config.options.join(' · ')}</div>
                              {:else if p.kind === 'scale' && (p.config?.low || p.config?.high)}
                                <div class="popts">{p.config.low} → {p.config.high}</div>
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
  templateId={editingId}
  {categories}
  {presetCategoryId}
  onsaved={load}
/>

<CategoryManager
  bind:open={showCategories}
  {categories}
  api={mindsetApi}
  onsaved={load}
/>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('mindset.builder.deleteTemplate')}
  message={$t('mindset.builder.confirmDelete')}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={onConfirm}
  oncancel={() => (pending = null)}
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

  .layout {
    display: grid;
    grid-template-columns: 216px minmax(0, 1fr);
    gap: var(--space-6);
    align-items: start;
  }
  @media (max-width: 900px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-4);
    }
  }

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
  /* The plain search field the other modules use — one border, no wrapper. */
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
  .dot.all {
    background: transparent !important;
    border: 1.5px solid var(--muted);
  }
  /* Not `.badge`: that global class is a bordered uppercase status pill. */
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

  .sidefoot {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-1);
    padding-top: var(--space-2);
    border-top: 0.5px solid var(--border);
  }
  .link {
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
  .link:hover {
    color: var(--text);
  }

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

  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
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
  .pcount {
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
  .phase {
    color: var(--text);
  }

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

  .plist {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .plist li {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    font-size: var(--text-sm);
  }
  .plist li + li {
    border-top: 0.5px solid var(--border);
  }
  .pn {
    flex: none;
    width: 16px;
    text-align: right;
    color: var(--faint, var(--muted));
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    line-height: 1.6;
  }
  .pbody {
    min-width: 0;
    flex: 1;
  }
  .ptext {
    color: var(--text);
  }
  .pkind {
    margin-left: var(--space-2);
    font-size: var(--text-xs);
    color: var(--faint, var(--muted));
  }
  .phint,
  .popts {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-top: 2px;
  }
</style>
