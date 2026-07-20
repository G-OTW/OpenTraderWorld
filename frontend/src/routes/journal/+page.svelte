<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Trading Journal module shell. Left sidebar = sub-views (Breakdown, Trades,
  // Strategies, Templates) + a category selector that scopes Breakdown and Trades.
  // The right pane swaps in the active view. Categories are renamable folders; the
  // default category is protected from deletion.
  import { onMount } from 'svelte';
  import { journalApi } from '$lib/modules/journal/api.js';
  import Breakdown from '$lib/modules/journal/Breakdown.svelte';
  import CalendarView from '$lib/modules/journal/CalendarView.svelte';
  import Trades from '$lib/modules/journal/Trades.svelte';
  import Strategies from '$lib/modules/journal/Strategies.svelte';
  import Templates from '$lib/modules/journal/Templates.svelte';
  import FeesSchedules from '$lib/modules/journal/FeesSchedules.svelte';
  import PendingTasks from '$lib/modules/journal/PendingTasks.svelte';
  import ExportModal from '$lib/modules/journal/ExportModal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import { t } from '$lib/i18n';

  let view = $state('breakdown'); // breakdown | calendar | trades | strategies | templates | fees | pending
  // Set when navigating from a calendar cell → pins the Trades list to that 'YYYY-MM-DD'.
  let tradesInitialDay = $state('');
  // '' = all categories (only meaningful for breakdown/trades).
  let categoryId = $state('');

  // ── UI preferences (persisted in localStorage, per-browser) ──
  // The active view + category scope survive a refresh, like the news feed.
  const PREFS_KEY = 'otw.journal.prefs.v1';
  const NAV_IDS = ['breakdown', 'calendar', 'trades', 'strategies', 'templates', 'fees', 'pending'];
  let prefsLoaded = $state(false);

  function loadPrefs() {
    try {
      const p = JSON.parse(localStorage.getItem(PREFS_KEY) || '{}');
      if (NAV_IDS.includes(p.view)) view = p.view;
      if (typeof p.categoryId === 'string') return p.categoryId; // restored last scope
    } catch {
      /* corrupt prefs — ignore */
    }
    return null;
  }

  $effect(() => {
    const snapshot = { view, categoryId };
    if (!prefsLoaded) return;
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify(snapshot));
    } catch {
      /* quota / unavailable — non-fatal */
    }
  });

  // ── Modal-based confirm/prompt (replaces native confirm()/prompt()) ──
  let confirmOpen = $state(false);
  let confirmTitle = $state('');
  let confirmMessage = $state('');
  let confirmDanger = $state(false);
  let onConfirmYes = $state(() => {});
  function askConfirm({ title, message, danger = false }, onyes) {
    confirmTitle = title;
    confirmMessage = message;
    confirmDanger = danger;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  let promptOpen = $state(false);
  let promptTitle = $state('');
  let promptFields = $state([]);
  let onPromptConfirm = $state(() => {});
  function askPrompt({ title, label, value = '' }, onconfirm) {
    promptTitle = title;
    promptFields = [{ key: 'name', label, value, required: true }];
    onPromptConfirm = onconfirm;
    promptOpen = true;
  }

  let categories = $state([]);
  let strategies = $state([]);
  let templates = $state([]);
  let feeSchedules = $state([]);
  let settings = $state({ display_currency: 'USD' });
  let pendingCount = $state(0);
  // Autocomplete pools for the breakdown filter bar (tickers/signals).
  let suggestions = $state({ tickers: [], exchanges: [], signals: [] });

  // Bump to force child views to re-fetch after a trade/capital change.
  let dataVersion = $state(0);

  onMount(loadAll);

  async function loadAll() {
    const lastCat = loadPrefs(); // null | '' (all) | id
    [categories, strategies, templates, feeSchedules, settings, suggestions] = await Promise.all([
      journalApi.listCategories(),
      journalApi.listStrategies(),
      journalApi.listTemplates(),
      journalApi.listFeeSchedules(),
      journalApi.getSettings(),
      journalApi.suggestions()
    ]);
    // Restore the persisted scope: '' = all categories; an id if it still exists;
    // otherwise fall back to the default (or first) category.
    if (lastCat === '') {
      categoryId = '';
    } else if (lastCat && categories.some((c) => c.id === lastCat)) {
      categoryId = lastCat;
    } else {
      const def = categories.find((c) => c.is_default) ?? categories[0];
      categoryId = def?.id ?? '';
    }
    prefsLoaded = true; // enable persistence now that restore is complete
    refreshPendingCount();
  }

  async function refreshPendingCount() {
    try {
      pendingCount = (await journalApi.fxPending()).length;
    } catch {
      pendingCount = 0;
    }
  }

  async function reloadCategories() {
    categories = await journalApi.listCategories();
  }
  async function reloadStrategies() {
    strategies = await journalApi.listStrategies();
  }
  async function reloadTemplates() {
    templates = await journalApi.listTemplates();
  }
  async function reloadFeesSettings() {
    [feeSchedules, settings] = await Promise.all([
      journalApi.listFeeSchedules(),
      journalApi.getSettings()
    ]);
  }

  // ── Category management ──
  let addingCategory = $state(false);
  let newCatName = $state('');

  async function createCategory() {
    const name = newCatName.trim();
    if (!name) return;
    const cat = await journalApi.addCategory(name);
    newCatName = '';
    addingCategory = false;
    await reloadCategories();
    categoryId = cat.id;
  }

  function renameCategory(c) {
    askPrompt({ title: $t('journal.page.renameCategory.title'), label: $t('journal.page.renameCategory.label'), value: c.name }, async ({ name }) => {
      const trimmed = name.trim();
      if (!trimmed || trimmed === c.name) return;
      await journalApi.updateCategory(c.id, { name: trimmed });
      await reloadCategories();
    });
  }

  function deleteCategory(c) {
    if (c.is_default) return;
    askConfirm(
      {
        title: $t('journal.page.deleteCategory.title'),
        message: $t('journal.page.deleteCategory.message', { name: c.name }),
        danger: true
      },
      async () => {
        await journalApi.deleteCategory(c.id);
        await reloadCategories();
        if (categoryId === c.id) {
          const def = categories.find((x) => x.is_default) ?? categories[0];
          categoryId = def?.id ?? '';
        }
      }
    );
  }

  // ── Category color picking ──
  // Shared palette with the editor's select-option chips.
  const COLOR_SWATCHES = [
    { name: 'slate', hex: '#7d8a99' },
    { name: 'red', hex: '#c9776b' },
    { name: 'amber', hex: '#b79a6b' },
    { name: 'green', hex: '#7fb894' },
    { name: 'blue', hex: '#8494a7' },
    { name: 'violet', hex: '#9a95a3' },
    { name: 'pink', hex: '#b58a94' }
  ];
  let colorPickerFor = $state(null); // category id whose swatch popover is open

  async function setColor(c, hex) {
    colorPickerFor = null;
    // Optimistic local update so the dot recolors immediately.
    categories = categories.map((x) => (x.id === c.id ? { ...x, color: hex } : x));
    await journalApi.updateCategory(c.id, { color: hex });
  }

  // ── Category drag-reordering ──
  // HTML5 DnD: on drop, splice the dragged category to the target slot and persist
  // each affected category's new integer position.
  let dragId = $state(null);
  let dragOverId = $state(null);

  function onDragStart(e, c) {
    dragId = c.id;
    e.dataTransfer.effectAllowed = 'move';
  }
  function onDragOver(e, c) {
    e.preventDefault();
    dragOverId = c.id;
  }
  function onDragEnd() {
    dragId = null;
    dragOverId = null;
  }
  async function onDrop(e, target) {
    e.preventDefault();
    const fromId = dragId;
    dragId = null;
    dragOverId = null;
    if (!fromId || fromId === target.id) return;

    const order = [...categories];
    const from = order.findIndex((c) => c.id === fromId);
    const to = order.findIndex((c) => c.id === target.id);
    if (from < 0 || to < 0) return;
    const [moved] = order.splice(from, 1);
    order.splice(to, 0, moved);

    // Reassign sequential positions and persist them.
    categories = order.map((c, i) => ({ ...c, position: i }));
    await Promise.all(
      categories.map((c) => journalApi.updateCategory(c.id, { position: c.position }))
    );
  }

  const nav = [
    { id: 'breakdown', labelKey: 'journal.page.nav.breakdown', icon: '📊' },
    { id: 'calendar', labelKey: 'journal.page.nav.calendar', icon: '📅' },
    { id: 'trades', labelKey: 'journal.page.nav.trades', icon: '🧾' },
    { id: 'strategies', labelKey: 'journal.page.nav.strategies', icon: '🎯' },
    { id: 'templates', labelKey: 'journal.page.nav.templates', icon: '🧩' },
    { id: 'fees', labelKey: 'journal.page.nav.fees', icon: '💱' },
    { id: 'pending', labelKey: 'journal.page.nav.pending', icon: '📌' }
  ];

  const scoped = $derived(view === 'breakdown' || view === 'calendar' || view === 'trades');
  const currentCat = $derived(categories.find((c) => c.id === categoryId) ?? null);

  // Export dialog (trades CSV / weekly-monthly report).
  let exportOpen = $state(false);
</script>

<svelte:window onclick={() => (colorPickerFor = null)} />

<div class="journal-module">
  <aside class="sidebar">
    <nav class="views">
      {#each nav as n}
        <button
          class="navitem"
          class:active={view === n.id}
          onclick={() => {
            // Direct nav to Trades clears any day pinned by a calendar click-through.
            if (n.id === 'trades') tradesInitialDay = '';
            view = n.id;
          }}
        >
          <span class="nicon">{n.icon}</span>{$t(n.labelKey)}
          {#if n.id === 'pending' && pendingCount > 0}<span class="badge">{pendingCount}</span>{/if}
        </button>
      {/each}
    </nav>

    {#if scoped}
      <div class="cat-section">
        <div class="cat-head">
          <span>{$t('journal.page.categories.title')}</span>
          <button class="add" title={$t('journal.page.categories.new')} onclick={() => (addingCategory = !addingCategory)}
            ><Icon name="plus" size={16} /></button
          >
        </div>
        {#if addingCategory}
          <form
            class="cat-add"
            onsubmit={(e) => {
              e.preventDefault();
              createCategory();
            }}
          >
            <!-- svelte-ignore a11y_autofocus -->
            <input placeholder={$t('journal.page.categories.namePlaceholder')} bind:value={newCatName} autofocus />
            <button type="submit" class="cat-create" disabled={!newCatName.trim()}>{$t('journal.page.categories.create')}</button>
          </form>
        {/if}
        <ul class="cat-list">
          <li class="all-row">
            <button class="cat" class:active={categoryId === ''} onclick={() => (categoryId = '')}
              >{$t('journal.page.categories.all')}</button
            >
          </li>
          {#each categories as c (c.id)}
            <li
              class:dragging={dragId === c.id}
              class:dragover={dragOverId === c.id && dragId !== c.id}
              draggable="true"
              ondragstart={(e) => onDragStart(e, c)}
              ondragover={(e) => onDragOver(e, c)}
              ondragleave={() => (dragOverId = null)}
              ondrop={(e) => onDrop(e, c)}
              ondragend={onDragEnd}
            >
              <span class="handle" title={$t('journal.page.categories.dragToReorder')}>⠿</span>
              <span class="swatch-wrap">
                <button
                  class="dot"
                  style:background={c.color || 'var(--muted)'}
                  title={$t('journal.page.categories.setColor')}
                  aria-label={$t('journal.page.categories.setColor')}
                  onclick={(e) => {
                    e.stopPropagation();
                    colorPickerFor = colorPickerFor === c.id ? null : c.id;
                  }}
                ></button>
                {#if colorPickerFor === c.id}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <div class="swatches" onclick={(e) => e.stopPropagation()}>
                    {#each COLOR_SWATCHES as s}
                      <button
                        class="swatch"
                        class:selected={(c.color || '').toLowerCase() === s.hex.toLowerCase()}
                        style:background={s.hex}
                        title={s.name}
                        aria-label={s.name}
                        onclick={(e) => {
                          e.stopPropagation();
                          setColor(c, s.hex);
                        }}
                      ></button>
                    {/each}
                    <label class="custom" title={$t('journal.page.categories.customColor')}>
                      <input
                        type="color"
                        value={c.color || '#7d8a99'}
                        onchange={(e) => setColor(c, e.target.value)}
                      />
                      <span class="custom-ico">🎨</span>
                    </label>
                  </div>
                {/if}
              </span>
              <button class="cat" class:active={categoryId === c.id} onclick={() => (categoryId = c.id)}>
                {c.name}
                {#if c.is_default}<span class="def">{$t('journal.page.categories.default')}</span>{/if}
              </button>
              <span class="cat-actions">
                <button class="mini" title={$t('journal.page.categories.rename')} onclick={() => renameCategory(c)}><Icon name="pencil" size={14} /></button>
                {#if !c.is_default}
                  <button class="mini" title={$t('journal.fees.schedules.delete')} onclick={() => deleteCategory(c)}><Icon name="trash" size={14} /></button>
                {/if}
              </span>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </aside>

  <main class="content">
    <header class="content-head">
      <h1>
        {$t(nav.find((n) => n.id === view)?.labelKey)}
        {#if scoped}<span class="scope">· {currentCat ? currentCat.name : $t('journal.page.categories.all')}</span>{/if}
      </h1>
      <div class="head-actions">
        <button class="btn" onclick={() => (exportOpen = true)}>
          <Icon name="download" size={15} />
          {$t('journal.export.button')}
        </button>
        <QuickReminderButton title={$t('journal.page.addReminder')} />
      </div>
    </header>

    <div class="content-body">
      {#if view === 'breakdown'}
        {#key dataVersion}
          <Breakdown
            {categoryId}
            category={currentCat}
            {strategies}
            {suggestions}
            displayCurrency={settings.display_currency}
            oncategoryChanged={reloadCategories}
          />
        {/key}
      {:else if view === 'calendar'}
        {#key dataVersion}
          <CalendarView
            {categoryId}
            displayCurrency={settings.display_currency}
            onviewday={(day) => {
              tradesInitialDay = day;
              view = 'trades';
              dataVersion += 1; // remount Trades so it picks up the pinned day
            }}
          />
        {/key}
      {:else if view === 'trades'}
        {#key dataVersion}
          <Trades
            {categoryId}
            {categories}
            {strategies}
            {templates}
            {feeSchedules}
            {suggestions}
            initialDay={tradesInitialDay}
            onchanged={async () => {
              dataVersion += 1;
              suggestions = await journalApi.suggestions();
            }}
          />
        {/key}
      {:else if view === 'strategies'}
        <Strategies
          {categories}
          {strategies}
          oncategoriesChanged={reloadCategories}
          onstrategiesChanged={reloadStrategies}
        />
      {:else if view === 'templates'}
        <Templates {templates} {feeSchedules} onchanged={reloadTemplates} />
      {:else if view === 'fees'}
        <FeesSchedules schedules={feeSchedules} {settings} onchanged={reloadFeesSettings} />
      {:else if view === 'pending'}
        <PendingTasks onchanged={refreshPendingCount} />
      {/if}
    </div>
  </main>
</div>

<ExportModal
  bind:open={exportOpen}
  categoryId={categoryId}
  categoryName={currentCat?.name ?? ''}
/>

<ConfirmModal
  bind:open={confirmOpen}
  title={confirmTitle}
  message={confirmMessage}
  confirmLabel={$t('journal.fees.deleteModal.confirm')}
  danger={confirmDanger}
  onconfirm={onConfirmYes}
/>
<PromptModal
  bind:open={promptOpen}
  title={promptTitle}
  fields={promptFields}
  confirmLabel={$t('common.save')}
  onconfirm={onPromptConfirm}
/>

<style>
  .journal-module {
    display: grid;
    grid-template-columns: 260px 1fr;
    height: 100%;
    min-height: 0;
  }
  .sidebar {
    border-right: 0.5px solid var(--border);
    background: var(--surface);
    overflow-y: auto;
    min-height: 0;
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .views {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .navitem {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-left: var(--active-rule) solid transparent;
    color: var(--muted);
    text-align: left;
    padding: var(--pad-nav);
    border-radius: 0;
    cursor: pointer;
    font-size: var(--fs-body);
  }
  .navitem:hover {
    background: var(--surface-2);
  }
  .navitem.active {
    background: var(--surface-2);
    border-left-color: var(--accent);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .nicon {
    font-size: 14px;
  }
  .badge {
    margin-left: auto;
    color: var(--muted);
    font-size: var(--fs-section);
    letter-spacing: 0.06em;
    text-align: center;
    font-family: var(--mono);
  }
  .cat-section {
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-3);
  }
  .cat-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-family: var(--mono);
    font-size: var(--fs-section);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--dim);
    margin-bottom: var(--space-2);
  }
  .add {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-md);
  }
  .add:hover {
    color: var(--text);
  }
  .cat-add input {
    width: 100%;
    margin-bottom: var(--space-2);
  }
  .cat-create {
    width: 100%;
    background: transparent;
    color: var(--text);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    padding: 6px 8px;
    font: inherit;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    cursor: pointer;
    margin-bottom: var(--space-2);
  }
  .cat-create:hover {
    background: var(--surface-2);
  }
  .cat-create:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .cat-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .cat-list li {
    display: flex;
    align-items: center;
    gap: 2px;
    border-radius: 0;
    border-left: 1.5px solid transparent;
  }
  .cat-list li.dragging {
    opacity: 0.5;
  }
  .cat-list li.dragover {
    border-left-color: var(--accent);
    background: var(--surface-2);
  }
  .handle {
    cursor: grab;
    color: var(--border);
    font-size: var(--text-base);
    padding: 0 2px;
    flex-shrink: 0;
    user-select: none;
  }
  .cat-list li:hover .handle {
    color: var(--muted);
  }
  .swatch-wrap {
    position: relative;
    display: inline-flex;
    flex-shrink: 0;
  }
  .swatches {
    position: absolute;
    top: 16px;
    left: 0;
    z-index: 50;
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 4px;
    padding: 6px;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
  }
  .swatch {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 0.5px solid var(--border-control);
    cursor: pointer;
  }
  .swatch.selected {
    outline: 2px solid var(--text);
    outline-offset: 1px;
  }
  /* Custom color picker: a swatch-sized tile with a hidden native color input. */
  .custom {
    position: relative;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 0.5px dashed var(--muted);
    cursor: pointer;
    display: grid;
    place-items: center;
    overflow: hidden;
  }
  .custom input[type='color'] {
    width: 100%;
  }
  .custom-ico {
    font-size: 0.6rem;
    line-height: 1;
    pointer-events: none;
  }
  .cat {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: none;
    color: var(--muted);
    text-align: left;
    padding: 6px 8px;
    border-radius: 0;
    cursor: pointer;
    font-size: var(--fs-body);
  }
  .cat:hover {
    background: var(--surface-2);
  }
  .cat.active {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
    border: none;
    padding: 0;
    cursor: pointer;
  }
  .def {
    font-family: var(--mono);
    font-size: 9px;
    letter-spacing: 0.08em;
    color: var(--faint);
    text-transform: uppercase;
  }
  .cat-actions {
    display: flex;
    opacity: 0;
  }
  .cat-list li:hover .cat-actions {
    opacity: 1;
  }
  .mini {
    background: transparent;
    border: none;
    cursor: pointer;
    color: var(--muted);
    font-size: var(--text-xs);
    padding: 2px 4px;
  }
  .mini:hover {
    color: var(--text);
  }
  .content {
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .content-head {
    padding: var(--space-6) var(--space-6) 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .content-head h1 {
    font-size: var(--fs-page-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  .scope {
    color: var(--dim);
    font-weight: var(--fw-normal);
    font-size: var(--fs-body);
  }
  .head-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .content-body {
    padding: var(--space-6) var(--space-6) var(--space-8);
  }
</style>
