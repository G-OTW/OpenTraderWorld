/**
 * Dashboard model — five intent-based preset pages plus any number of user-defined pages,
 * each with its own free grid of tiles.
 *
 * Stored document (opaque in `app_settings.dashboard_layout`):
 *   { activePageId, favoriteIds, rail, pages: [ { id, name, description, tag, layout } ] }
 *
 * Preset pages are seeded once, remain in a stable order, and may be customized in place.
 *
 * - `favoriteIds` is the favourites bar, in display order: one to MAX_FAVORITES pages.
 *   Its first entry is the page the dashboard opens to.
 * - `rail` is the sidebar's own arrangement, `{ workspace: [pageId], tools: [moduleId] }`:
 *   what each rail section shows and in which order. Written by the shell, carried here
 *   because the dashboard list and the rail are the same list.
 * - `activePageId` is the page currently shown (session/last-viewed; we open on default).
 *
 * A page's `layout` is `{ cols, items }`: one ordered list of tiles flowing over a fixed
 * `cols` grid. The order is the only truth — a tile dropped at index i pushes every later
 * tile to the right, and onto the next line when the current one runs out of columns. No
 * rows are stored, so any tile can be dragged anywhere and the grid reflows around it.
 *  - module link: `{ id, moduleId, span }`
 *  - widget:      `{ id, type, span, config }` — `type` is a key in the widget registry,
 *                 `config` the widget's own opaque settings (feed id, portfolio id, …).
 *  - spacer:      `{ id, kind: 'spacer', span: COLS, height }` — a full-width gap.
 * Section titles are ordinary `text` widgets at the `title` height: full width like the
 * spacer, so they keep reading as a band across the page.
 *
 * Tiles are *links* — the same module may appear on any number of pages; only the link
 * tile is duplicated. A tile whose module is no longer installed renders nothing.
 */

export const COLS = 12;

/** How many dashboards the favourites bar holds. */
export const MAX_FAVORITES = 5;

/** Id of the retired generated module directory, still filtered out of saved documents. */
export const MODULES_PAGE_ID = '__modules__';

/**
 * The dashboard starts with workspaces for the ways people actually use OTW.  These are
 * ordinary saved pages after they are seeded, so users can rearrange their tiles and add
 * widgets without losing the intent of the original workspace.
 */
export const DASHBOARD_PRESETS = [
  {
    id: '__preset_investor__',
    name: 'Investor',
    description: 'Long-horizon holdings',
    // Bump only when the shipped Investor composition intentionally changes. Existing
    // workspaces below this version are replaced once, then remain user-customizable.
    version: 1,
    modules: ['wealth', 'portfolios', 'mportfolios', 'watchlists', 'news', 'economics', 'goals']
  },
  {
    id: '__preset_trader__',
    name: 'Trader',
    description: 'Execution and market session',
    version: 1,
    modules: ['journal', 'watchlists', 'histviz', 'backtest', 'routines', 'remindme', 'mindset']
  },
  {
    id: '__preset_quant__',
    name: 'Quant',
    description: 'Research, backtesting and algo work',
    version: 1,
    modules: ['backtest', 'quant', 'histdata', 'histviz', 'automator', 'agent', 'prompt-store', 'webhooks']
  },
  {
    id: '__preset_portfolio__',
    name: 'Portfolio',
    description: 'Holdings-focused workspace',
    version: 1,
    modules: ['portfolios', 'wealth', 'taxcalc', 'quant', 'subscriptions', 'goals']
  },
  {
    id: '__preset_utilities__',
    name: 'Utilities',
    description: 'Automation, notes and tools',
    version: 1,
    modules: [
      'automator',
      'agent',
      'editor',
      'todos',
      'calendar',
      'mailbox',
      'webhooks',
      'time',
      'prompt-store',
      'resources',
      'community-docs'
    ]
  }
];

const PRESET_IDS = new Set(DASHBOARD_PRESETS.map((preset) => preset.id));

export function isDashboardPreset(pageId) {
  return PRESET_IDS.has(pageId);
}

/** Widget height presets → pixel min-height of the cell. Standard is the default;
 *  `title` is a header band, the height of a text widget used as a section title. */
export const WIDGET_HEIGHTS = { title: 52, compact: 150, standard: 220, tall: 340 };

/** Min-height (px) an item asks for. Widgets carry the choice in their config; module
 *  links receive `rowHeight` when a user changes the height of their current row. */
export function itemMinHeight(item) {
  if (item?.type) return WIDGET_HEIGHTS[item.config?.height] ?? WIDGET_HEIGHTS.standard;
  return WIDGET_HEIGHTS[item?.rowHeight] ?? 0; // module link or spacer
}

/** Tiles that always take the whole line: spacers and section titles. They keep their own
 *  height and are excluded from ordinary row-height calculations. */
export function isFullWidth(item) {
  return item?.kind === 'spacer' || item?.config?.height === 'title';
}

/** Largest height requested by a set of ordinary tiles. Kept for callers that need a
 *  single height over an arbitrary item set; row rendering uses it one row at a time. */
export function pageTileHeight(items = []) {
  return Math.max(0, ...items.filter((i) => !isFullWidth(i)).map(itemMinHeight));
}

/** Split the ordered free-grid items into the rows produced by the 12-column flow.
 *  Rows are deliberately derived rather than stored: resizing or dragging a tile then
 *  moves it between rows without leaving stale row metadata behind. */
export function rowsForItems(items = []) {
  const rows = [];
  let row = [];
  let used = 0;

  const flush = () => {
    if (row.length) rows.push(row);
    row = [];
    used = 0;
  };

  for (const item of items) {
    const span = isFullWidth(item)
      ? COLS
      : Math.max(1, Math.min(COLS, Number(item?.span) || COLS / 3));
    if (row.length && used + span > COLS) flush();
    row.push(item);
    used += span;
    if (used >= COLS) flush();
  }
  flush();
  return rows;
}

let _seq = 0;
/** Short unique id for pages/items (dnd needs stable ids). */
export function rid() {
  _seq += 1;
  return `r${Date.now().toString(36)}${_seq}`;
}

/** A full-width gap tile. */
export function makeSpacer(height = 1) {
  return { id: rid(), kind: 'spacer', span: COLS, height };
}

/** A grid layout holding the given module ids, each at a third of the width. */
export function layoutForModules(moduleIds) {
  return {
    cols: COLS,
    items: moduleIds.map((moduleId) => ({ id: rid(), moduleId, span: COLS / 3 }))
  };
}

/** One data-backed card in a shipped dashboard composition. Presets use the same widget
 * contract as user-added cards, so every tile keeps its module link, empty state and
 * configuration flow. */
function presetWidget(type, variant, span, config = {}) {
  return {
    id: rid(),
    type,
    span,
    config: { ...config, variant, height: 'compact' }
  };
}

/** Investor is deliberately a dense, three-row overview: it shows the full long-horizon
 * workflow at a glance while every figure still comes from its owning module. */
function investorLayout() {
  return {
    cols: COLS,
    items: [
      // Balance sheet and the two most visual portfolio lenses.
      presetWidget('wealth', 'overview', 3, { display: 'overview', months: 12 }),
      presetWidget('portfolios', 'evolution', 6),
      presetWidget('portfolios', 'allocation', 3),

      // What is held, what is moving, and what long-horizon managers changed.
      presetWidget('portfolios', 'positions', 5, { limit: 3 }),
      presetWidget('watchlists', 'quotes', 4, { order: 'list', limit: 4 }),
      presetWidget('mportfolios', 'drift', 3, { limit: 3 }),

      // Context and progress, kept concise enough to complete the one-screen overview.
      presetWidget('news', 'latest', 5, { view: 'list', limit: 3 }),
      presetWidget('economics', 'calendar', 4, { importance: '0,1' }),
      presetWidget('goals', 'goal', 3, { scope: 'goal' })
    ]
  };
}

/** The live session from left to right: exposure, instruments, research, preparation and
 * discipline. Journal and watchlist appear more than once because their distinct views
 * answer different questions without inventing a combined data source. */
function traderLayout() {
  return {
    cols: COLS,
    items: [
      presetWidget('journal-stats', 'positions', 5, { limit: 4 }),
      presetWidget('journal-stats', 'exposure', 3),
      presetWidget('watchlists', 'quotes', 4, { order: 'list', limit: 4 }),

      presetWidget('histviz', 'workspaces', 3, { limit: 3 }),
      presetWidget('backtest', 'best', 3, { metric: 'return' }),
      presetWidget('backtest', 'favorites', 3, { limit: 3 }),
      presetWidget('routines', 'today', 3, { limit: 5 }),

      presetWidget('watchlists', 'alerts', 3, { limit: 3 }),
      presetWidget('remindme-stats', 'upcoming', 3, { limit: 3 }),
      presetWidget('mindset', 'today', 3),
      presetWidget('journal-stats', 'recent', 3, { limit: 3 })
    ]
  };
}

/** Research pipeline: test evidence, quantitative diagnostics, datasets and chart
 * workspaces lead into automation, the assistant and integration surfaces. */
function quantLayout() {
  return {
    cols: COLS,
    items: [
      presetWidget('backtest', 'best', 4, { metric: 'sharpe' }),
      presetWidget('backtest', 'leaderboard', 4, { limit: 4 }),
      presetWidget('quant', 'risk', 4),

      presetWidget('quant', 'seasonality', 5, { metric: 'return' }),
      presetWidget('histdata', 'datasets', 3),
      presetWidget('histviz', 'workspaces', 4, { limit: 4 }),

      presetWidget('automator', 'rates', 3, { window: 200, limit: 4 }),
      presetWidget('agent', 'quick-ask', 3),
      presetWidget('prompts', 'library', 3, { limit: 6 }),
      presetWidget('webhooks', 'feed', 3, { limit: 4 })
    ]
  };
}

/** Holdings cockpit: the first two rows explain the investment book and wider balance
 * sheet; the last row keeps tax, risk, recurring drag and funding goals in view. */
function portfolioLayout() {
  return {
    cols: COLS,
    items: [
      presetWidget('portfolios', 'evolution', 6),
      presetWidget('portfolios', 'cash', 3),
      presetWidget('portfolios', 'pnl', 3),

      presetWidget('portfolios', 'positions', 5, { limit: 4 }),
      presetWidget('portfolios', 'income', 3),
      presetWidget('wealth', 'top', 4, { months: 12, limit: 4 }),

      presetWidget('taxcalc', 'total', 3),
      presetWidget('quant', 'risk', 3),
      presetWidget('subscriptions', 'monthly', 3),
      presetWidget('goals', 'goal', 3, { scope: 'goal' })
    ]
  };
}

/** Utilities is the broadest profile, so each compact cell is one clear destination or
 * status. The 4 × 3 rhythm keeps eleven modules readable without a fourth page row. */
function utilitiesLayout() {
  return {
    cols: COLS,
    items: [
      presetWidget('automator', 'agenda', 3, { days: 7, limit: 3 }),
      presetWidget('automator', 'status', 3, { window: 200 }),
      presetWidget('agent', 'quick-ask', 3),
      presetWidget('calendar', 'week', 3, { days: 7, limit: 4 }),

      presetWidget('editor', 'recent', 3, { limit: 4 }),
      presetWidget('todos', 'upcoming', 3, { scope: 'upcoming', limit: 4 }),
      presetWidget('mailbox', 'unread', 3, { limit: 4 }),
      presetWidget('time', 'hours', 3, { period: 'week' }),

      presetWidget('webhooks', 'feed', 3, { limit: 4 }),
      presetWidget('prompts', 'library', 3, { limit: 6 }),
      presetWidget('resources', 'bookmarks', 3, { order: 'saved', limit: 4 }),
      presetWidget('community-docs', 'quick', 3, { limit: 4 })
    ]
  };
}

function layoutForPreset(preset) {
  switch (preset.id) {
    case '__preset_investor__': return investorLayout();
    case '__preset_trader__': return traderLayout();
    case '__preset_quant__': return quantLayout();
    case '__preset_portfolio__': return portfolioLayout();
    case '__preset_utilities__': return utilitiesLayout();
    default: return layoutForModules(preset.modules);
  }
}

/**
 * Migrate a row-based layout to the free grid: rows disappear, their items keep their
 * order and span, spacers become full-width tiles.
 */
function flattenRows(rows) {
  const items = [];
  for (const row of rows ?? []) {
    if (row.kind === 'spacer') {
      items.push({ id: row.id ?? rid(), kind: 'spacer', span: COLS, height: row.height ?? 1 });
      continue;
    }
    for (const item of row.items ?? []) {
      items.push({
        ...item,
        id: item.id ?? rid(),
        span: isFullWidth(item) ? COLS : Math.max(1, Math.min(COLS, item.span || COLS / 3))
      });
    }
  }
  return items;
}

/** Whatever a page carried (rows or items) as a free-grid layout. */
function normalizeLayout(layout) {
  if (Array.isArray(layout?.items)) {
    return {
      cols: COLS,
      items: layout.items.map((item) => ({
        ...item,
        id: item.id ?? rid(),
        span: isFullWidth(item) ? COLS : Math.max(1, Math.min(COLS, item.span || COLS / 3))
      }))
    };
  }
  return { cols: COLS, items: flattenRows(layout?.rows) };
}

/** A new user page with the given fields and an optional initial module set. */
export function makePage({ name, description = '', tag = '', moduleIds = [] }) {
  return {
    id: rid(),
    name: name?.trim() || 'Untitled',
    description: description?.trim() ?? '',
    tag: (tag?.trim() || (name?.trim() ?? 'Page')).slice(0, 24),
    layout: layoutForModules(moduleIds)
  };
}

function presetPage(preset) {
  return {
    id: preset.id,
    name: preset.name,
    description: preset.description,
    tag: preset.name,
    ...(preset.version ? { presetVersion: preset.version } : {}),
    layout: layoutForPreset(preset)
  };
}

/**
 * Add the five intent-based workspaces on first use. The old generated Modules page is
 * deliberately retired: module navigation now lives in the switcher, not in a permanent
 * dashboard directory. Existing user-created pages remain available after the presets.
 */
export function ensureDashboardPresets(doc) {
  const current = new Map(doc.pages.map((page) => [page.id, page]));
  const pages = DASHBOARD_PRESETS.map((preset) => {
    const saved = current.get(preset.id);
    // A versioned preset is refreshed once when its shipped composition changes. The
    // version is stored on the page, so Customize remains durable on every later load.
    if (!saved || (preset.version && (saved.presetVersion ?? 0) < preset.version)) {
      return presetPage(preset);
    }
    return saved;
  });
  const custom = doc.pages.filter(
    (page) => page.id !== MODULES_PAGE_ID && !isDashboardPreset(page.id)
  );
  const next = [...pages, ...custom];
  const changed =
    next.length !== doc.pages.length || next.some((page, index) => doc.pages[index] !== page);

  if (changed) doc.pages = next;
  return changed;
}

/**
 * Coerce whatever was saved into a valid stored document.
 *  - `null`/empty            → no user pages
 *  - legacy `{ cols, rows }` → migrate to one "Dashboard" user page
 *  - pages doc               → keep its pages, keep ids, flatten any stored rows
 * Never mutates the input. Presets are added afterward by `ensureDashboardPresets`.
 */
export function normalizeDoc(saved) {
  let doc;
  if (saved && Array.isArray(saved.rows)) {
    // Legacy single-layout format → one user page.
    doc = {
      activePageId: null,
      favoriteIds: [],
      rail: null,
      pages: [
        {
          id: rid(),
          name: 'Dashboard',
          description: 'Your activated modules, arranged your way.',
          tag: 'Dashboard',
          layout: normalizeLayout(saved)
        }
      ]
    };
  } else if (saved && Array.isArray(saved.pages)) {
    doc = {
      activePageId: saved.activePageId ?? null,
      favoriteIds: Array.isArray(saved.favoriteIds) ? [...saved.favoriteIds] : [],
      // Opaque here: the shell owns it, this only carries it across a dashboard save.
      rail: saved.rail ?? null,
      // A doc written by an older build could carry a stale `builtin` copy of the retired
      // Modules page, which is dropped rather than migrated.
      pages: saved.pages
        .filter((p) => !p.builtin && (Array.isArray(p.layout?.items) || Array.isArray(p.layout?.rows)))
        .map((p) => ({ ...p, layout: normalizeLayout(p.layout) }))
    };
  } else {
    doc = { activePageId: null, favoriteIds: [], rail: null, pages: [] };
  }
  return doc;
}

/**
 * The full ordered page list for display. Presets stay first and custom pages follow.
 * Returns `{ pages, defaultId }`. The default is the first favourite, falling back to
 * Investor when the bar is empty.
 */
export function pagesForDisplay(doc) {
  const all = doc.pages.filter((page) => page.id !== MODULES_PAGE_ID);
  const favorite = (doc.favoriteIds ?? []).find((id) => all.some((p) => p.id === id));
  return { pages: all, defaultId: favorite ?? DASHBOARD_PRESETS[0].id };
}

/**
 * Reconcile the favourites bar with the pages that exist: drop unknown and duplicate ids,
 * cap at MAX_FAVORITES, and seed a fresh workspace with the first presets so the bar is
 * never empty. Mutates `doc`; returns what it had to drop, so the caller can say so.
 */
export function normalizeFavorites(doc) {
  const known = new Set(doc.pages.filter((p) => p.id !== MODULES_PAGE_ID).map((p) => p.id));
  const before = Array.isArray(doc.favoriteIds) ? doc.favoriteIds : [];
  const kept = [];
  const overflow = [];
  const seen = new Set();
  for (const id of before) {
    if (!known.has(id) || seen.has(id)) continue;
    seen.add(id);
    (kept.length < MAX_FAVORITES ? kept : overflow).push(id);
  }
  if (!kept.length) {
    for (const preset of DASHBOARD_PRESETS.slice(0, MAX_FAVORITES)) {
      if (known.has(preset.id)) kept.push(preset.id);
    }
  }
  const changed = kept.length !== before.length || kept.some((id, i) => before[i] !== id);
  doc.favoriteIds = kept;
  return { changed, overflow };
}
