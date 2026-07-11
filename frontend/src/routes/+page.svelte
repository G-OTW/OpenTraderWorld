<script>
  import { moduleById } from '$lib/modules/registry';
  import { installedIds } from '$lib/modules/installed.js';
  import { dashboardApi } from '$lib/modules/dashboard/api.js';
  import {
    COLS,
    rid,
    makePage,
    makeWidgetRow,
    layoutForModules,
    normalizeDoc,
    pagesForDisplay,
    itemMinHeight,
    MODULES_PAGE_ID
  } from '$lib/modules/dashboard/layout.js';
  import PageModal from '$lib/modules/dashboard/PageModal.svelte';
  import WidgetShell from '$lib/modules/dashboard/widgets/WidgetShell.svelte';
  import WidgetPicker from '$lib/modules/dashboard/widgets/WidgetPicker.svelte';
  import WidgetConfig from '$lib/modules/dashboard/widgets/WidgetConfig.svelte';
  import { widgetByType } from '$lib/modules/dashboard/widgets/registry.js';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { dndzone } from 'svelte-dnd-action';
  import { flip } from 'svelte/animate';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  const installedList = $derived(
    ($installedIds ? [...$installedIds] : []) // ids only; order from registry not needed here
  );

  let doc = $state(null); // stored: { activePageId, defaultPageId, pages: [user pages] }
  let editing = $state(false);
  let loadError = $state('');
  let saving = $state(false);

  // Modal state.
  let modalOpen = $state(false);
  let modalPage = $state(null); // page being edited, or null for create

  // Widget add/config state.
  let pickerOpen = $state(false);
  let pickerRowId = $state(null); // widgets row the picker adds into
  let cfgOpen = $state(false);
  let cfgItem = $state(null); // widget item being configured

  // Full display list: built-in Modules page + user pages, default sorted first.
  const display = $derived(doc ? pagesForDisplay(doc, installedList) : { pages: [], defaultId: null });
  const activePage = $derived(display.pages.find((p) => p.id === doc?.activePageId) ?? null);
  const layout = $derived(activePage?.layout ?? null);
  const isBuiltin = $derived(activePage?.id === MODULES_PAGE_ID);
  const isDefault = $derived(activePage != null && activePage.id === display.defaultId);

  // Load + normalize once the installed set is known. Keyed so it runs only on a real
  // membership change, and never reads `doc` (which it writes) → no effect loop. On first
  // load we open on the effective default page.
  const installedKey = $derived(installedList.slice().sort().join(','));
  let loaded = false;
  let lastKey = null;
  $effect(() => {
    const key = installedKey;
    if (!$installedIds || key === lastKey) return;
    lastKey = key;
    if (loaded) return;
    loaded = true;
    dashboardApi
      .getLayout()
      .then((saved) => {
        doc = normalizeDoc(saved);
        openDefault();
      })
      .catch((e) => {
        loadError = e.message;
        doc = normalizeDoc(null);
        openDefault();
      });
  });

  // Point activePageId at the effective default (user default, else Modules page).
  function openDefault() {
    const { defaultId } = pagesForDisplay(doc, installedList);
    doc.activePageId = defaultId;
  }

  async function save() {
    if (!doc) return;
    saving = true;
    try {
      await dashboardApi.saveLayout(doc);
    } catch (e) {
      loadError = e.message;
    } finally {
      saving = false;
    }
  }

  function toggleEdit() {
    if (editing) {
      editing = false;
      save();
    } else {
      editing = true;
    }
  }

  function selectPage(id) {
    if (editing) return; // don't switch mid-edit
    doc.activePageId = id;
  }

  // ── Page CRUD (via modal) ────────────────────────────────────────────────
  const placedIds = $derived(
    layout
      ? layout.rows.flatMap((r) => (r.kind === 'modules' ? r.items.map((i) => i.moduleId) : []))
      : []
  );

  function openNewPage() {
    modalPage = null;
    modalOpen = true;
  }
  function openEditPage() {
    if (isBuiltin) return; // built-in Modules page isn't editable
    // Edit the stored user-page object (not the display copy), so changes persist.
    modalPage = doc.pages.find((p) => p.id === activePage.id) ?? null;
    modalOpen = true;
  }

  function onPageSave({ name, description, tag, moduleIds }) {
    if (modalPage) {
      modalPage.name = name;
      modalPage.description = description;
      modalPage.tag = tag || name;
      syncPageModules(modalPage, moduleIds);
    } else {
      const page = makePage({ name, description, tag, moduleIds });
      doc.pages = [...doc.pages, page];
      doc.activePageId = page.id;
    }
    save();
  }

  // Star toggle: make the current page the default (or, if it already is, clear the
  // default back to the built-in Modules page). Only one page can be default.
  function toggleDefault() {
    if (!activePage) return;
    if (activePage.id === display.defaultId) {
      doc.defaultPageId = null; // fall back to Modules page
    } else if (activePage.id === MODULES_PAGE_ID) {
      doc.defaultPageId = null; // Modules page is the default-when-unset
    } else {
      doc.defaultPageId = activePage.id;
    }
    save();
  }

  // Reconcile a page's tiles against a chosen module set: drop removed, append added.
  function syncPageModules(page, moduleIds) {
    const want = new Set(moduleIds);
    for (const row of page.layout.rows) {
      if (row.kind === 'modules') row.items = row.items.filter((i) => want.has(i.moduleId));
    }
    const have = new Set(
      page.layout.rows.flatMap((r) => (r.kind === 'modules' ? r.items.map((i) => i.moduleId) : []))
    );
    const added = moduleIds.filter((id) => !have.has(id));
    if (added.length) page.layout.rows = [...page.layout.rows, ...layoutForModules(added).rows];
  }

  function deleteActivePage() {
    if (isBuiltin) return; // can't delete the built-in Modules page
    const id = activePage.id;
    if (doc.defaultPageId === id) doc.defaultPageId = null;
    doc.pages = doc.pages.filter((p) => p.id !== id);
    openDefault(); // fall back to the default (Modules if no custom default)
    save();
  }

  // ── Row / tile operations (edit mode, on the active page) ─────────────────
  function addModuleRow() {
    layout.rows = [...layout.rows, { id: rid(), kind: 'modules', items: [] }];
  }
  function addSpacerRow() {
    layout.rows = [...layout.rows, { id: rid(), kind: 'spacer', height: 1 }];
  }
  function addWidgetRow() {
    layout.rows = [...layout.rows, { id: rid(), kind: 'widgets', items: [] }];
  }

  // ── Widget operations (edit mode) ─────────────────────────────────────────
  function openPicker(rowId) {
    pickerRowId = rowId;
    pickerOpen = true;
  }
  function onPickWidget(type) {
    const row = layout.rows.find((r) => r.id === pickerRowId);
    if (!row) return;
    const def = widgetByType(type);
    row.items = [
      ...row.items,
      { id: rid(), type, span: def?.defaultSpan ?? COLS / 3, config: {} }
    ];
  }
  function onPickModule(moduleId) {
    const row = layout.rows.find((r) => r.id === pickerRowId);
    if (!row) return;
    row.items = [...row.items, { id: rid(), moduleId, span: COLS / 3 }];
  }
  function openWidgetConfig(item) {
    cfgItem = item;
    cfgOpen = true;
  }
  function onWidgetConfigSave(config) {
    if (cfgItem) cfgItem.config = config;
    save();
  }
  function removeWidget(rowId, itemId) {
    const row = layout.rows.find((r) => r.id === rowId);
    if (row) row.items = row.items.filter((i) => i.id !== itemId);
  }
  function removeRow(rowId) {
    layout.rows = layout.rows.filter((r) => r.id !== rowId);
  }
  function moveRow(idx, dir) {
    const j = idx + dir;
    if (j < 0 || j >= layout.rows.length) return;
    const rows = [...layout.rows];
    [rows[idx], rows[j]] = [rows[j], rows[idx]];
    layout.rows = rows;
  }
  function setSpan(rowId, itemId, span) {
    const row = layout.rows.find((r) => r.id === rowId);
    const it = row?.items.find((i) => i.id === itemId);
    if (it) it.span = Math.max(1, Math.min(COLS, span));
  }
  function setSpacerHeight(rowId, h) {
    const row = layout.rows.find((r) => r.id === rowId);
    if (row) row.height = Math.max(1, Math.min(6, h));
  }
  function handleDnd(rowId, e) {
    const row = layout.rows.find((r) => r.id === rowId);
    if (row) row.items = e.detail.items;
  }

  const flipMs = 150;
</script>

<section class="page">
  <!-- Page tag chips: switch between dashboard pages; add a new one. -->
  {#if doc}
    <nav class="chips">
      {#each display.pages as p (p.id)}
        <button
          class="chip"
          class:active={p.id === doc.activePageId}
          disabled={editing}
          onclick={() => selectPage(p.id)}
        >
          {#if p.id === display.defaultId}<span class="chip-star"><Icon name="star" size={11} /></span>{/if}
          {p.tag}
        </button>
      {/each}
      <button class="chip add" onclick={openNewPage} title={$t('dashboard.addPageTitle')}><Icon name="plus" size={13} /></button>
    </nav>
  {/if}

  <PageHeader
    title={activePage?.name ?? $t('dashboard.title')}
    subtitle={activePage?.description ?? ''}
  >
    {#snippet actions()}
      {#if activePage}
        {#if editing && !isBuiltin}
          <Button variant="ghost" size="sm" icon="settings" onclick={openEditPage}>
            {$t('dashboard.editPage')}
          </Button>
          <Button variant="ghost" size="sm" icon="plus" onclick={addModuleRow}>
            {$t('dashboard.addRow')}
          </Button>
          <Button variant="ghost" size="sm" icon="plus" onclick={addWidgetRow}>
            {$t('dashboard.addWidgetRow')}
          </Button>
          <Button variant="ghost" size="sm" icon="plus" onclick={addSpacerRow}>
            {$t('dashboard.addSpacer')}
          </Button>
        {/if}
        <!-- The star is a toggle, not an action: it reports state (amber = this page
             is your default), so it keeps its own treatment rather than borrowing a
             Button variant. aria-pressed is what makes that state audible. -->
        <button
          class="btn star"
          class:on={isDefault}
          title={isDefault ? $t('dashboard.defaultPage') : $t('dashboard.setDefaultPage')}
          aria-label={$t('dashboard.toggleDefaultPage')}
          aria-pressed={isDefault}
          onclick={toggleDefault}
          disabled={saving}
        >
          <Icon name="star" size={15} />
        </button>
        {#if !isBuiltin}
          <Button
            variant={editing ? 'primary' : 'secondary'}
            onclick={toggleEdit}
            loading={saving}
          >
            {editing ? $t('dashboard.done') : $t('dashboard.edit')}
          </Button>
        {/if}
      {/if}
    {/snippet}
  </PageHeader>

  <ErrorText error={loadError} copyable />

  {#if !doc}
    <!-- Hold the grid's shape while the layout loads: an ellipsis makes the page
         jump when the tiles land. -->
    <div class="grid" style:--cols={COLS}>
      {#each [0, 1, 2] as i (i)}
        <div class="cell" style:grid-column={`span ${COLS / 3}`}>
          <Skeleton height="120px" />
        </div>
      {/each}
    </div>
  {:else if layout}
    <div class="rows" class:editing>
      {#each layout.rows as row, idx (row.id)}
        <div class="row-wrap" animate:flip={{ duration: flipMs }}>
          {#if editing}
            <div class="row-tools">
              <button class="mini" title={$t('dashboard.moveUp')} onclick={() => moveRow(idx, -1)} disabled={idx === 0}><Icon name="arrow-up" size={12} /></button>
              <button class="mini" title={$t('dashboard.moveDown')} onclick={() => moveRow(idx, 1)} disabled={idx === layout.rows.length - 1}><Icon name="arrow-down" size={12} /></button>
              {#if row.kind === 'spacer'}
                <label class="mini-field">{$t('dashboard.height')}
                  <input type="number" min="1" max="6" value={row.height}
                    oninput={(e) => setSpacerHeight(row.id, +e.currentTarget.value)} />
                </label>
              {/if}
              <button class="mini danger" title={$t('dashboard.removeRow')} onclick={() => removeRow(row.id)}><Icon name="x" size={12} /></button>
            </div>
          {/if}

          {#if row.kind === 'spacer'}
            <div class="spacer" style:height={`calc(${row.height} * var(--space-6))`}>
              {#if editing}<span class="spacer-label">{$t('dashboard.spacer')}</span>{/if}
            </div>
          {:else}
            {@const rowH = Math.max(0, ...row.items.map(itemMinHeight))}
            <div
              class="grid"
              style:--cols={COLS}
              style:--tile-h={rowH > 0 ? `${rowH}px` : null}
              use:dndzone={{ items: row.items, flipDurationMs: flipMs, dragDisabled: !editing, type: 'tile' }}
              onconsider={(e) => handleDnd(row.id, e)}
              onfinalize={(e) => handleDnd(row.id, e)}
            >
              {#each row.items as item (item.id)}
                <div class="cell" style:grid-column={`span ${item.span}`} animate:flip={{ duration: flipMs }}>
                  {#if item.type}
                    <!-- widget item -->
                    <WidgetShell
                      {item}
                      {editing}
                      onconfig={() => openWidgetConfig(item)}
                      onremove={() => removeWidget(row.id, item.id)}
                    />
                  {:else}
                    {@const mod = moduleById(item.moduleId)}
                    {#if mod}
                      <svelte:element
                        this={editing ? 'div' : 'a'}
                        class="tile"
                        class:edit={editing}
                        href={editing ? undefined : mod.base}
                      >
                        <span class="tile-icon"><Icon name={mod.icon} size={26} strokeWidth={1.8} /></span>
                        <span class="tile-name">{mod.name}</span>
                        {#if mod.descKey}<span class="tile-desc">{$t(mod.descKey)}</span>{/if}
                      </svelte:element>
                    {/if}
                  {/if}
                  {#if editing && (item.type || moduleById(item.moduleId))}
                    <div class="span-tools">
                      <button class="mini" onclick={() => setSpan(row.id, item.id, item.span - 1)} disabled={item.span <= 1}>−</button>
                      <span class="span-val">{item.span}/{COLS}</span>
                      <button class="mini" onclick={() => setSpan(row.id, item.id, item.span + 1)} disabled={item.span >= COLS}>+</button>
                      {#if item.type}
                        <button class="mini" title={$t('dashboard.widgets.shell.configure')} onclick={() => openWidgetConfig(item)}><Icon name="settings" size={12} /></button>
                      {/if}
                    </div>
                  {/if}
                </div>
              {/each}
              {#if editing}
                <div class="cell add-cell" style:grid-column={`span ${COLS / 3}`}>
                  <button class="add-tile" onclick={() => openPicker(row.id)}>
                    <Icon name="plus" size={18} /> {$t('dashboard.addTile')}
                  </button>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}

      {#if !editing && layout.rows.every((r) => !(r.kind === 'modules' || r.kind === 'widgets') || r.items.length === 0)}
        <EmptyState
          icon="grid"
          title={isBuiltin ? $t('dashboard.empty.title') : $t('dashboard.emptyPage.title')}
        >
          {#snippet body()}
            <!-- The hint names a menu path in <strong>, so it stays markup. -->
            {#if isBuiltin}
              {@html $t('dashboard.emptyPage.installHint')}
            {:else}
              {@html $t('dashboard.emptyPage.addHint')}
            {/if}
          {/snippet}
        </EmptyState>
      {/if}
    </div>
  {/if}
</section>

<PageModal
  bind:open={modalOpen}
  page={modalPage}
  placedIds={modalPage ? placedIds : []}
  onsave={onPageSave}
  ondelete={modalPage ? deleteActivePage : null}
/>

<WidgetPicker bind:open={pickerOpen} onpickWidget={onPickWidget} onpickModule={onPickModule} />
<WidgetConfig bind:open={cfgOpen} item={cfgItem} onsave={onWidgetConfigSave} />

<style>
  .page {
    padding: var(--space-6);
    height: 100%;
    overflow: auto;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-6);
  }
  .chip.add {
    padding: 4px var(--space-3);
  }
  .chip-star {
    display: inline-flex;
    color: var(--amber);
  }
  .chip-star :global(svg) {
    fill: currentColor;
  }
  .btn.star {
    color: var(--muted);
    padding: 0 var(--space-3);
  }
  .btn.star.on {
    color: var(--amber);
    border-color: var(--amber);
  }
  .btn.star.on :global(svg) {
    fill: currentColor;
  }

  /* Header is PageHeader.svelte; the empty state is EmptyState.svelte. */

  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .row-wrap {
    position: relative;
  }
  .rows.editing .row-wrap {
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
  }
  .row-tools {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }
  .mini {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
  }
  .mini:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--accent);
  }
  .mini:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .mini.danger:hover {
    color: var(--red);
    border-color: var(--red);
  }
  .mini-field {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .mini-field input {
    width: 52px;
    height: 24px;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(var(--cols), 1fr);
    gap: var(--space-4);
    min-height: 60px;
  }
  .cell {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  /* Row height is the chosen height of its tallest widget (--tile-h). The tile/widget is
     fixed to exactly that height so content-heavy widgets scroll internally instead of
     stretching the row; module link tiles in a mixed row match it too. The edit-mode span
     tools sit below at natural height. When a row has no widgets, --tile-h is unset and
     the tile falls back to its own min-height (module tiles stay content-sized as before).

     Named --tile-h, not --row-h: the latter is the global table-row token, and setting it
     here would cascade into any Table rendered inside a widget, stretching its rows to the
     widget's height. */
  .cell > :global(.tile),
  .cell > :global(.widget) {
    height: var(--tile-h);
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    color: var(--text);
    text-decoration: none;
    height: 100%;
  }
  a.tile:hover {
    border-color: var(--accent);
  }
  .tile.edit {
    cursor: grab;
  }
  .tile-icon {
    display: inline-flex;
    color: var(--accent);
  }
  .tile-name {
    font-weight: var(--fw-semibold);
  }
  .tile-desc {
    font-size: var(--text-sm);
    color: var(--muted);
  }

  .span-tools {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .span-val {
    font-size: var(--text-xs);
    color: var(--muted);
    min-width: 40px;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .spacer {
    border-radius: var(--radius);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .rows.editing .spacer {
    background: var(--surface-2);
    border: 1px dashed var(--border);
  }
  .spacer-label {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .add-cell {
    display: flex;
  }
  .add-tile {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    min-height: 120px;
    color: var(--muted);
    font-size: var(--text-sm);
    font-family: inherit;
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    cursor: pointer;
    transition:
      color var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }
  .add-tile:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--surface);
  }
</style>
