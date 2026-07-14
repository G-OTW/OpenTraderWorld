/**
 * Module registry — the single source of truth for installed/activated modules.
 *
 * The top-left switcher reads this list. Each module owns its full context: its own
 * route subtree, its own sidebar/menu and content. Switching modules navigates to the
 * module's base route, which swaps the whole working area.
 *
 * Phase: first-party modules are declared statically here. Later, community/installed
 * modules will be merged in from the backend module registry (otw-modules).
 */

export const modules = [
  {
    id: 'dashboard',
    name: 'Dashboard',
    icon: 'grid',
    base: '/',
    // The dashboard is the home/overview; it lists activated modules.
    home: true
  },
  {
    id: 'editor',
    name: 'Editor',
    icon: 'pencil',
    base: '/editor',
    descKey: 'module.editor.desc'
  },
  {
    id: 'news',
    name: 'News',
    icon: 'newspaper',
    base: '/news',
    descKey: 'module.news.desc'
  },
  {
    id: 'journal',
    name: 'Trading Journal',
    icon: 'trending-up',
    base: '/journal',
    descKey: 'module.journal.desc'
  },
  {
    id: 'subscriptions',
    name: 'Subscriptions',
    icon: 'refresh-cw',
    base: '/subscriptions',
    descKey: 'module.subscriptions.desc'
  },
  {
    id: 'time',
    name: 'Time Tracker',
    icon: 'timer',
    base: '/time',
    descKey: 'module.time.desc'
  },
  {
    id: 'wealth',
    name: 'MyWealth',
    icon: 'coins',
    base: '/wealth',
    descKey: 'module.wealth.desc'
  },
  {
    id: 'goals',
    name: 'Goals',
    icon: 'target',
    base: '/goals',
    descKey: 'module.goals.desc'
  },
  {
    id: 'todos',
    name: 'ToDo',
    icon: 'check-square',
    base: '/todos',
    descKey: 'module.todos.desc'
  },
  {
    id: 'routines',
    name: 'Trading Routines',
    icon: 'clipboard-list',
    base: '/routines',
    descKey: 'module.routines.desc'
  },
  {
    id: 'mindset',
    name: 'Mindset',
    icon: 'lightbulb',
    base: '/mindset',
    descKey: 'module.mindset.desc'
  },
  {
    id: 'remindme',
    name: 'RemindMe',
    icon: 'bell',
    base: '/remindme',
    descKey: 'module.remindme.desc'
  },
  {
    id: 'findb',
    name: 'FinanceDatabase',
    icon: 'search',
    base: '/findb',
    descKey: 'module.findb.desc'
  },
  {
    id: 'calendar',
    name: 'Calendar',
    icon: 'calendar',
    base: '/calendar',
    descKey: 'module.calendar.desc'
  },
  {
    id: 'economics',
    name: 'Economic Calendar',
    icon: 'calendar-days',
    base: '/economics',
    descKey: 'module.economics.desc'
  },
  {
    id: 'histdata',
    name: 'Historical Data',
    icon: 'download',
    base: '/histdata',
    descKey: 'module.histdata.desc'
  },
  {
    id: 'histviz',
    name: 'Historical Data Visualization',
    icon: 'candlestick',
    base: '/histviz',
    descKey: 'module.histviz.desc',
    // Charts the Historical Data catalog; unusable without it.
    requires: ['histdata']
  },
  {
    id: 'backtest',
    name: 'Backtest',
    icon: 'flask',
    base: '/backtest',
    descKey: 'module.backtest.desc',
    // Reads the Historical Data dataset catalog; unusable without it.
    requires: ['histdata']
  },
  {
    id: 'mportfolios',
    name: "Managers' Portfolios",
    icon: 'landmark',
    base: '/mportfolios',
    descKey: 'module.mportfolios.desc'
  },
  {
    id: 'portfolios',
    name: 'Portfolio Tracker',
    icon: 'briefcase',
    base: '/portfolios',
    descKey: 'module.portfolios.desc'
  },
  {
    id: 'taxcalc',
    name: 'Tax Calculator',
    icon: 'receipt',
    base: '/taxcalc',
    descKey: 'module.taxcalc.desc'
  },
  {
    id: 'quant',
    name: 'Quant Tools',
    icon: 'ruler',
    base: '/quant',
    descKey: 'module.quant.desc',
    // Analyses the Historical Data catalog; unusable without it.
    requires: ['histdata']
  },
  {
    id: 'resources',
    name: 'Resources',
    icon: 'book',
    base: '/resources',
    descKey: 'module.resources.desc'
  },
  {
    id: 'prompt-store',
    name: 'Prompt Store',
    icon: 'message-square',
    base: '/prompt-store',
    descKey: 'module.prompt-store.desc'
  },
  {
    id: 'community-docs',
    name: 'Community Docs',
    icon: 'book-open',
    base: '/community-docs',
    descKey: 'module.community-docs.desc'
  },
  {
    id: 'webhooks',
    name: 'Webhooks',
    icon: 'webhook',
    base: '/webhooks',
    descKey: 'module.webhooks.desc',
    // v1's only redirect target is RemindMe notifications; pointless without it.
    requires: ['remindme']
  }
];

/** Find the module that owns a given pathname (longest matching base wins). */
export function moduleForPath(pathname) {
  let best = modules.find((m) => m.home) ?? modules[0];
  let bestLen = -1;
  for (const m of modules) {
    if (m.base === '/') {
      if (pathname === '/' && bestLen < 0) best = m;
      continue;
    }
    if ((pathname === m.base || pathname.startsWith(m.base + '/')) && m.base.length > bestLen) {
      best = m;
      bestLen = m.base.length;
    }
  }
  return best;
}

export function moduleById(id) {
  return modules.find((m) => m.id === id);
}

/** Feature modules (everything except the always-on home/dashboard). */
export const featureModules = modules.filter((m) => !m.home);

/**
 * Modules visible given an installed-id set. The home/dashboard is always shown; feature
 * modules appear only when installed. A `null` set (not yet loaded) shows everything to
 * avoid a flash of an empty switcher.
 */
export function visibleModules(installed) {
  if (!installed) return modules;
  return modules.filter((m) => m.home || installed.has(m.id));
}

/**
 * Modules a given module depends on that are not in the installed set. Returns the
 * missing module descriptors (so callers can render names/icons), or `[]` when all
 * requirements are met. A `null` set (not yet loaded) is treated as "all present" to
 * avoid a flash of a false alert before the installed set arrives.
 */
export function missingRequirements(id, installed) {
  if (!installed) return [];
  const m = moduleById(id);
  if (!m?.requires?.length) return [];
  return m.requires.filter((r) => !installed.has(r)).map((r) => moduleById(r)).filter(Boolean);
}
