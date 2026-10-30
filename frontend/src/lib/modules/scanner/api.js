/** Scanner (module id `scanner`) API client.
 *
 * Items are instruments on a data connector at a timeframe, found with the shared symbol
 * lookup (on the scanner's own connector grants). Hives group items. Signals are reusable
 * conditions from four builders (signal, event, pattern, time). Alerts AND signals on an item
 * or a hive and notify; they never act. Boards are named sets of views; a view is a board's look at a hive. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, opts = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...opts
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

const send = (method, body) => ({ method, body: JSON.stringify(body) });

export const scannerApi = {
  settings: () => req('/scanner/settings'),
  saveSettings: (s) => req('/scanner/settings', send('PUT', s)),

  items: () => req('/scanner/items').then((r) => r.items),
  createItem: (b) => req('/scanner/items', send('POST', b)),
  saveItem: (id, b) => req(`/scanner/items/${id}`, send('PUT', b)),
  deleteItem: (id) => req(`/scanner/items/${id}`, { method: 'DELETE' }),

  hives: () => req('/scanner/hives'),
  createHive: (b) => req('/scanner/hives', send('POST', b)),
  saveHive: (id, b) => req(`/scanner/hives/${id}`, send('PUT', b)),
  deleteHive: (id) => req(`/scanner/hives/${id}`, { method: 'DELETE' }),
  setHiveItems: (id, itemIds) => req(`/scanner/hives/${id}/items`, send('PUT', { item_ids: itemIds })),
  board: (hiveId) => req(`/scanner/hives/${hiveId}/board`),

  signals: () => req('/scanner/signals').then((r) => r.signals),
  createSignal: (b) => req('/scanner/signals', send('POST', b)),
  saveSignal: (id, b) => req(`/scanner/signals/${id}`, send('PUT', b)),
  deleteSignal: (id) => req(`/scanner/signals/${id}`, { method: 'DELETE' }),
  /** Judge a draft or a saved signal on one item's stored bars: `{ held, held_count, bars }`. */
  testDraft: (itemId, builder, config) =>
    req('/scanner/signals/test', send('POST', { item_id: itemId, builder, config })),
  testSignal: (id, itemId) => req(`/scanner/signals/${id}/test`, send('POST', { item_id: itemId })),

  alerts: () => req('/scanner/alerts').then((r) => r.alerts),
  createAlert: (b) => req('/scanner/alerts', send('POST', b)),
  saveAlert: (id, b) => req(`/scanner/alerts/${id}`, send('PUT', b)),
  deleteAlert: (id) => req(`/scanner/alerts/${id}`, { method: 'DELETE' }),
  /** Bounded preview on the alert's target: records no hit, notifies nobody. */
  runNow: (id) => req(`/scanner/alerts/${id}/run-now`, { method: 'POST' }),

  hits: (q = {}) => {
    const qs = new URLSearchParams();
    for (const k of ['alert_id', 'item_id', 'hive_id', 'limit']) if (q[k]) qs.set(k, String(q[k]));
    const s = qs.toString();
    return req(`/scanner/hits${s ? `?${s}` : ''}`).then((r) => r.hits);
  },

  boards: () => req('/scanner/boards').then((r) => r.boards),
  createBoard: (b) => req('/scanner/boards', send('POST', b)),
  saveBoard: (id, b) => req(`/scanner/boards/${id}`, send('PUT', b)),
  deleteBoard: (id) => req(`/scanner/boards/${id}`, { method: 'DELETE' }),

  views: () => req('/scanner/views').then((r) => r.views),
  createView: (b) => req('/scanner/views', send('POST', b)),
  saveView: (id, b) => req(`/scanner/views/${id}`, send('PUT', b)),
  deleteView: (id) => req(`/scanner/views/${id}`, { method: 'DELETE' }),

  /** Instrument lookup across the connectors granted to the scanner. */
  symbols: ({ q, asset_type, limit } = {}) => {
    const qs = new URLSearchParams({ module: 'scanner' });
    if (q) qs.set('q', q);
    if (asset_type) qs.set('asset_type', asset_type);
    if (limit) qs.set('limit', String(limit));
    return req(`/histdata/symbols?${qs}`);
  },

  /** The chart's own instrument lists: resolved coordinates, imported as they are. */
  chartLists: () => req('/histviz/lists').then((r) => r.lists),

  /** SSE URL for a board's live items (open it through `liveStream`, never a bare EventSource). */
  streamsUrl: (items) => {
    const list = items.map((it) => ({
      provider: it.provider,
      asset_type: it.asset_type,
      ticker: it.ticker,
      timeframe: it.timeframe,
      ...(it.connector_id ? { connector: it.connector_id } : {})
    }));
    return `/api/scanner/streams?instruments=${encodeURIComponent(JSON.stringify(list))}`;
  }
};

/** The chart for an item. */
export function chartUrl(it) {
  const qs = new URLSearchParams({
    provider: it.provider,
    asset_type: it.asset_type,
    ticker: it.ticker,
    tf: it.timeframe
  });
  if (it.connector_id) qs.set('connector', it.connector_id);
  return `/histviz?${qs}`;
}

export const BUILDERS = ['signal', 'event', 'pattern', 'time'];

export const EVENT_KINDS = [
  'move_from_open',
  'move',
  'new_high',
  'new_low',
  'gap',
  'volume_spike',
  'range_expansion'
];

export const PATTERN_KINDS = [
  'engulfing',
  'pin_bar',
  'inside_bar',
  'outside_bar',
  'doji',
  'hammer',
  'shooting_star',
  'three_bar_reversal',
  'breakout',
  'consolidation',
  'hh_hl',
  'lh_ll'
];

/** Fields each event or pattern kind takes, with defaults. */
export const KIND_FIELDS = {
  move_from_open: { pct: 1, direction: 'up' },
  move: { pct: 3, bars: 5, direction: 'either' },
  new_high: { bars: 20 },
  new_low: { bars: 20 },
  gap: { pct: 2, direction: 'either' },
  volume_spike: { factor: 3, period: 20 },
  range_expansion: { factor: 2, period: 20 },
  engulfing: { direction: 'either' },
  pin_bar: { direction: 'either' },
  inside_bar: {},
  outside_bar: {},
  doji: {},
  hammer: {},
  shooting_star: {},
  three_bar_reversal: { direction: 'either' },
  breakout: { bars: 20, direction: 'up' },
  consolidation: { bars: 10, max_range_pct: 3 },
  hh_hl: { count: 3 },
  lh_ll: { count: 3 }
};

/** A fresh config for a builder. */
export function defaultConfig(builder) {
  if (builder === 'signal') {
    return {
      logic: 'all',
      conditions: [
        {
          left: { kind: 'indicator', indicator: 'sma', period: 9 },
          op: 'crosses_above',
          right: { kind: 'indicator', indicator: 'sma', period: 21 }
        }
      ]
    };
  }
  if (builder === 'event') return { kind: 'move_from_open', ...KIND_FIELDS.move_from_open };
  if (builder === 'pattern') return { kind: 'engulfing', ...KIND_FIELDS.engulfing };
  return { sessions: [], weekdays: 31, from: '09:30', to: '16:00', timezone: 'America/New_York', at: null };
}
