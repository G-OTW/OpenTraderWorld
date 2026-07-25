/**
 * Widget registry — the single source of truth for dashboard widgets.
 *
 * A widget is an interactive preview of a module (or free text) that lives in a `widgets`
 * row on a dashboard page. Each entry declares:
 *   - `type`        stable key stored in the layout item
 *   - `label`       human name shown in the widget picker
 *   - `icon`        Icon name (usually the owning module's icon)
 *   - `moduleId`    module this widget previews (used to gate on install); null for free text
 *   - `component`   the Svelte component, lazily imported
 *   - `defaultSpan` initial column span when added
 *   - `blurb`       one line shown in the picker
 *
 * Widgets read/write through the owning module's existing api.js — no new backend.
 * A widget component receives `{ item, editing }` props: `item.config` is its opaque,
 * bindable settings object, and `editing` is true while the page is in edit mode (so the
 * widget can suppress live actions / show a config affordance).
 */

export const WIDGETS = [
  {
    type: 'text',
    label: 'Free text',
    icon: 'text-quote',
    moduleId: null,
    defaultSpan: 4,
    blurb: 'A note or heading — markdown-lite text you write yourself.',
    loader: () => import('./TextWidget.svelte')
  },
  {
    type: 'news',
    label: 'News feed',
    icon: 'newspaper',
    moduleId: 'news',
    defaultSpan: 4,
    blurb: 'The latest items from a chosen feed, as a scrollable list or grid.',
    loader: () => import('./NewsWidget.svelte')
  },
  {
    type: 'time',
    label: 'Time tracker',
    icon: 'timer',
    moduleId: 'time',
    defaultSpan: 4,
    blurb: 'Start/stop a project timer straight from the dashboard.',
    loader: () => import('./TimeWidget.svelte')
  },
  {
    type: 'journal',
    label: 'Quick trade',
    icon: 'trending-up',
    moduleId: 'journal',
    defaultSpan: 4,
    blurb: 'Pick a category + template and open the add-trade form.',
    loader: () => import('./JournalWidget.svelte')
  },
  {
    type: 'goals',
    label: 'Goals',
    icon: 'target',
    moduleId: 'goals',
    defaultSpan: 4,
    blurb: 'A short scrollable list of goals with progress; add one inline.',
    loader: () => import('./GoalsWidget.svelte')
  },
  {
    type: 'todos',
    label: 'ToDo',
    icon: 'check-square',
    moduleId: 'todos',
    defaultSpan: 4,
    blurb: "Today's tasks — tick them off without leaving the dashboard.",
    loader: () => import('./TodosWidget.svelte')
  },
  {
    type: 'routines',
    label: 'Trading routine',
    icon: 'clipboard-list',
    moduleId: 'routines',
    defaultSpan: 4,
    blurb: "Today's routine checklist; tick items inline.",
    loader: () => import('./RoutinesWidget.svelte')
  },
  {
    type: 'mindset',
    label: 'Mindset',
    icon: 'lightbulb',
    moduleId: 'mindset',
    defaultSpan: 4,
    blurb: "Today's pre/post-mortem check-in status; jump in to fill it.",
    loader: () => import('./MindsetWidget.svelte')
  },
  {
    type: 'remindme',
    label: 'Reminder',
    icon: 'bell',
    moduleId: 'remindme',
    defaultSpan: 4,
    blurb: 'A quick add-reminder form.',
    loader: () => import('./RemindWidget.svelte')
  },
  {
    type: 'calendar',
    label: 'Calendar',
    icon: 'calendar',
    moduleId: 'calendar',
    defaultSpan: 4,
    blurb: 'Today & this week at a glance, scrollable.',
    loader: () => import('./CalendarWidget.svelte')
  },
  {
    type: 'economics',
    label: 'Economic calendar',
    icon: 'calendar-days',
    moduleId: 'economics',
    defaultSpan: 4,
    blurb: 'TradingView economic calendar, compressed and scrollable.',
    loader: () => import('./EconomicsWidget.svelte')
  },
  {
    type: 'portfolios',
    label: 'Portfolio',
    icon: 'briefcase',
    moduleId: 'portfolios',
    defaultSpan: 4,
    blurb: 'A portfolio summary thumbnail with live value.',
    loader: () => import('./PortfolioWidget.svelte')
  },
  {
    type: 'prompts',
    label: 'Prompt store',
    icon: 'message-square',
    moduleId: 'prompt-store',
    defaultSpan: 4,
    blurb: 'Your prompts by tag — click one to copy it.',
    loader: () => import('./PromptsWidget.svelte')
  },
  {
    type: 'agent',
    label: 'Agent',
    icon: 'brain',
    moduleId: 'agent',
    defaultSpan: 4,
    blurb: 'Ask the agent — pick model and tools, send, land in the conversation.',
    loader: () => import('./AgentWidget.svelte')
  },
  {
    type: 'watchlists',
    label: 'Watchlist',
    icon: 'star',
    moduleId: 'watchlists',
    defaultSpan: 4,
    blurb: 'Live quotes for a chosen watchlist — price, 24h and 7d change.',
    loader: () => import('./WatchlistWidget.svelte')
  },
  {
    type: 'resources',
    label: 'Resources',
    icon: 'book',
    moduleId: 'resources',
    defaultSpan: 4,
    blurb: 'A scrollable list of bookmarks from a chosen category.',
    loader: () => import('./ResourcesWidget.svelte')
  }
];

export function widgetByType(type) {
  return WIDGETS.find((w) => w.type === type) ?? null;
}

/** Widgets addable given the installed set: free text always, module widgets when installed. */
export function availableWidgets(installedIds) {
  return WIDGETS.filter((w) => !w.moduleId || !installedIds || installedIds.has(w.moduleId));
}
