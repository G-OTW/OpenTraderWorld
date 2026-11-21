/** Display helpers for the robot fleet and a robot's header.
 *
 * The server is the authority on every figure (`GET /api/robots/fleet`, the dashboard's
 * `robot` block): this only says which tone a state takes, which badge an environment wears,
 * how long until the next tick and how old the last sync is. Nothing here re-derives a P&L. */

/** The one word a robot's status dot carries. */
export function robotState(r) {
  if (!r) return 'stopped';
  if (r.running) return 'running';
  if (r.status === 'error' || r.last_error) return 'error';
  if (r.status !== 'active') return 'stopped';
  // Disarmed by an act at the broker it cannot follow: the user arms it again.
  if (r.exec_state === 'stopped') return 'interrupted';
  if (r.guard_holds) return 'held';
  if (['degraded', 'blocked', 'error', 'unreachable'].includes(r.exec_state)) return r.exec_state;
  return 'active';
}

/** The tone of a state: calm when it trades as intended, amber when it holds back, red when
 *  the user has to act. */
export function stateTone(state) {
  if (state === 'active' || state === 'running') return 'ok';
  if (state === 'held' || state === 'degraded' || state === 'stopped') return 'warn';
  if (['blocked', 'error', 'unreachable', 'interrupted'].includes(state)) return 'bad';
  return '';
}

/** The environment badge: where the robot's orders go. */
export const ENVIRONMENTS = ['simulated', 'paper', 'testnet', 'live'];
export function envTone(env) {
  if (env === 'live') return 'danger';
  if (env === 'testnet') return 'warn';
  if (env === 'paper') return 'accent';
  return 'neutral';
}

/** Whole seconds from `now` to an ISO instant (negative once it passed), or null. */
export function secondsTo(iso, now = Date.now()) {
  const at = Date.parse(iso ?? '');
  return Number.isFinite(at) ? Math.round((at - now) / 1000) : null;
}

/** A duration as the header says it: `45s`, `3m 05s`, `2h 10m`, `3d 4h`. */
export function shortDuration(secs) {
  if (secs === null || secs === undefined || !Number.isFinite(secs)) return '';
  const s = Math.max(0, Math.round(Math.abs(secs)));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}m`;
  return `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`;
}

/** The next tick as a countdown, `due` once it is past (the worker picks it up), or null when
 *  none is planned. */
export function countdown(iso, now = Date.now()) {
  const s = secondsTo(iso, now);
  if (s === null) return null;
  return s <= 0 ? 'due' : shortDuration(s);
}

/** How old a stamp is, or null. */
export function age(iso, now = Date.now()) {
  const s = secondsTo(iso, now);
  return s === null ? null : shortDuration(-s);
}

/** The fleet filtered by status word and account (`''` is every one). */
export function filterFleet(robots, { status = '', account = '' } = {}) {
  return (robots ?? []).filter((r) => {
    if (status && robotState(r) !== status && !(status === 'active' && robotState(r) === 'running')) return false;
    if (account === 'simulated') return !r.account_id;
    if (account && r.account_id !== account) return false;
    return true;
  });
}

/** The accounts the fleet runs on, for the filter: `{ id, name }`, each once. */
export function fleetAccounts(robots) {
  const seen = new Map();
  for (const r of robots ?? []) if (r.account_id && !seen.has(r.account_id)) seen.set(r.account_id, r.account ?? '');
  return [...seen].map(([id, name]) => ({ id, name }));
}

/** The environments the fleet runs in (`simulated`, `paper`, `testnet`, `live`), each once. */
export function fleetEnvironments(robots) {
  return [...new Set((robots ?? []).map((r) => r.environment).filter(Boolean))];
}

/**
 * The fleet's P&L over the robots of one environment (`''`: all) and a date range
 * (`YYYY-MM-DD`, local days, `''`: open). `points` is the summed equity curve: the starting
 * equity plus every trade closed before the range, then one point per trade closed inside it,
 * and, with no end date, a last point marked to the open positions. `realized` is what closed
 * inside the range, `unrealized` what the open positions carry now (null when none is marked).
 */
export function fleetPnl(robots, { env = '', from = '', to = '' } = {}, now = Date.now()) {
  const list = (robots ?? []).filter((r) => !env || r.environment === env);
  const lo = from ? new Date(`${from}T00:00:00`).getTime() : -Infinity;
  const hi = to ? new Date(`${to}T23:59:59.999`).getTime() : Infinity;
  const fin = (v) => (Number.isFinite(v) ? v : 0);
  const start = list.reduce((n, r) => n + fin(r.start_equity), 0);
  const closed = list
    .flatMap((r) => (r.closed ?? []).map(([at, pnl]) => ({ t: Date.parse(at), at, pnl: fin(pnl) })))
    .filter((c) => Number.isFinite(c.t))
    .sort((a, b) => a.t - b.t);
  let equity = start + closed.filter((c) => c.t < lo).reduce((n, c) => n + c.pnl, 0);
  const first = from
    ? new Date(lo).toISOString()
    : list.map((r) => r.started_at).filter((x) => Number.isFinite(Date.parse(x ?? ''))).sort()[0] ?? closed[0]?.at ?? new Date(now).toISOString();
  const points = [{ at: first, equity }];
  let realized = 0;
  for (const c of closed) {
    if (c.t < lo || c.t > hi) continue;
    equity += c.pnl;
    realized += c.pnl;
    points.push({ at: c.at, equity });
  }
  const marked = list.filter((r) => Number.isFinite(r.unrealized));
  const unrealized = marked.length ? marked.reduce((n, r) => n + r.unrealized, 0) : null;
  if (!to) points.push({ at: new Date(now).toISOString(), equity: equity + (unrealized ?? 0) });
  return { points, realized, unrealized, total: realized + (unrealized ?? 0), count: list.length };
}

/** The ranking periods: today (local day), the last 7 days, the last 30 days, since the start. */
export const RANK_PERIODS = ['today', 'week', 'month', 'all'];

/** Where a ranking period starts, in ms (`-Infinity`: since the start). */
export function periodStart(period, now = Date.now()) {
  if (period === 'today') {
    const d = new Date(now);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  }
  if (period === 'week') return now - 7 * 86400000;
  if (period === 'month') return now - 30 * 86400000;
  return -Infinity;
}

/**
 * The robots ranked by the P&L of the trades they closed in a period, best first: `{ id, name,
 * environment, currency, pnl, pct, trades }`, `pct` against the robot's starting equity (null
 * without one). A robot that closed nothing in the period ranks at zero.
 */
export function fleetRanking(robots, period = 'all', now = Date.now()) {
  const lo = periodStart(period, now);
  return (robots ?? [])
    .map((r) => {
      const inside = (r.closed ?? []).filter(([at]) => {
        const t = Date.parse(at);
        return Number.isFinite(t) && t >= lo && t <= now;
      });
      const pnl = inside.reduce((n, [, v]) => n + (Number.isFinite(v) ? v : 0), 0);
      const start = Number.isFinite(r.start_equity) && r.start_equity > 0 ? r.start_equity : null;
      return { id: r.id, name: r.name, environment: r.environment, currency: r.currency || '', pnl, pct: start ? (pnl / start) * 100 : null, trades: inside.length };
    })
    .sort((a, b) => b.pnl - a.pnl || b.trades - a.trades || String(a.name).localeCompare(String(b.name)));
}

/**
 * What the fleet has at work in the market, by asset type: the open positions' size at the mark
 * summed over the robots, largest first, `{ kind, value, pct, robots }`. Empty when nothing is
 * open.
 */
export function fleetExposure(robots) {
  const by = new Map();
  for (const r of robots ?? []) {
    for (const [kind, v] of Object.entries(r.exposure ?? {})) {
      if (!Number.isFinite(v) || v <= 0) continue;
      const e = by.get(kind) ?? { kind, value: 0, robots: 0 };
      e.value += v;
      e.robots += 1;
      by.set(kind, e);
    }
  }
  const total = [...by.values()].reduce((n, e) => n + e.value, 0);
  return total > 0 ? [...by.values()].map((e) => ({ ...e, pct: (e.value / total) * 100 })).sort((a, b) => b.value - a.value) : [];
}

/** The guard rules a form edits, as the server checks them. Only what the user set is sent. */
export const WHEN_KINDS = ['consecutive_losses', 'consecutive_wins', 'drawdown_pct', 'loss_pct', 'loss_amount', 'losses_in_window', 'gain_pct'];
export const THEN_KINDS = ['pause', 'halt', 'scale'];
export const RESTORE_KINDS = ['after_trades', 'after_wins', 'after_days', 'manual'];
export const WINDOWS = ['day', 'week', 'month', 'all'];

/** Which field a condition reads: a count, a percentage or an amount. */
export function whenField(kind) {
  if (['consecutive_losses', 'consecutive_wins', 'losses_in_window'].includes(kind)) return 'n';
  if (kind === 'loss_amount') return 'amount';
  return 'pct';
}

/** Whether a condition sums over a window. */
export const windowed = (kind) => ['loss_pct', 'loss_amount', 'losses_in_window', 'gain_pct'].includes(kind);

/** A fresh rule of a kind, with the field it needs. */
export function newRule() {
  return { when: { kind: 'consecutive_losses', n: 3 }, then: { kind: 'pause', days: 1 } };
}

/** Rebuild a condition when its kind changes: keep the number, drop what the new kind lacks. */
export function setWhenKind(when, kind) {
  const old = when?.[whenField(when?.kind)];
  const field = whenField(kind);
  const next = { kind, [field]: Number.isFinite(Number(old)) && Number(old) > 0 ? Number(old) : field === 'n' ? 3 : 5 };
  if (windowed(kind)) next.window = when?.window ?? 'day';
  if (kind === 'drawdown_pct') next.from = when?.from ?? 'peak';
  return next;
}

/** Rebuild an action when its kind changes. */
export function setThenKind(kind) {
  if (kind === 'halt') return { kind };
  if (kind === 'scale') return { kind, divisor: 2, restore: { kind: 'after_trades', n: 5 } };
  return { kind: 'pause', days: 1 };
}

/** The capital policy as the server takes it: numbers as strings, nothing empty. */
export function capitalBody(c) {
  const out = {};
  const dec = (v) => String(v ?? '').trim();
  if (dec(c?.allocation)) out.allocation = dec(c.allocation);
  if (dec(c?.currency)) out.currency = dec(c.currency).toUpperCase();
  if (dec(c?.percent_in)) out.percent_in = dec(c.percent_in);
  if (dec(c?.max_positions)) out.max_positions = Number(dec(c.max_positions));
  if (dec(c?.max_orders_per_day)) out.max_orders_per_day = Number(dec(c.max_orders_per_day));
  if (dec(c?.leverage)) out.leverage = dec(c.leverage);
  if (dec(c?.pyramiding)) out.pyramiding = Number(dec(c.pyramiding));
  return out;
}
