<script>
  import { moduleById } from '$lib/modules/registry';
  import { installedIds } from '$lib/modules/installed.js';
  import { dashboardApi } from '$lib/modules/dashboard/api.js';
  import {
    COLS,
    rid,
    makePage,
    layoutForModules,
    normalizeDoc,
    ensureDashboardPresets,
    normalizeFavorites,
    MAX_FAVORITES,
    WIDGET_HEIGHTS,
    pagesForDisplay,
    pageTileHeight,
    rowsForItems,
    isFullWidth,
    makeSpacer,
    DASHBOARD_PRESETS,
    isDashboardPreset
  } from '$lib/modules/dashboard/layout.js';
  import PageModal from '$lib/modules/dashboard/PageModal.svelte';
  import WidgetShell from '$lib/modules/dashboard/widgets/WidgetShell.svelte';
  import WidgetPicker from '$lib/modules/dashboard/widgets/WidgetPicker.svelte';
  import WidgetConfig from '$lib/modules/dashboard/widgets/WidgetConfig.svelte';
  import { widgetDefaults, widgetPlacement } from '$lib/modules/dashboard/widgets/registry.js';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { dndzone } from 'svelte-dnd-action';
  import { flip } from 'svelte/animate';
  import { t, locale } from '$lib/i18n';
  import { tz } from '$lib/tz.svelte.js';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { toast } from '$lib/ui/toast.svelte.js';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';

  const installedList = $derived(
    ($installedIds ? [...$installedIds] : []) // ids only; order from registry not needed here
  );

  let doc = $state(null); // stored: { activePageId, favoriteIds, pages: [user pages] }
  let editing = $state(false);
  // Plain snapshot of the last saved state. Svelte's reactive proxies cannot be cloned
  // directly, so take a non-reactive snapshot before copying it for the edit transaction.
  let editSnapshot = null;
  let loadError = $state('');
  let saving = $state(false);

  // Modal state.
  let modalOpen = $state(false);
  let modalPage = $state(null); // page being edited, or null for create

  // Widget add/config state.
  let pickerOpen = $state(false);
  let pickerWidgetsOnly = $state(false); // header add offers widgets only
  let cfgOpen = $state(false);
  let cfgItem = $state(null); // widget item being configured

  // Presets stay in a stable order, followed by user-created dashboards.
  const display = $derived(
    doc ? pagesForDisplay(doc) : { pages: [], defaultId: null }
  );
  const activePage = $derived(display.pages.find((p) => p.id === doc?.activePageId) ?? null);
  const layout = $derived(activePage?.layout ?? null);
  const isPreset = $derived(isDashboardPreset(activePage?.id));
  // The fixed-height treatment belongs to a shipped compact composition, not to every
  // future customization of a preset. A changed height or extra row falls back to the
  // ordinary scrolling dashboard automatically.
  const isPresetShowcase = $derived(
    isPreset &&
      activePage?.presetVersion === 1 &&
      layout?.items?.length > 0 &&
      layout.items.every((item) => item.type && item.config?.height === 'compact') &&
      rowsForItems(layout.items).length === 3
  );
  // The favourites bar: the pages behind `favoriteIds`, in the stored order.
  const favoritePages = $derived(
    (doc?.favoriteIds ?? [])
      .map((id) => display.pages.find((p) => p.id === id))
      .filter(Boolean)
  );
  const isFavorite = $derived(activePage != null && (doc?.favoriteIds ?? []).includes(activePage.id));

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
        const seeded = ensureDashboardPresets(doc);
        const { changed, overflow } = normalizeFavorites(doc);
        if (overflow.length) {
          toast.warn($t('dashboard.favorites.trimmed', { count: overflow.length, max: MAX_FAVORITES }));
        }
        if (seeded || changed) save();
        openDefault();
      })
      .catch((e) => {
        loadError = e.message;
        doc = normalizeDoc(null);
        ensureDashboardPresets(doc);
        normalizeFavorites(doc);
        openDefault();
      });
  });

  // Point activePageId at the effective default (Investor on a fresh workspace).
  function openDefault() {
    const { defaultId } = pagesForDisplay(doc);
    doc.activePageId = defaultId;
  }

  async function save() {
    // Everything changed while the layout editor is open belongs to the same transaction.
    // Individual modals can still apply their changes to the preview, but only Done writes
    // the resulting document to the server.
    if (!doc || editing) return false;
    saving = true;
    try {
      await dashboardApi.saveLayout(doc);
      return true;
    } catch (e) {
      loadError = e.message;
      return false;
    } finally {
      saving = false;
    }
  }

  function beginEdit() {
    if (!doc) return;
    editSnapshot = structuredClone($state.snapshot(doc));
    editing = true;
  }

  async function finishEdit() {
    if (saving) return;
    // Leave edit mode while the request is in flight so no further layout mutation can
    // slip in after the payload has been serialized. A failure reopens the transaction.
    editing = false;
    if (await save()) {
      editSnapshot = null;
    } else {
      editing = true;
    }
  }

  function cancelEdit() {
    if (editSnapshot) doc = structuredClone(editSnapshot);
    editSnapshot = null;
    editing = false;
    modalOpen = false;
    pickerOpen = false;
    cfgOpen = false;
    modalPage = null;
    cfgItem = null;
  }

  // The `?dashboard=` parameter is what the rail reads to light its entry, so selecting a
  // dashboard here writes it too. Without that the parameter kept naming the page the user
  // arrived on and the effect below put it straight back, which read as a dead nav bar.
  function selectPage(id) {
    if (editing) return; // don't switch mid-edit
    doc.activePageId = id;
    const slug = isDashboardPreset(id) ? id.replace(/^__preset_|__$/g, '') : id;
    goto(`/?dashboard=${encodeURIComponent(slug)}`, {
      replaceState: true,
      keepFocus: true,
      noScroll: true
    });
  }

  // Workspace stamp in the header cluster: the user's timezone, not the browser's.
  let now = $state(new Date());
  $effect(() => {
    const id = setInterval(() => (now = new Date()), 30_000);
    return () => clearInterval(id);
  });
  const stampDate = $derived(
    new Intl.DateTimeFormat($locale, {
      timeZone: tz.value,
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    }).format(now)
  );
  const stampTime = $derived(
    new Intl.DateTimeFormat($locale, {
      timeZone: tz.value,
      hour: '2-digit',
      minute: '2-digit',
      hourCycle: 'h23',
      timeZoneName: 'short'
    }).format(now)
  );
  const calendarModule = $derived(
    installedList.includes('calendar') ? moduleById('calendar') : null
  );

  // ── Page CRUD (via modal) ────────────────────────────────────────────────
  const placedIds = $derived(
    layout ? layout.items.filter((i) => i.moduleId).map((i) => i.moduleId) : []
  );

  function createCustomDashboard() {
    const page = makePage({
      name: 'Custom dashboard',
      description: 'A workspace you build from scratch.',
      tag: 'Custom'
    });
    doc.pages = [...doc.pages, page];
    doc.activePageId = page.id;
    save();
    beginEdit();
  }

  // The shell's "Custom +" shortcut lands on / with this query flag. Wait until the
  // persisted layout is ready, then create the blank workspace and clear the one-shot URL.
  let handledCustomShortcut = false;
  $effect(() => {
    const requested = $page.url.searchParams.has('new-dashboard');
    if (!requested) {
      handledCustomShortcut = false;
      return;
    }
    if (!doc || handledCustomShortcut) return;
    handledCustomShortcut = true;
    createCustomDashboard();
    goto('/', { replaceState: true, keepFocus: true });
  });

  // Rail links keep the dashboard workspace in sync even when the user arrives from
  // another module. Presets travel as a short slug, user pages as their id. Applied once
  // per value: re-applying it on every state change would overrule the favourites bar.
  let appliedDashboardParam = null;
  $effect(() => {
    const requested = $page.url.searchParams.get('dashboard');
    if (!requested) {
      appliedDashboardParam = null;
      return;
    }
    if (!doc || requested === appliedDashboardParam) return;
    appliedDashboardParam = requested;
    const preset = DASHBOARD_PRESETS.find((item) => item.id === `__preset_${requested}__`);
    const target = preset ?? doc.pages.find((p) => p.id === requested);
    if (target) doc.activePageId = target.id;
  });
  function openEditPage() {
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

  // Star toggle: put the current page in the favourites bar, or take it out. The bar
  // holds MAX_FAVORITES and never empties: once full, a new star asks which one to replace.
  let swapOpen = $state(false);

  function toggleFavorite() {
    if (!activePage) return;
    const ids = doc.favoriteIds ?? [];
    if (ids.includes(activePage.id)) {
      if (ids.length === 1) {
        toast.warn($t('dashboard.favorites.keepOne'));
        return;
      }
      doc.favoriteIds = ids.filter((id) => id !== activePage.id);
      save();
      return;
    }
    if (ids.length >= MAX_FAVORITES) {
      swapOpen = true; // full: the user picks what goes
      return;
    }
    doc.favoriteIds = [...ids, activePage.id];
    save();
  }

  // Replace one favourite with the active page, from the "bar is full" prompt.
  function swapFavorite(id) {
    doc.favoriteIds = (doc.favoriteIds ?? []).map((fav) => (fav === id ? activePage.id : fav));
    swapOpen = false;
    save();
  }

  // Reconcile a page's tiles against a chosen module set: drop removed, append added.
  function syncPageModules(page, moduleIds) {
    const want = new Set(moduleIds);
    const kept = page.layout.items.filter((i) => !i.moduleId || want.has(i.moduleId));
    const have = new Set(kept.filter((i) => i.moduleId).map((i) => i.moduleId));
    const added = moduleIds.filter((id) => !have.has(id));
    page.layout.items = [...kept, ...layoutForModules(added).items];
  }

  function deleteActivePage() {
    if (isPreset) return; // intent presets are always available
    const id = activePage.id;
    doc.pages = doc.pages.filter((p) => p.id !== id);
    doc.favoriteIds = (doc.favoriteIds ?? []).filter((fav) => fav !== id);
    normalizeFavorites(doc);
    openDefault(); // fall back to the selected default (Investor when unset)
    save();
  }

  // ── Tile operations (edit mode, on the active page) ───────────────────────
  // A page is one ordered list over the 12 columns: a tile dropped at an index pushes the
  // following tiles right, and onto the next line when the current one runs out of room.
  // Nothing else has to be moved by hand, so there are no rows to manage.
  function addSpacer() {
    layout.items = [...layout.items, makeSpacer()];
  }

  // ── Widget operations ─────────────────────────────────────────────────────
  function openPicker() {
    pickerWidgetsOnly = false;
    pickerOpen = true;
  }

  // Header "Add widget": place one without entering edit mode. It lands at the end of the
  // page, where the user can then drag it wherever they want.
  function openHeaderPicker() {
    if (!layout) return;
    pickerWidgetsOnly = true;
    pickerOpen = true;
  }

  // One pick from the picker → one tile. A new widget starts from its documented data
  // presentation; existing saved widgets remain untouched and resolve their legacy
  // configuration in the registry.
  function tileFor(pick) {
    if (pick.moduleId) return { id: rid(), moduleId: pick.moduleId, span: COLS / 3 };
    const config = widgetDefaults(pick.type, pick.variant);
    const span = widgetPlacement(pick.type, pick.variant).span ?? COLS / 3;
    return { id: rid(), type: pick.type, span: config?.height === 'title' ? COLS : span, config };
  }

  function onAddTiles(picks) {
    layout.items = [...layout.items, ...picks.map(tileFor)];
    if (!editing) save(); // outside edit mode nothing else will persist it
  }
  function openWidgetConfig(item) {
    cfgItem = item;
    cfgOpen = true;
  }
  function onWidgetConfigSave(config) {
    if (cfgItem) {
      // A section title is a band across the page: it takes the whole line, and gives the
      // width back when the widget stops being one.
      const wasTitle = cfgItem.config?.height === 'title';
      cfgItem.config = config;
      if (config?.height === 'title') cfgItem.span = COLS;
      else if (wasTitle) cfgItem.span = COLS / 3;
    }
    save();
  }
  function removeItem(itemId) {
    layout.items = layout.items.filter((i) => i.id !== itemId);
    if (!editing) save();
  }
  function setSpan(item, span) {
    item.span = Math.max(1, Math.min(COLS, span));
  }
  function setSpacerHeight(item, h) {
    item.height = Math.max(1, Math.min(6, h));
  }
  function setRowHeight(row, height) {
    const ids = new Set(
      row.items.filter((item) => item.kind !== 'spacer').map((item) => item.id)
    );
    layout.items = layout.items.map((item) =>
      ids.has(item.id)
        ? item.type
          ? { ...item, config: { ...(item.config ?? {}), height } }
          : { ...item, rowHeight: height }
        : item
    );
  }
  function handleDnd(e) {
    layout.items = e.detail.items;
  }

  // Rows remain implicit in storage and are recomputed after every drag or width change.
  // Each row adopts its tallest widget, keeping all of its ordinary tiles aligned.
  const itemRows = $derived.by(() => {
    const byId = new Map();
    for (const items of rowsForItems(layout?.items ?? [])) {
      const titleRow = items.length === 1 && items[0]?.config?.height === 'title';
      const height = titleRow
        ? WIDGET_HEIGHTS.title
        : pageTileHeight(items) || WIDGET_HEIGHTS.compact;
      const preset = titleRow
        ? 'title'
        : height <= WIDGET_HEIGHTS.compact
          ? 'compact'
          : height >= WIDGET_HEIGHTS.tall
            ? 'tall'
            : 'standard';
      const controlId = items.find(
        (item) =>
          item.kind !== 'spacer' &&
          (item.type || (moduleById(item.moduleId) && installedList.includes(item.moduleId)))
      )?.id ?? null;
      const row = { items, height, preset, controlId };
      for (const item of items) byId.set(item.id, row);
    }
    return byId;
  });

  const rowHeightOptions = $derived([
    { value: 'compact', label: $t('dashboard.widgets.config.compact') },
    { value: 'standard', label: $t('dashboard.widgets.config.standard') },
    { value: 'tall', label: $t('dashboard.widgets.config.tall') }
  ]);
  const titleRowHeightOptions = $derived([
    { value: 'title', label: $t('dashboard.widgets.config.titleHeight') },
    ...rowHeightOptions
  ]);

  const flipMs = 150;
</script>

<div
  class="page"
  class:preset-showcase={isPresetShowcase && !editing}
>
  <header class="dash-head">
    <!-- The title block is the favourites bar: one segmented control over the one to
         five dashboards the user keeps at hand. -->
    <nav class="fav-bar" aria-label={$t('dashboard.favorites.label')}>
      {#each favoritePages as p (p.id)}
        <button
          class="fav"
          class:active={p.id === doc.activePageId}
          disabled={editing || saving}
          aria-current={p.id === doc.activePageId ? 'page' : undefined}
          onclick={() => selectPage(p.id)}
        >
          {p.tag}
        </button>
      {/each}
    </nav>

    {#if activePage}
      <div class="dashboard-actions">
        <!-- The star is a stateful preference, separated from the primary workflow. -->
        <button
          class="btn pill square star"
          class:on={isFavorite}
          title={isFavorite ? $t('dashboard.favorites.remove') : $t('dashboard.favorites.add')}
          aria-label={isFavorite ? $t('dashboard.favorites.remove') : $t('dashboard.favorites.add')}
          aria-pressed={isFavorite}
          onclick={toggleFavorite}
          disabled={saving}
        >
          <Icon name="star" size={15} />
        </button>
        {#if editing}
          <Button variant="ghost" icon="x" onclick={cancelEdit} disabled={saving}>
            {$t('common.cancel')}
          </Button>
        {/if}
        <Button
          variant={editing ? 'solid' : 'pill'}
          icon={editing ? 'check' : 'settings'}
          onclick={editing ? finishEdit : beginEdit}
          loading={saving}
        >
          {editing ? $t('dashboard.done') : $t('dashboard.customize')}
        </Button>
        <!-- Adding a widget is the one layout change worth doing without entering
             edit mode, so it stays promoted next to Customize. -->
        <Button variant="solid" icon="plus" onclick={openHeaderPicker} disabled={saving}>
          {$t('dashboard.addWidget')}
        </Button>
        <div class="stamp" aria-hidden="true">
          <span class="stamp-date">{stampDate}</span>
          <span class="stamp-time">{stampTime}</span>
        </div>
        {#if calendarModule}
          <a
            class="btn pill square cal"
            href={calendarModule.base}
            title={$t('dashboard.openCalendar')}
            aria-label={$t('dashboard.openCalendar')}
          >
            <Icon name="calendar" size={15} />
          </a>
        {/if}
      </div>
    {/if}
  </header>

  <div class="workspace-content">

  {#if editing}
    <div class="edit-toolbar" role="toolbar" aria-label={$t('dashboard.edit')}>
      <div class="edit-context">
        <span class="edit-context-icon"><Icon name="settings" size={15} /></span>
        <span>{$t('dashboard.edit')}</span>
      </div>
      <div class="edit-tools">
        <Button variant="ghost" size="sm" icon="settings" onclick={openEditPage}>
          {$t('dashboard.editPage')}
        </Button>
        <Button variant="ghost" size="sm" icon="plus" onclick={addSpacer}>
          {$t('dashboard.addSpacer')}
        </Button>
      </div>
    </div>
  {/if}

  <ErrorText error={loadError} copyable />

  {#if !doc}
    <div class="grid" style:--cols={COLS}>
      {#each [0, 1, 2] as i (i)}
        <div class="cell" style:grid-column={`span ${COLS / 3}`}>
          <Skeleton height="120px" />
        </div>
      {/each}
    </div>
  {:else if layout}
    <!-- Free grid: one dnd zone over the whole page. The list order is the layout, so a
         tile dropped anywhere pushes the rest right and, when the line is full, down. -->
    <div
      class="grid free"
      class:editing
      style:--cols={COLS}
      use:dndzone={{ items: layout.items, flipDurationMs: flipMs, dragDisabled: !editing, type: 'tile' }}
      onconsider={handleDnd}
      onfinalize={handleDnd}
    >
      {#each layout.items as item (item.id)}
        {@const full = isFullWidth(item)}
        {@const row = itemRows.get(item.id)}
        <div
          class="cell"
          class:full
          style:grid-column={full ? '1 / -1' : `span ${item.span}`}
          style:--tile-h={row?.height > 0 ? `${row.height}px` : null}
          animate:flip={{ duration: flipMs }}
        >
          {#if item.kind === 'spacer'}
            <div class="spacer" style:height={`calc(${item.height} * var(--space-6))`}>
              {#if editing}<span class="spacer-label">{$t('dashboard.spacer')}</span>{/if}
            </div>
          {:else if item.type}
            <WidgetShell
              {item}
              {editing}
              onconfig={() => openWidgetConfig(item)}
              onremove={() => removeItem(item.id)}
            />
          {:else}
            {@const mod = moduleById(item.moduleId)}
            {#if mod && installedList.includes(mod.id)}
              <svelte:element
                this={editing ? 'div' : 'a'}
                class="tile"
                class:edit={editing}
                href={editing ? undefined : mod.base}
              >
                <span class="tile-icon"><Icon name={mod.icon} size={17} strokeWidth={1.8} /></span>
                <span class="tile-name">{mod.name}</span>
                {#if mod.descKey}<span class="tile-desc">{$t(mod.descKey)}</span>{/if}
              </svelte:element>
            {/if}
          {/if}

          {#if editing && item.kind === 'spacer'}
            <div class="span-tools">
              <label class="mini-field">{$t('dashboard.height')}
                <input type="number" min="1" max="6" value={item.height}
                  oninput={(e) => setSpacerHeight(item, +e.currentTarget.value)} />
              </label>
              <button class="mini danger" title={$t('dashboard.widgets.shell.remove')} onclick={() => removeItem(item.id)}><Icon name="x" size={12} /></button>
            </div>
          {:else if editing && (item.type || (moduleById(item.moduleId) && installedList.includes(item.moduleId)))}
            <div class="span-tools">
              <button class="mini" onclick={() => setSpan(item, item.span - 1)} disabled={full || item.span <= 1}>−</button>
              <span class="span-val">{full ? COLS : item.span}/{COLS}</span>
              <button class="mini" onclick={() => setSpan(item, item.span + 1)} disabled={full || item.span >= COLS}>+</button>
              {#if item.type}
                <button class="mini" title={$t('dashboard.widgets.shell.configure')} onclick={() => openWidgetConfig(item)}><Icon name="settings" size={12} /></button>
              {:else}
                <button class="mini danger" title={$t('dashboard.widgets.shell.remove')} onclick={() => removeItem(item.id)}><Icon name="x" size={12} /></button>
              {/if}
            </div>
            {#if row?.controlId === item.id}
              <div class="row-height-picker">
                <Dropdown
                  value={row.preset}
                  options={row.preset === 'title' ? titleRowHeightOptions : rowHeightOptions}
                  onpick={(height) => setRowHeight(row, height)}
                  ariaLabel={$t('dashboard.rowHeight')}
                  title={$t('dashboard.rowHeight')}
                  vertical
                  float
                />
              </div>
            {/if}
          {/if}
        </div>
      {/each}
    </div>

    {#if editing}
      <!-- Outside the dnd zone: it owns its children one for one with the item list. -->
      <div class="grid add-row" style:--cols={COLS}>
        <div class="cell add-cell" style:grid-column={`span ${COLS / 3}`}>
          <button class="add-tile" onclick={openPicker}>
            <Icon name="plus" size={18} /> {$t('dashboard.addTile')}
          </button>
        </div>
      </div>
    {/if}

    {#if !editing && !layout.items.some((i) => i.kind !== 'spacer')}
      <EmptyState
        icon="grid"
        title={$t('dashboard.emptyPage.title')}
      >
        {#snippet body()}
          <!-- The hint names a menu path in <strong>, so it stays markup. -->
          {@html $t('dashboard.emptyPage.addHint')}
        {/snippet}
      </EmptyState>
    {/if}
  {/if}
  </div>
</div>

<PageModal
  bind:open={modalOpen}
  page={modalPage}
  placedIds={modalPage ? placedIds : []}
  onsave={onPageSave}
  ondelete={modalPage && !isPreset ? deleteActivePage : null}
/>

<!-- Fifth favourite: the bar is full, so one of the four has to go. -->
<Modal bind:open={swapOpen} size="sm" title={$t('dashboard.favorites.fullTitle')}>
  <p class="swap-msg">
    {$t('dashboard.favorites.fullBody', { max: MAX_FAVORITES, name: activePage?.name ?? '' })}
  </p>
  <div class="swap-list">
    {#each favoritePages as p (p.id)}
      <button class="swap-item" onclick={() => swapFavorite(p.id)}>
        <span class="swap-name">{p.tag}</span>
        <Icon name="x" size={13} />
      </button>
    {/each}
  </div>
</Modal>

<WidgetPicker
  bind:open={pickerOpen}
  widgetsOnly={pickerWidgetsOnly}
  title={pickerWidgetsOnly ? $t('dashboard.addWidget') : ''}
  onadd={onAddTiles}
/>
<WidgetConfig bind:open={cfgOpen} item={cfgItem} onsave={onWidgetConfigSave} />

<style>
  .page {
    padding: 24px 24px 44px;
    height: 100%;
    overflow: auto;
  }
  /* Header row: favourites bar left, action cluster right. */
  .dash-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
    margin-bottom: 18px;
  }

  /* Segmented control: one tinted track, the active tab a raised surface pill.
     One frame for the whole group, none per tab. */
  .fav-bar {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border-radius: var(--pill-radius);
    background: var(--pill-bg);
  }
  .fav {
    height: calc(var(--pill-h) - 8px);
    padding: 0 18px;
    border: none;
    border-radius: calc(var(--pill-radius) - 4px);
    background: transparent;
    color: var(--muted);
    font-family: inherit;
    font-size: var(--pill-fs);
    font-weight: var(--fw-medium);
    line-height: 1;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }
  .fav:hover:not(:disabled):not(.active) {
    color: var(--pill-fg);
  }
  .fav.active {
    background: var(--surface);
    color: var(--pill-fg);
    box-shadow: var(--shadow-1);
  }
  .fav:disabled {
    cursor: not-allowed;
  }
  .fav:focus-visible {
    outline: none;
    box-shadow: var(--ring);
  }

  .swap-msg {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  .swap-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .swap-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    height: var(--pill-h);
    padding: 0 var(--pill-pad-x);
    border: none;
    border-radius: var(--pill-radius);
    background: var(--pill-bg);
    color: var(--pill-fg);
    font-family: inherit;
    font-size: var(--pill-fs);
    font-weight: var(--fw-medium);
    cursor: pointer;
  }
  .swap-item:hover {
    background: var(--pill-bg-hover);
  }
  .workspace-content {
    min-width: 0;
  }
  .dashboard-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  /* Cancel keeps the quiet ghost treatment but shares the header action height. */
  .dashboard-actions :global(.btn.ghost) {
    height: var(--pill-h);
  }
  .dashboard-actions .star.on {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--pill-bg));
  }
  .dashboard-actions .star.on :global(svg) {
    fill: currentColor;
  }
  /* The clock reads as a caption between two controls, not as a third button. */
  .stamp {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    margin: 0 var(--space-1) 0 var(--space-3);
    color: var(--dim);
    font-size: var(--pill-fs);
    line-height: 1.2;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .edit-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-height: 46px;
    margin: 0 0 var(--space-4);
    padding: 6px 8px 6px 12px;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface) 82%, var(--surface-2));
  }
  .edit-context,
  .edit-tools {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .edit-context {
    color: var(--muted);
    font-size: var(--fs-body);
    font-weight: var(--fw-medium);
  }
  .edit-context-icon {
    display: inline-flex;
    color: var(--accent);
  }
  .edit-tools {
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  /* The empty state is EmptyState.svelte. */

  .mini {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
    border: var(--hairline) solid var(--border-control);
    background: var(--surface);
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

  /* One free grid per page: tiles flow in list order over the 12 columns, wrapping to the
     next line when the current one runs out of room. No row containers, so a drop anywhere
     is just an index in the list. */
  .grid {
    display: grid;
    grid-template-columns: repeat(var(--cols), 1fr);
    gap: 14px;
    grid-auto-rows: min-content;
    align-items: start;
  }
  .grid.free.editing {
    min-height: 120px;
    padding-left: 38px;
  }
  .add-row {
    margin-top: 14px;
  }
  .cell {
    min-width: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }
  /* One height for every ordinary tile in a row (--tile-h): content-heavy widgets scroll
     internally instead of stretching their line, and module link tiles match them. The
     edit-mode span tools sit below at natural height. In rows without a widget --tile-h is
     unset and module tiles fall back to their own min-height.

     Named --tile-h, not --row-h: the latter is the global table-row token, and setting it
     here would cascade into any Table rendered inside a widget, stretching its rows to the
     widget's height. */
  .cell > :global(.tile),
  .cell > :global(.widget) {
    height: var(--tile-h);
  }
  /* Full-width bands (section titles, spacers) own their height. */
  .cell.full > :global(.widget) {
    height: auto;
  }

  .tile {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    min-height: 108px;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
    padding: 15px 16px;
    color: var(--text);
    text-decoration: none;
    box-shadow: var(--shadow-1);
    transition: background-color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
  }
  a.tile:hover {
    background: var(--surface-2);
    border-color: var(--border-strong);
  }
  .tile.edit {
    cursor: grab;
    border: var(--hairline) solid var(--border-strong);
  }
  .tile-icon {
    display: inline-flex;
    color: var(--accent);
  }
  .tile-name {
    margin: 13px 0 4px;
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  .tile-desc {
    font-size: var(--fs-desc);
    color: var(--dim);
  }

  .span-tools {
    display: flex;
    align-items: center;
    justify-content: flex-start;
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
  .row-height-picker {
    position: absolute;
    top: 0;
    /* Follow the rendered card, including widget-enforced minimum heights. The 32px
       reserved at the bottom is the 24px edit toolbar plus its 8px top gap. */
    bottom: calc(24px + var(--space-2));
    left: -38px;
    width: 28px;
    --fs-body: var(--text-xs);
  }

  .spacer {
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .grid.editing .spacer {
    background: var(--surface-2);
    border: 0.5px dashed var(--border);
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
    border: var(--hairline) dashed var(--border-strong);
    border-radius: var(--radius);
    cursor: pointer;
    transition:
      color var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }
  .add-tile:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--surface-2);
  }

  /* Shipped presets are one-screen overviews on desktop. Their compact cards form exactly
     three rows; the grid shares the available height between them and widget bodies keep
     their own internal overflow. Edit mode returns to the ordinary document flow so
     layout controls are never clipped. */
  @media (min-width: 1100px) and (min-height: 700px) {
    .page.preset-showcase {
      display: flex;
      flex-direction: column;
      overflow: hidden;
      padding-bottom: 24px;
    }
    .preset-showcase .dash-head {
      flex: 0 0 auto;
    }
    .preset-showcase .workspace-content {
      display: flex;
      flex: 1;
      flex-direction: column;
      min-height: 0;
    }
    .preset-showcase .grid.free {
      flex: 1;
      grid-auto-rows: minmax(0, 1fr);
      align-items: stretch;
      min-height: 0;
    }
    .preset-showcase .cell {
      min-height: 0;
    }
    .preset-showcase .cell > :global(.widget) {
      height: 100%;
      min-height: 0;
    }
  }

  @media (max-width: 720px) {
    .page {
      padding: var(--space-4);
    }
    .edit-toolbar {
      align-items: flex-start;
      flex-direction: column;
      padding: var(--space-3);
    }
    .edit-tools {
      justify-content: flex-start;
    }
  }
</style>
