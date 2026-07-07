<script>
  import { copyLog } from '$lib/ui/copyLog.js';
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
  import { dndzone } from 'svelte-dnd-action';
  import { flip } from 'svelte/animate';
  import { t } from '$lib/i18n';

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

  <header class="head">
    <div>
      <h1>{activePage?.name ?? $t('dashboard.title')}</h1>
      {#if activePage?.description}<p class="subtitle">{activePage.description}</p>{/if}
    </div>
    {#if activePage}
      <div class="actions">
        {#if editing && !isBuiltin}
          <button class="ghost" onclick={openEditPage}><Icon name="settings" size={14} /> {$t('dashboard.editPage')}</button>
          <button class="ghost" onclick={addModuleRow}><Icon name="plus" size={14} /> {$t('dashboard.addRow')}</button>
          <button class="ghost" onclick={addWidgetRow}><Icon name="plus" size={14} /> {$t('dashboard.addWidgetRow')}</button>
          <button class="ghost" onclick={addSpacerRow}><Icon name="plus" size={14} /> {$t('dashboard.addSpacer')}</button>
        {/if}
        <button
          class="btn star"
          class:on={isDefault}
          title={isDefault ? $t('dashboard.defaultPage') : $t('dashboard.setDefaultPage')}
          aria-label={$t('dashboard.toggleDefaultPage')}
          onclick={toggleDefault}
          disabled={saving}
        >
          <Icon name="star" size={15} />
        </button>
        {#if !isBuiltin}
          <button class="btn" class:primary={editing} onclick={toggleEdit} disabled={saving}>
            {editing ? $t('dashboard.done') : $t('dashboard.edit')}
          </button>
        {/if}
      </div>
    {/if}
  </header>

  {#if loadError}<p class="err" title={$t('dashboard.clickToCopy')} use:copyLog={loadError}>{loadError}</p>{/if}

  {#if !doc}
    <p class="muted">…</p>
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
              style:--row-h={rowH > 0 ? `${rowH}px` : null}
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
        <div class="empty">
          <h2>{isBuiltin ? $t('dashboard.empty.title') : $t('dashboard.emptyPage.title')}</h2>
          <p>
            {#if isBuiltin}
              {@html $t('dashboard.emptyPage.installHint')}
            {:else}
              {@html $t('dashboard.emptyPage.addHint')}
            {/if}
          </p>
        </div>
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

  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    margin-bottom: var(--space-8);
  }
  h1 {
    font-size: 1.5rem;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .subtitle {
    color: var(--muted);
    font-size: 0.9rem;
    margin-top: var(--space-2);
  }
  .actions {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  .err {
    color: var(--red);
    font-size: 0.85rem;
    margin-bottom: var(--space-4);
  }
  .muted {
    color: var(--muted);
  }

  .empty h2 {
    font-size: 1rem;
    font-weight: 600;
    color: var(--text);
    margin-bottom: var(--space-2);
  }
  .empty p {
    font-size: 0.85rem;
  }

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
    font-size: 0.75rem;
    color: var(--muted);
  }
  .mini-field input {
    width: 52px;
    height: 24px;
    padding: 0 var(--space-2);
    font-size: 0.75rem;
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
  /* Row height is the chosen height of its tallest widget (--row-h). The tile/widget is
     fixed to exactly that height so content-heavy widgets scroll internally instead of
     stretching the row; module link tiles in a mixed row match it too. The edit-mode span
     tools sit below at natural height. When a row has no widgets, --row-h is 0px and the
     tile falls back to its own min-height (module tiles stay content-sized as before). */
  .cell > :global(.tile),
  .cell > :global(.widget) {
    height: var(--row-h);
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
    font-weight: 600;
  }
  .tile-desc {
    font-size: 0.8rem;
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
    font-size: 0.75rem;
    color: var(--muted);
    min-width: 40px;
    text-align: center;
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
    font-size: 0.75rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .drop-hint {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    font-size: 0.8rem;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    padding: var(--space-6);
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
    font-size: 0.85rem;
    font-family: inherit;
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    cursor: pointer;
  }
  .add-tile:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--surface);
  }
</style>
