/**
 * Dashboard model — a built-in "Modules" page plus any number of user-defined pages,
 * each with its own module grid.
 *
 * Stored document (opaque in `app_settings.dashboard_layout`):
 *   { activePageId, defaultPageId, pages: [ { id, name, description, tag, layout } ] }
 *
 * The Modules page (id `MODULES_PAGE_ID`) is stored like any other page, but it is owned
 * by the app: it is seeded from `SECTIONS` on first sight, reconciled against the installed
 * set on every load (a new module joins its section, a detached one loses its tile), always
 * sorts first, and cannot be deleted or have its module set edited by hand. Everything else
 * about it is the user's: titles, order, spans, extra rows and widgets.
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

/** Widget height presets → pixel min-height of the cell. Standard is the default;
 *  `title` is a header band, the height of a text widget used as a section title. */
export const WIDGET_HEIGHTS = { title: 52, compact: 150, standard: 220, tall: 340 };

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

/**
 * Sections of the Modules page: an ordered grouping of module ids, each rendered as a title
 * row (a `text` widget at the `title` height) followed by its module rows. A module absent
 * from every list lands in `other`. The grouping only *seeds* the page: once seeded the user
 * owns the layout, and the titles are ordinary editable widgets.
 */
export const SECTIONS = [
  {
    id: 'markets',
    key: 'dashboard.section.markets',
    modules: ['histdata', 'histviz', 'watchlists', 'findb', 'economics', 'mportfolios']
  },
  {
    id: 'trading',
    key: 'dashboard.section.trading',
    modules: ['journal', 'backtest', 'quant', 'routines', 'mindset']
  },
  {
    id: 'wealth',
    key: 'dashboard.section.wealth',
    modules: ['wealth', 'portfolios', 'subscriptions', 'taxcalc']
  },
  {
    id: 'info',
    key: 'dashboard.section.info',
    modules: ['news', 'mailbox', 'resources', 'community-docs']
  },
  {
    id: 'productivity',
    key: 'dashboard.section.productivity',
    modules: ['editor', 'todos', 'goals', 'calendar', 'time', 'remindme']
  },
  {
    id: 'automation',
    key: 'dashboard.section.automation',
    modules: ['agent', 'automator', 'webhooks', 'prompt-store']
  },
  { id: 'other', key: 'dashboard.section.other', modules: [] }
];

/** The section a module belongs to (`other` when it is in none). */
export function sectionOf(moduleId) {
  return SECTIONS.find((s) => s.modules.includes(moduleId))?.id ?? 'other';
}

function sectionById(id) {
  return SECTIONS.find((s) => s.id === id) ?? SECTIONS[SECTIONS.length - 1];
}

/** Installed ids minus the home module, which is the dashboard itself. */
function placeable(installedIds) {
  return [...installedIds].filter((id) => id !== 'dashboard');
}

/** Title row + module rows for one section. `tr` translates a key. */
function sectionRows(sec, moduleIds, tr) {
  const rows = [
    {
      id: rid(),
      kind: 'widgets',
      section: sec.id,
      items: [
        { id: rid(), type: 'text', span: COLS, config: { title: tr(sec.key), height: 'title' } }
      ]
    }
  ];
  for (let i = 0; i < moduleIds.length; i += 3) {
    rows.push({
      id: rid(),
      kind: 'modules',
      section: sec.id,
      items: moduleIds.slice(i, i + 3).map((moduleId) => ({ id: rid(), moduleId, span: COLS / 3 }))
    });
  }
  return rows;
}

/** The seed layout of the Modules page: one titled section per non-empty group. */
export function sectionedLayout(installedIds, tr = (k) => k) {
  const have = placeable(installedIds);
  const rows = [];
  for (const sec of SECTIONS) {
    const mods = sec.modules.filter((id) => have.includes(id));
    if (mods.length) rows.push(...sectionRows(sec, mods, tr));
  }
  const known = new Set(SECTIONS.flatMap((s) => s.modules));
  const rest = have.filter((id) => !known.has(id));
  if (rest.length) rows.push(...sectionRows(sectionById('other'), rest, tr));
  return { cols: COLS, rows };
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

/** A freshly seeded Modules page: the installed set laid out by section. */
export function modulesPage(installedIds, tr = (k) => k) {
  return {
    id: MODULES_PAGE_ID,
    name: tr('dashboard.modulesPage.name'),
    description: tr('dashboard.modulesPage.desc'),
    tag: tr('dashboard.modulesPage.tag'),
    seen: placeable(installedIds),
    layout: sectionedLayout(installedIds, tr)
  };
}

/**
 * Reconcile a stored Modules page against the installed set: drop the tiles of modules that
 * are gone, append the ones never seen before to their section (recreating it at the end
 * when the user has deleted it). `seen` is what stops a tile the user removed by hand from
 * growing back, while a detached-then-reinstalled module does come back.
 * Mutates `page`; returns true when something changed, so the caller can save.
 */
export function syncModulesPage(page, installedIds, tr = (k) => k) {
  const have = placeable(installedIds);
  const has = new Set(have);
  let changed = false;

  for (const row of page.layout.rows) {
    if (row.kind !== 'modules') continue;
    const kept = row.items.filter((i) => has.has(i.moduleId));
    if (kept.length !== row.items.length) {
      row.items = kept;
      changed = true;
    }
  }

  // A section whose last module was detached takes its title row with it, but only when the
  // section holds nothing else: a widget the user dropped in there keeps it alive.
  for (const sec of SECTIONS) {
    const rows = page.layout.rows.filter((r) => r.section === sec.id);
    if (!rows.length) continue;
    const alive = rows.some((r) => r.kind === 'modules' && r.items.length);
    const extra = rows.some(
      (r) => r.kind === 'widgets' && r.items.some((i) => i.config?.height !== 'title')
    );
    if (alive || extra) continue;
    page.layout.rows = page.layout.rows.filter((r) => r.section !== sec.id);
    changed = true;
  }

  const seen = new Set(page.seen ?? []);
  const placed = new Set(
    page.layout.rows.flatMap((r) => (r.kind === 'modules' ? r.items.map((i) => i.moduleId) : []))
  );
  for (const id of have) {
    if (seen.has(id) || placed.has(id)) continue;
    const secId = sectionOf(id);
    const rows = page.layout.rows;
    let last = -1;
    for (let i = 0; i < rows.length; i += 1) {
      if (rows[i].kind === 'modules' && rows[i].section === secId) last = i;
    }
    const tile = { id: rid(), moduleId: id, span: COLS / 3 };
    if (last >= 0 && rows[last].items.length < 3) {
      rows[last].items = [...rows[last].items, tile];
    } else if (last >= 0) {
      rows.splice(last + 1, 0, { id: rid(), kind: 'modules', section: secId, items: [tile] });
    } else {
      page.layout.rows = [...rows, ...sectionRows(sectionById(secId), [id], tr)];
    }
    changed = true;
  }

  if (changed || (page.seen ?? []).length !== have.length) {
    page.seen = have;
    changed = true;
  }
  return changed;
}

/**
 * Make sure the doc holds a Modules page: seeded when it is missing, reconciled when it is
 * there. Mutates `doc`; returns true when the caller should persist.
 */
export function ensureModulesPage(doc, installedIds, tr = (k) => k) {
  const page = doc.pages.find((p) => p.id === MODULES_PAGE_ID);
  if (!page) {
    doc.pages = [modulesPage(installedIds, tr), ...doc.pages];
    return true;
  }
  return syncModulesPage(page, installedIds, tr);
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
 *  - pages doc               → keep its pages (the Modules page included), keep ids
 * Never mutates the input. A missing Modules page is seeded by `ensureModulesPage`.
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
      // A pre-sections doc never stored the Modules page; one written by an older build
      // could carry a stale `builtin` copy, which is dropped so the page is re-seeded.
      pages: saved.pages
        .filter((p) => !p.builtin && Array.isArray(p.layout?.rows))
        .map((p) => ({ ...p }))
    };
  } else {
    doc = { activePageId: null, defaultPageId: null, pages: [] };
  }
  return doc;
}

/**
 * The full ordered page list for display: the Modules page first, then user pages, with the
 * default page sorted first. Returns `{ pages, defaultId }` where `defaultId` is the
 * effective default (user default if valid, else the Modules page). The Modules page's
 * name/description/tag follow the UI language, not the stored copy.
 */
export function pagesForDisplay(doc, installedIds, tr = (k) => k) {
  const stored = doc.pages.find((p) => p.id === MODULES_PAGE_ID);
  const mods = stored
    ? {
        ...stored,
        name: tr('dashboard.modulesPage.name'),
        description: tr('dashboard.modulesPage.desc'),
        tag: tr('dashboard.modulesPage.tag')
      }
    : modulesPage(installedIds, tr);
  const all = [mods, ...doc.pages.filter((p) => p.id !== MODULES_PAGE_ID)];
  const defaultId =
    doc.defaultPageId && all.some((p) => p.id === doc.defaultPageId)
      ? doc.defaultPageId
      : MODULES_PAGE_ID;
  all.sort((a, b) => (a.id === defaultId ? -1 : b.id === defaultId ? 1 : 0));
  return { pages: all, defaultId };
}
