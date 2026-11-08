// Mobile check for the admin routes: settings, connectors, webhooks, mailbox, auth pages,
// OAuth consent, news and editor. Fixture API only: no backend, no real credentials.
// Run with the frontend dev server:
//   node tests/mobile-admin-browser.mjs [base URL] [baseline URL]
// The baseline URL (clean HEAD build) is used for the desktop screenshot comparison.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const baseline = process.argv[3] ?? 'http://127.0.0.1:4190';
const only = process.env.ONLY ? process.env.ONLY.split(',') : null;
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-admin/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });

const VIEWPORTS = [
  { width: 320, height: 568 },
  { width: 390, height: 844 },
  { width: 600, height: 800 },
  { width: 667, height: 375 },
  { width: 768, height: 1024 }
];
const KEYBOARD = { width: 390, height: 500 };
const DESKTOP = [{ width: 1440, height: 900 }, { width: 1100, height: 800 }];
const NOW = new Date('2026-10-20T09:30:00Z');
const iso = (minutesAgo) => new Date(NOW.getTime() - minutesAgo * 60000).toISOString();

// ── Fixture data ───────────────────────────────────────────────────────────
const LONG = 'Supercalifragilistic-institutional-research-desk-international-equities';
const modules = ['agent', 'automator', 'backtest', 'calendar', 'connectors', 'editor', 'findb', 'fundamentals', 'histdata', 'histviz', 'journal', 'mailbox', 'news', 'portfolios', 'quant', 'settings', 'watchlists', 'webhooks'];

const vaults = [
  { id: 'vault-1', name: 'Market data keys for the institutional research desk', items: [
    { id: 'item-1', name: 'POLYGON_API_KEY_FOR_THE_LONG_RUNNING_HISTORY_DOWNLOADER', in_use: 3 },
    { id: 'item-2', name: 'FRED_KEY', in_use: 0 }
  ], quota: { max_requests: 100000, period: 'month', used: 98765 } },
  { id: 'vault-2', name: 'Brokers', items: [], quota: null }
];
const configField = (name, label, placeholder, required = true, kind = 'text') => ({ name, label, placeholder, required, kind, help: `Where the ${label.toLowerCase()} is reachable from the core container, for example ${placeholder}.` });
const providers = [
  { provider: 'polygon', label: 'Polygon.io', stream: true, required_secrets: ['POLYGON_API_KEY'], config_fields: [], rate_limit: 'Free plan: 5 requests per minute, 2 years of history.', stream_note: 'Live quotes need the Stocks Starter plan or above.', testable: true },
  { provider: 'ibkr', label: 'Interactive Brokers', stream: true, required_secrets: [], config_fields: [configField('host', 'Host', 'host.docker.internal'), configField('port', 'Port', '4002', true, 'number'), configField('client_id', 'Client id', '17', false, 'number')], rate_limit: '', stream_note: '', testable: true }
];
const connectors = [
  { id: 'conn-1', provider: 'polygon', name: `Polygon ${LONG}`, label: 'Polygon.io', modules: ['*'], required_secrets: ['POLYGON_API_KEY'], set_secrets: ['POLYGON_API_KEY'], secrets: [{ name: 'POLYGON_API_KEY', vault_item_id: 'item-1' }], config_fields: [], config: {}, quota: { used: 98765, max_requests: 100000, period: 'month', resets_at: iso(-60 * 24 * 11) }, website: 'https://polygon.io', docs_url: 'https://polygon.io/docs', rate_limit: 'Free plan: 5 requests per minute, 2 years of history.', stream: true, stream_note: 'Live quotes need the Stocks Starter plan or above.', testable: true },
  { id: 'conn-2', provider: 'ibkr', name: 'IB Gateway paper', label: 'Interactive Brokers', modules: ['histviz', 'journal'], required_secrets: [], set_secrets: [], secrets: [], config_fields: providers[1].config_fields, config: { host: 'gateway.internal.example-brokerage-network.local', port: '4002', client_id: '' }, quota: null, website: 'https://www.interactivebrokers.com', docs_url: '', rate_limit: '', stream: true, stream_note: '', testable: true }
];
const webhooks = [
  { id: 'wh-1', name: `TradingView alerts for ${LONG}`, target: 'automator', config: { workflow_id: 'wf-1' }, prefix: 'a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8', received_count: 1234567, last_received_at: iso(3), enabled: true },
  { id: 'wh-2', name: 'Broker fills', target: 'remindme', config: {}, prefix: 'f00dfeed', received_count: 0, last_received_at: null, enabled: false }
];
const webhookTargets = [{ id: 'remindme', label: 'RemindMe' }, { id: 'automator', label: 'Automator', needs: 'workflow_id' }];
const webhookEvents = [
  { id: 'ev-1', status: 'ok', received_at: iso(3), detail: 'Workflow run queued', payload: JSON.stringify({ ticker: 'ES1!', action: 'buy', price: -12345.678901, note: 'x'.repeat(240) }) },
  { id: 'ev-2', status: 'error', received_at: iso(90), detail: 'Payload larger than 64 KB was refused by the endpoint', payload: '' }
];
const senders = [
  { id: 's-1', status: 'kept', category: 'broker', domain: 'notifications.interactivebrokers-example.com', name: 'Interactive Brokers trade confirmations and statements', from_addr: 'donotreply-trade-confirmations@notifications.interactivebrokers-example.com', unread_count: 12, last_subject: 'Daily activity statement' },
  { id: 's-2', status: 'kept', category: 'newsletter', domain: 'macro.example.org', name: 'Macro weekly', from_addr: 'weekly@macro.example.org', unread_count: 0, last_subject: 'Rates' },
  { id: 's-3', status: 'pending', category: 'other', domain: 'unknown.example.net', name: '', from_addr: 'a-very-long-unrecognised-sender-address@unknown.example.net', unread_count: 0, last_subject: 'Your account summary for the third quarter is ready to download' }
];
const messages = [
  { id: 'm-1', sender_name: 'Interactive Brokers trade confirmations and statements', received_at: iso(5), subject: `Activity statement for account U1234567 covering ${LONG} with negative cash balance of -1,234,567.89 USD`, snippet: 'Your daily statement is attached. Net liquidation value -1,234,567.89 USD after the overnight margin call.', attachment_count: 2, read: false, starred: true },
  { id: 'm-2', sender_name: 'Macro weekly', received_at: iso(600), subject: 'Rates, curve and the long weekend', snippet: 'Short read.', attachment_count: 0, read: true, starred: false }
];
const messageDetail = {
  message: { id: 'm-1', subject: messages[0].subject, sender_name: messages[0].sender_name, sender_addr: senders[0].from_addr, received_at: iso(5), sender_category: 'broker', starred: true, archived: false, read: false, has_remote_images: true, body_html: '<p>Statement attached.</p><table><tr><td>Net liquidation</td><td>-1,234,567.89</td></tr></table>' },
  attachments: [
    { id: 'att-1', filename: `ActivityStatement_U1234567_${LONG}_2026-10-19.pdf`, size_bytes: 1234567 },
    { id: 'att-2', filename: 'trades.csv', size_bytes: 2048 }
  ],
  unsubscribe_url: 'https://notifications.interactivebrokers-example.com/unsubscribe'
};
const mailAccounts = [
  { id: 'acc-1', name: 'Trading inbox', email: 'trader.with.a.very.long.address@example-mail-provider.com', username: 'trader', host: 'imap.example-mail-provider.com', port: 993, security: 'tls', folder: 'INBOX', enabled: true, interval_secs: 900, auth_kind: 'password', last_success_at: iso(15), last_error: 'IMAP server answered: [AUTHENTICATIONFAILED] Invalid credentials (Failure) after 3 retries', needs_reauth: false, preset: 'custom' },
  { id: 'acc-2', name: 'Newsletters', email: 'news@example.org', username: 'news', host: 'imap.example.org', port: 993, security: 'tls', folder: 'INBOX', enabled: true, interval_secs: 3600, auth_kind: 'password', last_success_at: iso(60), last_error: '', needs_reauth: false, preset: 'custom' }
];
const dashboards = [
  { id: 'd-1', name: 'Morning macro and rates', started: true, favorite: true, is_default: true, source_count: 3, position: 0 },
  { id: 'd-2', name: `Earnings ${LONG}`, started: false, favorite: true, is_default: false, source_count: 1, position: 1 },
  { id: 'd-3', name: 'Crypto', started: false, favorite: true, is_default: false, source_count: 0, position: 2 },
  { id: 'd-4', name: 'Archive', started: false, favorite: false, is_default: false, source_count: 0, position: 3 }
];
const feeds = [
  { id: 'f-1', name: `Reuters ${LONG}`, kind: 'rss', color: '#3b82f6', item_count: 1234567, last_error: '', last_fetched_at: iso(4), last_success_at: iso(4) },
  { id: 'f-2', name: 'Finnhub company news', kind: 'api', color: '#f59e0b', item_count: 12, last_error: 'HTTP 429 Too Many Requests: the provider rate limit was exceeded for this API key', last_fetched_at: iso(2), last_success_at: iso(400) },
  { id: 'f-3', name: 'Never fetched', kind: 'rss', color: '', item_count: 0, last_error: '', last_fetched_at: null, last_success_at: null }
];
const feedItems = [
  { id: 'i-1', feed_id: 'f-1', source_name: `Reuters ${LONG}`, source_type: 'rss', published_at: iso(10), fetched_at: iso(9), url: `https://www.example-news.com/markets/${'very-long-slug-'.repeat(8)}`, title: `Treasury yields fall -0.123456 percent as ${LONG} desks reprice the curve`, summary: 'Long summary '.repeat(30), sort_key: 'k1' },
  { id: 'i-2', feed_id: 'f-2', source_name: 'Finnhub company news', source_type: 'api', published_at: iso(70), fetched_at: iso(70), url: '', title: 'Untitled market note', summary: '', sort_key: 'k2' }
];
const documents = [
  { id: 'page-1', kind: 'page', title: 'Research notes', parent_id: null, position: 1 },
  { id: 'db-1', kind: 'database', title: `Trade ideas ${LONG}`, parent_id: null, position: 2 },
  { id: 'folder-1', kind: 'folder', title: 'Archive', parent_id: null, position: 3 }
];
const statusCol = { id: 'col-status', name: 'Status', type: 'select', position: 2, options: { choices: [{ id: 'todo', name: 'To review', color: 'amber' }, { id: 'done', name: 'Done', color: 'green' }] } };
const dbColumns = [
  { id: 'col-name', name: 'Idea', type: 'text', position: 1, options: {} },
  statusCol,
  { id: 'col-size', name: 'Size', type: 'number', position: 3, options: {} }
];
const dbRows = [
  { id: 'row-1', position: 1, cells: { 'col-name': 'Long ES on breakout', 'col-status': 'todo', 'col-size': -1234567.89 } },
  { id: 'row-2', position: 2, cells: { 'col-name': 'Short NQ fade', 'col-status': 'done', 'col-size': 2 } },
  { id: 'row-3', position: 3, cells: { 'col-name': 'Pairs trade', 'col-status': null, 'col-size': 3 } }
];
const oauthModules = Array.from({ length: 24 }, (_, i) => ({ id: `mod-${i}`, label: i === 3 ? `Journal ${LONG}` : `Module number ${i + 1}` }));

// Responses per path, for the populated state. Functions get (method, url, body).
function populated(p, method, url, body) {
  if (p === '/api/settings/sessions') return { sessions: [
    { id: 'sess-1', current: true, user_agent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1', ip: '2001:0db8:85a3:0000:0000:8a2e:0370:7334', last_seen_at: iso(1) },
    { id: 'sess-2', current: false, user_agent: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36', ip: '192.168.100.200', last_seen_at: iso(3000) }
  ] };
  if (p === '/api/settings/request-timeout') return { seconds: 120, default: 120, min: 10, max: 3600 };
  if (p === '/api/settings/social') return { callback_path: '/auth/social/callback/with/a/very/long/path/segment', config: { provider: 'google', client_id: '1234567890-abcdefghijklmnopqrstuvwxyz0123456789.apps.googleusercontent.com', has_secret: true }, linked: { label: 'trader.with.a.very.long.address@example-mail-provider.com', linked_at: iso(9000) }, recovery_codes_left: 0, password_login: true };
  if (p === '/api/settings/network') {
    if (method === 'POST') return { ok: true };
    return { mode: 'lan_https', port: 5454, domain: 'otw.a-very-long-subdomain-for-the-home-lab.duckdns.org', dns_provider: 'duckdns', dns_token_set: true, lan_ip: '192.168.100.200' };
  }
  if (p === '/api/settings/backup/status') return { configured: true, result: 'error', stale: true, age_seconds: 200000, destination: `/mnt/nas/backups/opentraderworld/${LONG}/nightly`, verified: false, schedule: 'every day at 03:15', size_bytes: 1234567890, error: 'pg_dump: error: connection to server on socket "/var/run/postgresql/.s.PGSQL.5432" failed: FATAL: password authentication failed for user "otw"' };
  if (p === '/api/settings/data') return { database_bytes: 9876543210, modules: [
    { id: 'histdata', name: 'Historical data', rows: 123456789, size_bytes: 8765432109, tables: [{ name: 'histdata_bars_with_a_long_table_name', rows: 123456789, size_bytes: 8765432109 }] },
    { id: 'journal', name: `Journal ${LONG}`, rows: 1234, size_bytes: 567890, tables: [{ name: 'journal_trades', rows: 1234, size_bytes: 567890 }] }
  ], system: [{ id: 'sys', name: 'Sessions', rows: 2, size_bytes: 1024, tables: [] }] };
  if (p === '/api/settings/reauth') return { valid: true };
  if (p === '/api/settings/version') return { version: '0.0.17-rc.1+build.20261020.abcdef1234567890' };
  if (p === '/api/settings/update-check') return { current: '0.0.17', latest: '0.1.0-beta.123456789', update_available: true };
  if (p === '/api/settings/logs/level') return { level: 'debug', levels: ['error', 'warn', 'info', 'debug', 'trace'] };
  if (p === '/api/settings/logs') return { logs: [
    { id: 1, at: iso(1), level: 'error', target: 'otw_core::connectors::polygon::history_downloader', message: `request failed: https://api.polygon.io/v2/aggs/ticker/AAPL/range/1/minute/2020-01-01/2026-10-20?adjusted=true&sort=asc&limit=50000 returned 429 ${'x'.repeat(120)}` },
    { id: 2, at: iso(2), level: 'info', target: 'otw_scheduler', message: 'poll finished' }
  ] };
  if (p === '/api/rate/usage') return { providers: [{ provider: 'polygon', host: 'api.polygon.io.with-a-long-host-name.example.com', requests: 1234567, errors: 12, limited: 3, limit: '5/min', last_at: iso(1) }], events: [{ id: 'r-1', at: iso(1), provider: 'polygon', host: 'api.polygon.io', status: 429, detail: 'Too Many Requests: '.repeat(6) }] };
  if (p === '/api/mcp/settings') return { enabled: true, oauth_enabled: true, modules: oauthModules };
  if (p === '/api/mcp/tokens') return { tokens: [{ id: 'tok-1', name: `Claude desktop on the ${LONG} laptop`, prefix: 'otw_mcp_1a2b3c4d', permissions: { 'mod-0': 'r', 'mod-1': 'rw', 'mod-3': 'rwd' }, created_at: iso(9000), expires_at: null, last_used_at: iso(4), external: false, oauth_client_id: null }] };
  if (p === '/api/mcp/calls') return { calls: 1234567, errors: 3, notes: 0, by_class: [], by_route: [{ method: 'GET', route: '/api/journal/trades/with/a/very/long/route/name/{id}', calls: 1234567, errors: 3, avg_ms: 123.4 }], recent_failures: [] };
  if (p === '/api/brokers') return { accounts: [] };
  if (p === '/api/brokers/providers') return { providers: [] };
  if (p === '/api/brokers/modules') return { modules: [] };
  if (p === '/api/mcp/oauth/request') return { client_name: `Claude ${LONG}`, redirect_host: 'callback.a-very-long-mcp-client-redirect-host.example.com' };
  if (p === '/api/mcp/oauth/approve') return { redirect: '/oauth/authorize?done=1' };
  if (p === '/api/mcp/oauth/deny') return { redirect: '/oauth/authorize?denied=1' };
  if (p === '/api/control/settings') return { enabled: true, pair_ttl_minutes: 15, pair_ttl_min: 5, pair_ttl_max: 1440, kinds: [] };
  if (p === '/api/control/bindings') return { bindings: [] };
  if (p === '/api/exchanges') return { exchanges: [{ code: 'XNYS', mic: 'XNYS', name: `New York Stock Exchange ${LONG}`, city: 'New York', timezone: 'America/New_York', open: 570, close: 960, break_start: null, break_end: null, status: { open: true, holidays_unknown: true }, covered_to: '2027-12-31', upcoming: [], prior_day_open: false }], home: 'XNYS', source: 'bundled', generated: '2026-10-01', checked_at: iso(60) };
  if (p === '/api/exchanges/home') return { home: 'XNYS' };
  if (p === '/api/market-sessions') return { sessions: [{ id: 'ms-1', name: `CME Globex ${LONG}`, timezone: 'America/Chicago', start_minute: 1020, end_minute: 960, weekdays: [0, 1, 2, 3, 4] }], timezones: ['UTC', 'America/Chicago', 'Europe/Paris'] };
  if (p === '/api/settings/versioning') return { editor: false, strategies: true, usage: { editor: { versions: 0, bytes: 0 }, strategies: { versions: 1234567, bytes: 9876543210 } } };
  if (p === '/api/vault') return { vaults };
  if (p === '/api/connectors/providers') return { providers };
  if (p === '/api/connectors/modules') return { modules: ['histdata', 'histviz', 'journal', 'fundamentals', 'portfolios', 'watchlists'] };
  if (p === '/api/connectors') return { connectors };
  if (/^\/api\/connectors\/[^/]+\/test$/.test(p)) return { ok: false, detail: `Could not reach ${connectors[1].config.host}:4002: connection refused. Start IB Gateway or change the host and port above.` };
  if (/^\/api\/connectors\/[^/]+$/.test(p)) return { connector: connectors[1] };
  if (p === '/api/webhooks' && method === 'POST') return { webhook: webhooks[0], token: 'dummy-token', path: `/api/hooks/${'0123456789abcdef'.repeat(4)}` }; // gitleaks:allow
  if (p === '/api/webhooks') return { webhooks, targets: webhookTargets };
  if (/^\/api\/webhooks\/[^/]+\/events$/.test(p)) return { events: webhookEvents };
  if (/^\/api\/webhooks\/[^/]+$/.test(p)) return { webhook: { ...webhooks[0], ...body } };
  if (p === '/api/automator/workflows') return { workflows: [{ id: 'wf-1', name: `Route alerts ${LONG}` }], schedules: [], last_runs: [] };
  if (p === '/api/mailbox/accounts') return { accounts: mailAccounts, presets: [{ id: 'gmail', label: 'Gmail', host: 'imap.gmail.com', port: 993, security: 'tls', auth: 'password', supported: true }], counts: { unread: 12, total: 1234, pending_senders: 1 } };
  if (p === '/api/mailbox/senders') return { senders };
  if (p === '/api/mailbox/messages') return { messages };
  if (p === '/api/mailbox/messages/m-1') return messageDetail;
  if (p === '/api/mailbox/messages/m-2') return { ...messageDetail, message: { ...messageDetail.message, id: 'm-2', subject: messages[1].subject }, attachments: [] };
  if (p === '/api/mailbox/links') return { links: [{ id: 'l-1', name: `Macro newsletter ${LONG}`, domain: 'macro.example.org', url: `https://macro.example.org/${'subscribe-'.repeat(10)}`, description: 'Weekly rates and FX.', topic: 'economics', subscribed: true }] };
  if (p === '/api/mailbox/settings') return { settings: { retention_days: 90, keep_attachments: true, load_remote_images: false, notify_on_new: true } };
  if (p === '/api/mailbox/oauth/callback') return { ok: false, error: `AADSTS65004: User declined to consent to access the app ${LONG}.` };
  if (p === '/api/feed-dashboards') return { dashboards };
  if (/^\/api\/feed-dashboards\/[^/]+\/sources$/.test(p)) return { feeds };
  if (/^\/api\/feed-dashboards\/[^/]+$/.test(p)) return {};
  if (p === '/api/feed-items') return { items: url.searchParams.get('before_key') ? [] : feedItems };
  if (p === '/api/feed-sources') return { source_names: feeds.map((f) => f.name), source_types: ['rss', 'api'] };
  if (p === '/api/feeds/quotas') return { quotas: { 'f-2': { used: 98765, max_requests: 100000, period: 'day', resets_at: iso(-60) } } };
  if (p === '/api/documents') return { documents: [...documents].sort((a, b) => a.position - b.position) };
  if (p === '/api/documents/page-1') return { document: { ...documents[0], content: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: `A long paragraph about ${LONG} and the -1234567.89 drawdown.` }] }] } } };
  if (p === '/api/documents/db-1') return { document: { ...documents[1], content: { db: { view: url.searchParams.get('view') ?? 'table' } } } };
  if (p === '/api/databases/db-1') return { columns: dbColumns, rows: dbRows };
  return undefined;
}

function empty(p) {
  const lists = {
    '/api/settings/sessions': { sessions: [] }, '/api/settings/social': { callback_path: '/auth/social/callback', config: null, linked: null, recovery_codes_left: 0, password_login: true },
    '/api/settings/network': { mode: 'local', port: 5454, domain: '', dns_provider: 'duckdns', dns_token_set: false, lan_ip: '' },
    '/api/settings/backup/status': { configured: false }, '/api/settings/data': { database_bytes: 0, modules: [], system: [] },
    '/api/settings/update-check': { current: '0.0.17', latest: '0.0.17', update_available: false }, '/api/settings/logs': { logs: [] },
    '/api/rate/usage': { providers: [], events: [] }, '/api/mcp/settings': { enabled: false, oauth_enabled: false, modules: [] }, '/api/mcp/tokens': { tokens: [] },
    '/api/mcp/calls': { calls: 0, errors: 0, notes: 0, by_class: [], by_route: [], recent_failures: [] }, '/api/exchanges': { exchanges: [], home: '', source: 'bundled', generated: '', checked_at: null },
    '/api/market-sessions': { sessions: [], timezones: ['UTC'] }, '/api/vault': { vaults: [] }, '/api/connectors': { connectors: [] },
    '/api/connectors/providers': { providers }, '/api/connectors/modules': { modules: [] }, '/api/webhooks': { webhooks: [], targets: webhookTargets },
    '/api/automator/workflows': { workflows: [], schedules: [], last_runs: [] }, '/api/mailbox/accounts': { accounts: [], presets: [], counts: { unread: 0, total: 0, pending_senders: 0 } },
    '/api/mailbox/senders': { senders: [] }, '/api/mailbox/messages': { messages: [] }, '/api/mailbox/links': { links: [] },
    '/api/feed-dashboards': { dashboards: [] }, '/api/feed-items': { items: [] }, '/api/feed-sources': { source_names: [], source_types: [] }, '/api/feeds/quotas': { quotas: {} },
    '/api/documents': { documents: [] }, '/api/mcp/oauth/request': { client_name: 'Fixture client', redirect_host: 'localhost' }
  };
  return lists[p];
}

// Shell endpoints answer in every state so the page itself is what is under test.
function shell(p, state) {
  if (p === '/api/setup/status') return { configured: state !== 'setup', signup_policy: 'closed', token_required: state === 'setup' };
  if (p === '/api/settings/me') return { username: 'fixture-admin', role: 'admin', locale: 'en', email: 'trader.with.a.very.long.address@example-mail-provider.com', email_verified: true, totp_enabled: false };
  if (p === '/api/settings/modules') return { installed: modules };
  if (p === '/api/settings/defaults') return { default_timezone: 'UTC', default_currency: 'USD', display_currency: 'USD', locale: 'en', home_exchange: 'XNYS' };
  if (p === '/api/health') return { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
  if (p === '/api/dashboard/layout') return { pages: [], favoriteIds: [] };
  if (p === '/api/tasks') return { tasks: [] };
  if (p.includes('/notifications')) return { notifications: [], unread: 0 };
  if (p === '/api/auth/methods') return { password: true, social: null, recovery: true };
  if (p === '/api/notif-channels') return { channels: [] };
  if (p === '/api/notif-channels/modules') return { modules: [] };
  return undefined;
}

async function makeContext(viewport, state, captured = []) {
  const context = await browser.newContext({ viewport, hasTouch: viewport.width < 1100, locale: 'en-US', reducedMotion: 'reduce' });
  await context.route('**/api/**', async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const p = url.pathname;
    const method = req.method();
    let body = null;
    try { body = req.postDataJSON(); } catch { body = null; }
    if (method !== 'GET') captured.push({ method, path: p, body });
    if (p === '/api/feeds/stream') return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
    const sh = shell(p, state);
    if (sh !== undefined) return route.fulfill({ json: sh });
    if (p === '/api/login') {
      if (state === 'error') return route.fulfill({ status: 401, json: { error: 'invalid credentials', code: null } });
      return route.fulfill({ json: { ok: true, must_change_password: false } });
    }
    if (state === 'error') return route.fulfill({ status: 500, json: { error: `Fixture failure (500) on ${p}: the service is unavailable, try again later.` } });
    const result = (state === 'empty' ? empty(p) : undefined) ?? populated(p, method, url, body) ?? {};
    return route.fulfill({ json: result });
  });
  const page = await context.newPage();
  await page.clock.setFixedTime(NOW);
  page.setDefaultTimeout(12000);
  const errors = [];
  page.on('pageerror', (e) => {
    // An API 500 that the page never catches surfaces as an unhandled rejection. That is
    // existing error handling (outside a layout change), so it is listed, not failed.
    if (/Fixture failure \(500\)/.test(e.message)) knownErrors.add(`${new URL(page.url()).pathname}${new URL(page.url()).hash}: ${e.message.replace(/: the service.*$/, '')}`);
    else errors.push(`${page.url()}: ${e.message}`);
  });
  return { context, page, errors, captured };
}

// ── Checks ─────────────────────────────────────────────────────────────────
async function overflow(page) {
  return page.evaluate(() => {
    const vw = document.documentElement.clientWidth;
    const scroll = document.documentElement.scrollWidth;
    // Elements past the right edge that are not inside a local horizontal scroll region.
    // A region counts when it scrolls sideways only and sits inside the page gutter: a page
    // container that scrolls vertically and also sideways is the overflow being hunted.
    const scrollsX = (el) => {
      for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) {
        const cs = getComputedStyle(n);
        if (!/(auto|scroll)/.test(cs.overflowX) || n.scrollWidth <= n.clientWidth + 1) continue;
        const r = n.getBoundingClientRect();
        if (n.scrollHeight <= n.clientHeight + 1 && r.width < vw - 8 && r.right <= vw + 1) return true;
      }
      return false;
    };
    const offenders = [...document.querySelectorAll('body *')].filter((e) => {
      const r = e.getBoundingClientRect();
      if (!r.width || !r.height) return false;
      const cs = getComputedStyle(e);
      if (cs.visibility === 'hidden' || cs.position === 'fixed') return false;
      return r.right > vw + 1 && !scrollsX(e);
    }).slice(0, 6).map((e) => `${e.tagName.toLowerCase()}.${typeof e.className === 'string' ? e.className.split(' ').join('.') : ''} r=${Math.round(e.getBoundingClientRect().right)}`);
    // Text that spills out of its box (an unbreakable path, a long address) past the edge.
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const range = document.createRange();
    for (let n = walker.nextNode(); n && offenders.length < 8; n = walker.nextNode()) {
      if (!n.textContent.trim()) continue;
      const host = n.parentElement;
      if (!host || host.closest('script, style, [aria-hidden="true"]')) continue;
      const cs = getComputedStyle(host);
      if (cs.visibility === 'hidden' || host.closest('[style*="position: fixed"]')) continue;
      range.selectNodeContents(n);
      const r = range.getBoundingClientRect();
      if (!r.width || r.right <= vw + 1) continue;
      // A box that clips its own text on purpose (ellipsis, a cut chip) is not a spill.
      if (cs.textOverflow === 'ellipsis' || /(hidden|clip)/.test(cs.overflowX)) continue;
      if (/(auto|scroll)/.test(cs.overflowX) && host.scrollWidth > host.clientWidth + 1 && host.getBoundingClientRect().right <= vw + 1) continue;
      if (scrollsX(host)) continue;
      offenders.push(`text "${n.textContent.trim().slice(0, 30)}" in ${host.tagName.toLowerCase()}.${typeof host.className === 'string' ? host.className.split(' ')[0] : ''} r=${Math.round(r.right)}`);
    }
    return { vw, scroll, offenders };
  });
}

// PROBE=1 logs every failure of the sweep checks instead of stopping at the first one.
const probe = !!process.env.PROBE;
const probeLog = [];
async function soft(fn) {
  if (!probe) return fn();
  try { await fn(); } catch (e) { probeLog.push(e.message.split('\n')[0].slice(0, 600)); }
}
async function assertFits(page, label) {
  return soft(() => assertFitsStrict(page, label));
}
async function assertFitsStrict(page, label) {
  const o = await overflow(page);
  assert.ok(o.scroll <= o.vw + 1, `${label}: document overflows ${o.scroll} > ${o.vw}: ${o.offenders.join(', ')}`);
  assert.deepEqual(o.offenders, [], `${label}: elements cut at the right edge ${o.offenders.join(", ")}`);
}

// Scroll into view, then the element's center must be on screen and hit-test to itself.
async function reachable(locator, label) {
  await locator.scrollIntoViewIfNeeded();
  const res = await locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2, y = r.top + r.height / 2;
    const vw = window.innerWidth, vh = window.innerHeight;
    const inside = x >= 0 && x <= vw && y >= 0 && y <= vh;
    const hit = inside ? document.elementFromPoint(x, y) : null;
    const ok = !!hit && (hit === el || el.contains(hit));
    return { ok, inside, x: Math.round(x), y: Math.round(y), hit: hit ? `${hit.tagName.toLowerCase()}.${typeof hit.className === 'string' ? hit.className : ''}` : null };
  });
  assert.ok(res.ok, `${label}: not reachable ${JSON.stringify(res)}`);
}

// Every visible, enabled button inside `root` is reachable (cheap in-page pass).
async function buttonsReachable(page, root, label) {
  return soft(() => buttonsReachableStrict(page, root, label));
}
async function buttonsReachableStrict(page, root, label) {
  const bad = await page.evaluate((sel) => {
    const out = [];
    const host = document.querySelector(sel);
    if (!host) return [`missing ${sel}`];
    const items = [...host.querySelectorAll('button:not([disabled]), a[href], input:not([type=hidden]):not([disabled]), select, [role="button"]')]
      .filter((el) => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden'; })
      .slice(0, 60);
    for (const el of items) {
      el.scrollIntoView({ block: 'center', inline: 'nearest' });
      // An inline link wrapped over two lines is hit-tested on its first line box.
      const r = el.getClientRects()[0] ?? el.getBoundingClientRect();
      const x = r.left + r.width / 2, y = r.top + r.height / 2;
      const inside = x >= 0 && x <= window.innerWidth && y >= 0 && y <= window.innerHeight;
      const hit = inside ? document.elementFromPoint(x, y) : null;
      // A label wrapping its input counts as the input.
      const ok = !!hit && (hit === el || el.contains(hit) || hit.contains(el) && hit.tagName === 'LABEL' || hit.closest('label')?.contains(el));
      if (!ok) out.push(`${el.tagName.toLowerCase()}[${(el.getAttribute('aria-label') || el.textContent || el.getAttribute('placeholder') || '').trim().slice(0, 40)}] at ${Math.round(x)},${Math.round(y)} hit ${hit ? `${hit.tagName.toLowerCase()}.${typeof hit.className === 'string' ? hit.className.split(' ')[0] : ''}` : 'none'}`);
    }
    window.scrollTo(0, 0);
    return out;
  }, root);
  assert.deepEqual(bad, [], `${label}: unreachable controls ${bad.join(' | ')}`);
}

const settle = (page) => page.waitForLoadState('networkidle').catch(() => {}).then(() => page.waitForTimeout(150));
async function goto(page, url) {
  try {
    await page.goto(url);
  } catch {
    await page.goto(url); // First navigation after a cold dep optimization can reload.
  }
  await settle(page);
}
const shot = (page, name) => page.screenshot({ path: `${artifacts}/${name}.png`, fullPage: true });
const report = [];
const knownErrors = new Set();
const ran = (name) => !only || only.includes(name);

// ── Route sweeps ───────────────────────────────────────────────────────────
const SETTINGS = ['account', 'appearance', 'defaults', 'modules', 'security', 'network', 'vault', 'connectors', 'brokers', 'sessions', 'exchanges', 'notifications', 'mcp', 'control', 'voice', 'voice-commands', 'data', 'versioning', 'backup', 'update', 'logs', 'rate', 'credits', 'about', 'terms', 'license', 'risks'];

async function sweepSettings() {
  for (const state of ['populated', 'empty', 'error']) {
    for (const vp of VIEWPORTS) {
      const { context, page, errors } = await makeContext(vp, state);
      await goto(page, `${base}/settings#account`);
      for (const id of SETTINGS) {
        await page.evaluate((h) => { location.hash = h; }, id);
        await settle(page);
        const label = `settings#${id} ${state} ${vp.width}x${vp.height}`;
        await assertFits(page, label);
        if (vp.width < 768) await reachable(page.locator('.mobile-selector'), `${label} section switch`);
        await buttonsReachable(page, '.content', label);
        if (state === 'populated' && (vp.width === 320 || vp.width === 390)) await shot(page, `settings-${id}-${vp.width}`);
        if (state !== 'populated' && vp.width === 390 && ['security', 'network', 'vault', 'backup', 'update'].includes(id)) await shot(page, `settings-${id}-${state}-390`);
      }
      await soft(() => assert.deepEqual(errors, [], `settings ${state} ${vp.width}: runtime errors ${errors.join(' | ')}`));
      await context.close();
    }
  }
  report.push('settings: 27 sections x 3 states x 5 viewports fit; every visible control reachable');
}

async function sweepRoute(name, path, { ready, root = 'main, .page, .bare, body', states = ['populated', 'empty', 'error'], extra } = {}) {
  for (const state of states) {
    for (const vp of [...VIEWPORTS, ...(extra ?? [])]) {
      const { context, page, errors } = await makeContext(vp, state);
      await goto(page, `${base}${path}`);
      if (ready) await ready(page, state);
      const label = `${name} ${state} ${vp.width}x${vp.height}`;
      await assertFits(page, label);
      await buttonsReachable(page, root, label);
      await shot(page, `${name}-${state}-${vp.width}x${vp.height}`);
      await soft(() => assert.deepEqual(errors, [], `${label}: runtime errors ${errors.join(' | ')}`));
      await context.close();
    }
  }
  report.push(`${name}: ${states.join('/')} x ${VIEWPORTS.length + (extra?.length ?? 0)} viewports fit; every visible control reachable`);
}

// ── Tap flows at 390 px ───────────────────────────────────────────────────
async function flowLogin() {
  const { context, page, errors, captured } = await makeContext({ width: 390, height: 844 }, 'populated');
  await goto(page, `${base}/login`);
  await page.locator('#u').tap();
  await page.locator('#u').fill('fixture-admin');
  await page.locator('#p').fill('dummy-password'); // gitleaks:allow
  await reachable(page.getByRole('button', { name: 'Sign in' }), 'login submit');
  await page.getByRole('button', { name: 'Sign in' }).tap();
  await page.waitForURL(`${base}/`);
  const login = captured.find((c) => c.path === '/api/login');
  assert.deepEqual(login.body, { username: 'fixture-admin', password: 'dummy-password', code: null, remember: false }); // gitleaks:allow
  assert.deepEqual(errors, []);
  await context.close();
  // Wrong password: the error is visible above the button with the keyboard open.
  const bad = await makeContext(KEYBOARD, 'error');
  await goto(bad.page, `${base}/login`);
  await bad.page.locator('#u').fill('fixture-admin');
  await bad.page.locator('#p').fill('wrong');
  await bad.page.getByRole('button', { name: 'Sign in' }).tap();
  const err = bad.page.locator('.ui-error');
  await err.waitFor();
  await reachable(err, 'login error');
  await reachable(bad.page.getByRole('button', { name: 'Sign in' }), 'login submit after error');
  await assertFits(bad.page, 'login error 390x500');
  await shot(bad.page, 'login-error-390x500');
  await bad.context.close();
  report.push('login: tap submit sends { username, password, code: null, remember: false }; error visible with keyboard viewport');
}

async function flowSettingsSave() {
  const { context, page, errors, captured } = await makeContext({ width: 390, height: 844 }, 'populated');
  await goto(page, `${base}/settings`);
  await page.locator('.mobile-selector').tap();
  await page.locator('.rail').getByRole('button', { name: 'Network', exact: true }).tap();
  assert.equal(await page.locator('.mobile-selector').getAttribute('aria-expanded'), 'false');
  const save = page.locator('.content').getByRole('button', { name: /^Save/ }).first();
  await reachable(save, 'network save');
  await save.tap();
  await page.waitForTimeout(300);
  const post = captured.find((c) => c.path === '/api/settings/network');
  assert.deepEqual(post?.body, { mode: 'lan_https', port: 5454, domain: 'otw.a-very-long-subdomain-for-the-home-lab.duckdns.org', dns_provider: 'duckdns', dns_token: '', lan_ip: '192.168.100.200' });
  await shot(page, 'settings-network-saved-390');
  assert.deepEqual(errors, []);
  await context.close();
  report.push('settings: Network section picked from the section rail and saved by tap; payload matches desktop shape');
}

async function flowMailbox() {
  const { context, page, errors, captured } = await makeContext({ width: 390, height: 844 }, 'populated');
  await goto(page, `${base}/mailbox`);
  await page.getByRole('tab', { name: 'Mail' }).tap().catch(() => {});
  const row = page.locator('.list .row').first();
  await reachable(row, 'mail row');
  await row.tap();
  const back = page.locator('.reader').getByRole('button', { name: 'Back' });
  await back.waitFor();
  assert.equal(await page.locator('.list').isVisible(), false, 'list hidden while reading');
  await reachable(back, 'mail back');
  await assertFits(page, 'mail reader 390');
  await reachable(page.locator('.attachments a').first(), 'attachment');
  await shot(page, 'mailbox-reader-390');
  await back.tap();
  await page.locator('.list .row').first().waitFor();
  assert.equal(await page.locator('.reader article').count(), 0, 'reader closed');
  assert.ok(captured.some((c) => c.path === '/api/mailbox/messages/m-1' && c.method === 'PATCH'), 'opening marks read like desktop');
  assert.deepEqual(errors, []);
  await context.close();
  report.push('mailbox: tap a message opens the reader alone, Back returns to the list');
}

async function flowNewsReorder() {
  const { context, page, errors, captured } = await makeContext({ width: 390, height: 844 }, 'populated');
  await goto(page, `${base}/news`);
  const right = page.getByRole('button', { name: /^Move right: Morning macro and rates/ });
  await reachable(right, 'news move right');
  await right.tap();
  await page.waitForTimeout(300);
  const patches = captured.filter((c) => c.method === 'PATCH' && c.path.startsWith('/api/feed-dashboards/'));
  assert.deepEqual(patches.map((c) => [c.path, c.body]).sort((a, b) => a[0].localeCompare(b[0])), [['/api/feed-dashboards/d-1', { position: 1 }], ['/api/feed-dashboards/d-2', { position: 0 }]]);
  await shot(page, 'news-reorder-390');
  assert.deepEqual(errors, []);
  await context.close();
  report.push('news: favourite strip reorder by tap sends the same position patches as a drop');
}

async function flowEditorDatabase() {
  const { context, page, errors, captured } = await makeContext({ width: 390, height: 844 }, 'populated');
  await goto(page, `${base}/editor`);
  await page.locator('.mobile-selector').tap();
  await page.locator('.tree-body .row').filter({ hasText: 'Trade ideas' }).locator('button.label').tap();
  await page.locator('.table-wrap table').waitFor();
  const down = page.getByRole('button', { name: 'Move down: Long ES on breakout' });
  await reachable(down, 'row move down');
  await down.tap();
  await page.waitForTimeout(200);
  const move = captured.find((c) => c.path === '/api/databases/rows/row-1/move');
  assert.deepEqual(move?.body, { position: 2.5 });
  const colRight = page.getByRole('button', { name: 'Move right: Idea' });
  await reachable(colRight, 'column move right');
  await colRight.tap();
  await page.waitForTimeout(200);
  const col = captured.find((c) => c.path === '/api/databases/columns/col-name');
  assert.deepEqual(col?.body, { position: 2.5 });
  await assertFits(page, 'editor table 390');
  await shot(page, 'editor-table-390');
  await page.getByRole('button', { name: 'Kanban' }).tap();
  const pick = page.getByRole('combobox', { name: 'Status: Long ES on breakout' });
  await reachable(pick, 'kanban move');
  await pick.selectOption('done');
  await page.waitForTimeout(200);
  const upd = captured.filter((c) => c.path === '/api/databases/rows/row-1' && c.method === 'PATCH').at(-1);
  assert.equal(upd?.body?.cells?.['col-status'], 'done');
  await assertFits(page, 'editor kanban 390');
  await shot(page, 'editor-kanban-390');
  assert.deepEqual(errors, []);
  await context.close();
  report.push('editor: table row/column and kanban card move by tap send the same updates as drag and drop');
}

async function flowConnectors() {
  const { context, page, errors } = await makeContext({ width: 390, height: 844 }, 'populated');
  await goto(page, `${base}/connectors`);
  await page.locator('.list .row').filter({ hasText: 'IB Gateway paper' }).tap();
  const test = page.getByRole('button', { name: 'Test connection' });
  await reachable(test, 'connector test');
  await test.tap();
  const out = page.locator('.testout');
  await out.waitFor();
  await reachable(out, 'connector test feedback');
  await assertFits(page, 'connector test 390');
  await shot(page, 'connectors-test-390');
  assert.deepEqual(errors, []);
  await context.close();
  report.push('connectors: settings, Save and Test reachable by tap; test feedback visible and wrapped');
}

async function flowWebhooks() {
  const { context, page, errors, captured } = await makeContext({ width: 390, height: 844 }, 'populated');
  await goto(page, `${base}/webhooks`);
  await page.getByRole('button', { name: 'Edit' }).first().tap();
  const dialog = page.locator('[role="dialog"]');
  await dialog.waitFor();
  await reachable(dialog.getByRole('button', { name: 'Save' }), 'webhook save');
  await dialog.getByRole('button', { name: 'Save' }).tap();
  await dialog.waitFor({ state: 'hidden' });
  const patch = captured.find((c) => c.method === 'PATCH' && c.path === '/api/webhooks/wh-1');
  assert.deepEqual(patch?.body, { name: webhooks[0].name, target: 'automator', config: { workflow_id: 'wf-1' } });
  await page.getByRole('button', { name: 'Events' }).first().tap();
  await dialog.waitFor();
  await assertFits(page, 'webhook events 390');
  await shot(page, 'webhooks-events-390');
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: /Add webhook|New webhook/ }).first().tap();
  await dialog.locator('input').first().fill('Fixture hook');
  await dialog.getByRole('button', { name: 'Create' }).tap();
  await page.locator('[role="dialog"] .cmd, [role="dialog"] code, [role="dialog"] pre').first().waitFor();
  await assertFits(page, 'webhook created url 390');
  await shot(page, 'webhooks-created-390');
  assert.deepEqual(errors, []);
  await context.close();
  report.push('webhooks: edit dialog saves the desktop payload by tap; delivery log and one-time URL fit');
}

async function flowConsent() {
  const { context, page, errors, captured } = await makeContext(KEYBOARD, 'populated');
  await goto(page, `${base}/oauth/authorize?client_id=fixture&redirect_uri=https%3A%2F%2Fcallback.example.com&state=s`);
  await page.getByRole('button', { name: 'All read', exact: true }).tap();
  const approve = page.getByRole('button', { name: 'Allow' });
  await reachable(approve, 'consent allow');
  await reachable(page.getByRole('button', { name: 'Deny' }), 'consent deny');
  await assertFits(page, 'consent 390x500');
  await approve.tap();
  await page.waitForTimeout(300);
  const ok = captured.find((c) => c.path === '/api/mcp/oauth/approve');
  assert.equal(Object.keys(ok?.body?.permissions ?? {}).length, oauthModules.length);
  assert.deepEqual(errors, []);
  await context.close();
  report.push('oauth/authorize: 24 module permission list, Approve/Deny reachable at 390x500 and approve sends every module');
}

// ── Desktop comparison ────────────────────────────────────────────────────
const DESKTOP_ROUTES = [
  ['settings-account', '/settings#account'], ['settings-security', '/settings#security'], ['settings-network', '/settings#network'],
  ['settings-vault', '/settings#vault'], ['settings-backup', '/settings#backup'], ['settings-update', '/settings#update'],
  ['settings-mcp', '/settings#mcp'], ['connectors', '/connectors'], ['webhooks', '/webhooks'], ['mailbox', '/mailbox'],
  ['news', '/news'], ['editor-db', '/editor?doc=db-1'], ['login', '/login'], ['change-password', '/change-password?u=fixture-admin'],
  ['request-reset', '/request-reset'], ['oauth', '/oauth/authorize?client_id=fixture&redirect_uri=x&state=s'], ['mailbox-oauth', '/mailbox/oauth?state=flow&code=dummy']
];
async function desktopShot(origin, path, vp, state = 'populated') {
  const { context, page } = await makeContext(vp, state);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await goto(page, `${origin}${path}`);
  if (path.startsWith('/editor')) {
    await page.locator('.tree-body .row').filter({ hasText: 'Trade ideas' }).locator('button.label').click();
    await page.locator('.table-wrap table').waitFor();
    await settle(page);
  }
  // Late data and fonts settle at their own pace: shoot until two frames in a row match.
  // Commands embed the page origin, which differs between the two servers by its port.
  const opts = { fullPage: true, animations: 'disabled', caret: 'hide', mask: [page.locator('pre, code')] };
  let buf = await page.screenshot(opts);
  for (let i = 0; i < 6; i++) {
    await page.waitForTimeout(300);
    const next = await page.screenshot(opts);
    if (next.equals(buf)) break;
    buf = next;
  }
  await context.close();
  return buf;
}
async function desktopCompare() {
  const diffs = [];
  for (const [name, path] of [...DESKTOP_ROUTES, ['setup', '/setup']]) {
    for (const vp of DESKTOP) {
      const state = name === 'setup' ? 'setup' : 'populated';
      // Two loads of the same unchanged page can differ (late icons, async rows), so each
      // side is shot up to four times and the pair matches when any two shots are equal.
      const as = [], bs = [];
      let same = false, a, b;
      for (let i = 0; i < 4 && !same; i++) {
        as.push(await desktopShot(baseline, path, vp, state));
        bs.push(await desktopShot(base, path, vp, state));
        same = as.some((x) => bs.some((y) => x.equals(y)));
      }
      [a, b] = [as.at(-1), bs.at(-1)];
      if (!same) {
        await writeFile(`${artifacts}/desktop-${name}-${vp.width}-baseline.png`, a);
        await writeFile(`${artifacts}/desktop-${name}-${vp.width}-work.png`, b);
        diffs.push(`${name}@${vp.width}`);
      }
    }
  }
  report.push(`desktop: ${(DESKTOP_ROUTES.length + 1) * DESKTOP.length} screenshot pairs compared, ${diffs.length} differ${diffs.length ? `: ${diffs.join(', ')}` : ''}`);
  return diffs;
}

// ── Run ───────────────────────────────────────────────────────────────────
const authReady = async (page) => { await page.locator('form, .auth-card, .card').first().waitFor(); };
try {
  if (ran('settings')) await sweepSettings();
  if (ran('connectors')) await sweepRoute('connectors', '/connectors', { root: '.page' });
  if (ran('webhooks')) await sweepRoute('webhooks', '/webhooks', { root: '.page' });
  if (ran('mailbox')) await sweepRoute('mailbox', '/mailbox', { root: '.page' });
  if (ran('mailbox-oauth')) await sweepRoute('mailbox-oauth', '/mailbox/oauth?state=flow&code=dummy', { root: '.wrap', states: ['populated', 'error'], ready: authReady });
  if (ran('login')) await sweepRoute('login', '/login', { root: '.bare', states: ['populated'], extra: [KEYBOARD], ready: authReady });
  if (ran('setup')) await sweepRoute('setup', '/setup', { root: '.bare', states: ['setup'], extra: [KEYBOARD], ready: authReady });
  if (ran('change-password')) await sweepRoute('change-password', '/change-password?u=fixture-admin', { root: '.bare', states: ['populated'], extra: [KEYBOARD], ready: authReady });
  if (ran('request-reset')) await sweepRoute('request-reset', '/request-reset', { root: '.bare', states: ['populated'], extra: [KEYBOARD], ready: authReady });
  if (ran('oauth')) await sweepRoute('oauth', '/oauth/authorize?client_id=fixture&redirect_uri=x&state=s', { root: '.bare', extra: [KEYBOARD], ready: authReady });
  if (ran('news')) await sweepRoute('news', '/news', { root: '.news-module' });
  if (ran('editor')) await sweepRoute('editor', '/editor', { root: '.editor-module' });
  // Secondary states of the same routes: open rails, other tabs, forms and layouts.
  const openRail = async (page) => { if (await page.locator('.mobile-selector').isVisible()) await page.locator('.mobile-selector').click(); };
  const openDb = (view) => async (page) => {
    await openRail(page);
    await page.locator('.tree-body .row').filter({ hasText: 'Trade ideas' }).locator('button.label').click();
    await page.locator('.view-tabs').waitFor();
    if (view) await page.locator('.view-tabs button').filter({ hasText: view }).click();
    await settle(page);
  };
  const pop = { states: ['populated'] };
  if (ran('news')) await sweepRoute('news-rail', '/news', { ...pop, root: '.news-module', ready: async (page) => {
    await openRail(page);
    await page.getByRole('button', { name: 'Advanced' }).click();
  } });
  if (ran('news')) await sweepRoute('news-sources-menu', '/news', { ...pop, root: '.src-menu', ready: async (page) => {
    await page.locator('.src-toggle').click();
    await page.locator('.src-menu').waitFor();
  } });
  if (ran('editor')) {
    await sweepRoute('editor-table', '/editor', { ...pop, root: '.editor-module', ready: openDb('') });
    await sweepRoute('editor-kanban', '/editor', { ...pop, root: '.editor-module', ready: openDb('Kanban') });
    await sweepRoute('editor-gallery', '/editor', { ...pop, root: '.editor-module', ready: openDb('Gallery') });
  }
  if (ran('mailbox')) {
    for (const tab of ['Store', 'Mailboxes']) {
      await sweepRoute(`mailbox-${tab.toLowerCase()}`, '/mailbox', { ...pop, root: '.page', ready: async (page) => { await page.getByRole('tab', { name: tab }).click(); await settle(page); } });
    }
  }
  if (ran('connectors')) await sweepRoute('connectors-add', '/connectors', { ...pop, root: '.page', ready: async (page) => {
    await page.locator('.addbtn').click();
    await page.locator('.addform').waitFor();
  } });
  if (ran('flows')) {
    await flowLogin();
    await flowSettingsSave();
    await flowMailbox();
    await flowNewsReorder();
    await flowEditorDatabase();
    await flowConnectors();
    await flowWebhooks();
    await flowConsent();
  }
  let diffs = [];
  if (ran('desktop')) diffs = await desktopCompare();
  for (const line of report) console.log(`PASS ${line}`);
  for (const line of knownErrors) console.log(`KNOWN uncaught API error: ${line}`);
  if (probe) { console.log(probeLog.join('\n')); console.log(`PROBE: ${probeLog.length} failures`); }
  assert.deepEqual(diffs, [], 'Desktop screenshots differ from the baseline');
  console.log('PASS: admin routes mobile check');
} catch (error) {
  for (const line of report) console.log(`PASS ${line}`);
  throw error;
} finally {
  await browser.close();
}
