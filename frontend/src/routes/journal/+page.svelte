<script>
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  // Trading Journal module shell. Left sidebar = sub-views (Breakdown, Trades,
  // Strategies, Templates) + a category selector that scopes Breakdown and Trades.
  // The right pane swaps in the active view. Categories are renamable folders; the
  // default category is protected from deletion.
  import { onMount } from 'svelte';
  import { journalApi, CURRENCIES } from '$lib/modules/journal/api.js';
  import Breakdown from '$lib/modules/journal/Breakdown.svelte';
  import Analytics from '$lib/modules/journal/Analytics.svelte';
  import TagsAdmin from '$lib/modules/journal/TagsAdmin.svelte';
  import CalendarView from '$lib/modules/journal/CalendarView.svelte';
  import Trades from '$lib/modules/journal/Trades.svelte';
  import Strategies from '$lib/modules/journal/Strategies.svelte';
  import Templates from '$lib/modules/journal/Templates.svelte';
  import FeesSchedules from '$lib/modules/journal/FeesSchedules.svelte';
  import PendingTasks from '$lib/modules/journal/PendingTasks.svelte';
  import ImportView from '$lib/modules/journal/ImportView.svelte';
  import ExportModal from '$lib/modules/journal/ExportModal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import QuickReminderButton from '$lib/modules/remindme/QuickReminderButton.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import { t } from '$lib/i18n';

  // breakdown | analytics | calendar | trades | strategies | tags | templates | fees | import | pending
  let view = $state('breakdown');
  // Set when navigating from a calendar cell → pins the Trades list to that 'YYYY-MM-DD',
  // and doubles as the way back: the calendar reopens on that day's month.
  let tradesInitialDay = $state('');
  let calendarInitialDay = $state('');
  // '' = all categories (only meaningful for breakdown/trades).
  let categoryId = $state('');

  // ── UI preferences (persisted in localStorage, per-browser) ──
  // The active view + category scope survive a refresh, like the news feed.
  const PREFS_KEY = 'otw.journal.prefs.v1';
  const NAV_IDS = [
    'breakdown',
    'analytics',
    'calendar',
    'trades',
    'strategies',
    'tags',
    'templates',
    'fees',
    'import',
    'pending'
  ];
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
  function askPrompt({ title, fields }, onconfirm) {
    promptTitle = title;
    promptFields = fields;
    onPromptConfirm = onconfirm;
    promptOpen = true;
  }

  let categories = $state([]);
  let strategies = $state([]);
  let templates = $state([]);
  let feeSchedules = $state([]);
  let settings = $state({ display_currency: 'USD' });
  let tags = $state([]);
  let pendingCount = $state(0);
  // Autocomplete pools for the breakdown filter bar (tickers/signals).
  let suggestions = $state({ tickers: [], exchanges: [], signals: [] });

  // Bump to force child views to re-fetch after a trade/capital change.
  let dataVersion = $state(0);

  onMount(loadAll);

  async function loadAll() {
    const lastCat = loadPrefs(); // null | '' (all) | id
    [categories, strategies, templates, feeSchedules, settings, suggestions, tags] =
      await Promise.all([
        journalApi.listCategories(),
        journalApi.listStrategies(),
        journalApi.listTemplates(),
        journalApi.listFeeSchedules(),
        journalApi.getSettings(),
        journalApi.suggestions(),
        journalApi.listTags()
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
  async function reloadTags() {
    tags = await journalApi.listTags();
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

  // Name and colour are edited together, in the category's own modal — same as
  // everywhere else a coloured thing is edited.
  function editCategory(c) {
    askPrompt(
      {
        title: $t('journal.page.editCategory.title'),
        fields: [
          { key: 'name', label: $t('journal.page.renameCategory.label'), value: c.name, required: true },
          { key: 'color', label: $t('journal.page.categories.color'), type: 'color', value: c.color ?? '' }
        ]
      },
      async ({ name, color }) => {
        const trimmed = name.trim();
        if (!trimmed) return;
        if (trimmed === c.name && (color ?? '') === (c.color ?? '')) return;
        // '' clears the colour: the patch COALESCEs, so null would mean "keep".
        await journalApi.updateCategory(c.id, { name: trimmed, color: color || '' });
        await reloadCategories();
      }
    );
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
    { id: 'breakdown', labelKey: 'journal.page.nav.breakdown', icon: 'bar-chart' },
    { id: 'analytics', labelKey: 'journal.page.nav.analytics', icon: 'trending-up' },
    { id: 'calendar', labelKey: 'journal.page.nav.calendar', icon: 'calendar-days' },
    { id: 'trades', labelKey: 'journal.page.nav.trades', icon: 'receipt' },
    { id: 'strategies', labelKey: 'journal.page.nav.strategies', icon: 'target' },
    { id: 'tags', labelKey: 'journal.page.nav.tags', icon: 'tag' },
    { id: 'templates', labelKey: 'journal.page.nav.templates', icon: 'layers' },
    { id: 'fees', labelKey: 'journal.page.nav.fees', icon: 'coins' },
    { id: 'import', labelKey: 'journal.page.nav.import', icon: 'download' },
    { id: 'pending', labelKey: 'journal.page.nav.pending', icon: 'check-square' }
  ];

  const scoped = $derived(
    view === 'breakdown' || view === 'analytics' || view === 'calendar' || view === 'trades'
  );
  const currentCat = $derived(categories.find((c) => c.id === categoryId) ?? null);

  // Export dialog (trades CSV / weekly-monthly report).
  let exportOpen = $state(false);

  /** Switch the currency every figure is converted to; children re-fetch server-side. */
  async function setDisplayCurrency(v) {
    settings = await journalApi.updateSettings({ display_currency: v });
    dataVersion += 1;
  }
</script>


<div class="journal-module">
  <aside class="sidebar">
    <nav class="views">
      {#each nav as n}
        <button
          class="navitem"
          class:active={view === n.id}
          onclick={() => {
            // Direct nav clears the click-through state: Trades opens unpinned, and the
            // calendar reopens on the month the user last browsed.
            if (n.id === 'trades') tradesInitialDay = '';
            if (n.id === 'calendar') calendarInitialDay = '';
            view = n.id;
          }}
        >
          <span class="nicon"><Icon name={n.icon} size={16} strokeWidth={1.6} /></span>
          <span class="nlabel">{$t(n.labelKey)}</span>
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
              <span class="dot" style:background={c.color || 'var(--muted)'}></span>
              <button class="cat" class:active={categoryId === c.id} onclick={() => (categoryId = c.id)}>
                {c.name}
                {#if c.is_default}<span class="def">{$t('journal.page.categories.default')}</span>{/if}
              </button>
              <span class="cat-actions">
                <button class="mini" title={$t('journal.page.categories.rename')} onclick={() => editCategory(c)}><Icon name="pencil" size={14} /></button>
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
        <!-- The breakdown, the calendar and the trades list are all money on screen, so
             the currency they are shown in belongs here rather than three views away in
             Fees & currency. Trades keep the currency they were entered in; this only
             changes what they are converted to. -->
        {#if scoped}
          <div class="cur-select">
            <span>{$t('journal.page.displayCurrency')}</span>
            <Dropdown
              value={settings.display_currency}
              onpick={setDisplayCurrency}
              ariaLabel={$t('journal.page.displayCurrency')}
              options={CURRENCIES.map((c) => ({ value: c.id, label: c.id }))}
            />
          </div>
        {/if}
        <!-- Both exports (trades CSV, periodic report) read the trade list, so the
             button only belongs on the views that show trades. -->
        {#if scoped}
          <button class="btn" onclick={() => (exportOpen = true)}>
            <Icon name="download" size={15} />
            {$t('journal.export.button')}
          </button>
        {/if}
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
            {tags}
            {suggestions}
            displayCurrency={settings.display_currency}
            oncategoryChanged={reloadCategories}
          />
        {/key}
      {:else if view === 'analytics'}
        {#key dataVersion}
          <Analytics
            {categoryId}
            {strategies}
            {tags}
            {suggestions}
            displayCurrency={settings.display_currency}
          />
        {/key}
      {:else if view === 'calendar'}
        {#key dataVersion}
          <CalendarView
            {categoryId}
            displayCurrency={settings.display_currency}
            initialDay={calendarInitialDay}
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
            {tags}
            initialDay={tradesInitialDay}
            onback={tradesInitialDay
              ? () => {
                  calendarInitialDay = tradesInitialDay;
                  tradesInitialDay = '';
                  view = 'calendar';
                  dataVersion += 1; // remount the calendar on that month
                }
              : null}
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
      {:else if view === 'tags'}
        <TagsAdmin {tags} ontagsChanged={reloadTags} />
      {:else if view === 'templates'}
        <Templates {templates} {feeSchedules} onchanged={reloadTemplates} />
      {:else if view === 'fees'}
        <FeesSchedules schedules={feeSchedules} {settings} onchanged={reloadFeesSettings} />
      {:else if view === 'import'}
        <ImportView
          {categoryId}
          {categories}
          {templates}
          onchanged={async () => {
            dataVersion += 1; // imported trades must show up in Trades/Breakdown
            suggestions = await journalApi.suggestions();
          }}
        />
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
  /* The nav is fixed furniture: it never scrolls away, only the category list below
     it does (see .cat-list). */
  .views {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex-shrink: 0;
  }
  .navitem {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    min-height: 30px;
    background: transparent;
    border: none;
    border-left: var(--active-rule) solid transparent;
    color: var(--muted);
    text-align: left;
    padding: 6px 10px;
    border-radius: 0;
    cursor: pointer;
    font-size: var(--fs-body);
  }
  .navitem:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .navitem.active {
    background: var(--surface-2);
    border-left-color: var(--accent);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  /* Fixed icon box so every label starts on the same optical column. */
  .nicon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    color: var(--faint);
  }
  .navitem:hover .nicon {
    color: var(--muted);
  }
  .navitem.active .nicon {
    color: var(--accent);
  }
  .nlabel {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Counter pill: reads as a value, not as a label — mono, outlined, never filled. */
  .badge {
    margin-left: auto;
    flex-shrink: 0;
    min-width: 18px;
    padding: 1px 5px;
    border: var(--hairline) solid var(--border-control);
    color: var(--dim);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.06em;
    text-align: center;
    font-family: var(--mono);
  }
  /* Takes the sidebar's leftover height so its list can scroll inside it. */
  .cat-section {
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-3);
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .cat-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
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
  /* The create form sits above the scroll area, not inside it. */
  .cat-add {
    flex-shrink: 0;
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
  /* Only the categories scroll — a long list can't push the nav off the panel.
     The scrollbar is thin and recessive (theme), so keep a little right padding:
     it must not sit on top of the row actions. */
  .cat-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    /* Floor rather than 0: on a short viewport the list keeps a few rows and the
       sidebar takes over the scrolling instead of collapsing it away. */
    min-height: 120px;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding-right: 2px;
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
  /* Identity marker, not a control: the colour is set in the category's edit modal. */
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
    display: inline-block;
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
  .cur-select {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .cur-select :global(.dd) {
    text-transform: none;
    letter-spacing: normal;
    min-width: 84px;
  }
  .content-body {
    padding: var(--space-6) var(--space-6) var(--space-8);
  }
</style>
