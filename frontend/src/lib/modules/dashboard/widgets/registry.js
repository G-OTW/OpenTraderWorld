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
 *   - `variants`    supported data presentations. A variant is deliberately part of
 *                   the saved widget configuration, never a visual skin.
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
    variants: [{ id: 'note', label: 'Note', size: 'M', defaults: {} }],
    loader: () => import('./TextWidget.svelte')
  },
  {
    type: 'editor',
    label: 'Notes',
    icon: 'file-text',
    moduleId: 'editor',
    defaultSpan: 4,
    blurb: 'Recently edited docs, how many you have, and a one-line note creator.',
    variants: [
      { id: 'recent', label: 'Recently Edited Docs', subtitle: 'Latest workspace activity', size: 'SR', defaults: { limit: 6 } },
      { id: 'counts', label: 'Docs / Pages', subtitle: 'Workspace size', size: 'S', defaults: {} },
      { id: 'quick', label: 'Quick Create Note', subtitle: 'Capture without leaving the dashboard', size: 'S', defaults: {} }
    ],
    loader: () => import('./EditorWidget.svelte')
  },
  {
    type: 'news',
    label: 'News feed',
    icon: 'newspaper',
    moduleId: 'news',
    defaultSpan: 4,
    blurb: 'The latest items from a chosen feed, as a scrollable list or grid.',
    variants: [
      { id: 'latest', label: 'Latest — Selected Feed', subtitle: 'Chosen feed', size: 'MR', defaults: { view: 'list', limit: 10 } },
      { id: 'grid', label: 'Multi-Feed Dashboard', subtitle: 'Live overview across selected feeds', size: 'MR', defaults: { view: 'grid', limit: 8 } }
    ],
    loader: () => import('./NewsWidget.svelte')
  },
  {
    type: 'mailbox',
    label: 'Mailbox',
    icon: 'mail',
    moduleId: 'mailbox',
    defaultSpan: 4,
    blurb: 'Unread mail, the account sync status and who writes to you most.',
    variants: [
      { id: 'unread', label: 'Latest Unread Messages', subtitle: 'Most recent items', size: 'MR', defaults: { limit: 8 } },
      { id: 'count', label: 'Unread', subtitle: 'Current mailbox', size: 'S', defaults: { window: 200 } },
      { id: 'sync', label: 'Account Sync Status', subtitle: 'Connected mailboxes', tone: 'green', size: 'SR', defaults: {} },
      { id: 'senders', label: 'Messages by Sender', subtitle: 'This week', size: 'M', defaults: { window: 200 } }
    ],
    loader: () => import('./MailboxWidget.svelte')
  },
  {
    type: 'time',
    label: 'Time tracker',
    icon: 'timer',
    moduleId: 'time',
    defaultSpan: 4,
    blurb: 'Running timers, hours and value tracked, budgets, deadlines and the entry feed.',
    variants: [
      { id: 'running', label: 'Running Timer', subtitle: 'Current session', tone: 'green', size: 'S', defaults: { scope: 'running', limit: 3 } },
      { id: 'projects', label: 'Project Timers', subtitle: 'Tracked sessions', size: 'MR', defaults: { scope: 'all', limit: 8 } },
      { id: 'hours', label: 'Tracked Time', subtitle: 'Today / week / month', size: 'S', defaults: { period: 'week' } },
      { id: 'value', label: 'Value of Time', subtitle: 'Tracked value', tone: 'green', size: 'S', defaults: { period: 'week' } },
      { id: 'budget', label: 'Project Budget Usage', subtitle: 'Hours consumed', tone: 'amber', size: 'SR', defaults: { limit: 6 } },
      { id: 'deadline', label: 'Projects Near Deadline', subtitle: 'Attention required', tone: 'red', size: 'SR', defaults: { limit: 6 } },
      { id: 'top', label: 'Top Projects', subtitle: 'By tracked hours', size: 'M', defaults: { period: 'month', limit: 6 } },
      { id: 'entries', label: 'Recent Entries', subtitle: 'Latest tracked sessions', size: 'MR', defaults: { limit: 8 } }
    ],
    loader: () => import('./TimeWidget.svelte')
  },
  {
    type: 'journal',
    label: 'Quick trade',
    icon: 'trending-up',
    moduleId: 'journal',
    defaultSpan: 4,
    blurb: 'Pick a category + template and open the add-trade form.',
    variants: [{ id: 'quick-add', label: 'Quick trade', size: 'MR', defaults: {} }],
    loader: () => import('./JournalWidget.svelte')
  },
  {
    type: 'journal-stats',
    label: 'Journal analytics',
    icon: 'bar-chart',
    moduleId: 'journal',
    defaultSpan: 4,
    blurb: 'Your trading numbers: PnL, win rate, equity curve, exposure, behaviour.',
    variants: [
      { id: 'pnl', label: 'Net PnL', size: 'S', defaults: {} },
      { id: 'winrate', label: 'Win rate', size: 'S', defaults: {} },
      { id: 'pf', label: 'Profit factor', size: 'S', defaults: {} },
      { id: 'equity', label: 'Equity curve', size: 'L', defaults: {} },
      { id: 'expectancy', label: 'Expectancy', size: 'SR', defaults: {} },
      { id: 'bestworst', label: 'Best / worst trade', size: 'SR', defaults: {} },
      { id: 'streak', label: 'Win / loss streak', size: 'SR', defaults: {} },
      { id: 'groups', label: 'Trades by group', size: 'M', defaults: { group: 'strategy' } },
      { id: 'bygroup', label: 'PnL by group', size: 'M', defaults: { group: 'strategy', limit: 6 } },
      { id: 'positions', label: 'Open positions', size: 'MR', defaults: { limit: 8 } },
      { id: 'exposure', label: 'Exposure concentration', size: 'SR', defaults: {} },
      { id: 'correlation', label: 'Correlation warning', size: 'SR', defaults: {} },
      { id: 'behavior', label: 'Behaviour insights', size: 'MR', defaults: { limit: 4 } },
      { id: 'heatmap', label: 'Daily PnL heatmap', size: 'MR', defaults: { weeks: 53 } },
      { id: 'recent', label: 'Recent trades', size: 'MR', defaults: { limit: 8 } }
    ],
    loader: () => import('./JournalStatsWidget.svelte')
  },
  {
    type: 'goals',
    label: 'Goals',
    icon: 'target',
    moduleId: 'goals',
    defaultSpan: 4,
    blurb: 'A short scrollable list of goals with progress; add one inline.',
    variants: [
      { id: 'open', label: 'Open Goals', subtitle: 'Current priorities', size: 'MR', defaults: { scope: 'open', limit: 8 } },
      { id: 'status', label: 'Goals by Status', subtitle: 'Current overview', size: 'S', defaults: { scope: 'status' } },
      { id: 'avg', label: 'Average Progress', subtitle: 'Across open goals', tone: 'green', size: 'S', defaults: { scope: 'avg' } },
      { id: 'due', label: 'Due Soon', subtitle: 'Urgency buckets', tone: 'amber', size: 'SR', defaults: { scope: 'due', limit: 6 } },
      { id: 'overdue', label: 'Overdue Goals', subtitle: 'Needs attention', tone: 'red', size: 'SR', defaults: { scope: 'overdue', limit: 6 } },
      { id: 'category', label: 'Goals by Category', subtitle: 'Distribution', size: 'M', defaults: { scope: 'category' } },
      { id: 'goal', label: 'Goal Progress', subtitle: 'Target progress', size: 'S', defaults: { scope: 'goal' } }
    ],
    loader: () => import('./GoalsWidget.svelte')
  },
  {
    type: 'todos',
    label: 'ToDo',
    icon: 'check-square',
    moduleId: 'todos',
    defaultSpan: 4,
    blurb: "Today's tasks — tick them off without leaving the dashboard.",
    variants: [
      { id: 'upcoming', label: 'Upcoming Tasks', subtitle: 'Next actions', size: 'MR', defaults: { scope: 'upcoming', limit: 8 } },
      { id: 'status', label: 'Completion', subtitle: 'Open vs done', size: 'S', defaults: { scope: 'status' } },
      { id: 'buckets', label: 'Due Buckets', subtitle: 'Immediate focus', tone: 'amber', size: 'SR', defaults: { scope: 'buckets' } },
      { id: 'overdue', label: 'Overdue', subtitle: 'Needs attention', tone: 'red', size: 'SR', defaults: { scope: 'overdue', limit: 8 } },
      { id: 'category', label: 'By Category', subtitle: 'Open tasks', size: 'S', defaults: { scope: 'category' } },
      { id: 'backlog', label: 'Unscheduled Backlog', subtitle: 'No due date', size: 'S', defaults: { scope: 'backlog' } }
    ],
    loader: () => import('./TodosWidget.svelte')
  },
  {
    type: 'routines',
    label: 'Trading routine',
    icon: 'clipboard-list',
    moduleId: 'routines',
    defaultSpan: 4,
    blurb: "Today's checklist, consistency streak, completion rate and quick tasks.",
    variants: [
      { id: 'today', label: "Today's Checklist", subtitle: 'Daily completion', size: 'MR', defaults: { limit: 12 } },
      { id: 'streak', label: 'Consistency Streak', subtitle: 'Active days', tone: 'green', size: 'S', defaults: {} },
      { id: 'completion', label: 'Completion Rate', subtitle: 'This week', size: 'SR', defaults: { period: 'week' } },
      { id: 'templates', label: 'Templates', subtitle: 'Routine library', size: 'S', defaults: {} },
      { id: 'tasks', label: 'Quick Tasks', subtitle: 'Open / done / overdue', tone: 'red', size: 'SR', defaults: {} }
    ],
    loader: () => import('./RoutinesWidget.svelte')
  },
  {
    type: 'mindset',
    label: 'Mindset',
    icon: 'lightbulb',
    moduleId: 'mindset',
    defaultSpan: 4,
    blurb: "Check-in status, streak, entry history and the answer breakdown.",
    variants: [
      { id: 'today', label: "Today's Check-in", subtitle: 'Pre / post session', size: 'S', defaults: {} },
      { id: 'streak', label: 'Check-in Streak', subtitle: 'Consecutive active days', tone: 'green', size: 'S', defaults: {} },
      { id: 'history', label: 'History of Entries', subtitle: 'Check-in history', size: 'MR', defaults: { weeks: 53 } },
      { id: 'breakdown', label: 'Mood / Answer Breakdown', subtitle: 'Recent check-ins', size: 'M', defaults: { days: 90 } }
    ],
    loader: () => import('./MindsetWidget.svelte')
  },
  {
    type: 'remindme',
    label: 'Reminder',
    icon: 'bell',
    moduleId: 'remindme',
    defaultSpan: 4,
    blurb: 'A quick add-reminder form.',
    variants: [{ id: 'quick-add', label: 'Quick reminder', size: 'MR', defaults: {} }],
    loader: () => import('./RemindWidget.svelte')
  },
  {
    type: 'remindme-stats',
    label: 'Reminder stats',
    icon: 'bell',
    moduleId: 'remindme',
    defaultSpan: 4,
    blurb: 'Unread notifications, what is due next, and the reminders that stalled.',
    variants: [
      { id: 'unread', label: 'Unread Notifications', subtitle: 'Current inbox', size: 'S', defaults: { days: 14 } },
      { id: 'upcoming', label: 'Upcoming', subtitle: 'Next reminders due', tone: 'green', size: 'SR', defaults: { limit: 6 } },
      { id: 'active', label: 'Active vs Inactive', subtitle: 'Reminder status', size: 'S', defaults: {} },
      { id: 'overdue', label: 'Overdue / Exhausted', subtitle: 'Needs review', tone: 'red', size: 'SR', defaults: { limit: 6 } },
      { id: 'feed', label: 'Recent Notifications', subtitle: 'Latest triggers', size: 'MR', defaults: { limit: 8 } },
      { id: 'mostTriggered', label: 'Most Triggered Reminder', subtitle: 'Last 30 days', tone: 'green', size: 'S', defaults: { days: 14 } }
    ],
    loader: () => import('./RemindStatsWidget.svelte')
  },
  {
    type: 'calendar',
    label: 'Calendar',
    icon: 'calendar',
    moduleId: 'calendar',
    defaultSpan: 4,
    blurb: 'Upcoming events, counts, categories, a mini month and what is due across modules.',
    variants: [
      { id: 'week', label: 'This Week', subtitle: 'Calendar activity', tone: 'green', size: 'MR', defaults: { days: 7, limit: 12 } },
      { id: 'upcoming', label: 'Upcoming Events', subtitle: 'Next 7 days', size: 'SR', defaults: { days: 7, limit: 6 } },
      { id: 'counts', label: 'Today', subtitle: 'Scheduled events', size: 'S', defaults: { days: 8 } },
      { id: 'category', label: 'Events by Category', subtitle: 'This month', size: 'M', defaults: { days: 7 } },
      { id: 'month', label: 'Mini Month View', subtitle: 'Current month', size: 'M', defaults: {} },
      { id: 'dueSoon', label: 'Cross-Module Due Soon', subtitle: 'Reminders, todos and goals', size: 'MR', defaults: { days: 7, limit: 8 } }
    ],
    loader: () => import('./CalendarWidget.svelte')
  },
  {
    type: 'economics',
    label: 'Economic calendar',
    icon: 'calendar-days',
    moduleId: 'economics',
    defaultSpan: 4,
    blurb: 'TradingView economic calendar, compressed and scrollable.',
    variants: [{ id: 'calendar', label: 'Upcoming Releases', subtitle: 'Next scheduled macro events', size: 'LR', defaults: {} }],
    loader: () => import('./EconomicsWidget.svelte')
  },
  {
    type: 'portfolios',
    label: 'Portfolio',
    icon: 'briefcase',
    moduleId: 'portfolios',
    defaultSpan: 4,
    blurb: 'Value, PnL, allocation, positions and movers for one portfolio.',
    variants: [
      { id: 'summary', label: 'Portfolio summary', size: 'S', defaults: {} },
      { id: 'value', label: 'Total value', size: 'S', defaults: {} },
      { id: 'pnl', label: 'Total PnL %', size: 'S', defaults: {} },
      { id: 'cash', label: 'Cash vs invested', size: 'S', defaults: {} },
      { id: 'income', label: 'Portfolio income', size: 'S', defaults: {} },
      { id: 'evolution', label: 'Portfolio evolution', size: 'L', defaults: {} },
      { id: 'movers', label: 'Top movers', size: 'SR', defaults: { limit: 5 } },
      { id: 'best', label: 'Best position', size: 'SR', defaults: {} },
      { id: 'worst', label: 'Worst position', size: 'SR', defaults: {} },
      { id: 'allocation', label: 'Allocation by asset class', size: 'M', defaults: {} },
      { id: 'compare', label: 'Multi-portfolio comparison', size: 'MR', defaults: { limit: 6 } },
      { id: 'positions', label: 'Positions', size: 'MR', defaults: { limit: 8 } }
    ],
    loader: () => import('./PortfolioWidget.svelte')
  },
  {
    type: 'subscriptions',
    label: 'Subscriptions',
    icon: 'refresh-cw',
    moduleId: 'subscriptions',
    defaultSpan: 4,
    blurb: 'Recurring spend, forecast, spend by category and what renews next.',
    variants: [
      { id: 'overview', label: 'Subscription Spend', subtitle: 'Current run-rate', size: 'S', defaults: { display: 'overview', limit: 5 } },
      { id: 'monthly', label: 'Monthly Recurring', subtitle: 'Current run-rate', size: 'S', defaults: {} },
      { id: 'yearly', label: 'Yearly Equivalent', subtitle: 'Annualized recurring cost', size: 'S', defaults: {} },
      { id: 'forecast', label: 'Next Month Forecast', subtitle: 'Projected recurring charges', tone: 'green', size: 'S', defaults: {} },
      { id: 'active', label: 'Active vs Inactive', subtitle: 'Subscription status', tone: 'green', size: 'S', defaults: {} },
      { id: 'category', label: 'Spend by Category', subtitle: 'Ranked recurring cost', size: 'M', defaults: {} },
      { id: 'months', label: 'Spend History / Forecast', subtitle: 'Monthly recurring cost', size: 'MR', defaults: { monthsBack: 6, monthsFwd: 6 } },
      { id: 'renewals', label: 'Upcoming Charges', subtitle: 'Soonest renewals', tone: 'amber', size: 'SR', defaults: { display: 'renewals', limit: 8 } },
      { id: 'expensive', label: 'Most Expensive', subtitle: 'Top recurring subscriptions', size: 'SR', defaults: { limit: 5 } }
    ],
    loader: () => import('./SubscriptionsWidget.svelte')
  },
  {
    type: 'wealth',
    label: 'Net worth',
    icon: 'coins',
    moduleId: 'wealth',
    defaultSpan: 4,
    blurb: 'Net worth, its change over the window, and a trend line.',
    variants: [
      { id: 'overview', label: 'Net Worth (Current)', subtitle: 'Current position', size: 'S', defaults: { display: 'overview', months: 12 } },
      { id: 'change', label: 'Net Worth Change', subtitle: '1 year', tone: 'green', size: 'S', defaults: { months: 12 } },
      { id: 'trend', label: 'Net Worth Trend', subtitle: 'Historical evolution', size: 'M', defaults: { display: 'trend', months: 12 } },
      { id: 'split', label: 'Assets vs Liabilities', subtitle: 'Balance structure', size: 'SR', defaults: { months: 12 } },
      { id: 'category', label: 'Net Worth by Category', subtitle: 'Current allocation', size: 'M', defaults: { months: 12 } },
      { id: 'stale', label: 'Stale Assets Needing Review', subtitle: 'Items requiring attention', tone: 'red', size: 'SR', defaults: { months: 12, limit: 5 } },
      { id: 'top', label: 'Top Assets by Value', subtitle: 'Largest contributors', tone: 'amber', size: 'SR', defaults: { months: 12, limit: 5 } }
    ],
    loader: () => import('./WealthWidget.svelte')
  },
  {
    type: 'prompts',
    label: 'Prompt store',
    icon: 'message-square',
    moduleId: 'prompt-store',
    defaultSpan: 4,
    blurb: 'Your prompts by tag, the latest changes, and the most-iterated ones.',
    variants: [
      { id: 'library', label: 'Prompts by Tag', subtitle: 'Saved research prompts · click to copy', size: 'MR', defaults: { limit: 12 } },
      { id: 'recent', label: 'Recently Updated Prompts', subtitle: 'Latest revisions', size: 'SR', defaults: { limit: 6 } },
      { id: 'iterated', label: 'Most Iterated', subtitle: 'Highest version counts', size: 'S', defaults: { limit: 5 } }
    ],
    loader: () => import('./PromptsWidget.svelte')
  },
  {
    type: 'agent',
    label: 'Agent',
    icon: 'brain',
    moduleId: 'agent',
    defaultSpan: 4,
    blurb: 'Ask the agent — pick model and tools, send, land in the conversation.',
    variants: [{ id: 'quick-ask', label: 'Quick Ask', subtitle: 'Start a focused agent request', size: 'MR', defaults: {} }],
    loader: () => import('./AgentWidget.svelte')
  },
  {
    type: 'agent-stats',
    label: 'Agent activity',
    icon: 'message-square',
    moduleId: 'agent',
    defaultSpan: 4,
    blurb: 'Recent conversations and how many tokens they have spent.',
    variants: [
      { id: 'recent', label: 'Recent Conversations', subtitle: 'Latest agent sessions', size: 'MR', defaults: { limit: 6 } },
      { id: 'tokens', label: 'Token Usage', subtitle: 'Current billing period', tone: 'green', size: 'MR', defaults: { scan: 10 } }
    ],
    loader: () => import('./AgentStatsWidget.svelte')
  },
  {
    type: 'watchlists',
    label: 'Watchlist',
    icon: 'star',
    moduleId: 'watchlists',
    defaultSpan: 4,
    blurb: 'Quotes, movers, best/worst, a sparkline, sync status and alerts near trigger.',
    variants: [
      { id: 'quotes', label: 'Primary Watchlist', subtitle: 'Live quotes', size: 'MR', defaults: { order: 'list', limit: 10 } },
      { id: 'movers', label: 'Top Movers', subtitle: 'Across tracked symbols', size: 'SR', defaults: { order: 'movers', limit: 6, window: '24h' } },
      { id: 'bestworst', label: 'Best / Worst Today', subtitle: 'Across tracked symbols', size: 'SR', defaults: { window: '24h' } },
      { id: 'spark', label: 'Symbol Overview', subtitle: 'Current quote', size: 'S', defaults: { window: '24h' } },
      { id: 'sync', label: 'Sync Status', subtitle: 'Quote freshness', tone: 'green', size: 'S', defaults: {} },
      { id: 'alerts', label: 'Price Alerts', subtitle: 'Near trigger', tone: 'amber', size: 'SR', defaults: { limit: 6 } },
      { id: 'classes', label: 'By Asset Type', subtitle: 'Tracked symbols', size: 'S', defaults: {} }
    ],
    loader: () => import('./WatchlistWidget.svelte')
  },
  {
    type: 'automator',
    label: 'Automator',
    icon: 'repeat',
    moduleId: 'automator',
    defaultSpan: 4,
    blurb: 'What is running, what runs next, and which workflows keep failing.',
    variants: [
      { id: 'running', label: 'Running Workflows', subtitle: 'Current activity', tone: 'green', size: 'S', defaults: { limit: 4 } },
      { id: 'agenda', label: 'Next Scheduled Runs', subtitle: 'Upcoming agenda', size: 'SR', defaults: { days: 7, limit: 6 } },
      { id: 'status', label: 'Run Status', subtitle: 'Last 30 days', size: 'SR', defaults: { window: 200 } },
      { id: 'rates', label: 'Success Rate by Workflow', subtitle: 'Last 30 days', size: 'M', defaults: { window: 200, limit: 6 } },
      { id: 'failures', label: 'Recent Failures', subtitle: 'Latest workflow errors', tone: 'red', size: 'MR', defaults: { limit: 5 } },
      { id: 'favorites', label: 'Favorite Quick Runs', subtitle: 'Launch manually', size: 'SR', defaults: { limit: 5 } }
    ],
    loader: () => import('./AutomatorWidget.svelte')
  },
  {
    type: 'webhooks',
    label: 'Webhooks',
    icon: 'webhook',
    moduleId: 'webhooks',
    defaultSpan: 4,
    blurb: 'How much arrives, which endpoints error, and which have gone silent.',
    variants: [
      { id: 'total', label: 'Total Received', subtitle: 'All endpoints · 30 days', size: 'M', defaults: {} },
      { id: 'lastSeen', label: 'Last Received per Endpoint', subtitle: 'Silent-webhook detection', size: 'SR', defaults: { limit: 6, quietHours: 2 } },
      { id: 'feed', label: 'Recent Events', subtitle: 'Inbound webhook activity', size: 'MR', defaults: { limit: 8 } },
      { id: 'errors', label: 'Error Rate per Endpoint', subtitle: 'Last 30 days', tone: 'red', size: 'SR', defaults: { limit: 6 } }
    ],
    loader: () => import('./WebhooksWidget.svelte')
  },
  {
    type: 'histdata',
    label: 'Historical data',
    icon: 'database',
    moduleId: 'histdata',
    defaultSpan: 4,
    blurb: 'Datasets stored, disk used, the job queue and what needs a refill.',
    variants: [
      { id: 'datasets', label: 'Catalog Coverage', subtitle: 'Downloaded datasets', size: 'S', defaults: {} },
      { id: 'storage', label: 'Storage Size', subtitle: 'Downloaded datasets', size: 'S', defaults: {} },
      { id: 'queue', label: 'Download Queue', subtitle: 'Progress and state', size: 'SR', defaults: {} },
      { id: 'parked', label: 'Download Issues', subtitle: 'Recent attention items', tone: 'red', size: 'SR', defaults: { limit: 5 } },
      { id: 'gaps', label: 'Catalog Coverage Detail', subtitle: 'Available vs missing history', size: 'SR', defaults: { limit: 5 } },
      { id: 'stale', label: 'Freshness', subtitle: 'Latest data state', tone: 'green', size: 'SR', defaults: { limit: 5 } },
      { id: 'failures', label: 'Failed / Partial Downloads', subtitle: 'Needs action', tone: 'red', size: 'MR', defaults: { limit: 5 } }
    ],
    loader: () => import('./HistdataWidget.svelte')
  },
  {
    type: 'backtest',
    label: 'Backtest',
    icon: 'flask',
    moduleId: 'backtest',
    defaultSpan: 4,
    blurb: 'Your best saved run, the win-rate leaderboard and the strategy library.',
    variants: [
      { id: 'best', label: 'Best Saved Run', subtitle: 'By selected metric', tone: 'green', size: 'SR', defaults: { metric: 'return' } },
      { id: 'leaderboard', label: 'Leaderboard', subtitle: 'Win rate / profit factor', size: 'M', defaults: { limit: 5 } },
      { id: 'library', label: 'Strategy Library', subtitle: 'Saved strategies', size: 'M', defaults: { limit: 5 } },
      { id: 'favorites', label: 'Quick Run Favorites', subtitle: 'Frequently used backtests', size: 'M', defaults: { limit: 5 } }
    ],
    loader: () => import('./BacktestWidget.svelte')
  },
  {
    type: 'findb',
    label: 'Finance database',
    icon: 'landmark',
    moduleId: 'findb',
    defaultSpan: 4,
    blurb: 'Your favourites by type and folder, catalog freshness, and what went stale.',
    variants: [
      { id: 'assetTypes', label: 'Favorites by Classification', subtitle: 'Asset type / sector / exchange', size: 'M', defaults: {} },
      { id: 'folders', label: 'Favorite Folders', subtitle: 'Quick navigation', size: 'SR', defaults: { limit: 8 } },
      { id: 'update', label: 'Catalog Update', subtitle: 'FinanceDatabase catalog', tone: 'green', size: 'SR', defaults: {} },
      { id: 'stale', label: 'Stale Favorites', subtitle: 'Delisted or unmatched after re-import', tone: 'red', size: 'SR', defaults: { limit: 5 } }
    ],
    loader: () => import('./FindbWidget.svelte')
  },
  {
    type: 'histviz',
    label: 'Charts',
    icon: 'candlestick',
    moduleId: 'histviz',
    defaultSpan: 4,
    blurb: 'Saved workspaces and lists, active price alerts, and what fired recently.',
    variants: [
      { id: 'workspaces', label: 'Saved Layout', subtitle: 'Persistent chart configuration', tone: 'green', size: 'SR', defaults: { limit: 6 } },
      { id: 'alerts', label: 'Chart Alerts', subtitle: 'Active alerts on this symbol', tone: 'red', size: 'SR', defaults: { limit: 6 } },
      { id: 'triggered', label: 'Recent Alert Activity', subtitle: 'Alerts triggered recently', size: 'SR', defaults: { limit: 6, hours: 24 } },
      { id: 'lists', label: 'Saved Lists', subtitle: 'Chart symbol lists', size: 'SR', defaults: { limit: 6 } }
    ],
    loader: () => import('./HistvizWidget.svelte')
  },
  {
    type: 'mportfolios',
    label: "Managers' portfolios",
    icon: 'user',
    moduleId: 'mportfolios',
    defaultSpan: 4,
    blurb: 'Tracked managers, the biggest books, and what one manager moved.',
    variants: [
      { id: 'count', label: 'Tracked Managers', subtitle: 'Manager portfolios', size: 'S', defaults: {} },
      { id: 'largest', label: 'Largest Books', subtitle: 'Highest disclosed value', size: 'M', defaults: { limit: 5 } },
      { id: 'drift', label: 'Holdings Drift', subtitle: 'Latest portfolio change', size: 'SR', defaults: { limit: 5 } },
      { id: 'movers', label: 'Top Holdings by Change', subtitle: 'Across tracked managers', size: 'SR', defaults: { limit: 6 } }
    ],
    loader: () => import('./MportfoliosWidget.svelte')
  },
  {
    type: 'community-docs',
    label: 'Community Docs',
    icon: 'book-open',
    moduleId: 'community-docs',
    defaultSpan: 4,
    blurb: 'Saved community references, their categories, recent additions and quick access.',
    variants: [
      { id: 'library', label: 'Reference Library', subtitle: 'Saved community docs', size: 'S', defaults: {} },
      { id: 'category', label: 'By Category', subtitle: 'Reference distribution', size: 'M', defaults: { limit: 6 } },
      { id: 'recent', label: 'Recently Added', subtitle: 'Latest saved references', tone: 'green', size: 'SR', defaults: { limit: 6 } },
      { id: 'quick', label: 'Quick Access', subtitle: 'Frequently used reference links', size: 'SR', defaults: { limit: 6 } },
      { id: 'mix', label: 'Reference Mix', subtitle: 'Saved material', size: 'M', defaults: {} }
    ],
    loader: () => import('./CommunityDocsWidget.svelte')
  },
  {
    type: 'quant',
    label: 'Quant tools',
    icon: 'flask',
    moduleId: 'quant',
    defaultSpan: 4,
    blurb: 'What is available to analyze, a risk snapshot, correlations and seasonality.',
    variants: [
      { id: 'count', label: 'Available for Analysis', subtitle: 'Datasets + backtest runs', size: 'S', defaults: {} },
      { id: 'risk', label: 'Single-Asset Risk Snapshot', subtitle: 'Selected dataset', size: 'SR', defaults: {} },
      { id: 'correlation', label: 'Correlation Matrix', subtitle: 'Chosen basket · 1Y', size: 'M', defaults: { datasetIds: [] } },
      { id: 'seasonality', label: 'Seasonality Heatmap', subtitle: 'Month × weekday average return', size: 'M', defaults: { metric: 'return' } }
    ],
    loader: () => import('./QuantWidget.svelte')
  },
  {
    type: 'taxcalc',
    label: 'Tax calculator',
    icon: 'receipt',
    moduleId: 'taxcalc',
    defaultSpan: 4,
    blurb: 'Estimated tax for the year, the effective-rate trend, and a year-over-year table.',
    variants: [
      { id: 'total', label: 'Total estimated tax', size: 'SR', defaults: {} },
      { id: 'rate', label: 'Effective tax rate', size: 'M', defaults: {} },
      { id: 'compare', label: 'Year-over-year comparison', size: 'MR', defaults: { limit: 8 } }
    ],
    loader: () => import('./TaxcalcWidget.svelte')
  },
  {
    type: 'resources',
    label: 'Resources',
    icon: 'book',
    moduleId: 'resources',
    defaultSpan: 4,
    blurb: 'Bookmarks, counts per category, entries needing work, and top domains.',
    variants: [
      { id: 'bookmarks', label: 'Quick Access', subtitle: 'Favorites from Research', size: 'MR', defaults: { order: 'saved', limit: 10 } },
      { id: 'counts', label: 'Total Resources', subtitle: 'Saved library', size: 'S', defaults: {} },
      { id: 'perCategory', label: 'Resources per Category', subtitle: 'Top-N distribution', size: 'SR', defaults: { limit: 6 } },
      { id: 'recent', label: 'Recently Added', subtitle: 'Latest resources', tone: 'green', size: 'SR', defaults: { order: 'recent', limit: 6 } },
      { id: 'enrich', label: 'Needs Enrichment', subtitle: 'Missing metadata', tone: 'amber', size: 'SR', defaults: { limit: 6 } },
      { id: 'domains', label: 'Top Domains Saved', subtitle: 'Library sources', size: 'M', defaults: {} }
    ],
    loader: () => import('./ResourcesWidget.svelte')
  }
];

export function widgetByType(type) {
  return WIDGETS.find((w) => w.type === type) ?? null;
}

/** The active data presentation for a widget. Older saved widgets gracefully use first. */
export function widgetVariant(widget, config = {}) {
  const variants = widget?.variants ?? [];
  return variants.find((variant) => variant.id === config.variant) ?? variants[0] ?? null;
}

/** The shared widget-size codes from ProjectSpecs/Widgets. Width stays user-adjustable
 * after insertion; this only makes a new widget land at a usable minimum size. */
export const WIDGET_SIZE_PRESETS = {
  S: { span: 3, height: 'compact' },
  SR: { span: 5, height: 'compact' },
  M: { span: 4, height: 'standard' },
  MR: { span: 6, height: 'standard' },
  L: { span: 12, height: 'tall' },
  LR: { span: 12, height: 'tall' }
};

export function widgetPlacement(type, variantId) {
  const widget = widgetByType(type);
  const variant = widgetVariant(widget, { variant: variantId });
  return WIDGET_SIZE_PRESETS[variant?.size] ?? { span: widget?.defaultSpan ?? 4, height: 'standard' };
}

/** Defaults are written only when a widget is first created or its type is changed. */
export function widgetDefaults(type, variantId) {
  const widget = widgetByType(type);
  const variant = widgetVariant(widget, { variant: variantId });
  const placement = widgetPlacement(type, variantId);
  return {
    ...(variant?.defaults ?? {}),
    ...(variant ? { variant: variant.id } : {}),
    height: placement.height
  };
}

export function widgetVariantOptions(widget) {
  return (widget?.variants ?? []).map((variant) => ({
    value: variant.id,
    label: `${variant.label} · ${variant.size}`
  }));
}

/** Widgets addable given the installed set: free text always, module widgets when installed. */
export function availableWidgets(installedIds) {
  return WIDGETS.filter((w) => !w.moduleId || !installedIds || installedIds.has(w.moduleId));
}
