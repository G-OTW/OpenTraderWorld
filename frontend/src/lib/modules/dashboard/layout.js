/**
 * Dashboard model — a built-in "Modules" page plus any number of user-defined pages,
 * each with its own module grid.
 *
 * Stored document (opaque in `app_settings.dashboard_layout`):
 *   { activePageId, defaultPageId, pages: [ { id, name, description, tag, layout } ] }
 *
 * Only *user* pages are stored. The built-in Modules page (id `MODULES_PAGE_ID`) is
 * synthesized at render time from the installed-module set, so it always reflects what's
 * installed and is never persisted, edited, or deleted.
 *
 * - `defaultPageId` is the page the dashboard opens to and whose chip sorts first. When
 *   unset (or pointing at a missing page) the Modules page is the default.
 * - `activePageId` is the page currently shown (session/last-viewed; we open on default).
 *
 * A page's `layout` is an ordered list of rows over a fixed `cols` grid:
 *  - `modules`: placed tiles `{ id, moduleId, span }`; `span` is a column count.
 *  - `widgets`: interactive widget tiles `{ id, type, span, config }` — live previews of a
 *               module (or free text). `type` is a key in the widget registry; `config` is
 *               the widget's own opaque settings (feed id, portfolio id, …).
 *  - `spacer`:  an empty horizontal gap of `height` units.
 *
 * Tiles are *links* — the same module may appear on any number of pages; only the link
 * tile is duplicated. A tile whose module is no longer installed renders nothing.
 */

export const COLS = 12;
export const MODULES_PAGE_ID = '__modules__';

/** Widget height presets → pixel min-height of the cell. Standard is the default. */
export const WIDGET_HEIGHTS = { compact: 150, standard: 220, tall: 340 };

/** Min-height (px) an item contributes to its row; module links contribute nothing, so a
 *  module-only row stays content-sized as before. */
export function itemMinHeight(item) {
  if (!item?.type) return 0; // module link tile
  return WIDGET_HEIGHTS[item.config?.height] ?? WIDGET_HEIGHTS.standard;
}

let _seq = 0;
/** Short unique id for pages/rows/items (dnd needs stable ids). */
export function rid() {
  _seq += 1;
  return `r${Date.now().toString(36)}${_seq}`;
}

/** A grid layout containing the given module ids, flowing 3 per row at span 4. */
export function layoutForModules(moduleIds) {
  const rows = [];
  for (let i = 0; i < moduleIds.length; i += 3) {
    const chunk = moduleIds.slice(i, i + 3);
    rows.push({
      id: rid(),
      kind: 'modules',
      items: chunk.map((moduleId) => ({ id: rid(), moduleId, span: COLS / 3 }))
    });
  }
  return { cols: COLS, rows };
}

/** A fresh widgets row holding a single widget of the given type at a default span. */
export function makeWidgetRow(type, span = COLS / 3) {
  return {
    id: rid(),
    kind: 'widgets',
    items: [{ id: rid(), type, span, config: {} }]
  };
}

/** The built-in Modules page, rebuilt from the current installed set each render. */
export function modulesPage(installedIds) {
  return {
    id: MODULES_PAGE_ID,
    builtin: true,
    name: 'Modules',
    description: 'All your installed modules.',
    tag: 'Modules',
    layout: layoutForModules(installedIds)
  };
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

/**
 * Coerce whatever was saved into a valid stored doc of *user* pages.
 *  - `null`/empty            → no user pages
 *  - legacy `{ cols, rows }` → migrate to one "Dashboard" user page
 *  - pages doc               → keep its user pages (drop any stale builtin), keep ids
 * Never mutates the input. The Modules page is added separately at render time.
 */
export function normalizeDoc(saved) {
  let doc;
  if (saved && Array.isArray(saved.rows)) {
    // Legacy single-layout format → one user page.
    doc = {
      activePageId: null,
      defaultPageId: null,
      pages: [
        {
          id: rid(),
          name: 'Dashboard',
          description: 'Your activated modules, arranged your way.',
          tag: 'Dashboard',
          layout: { cols: saved.cols ?? COLS, rows: saved.rows }
        }
      ]
    };
  } else if (saved && Array.isArray(saved.pages)) {
    doc = {
      activePageId: saved.activePageId ?? null,
      defaultPageId: saved.defaultPageId ?? null,
      pages: saved.pages.filter((p) => p.id !== MODULES_PAGE_ID && !p.builtin)
    };
  } else {
    doc = { activePageId: null, defaultPageId: null, pages: [] };
  }
  return doc;
}

/**
 * The full ordered page list for display: the built-in Modules page plus user pages, with
 * the default page sorted first. Returns `{ pages, defaultId }` where `defaultId` is the
 * effective default (user default if valid, else the Modules page).
 */
export function pagesForDisplay(doc, installedIds) {
  const all = [modulesPage(installedIds), ...doc.pages];
  const defaultId =
    doc.defaultPageId && all.some((p) => p.id === doc.defaultPageId)
      ? doc.defaultPageId
      : MODULES_PAGE_ID;
  all.sort((a, b) => (a.id === defaultId ? -1 : b.id === defaultId ? 1 : 0));
  return { pages: all, defaultId };
}
