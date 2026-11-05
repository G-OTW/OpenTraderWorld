/** Trade on alert: the Trade section's draft, the ticket template it writes, and how a stored
 *  trade reads in the alert list.
 *
 *  The template is relative: the price that fires the alert is unknown when it is armed, so
 *  prices are distances from it (`abs`, `pct`, `ticks`, `atr`) and the size is units or an
 *  amount of money. The server makes it an order at fire time, on the broker's grid, and
 *  refuses what it cannot work out; the dialog previews the same thing at the chart's price.
 *  Numbers travel as decimal strings. */

import { typesFor, tifsFor, optionsFor } from './orders.js';

// The grants as `$lib/brokers/api.js` reads them, kept here so this file loads without the
// API client (and runs under Node in the tests).
const PAPER = 'paper';
const allows = (a, module) => (a?.modules ?? []).some((m) => m === '*' || m === module);
const trades = (a) => (a?.modules ?? []).includes('trading');

/** The entry types a trade on alert sends, in the ticket's order. */
export const TRADE_TYPES = ['market', 'limit', 'stop', 'stop_limit'];
/** How its size is given: the server's kinds, the ticket's words. */
export const TRADE_SIZINGS = ['units', 'quote_amount', 'risk_amount'];
export const OFFSET_KINDS = ['abs', 'pct', 'ticks', 'atr'];
export const FALLBACKS = ['none', 'cancel_symbol', 'close_symbol', 'flatten_account'];
/** Statuses a fire leaves a trade in. */
export const TRADE_STATUSES = ['idle', 'sending', 'sent', 'filled', 'cancelled', 'refused', 'unknown', 'fallback_done', 'fallback_failed'];

/** A fresh draft on `accountId`: a market buy of one unit, exits off, no fallback. */
export function blankTrade(accountId = '') {
  return {
    accountId,
    intent: 'enter',
    side: 'buy',
    type: 'market',
    tif: '',
    sizing: 'units',
    qty: 1,
    amount: '',
    limit: { kind: 'pct', value: '-0.5' },
    stop: { kind: 'pct', value: '0.5' },
    tp: { kind: 'pct', value: '2' },
    sl: { kind: 'pct', value: '1' },
    tpOn: false,
    slOn: false,
    reduceOnly: false,
    fallback: 'none'
  };
}

const num = (v) => (v === '' || v == null ? NaN : Number(v));
const dec = (v) => String(v).trim();

/** The accounts a trade on alert may go to: granted to the chart, trading on. The paper broker
 *  first (the place to start), then by name. */
export function tradeAccounts(accounts) {
  const paper = (a) => (a.broker === PAPER ? 0 : 1);
  return (accounts ?? [])
    .filter((a) => allows(a, 'histviz') && trades(a))
    .sort((a, b) => paper(a) - paper(b) || String(a.name).localeCompare(String(b.name)));
}

/** Whether an account moves real money: live trading turned on for it. */
export const isLive = (account) => !!account?.live_since;

/** What the account's broker takes, for the Trade section: types, times in force (never good
 *  till date, a trade armed for later has no date), exits, reduce-only. */
export function tradeOptions(account) {
  const types = TRADE_TYPES.filter((t) => typesFor(account).includes(t));
  const tifs = tifsFor(account).filter((t) => t !== 'gtd');
  const o = optionsFor(account);
  return { types: types.length ? types : ['market'], tifs, exits: o.exits, reduceOnly: o.reduceOnly };
}

/** The ticket template the server stores, or `{ error }` naming what is missing (an i18n key
 *  under `histviz.alerts.trade.err.`). */
export function ticketOf(d) {
  if (d.intent === 'exit') return { ticket: {} };
  if (d.side !== 'buy' && d.side !== 'sell') return { error: 'side' };
  if (!TRADE_TYPES.includes(d.type)) return { error: 'type' };
  const size = d.sizing === 'units' ? num(d.qty) : num(d.amount);
  if (!(size > 0)) return { error: 'size' };
  if (d.sizing === 'risk_amount' && !d.slOn) return { error: 'riskNeedsStop' };
  const off = (o, signed) => {
    const v = num(o?.value);
    if (!Number.isFinite(v) || (!signed && !(v > 0))) return null;
    return { kind: OFFSET_KINDS.includes(o.kind) ? o.kind : 'pct', value: dec(o.value) };
  };
  const t = { side: d.side, order_type: d.type, size: { kind: d.sizing, value: dec(d.sizing === 'units' ? d.qty : d.amount) } };
  if (d.tif) t.tif = d.tif;
  if (d.reduceOnly) t.reduce_only = true;
  if (d.type === 'limit' || d.type === 'stop_limit') {
    t.limit_offset = off(d.limit, true);
    if (!t.limit_offset) return { error: 'limit' };
  }
  if (d.type === 'stop' || d.type === 'stop_limit') {
    t.stop_offset = off(d.stop, true);
    if (!t.stop_offset) return { error: 'stop' };
  }
  if (d.tpOn) {
    t.take_profit = off(d.tp, false);
    if (!t.take_profit) return { error: 'tp' };
  }
  if (d.slOn) {
    t.stop_loss = off(d.sl, false);
    if (!t.stop_loss) return { error: 'sl' };
  }
  return { ticket: t };
}

/** The body the trade route takes, or `{ error }`. */
export function tradeBody(d) {
  const r = ticketOf(d);
  if (r.error) return r;
  if (!d.accountId) return { error: 'account' };
  return { body: { account_id: d.accountId, intent: d.intent, ticket: r.ticket, fallback: FALLBACKS.includes(d.fallback) ? d.fallback : 'none' } };
}

/** A stored trade back into a draft. */
export function draftOf(trade) {
  const d = blankTrade(trade?.account_id ?? '');
  if (!trade) return d;
  const t = trade.ticket ?? {};
  d.intent = trade.intent === 'exit' ? 'exit' : 'enter';
  d.fallback = trade.fallback ?? 'none';
  if (d.intent === 'exit') return d;
  d.side = t.side ?? 'buy';
  d.type = t.order_type ?? 'market';
  d.tif = t.tif ?? '';
  d.reduceOnly = !!t.reduce_only;
  if (t.size) {
    d.sizing = TRADE_SIZINGS.includes(t.size.kind) ? t.size.kind : 'units';
    if (d.sizing === 'units') d.qty = Number(t.size.value);
    else d.amount = String(t.size.value);
  }
  const back = (o, fallback) => (o ? { kind: o.kind, value: String(o.value) } : fallback);
  d.limit = back(t.limit_offset, d.limit);
  d.stop = back(t.stop_offset, d.stop);
  d.tp = back(t.take_profit, d.tp);
  d.sl = back(t.stop_loss, d.sl);
  d.tpOn = !!t.take_profit;
  d.slOn = !!t.stop_loss;
  return d;
}

/** Whether a draft would write what is stored: arming arms what was saved. */
export function sameTrade(d, trade) {
  if (!trade) return false;
  const b = tradeBody(d);
  if (b.error) return false;
  return (
    b.body.account_id === trade.account_id &&
    b.body.intent === trade.intent &&
    b.body.fallback === trade.fallback &&
    canonical(b.body.ticket) === canonical(trade.ticket ?? {})
  );
}

/** JSON with keys sorted at every depth, decimals compared as numbers. */
function canonical(v) {
  if (Array.isArray(v)) return `[${v.map(canonical).join(',')}]`;
  if (v && typeof v === 'object') {
    return `{${Object.keys(v)
      .filter((k) => v[k] != null && v[k] !== false)
      .sort()
      .map((k) => `${k}:${canonical(v[k])}`)
      .join(',')}}`;
  }
  if (typeof v === 'string' && v.trim() !== '' && Number.isFinite(Number(v))) return String(Number(v));
  return JSON.stringify(v);
}

/** The chip a trade shows in the alert list: `armed` while armed, else how its last fire went. */
export function statusOf(trade) {
  if (!trade) return null;
  if (trade.armed_at) return 'armed';
  return TRADE_STATUSES.includes(trade.status) ? trade.status : 'idle';
}

/** Whether the chip warns: the outcome needs a look. */
export const statusWarns = (s) => s === 'refused' || s === 'unknown' || s === 'fallback_failed';
