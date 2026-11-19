/** One trading day read as a report: the figures, the intraday curve, the fills, and the
 * day's check-ins. Pure functions over what the page already fetches:
 *
 *   points  `/api/journal/analytics` `points` for the day (closed trades, display currency,
 *           epoch milliseconds, oldest first)
 *   trades  `/api/journal/trades` rows (native currency, RFC3339 stamps, legs as JSON)
 *   hist    `/api/mindset/history?from=date&to=date`
 *
 * Nothing here decides which trades belong to the day: the server does, on the same local
 * effective date the calendar buckets by. */
import { normalize } from '$lib/modules/mindset/scoring.js';
import { docText } from '$lib/modules/mindset/text.js';

/** RFC3339 bounds of a local calendar day, inclusive, the way the Trades list sends them. */
export function dayBounds(date) {
  const start = new Date(`${date}T00:00:00`);
  const end = new Date(`${date}T23:59:59.999`);
  return { since: start.toISOString(), until: end.toISOString(), start: start.getTime(), end: end.getTime() };
}

/** Neighbouring local date, 'YYYY-MM-DD'. */
export function shiftDate(date, n) {
  const d = new Date(`${date}T12:00:00`);
  d.setDate(d.getDate() + n);
  const pad = (x) => String(x).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

const sum = (xs) => xs.reduce((a, b) => a + b, 0);
const meanOf = (xs) => (xs.length ? sum(xs) / xs.length : null);

/** Cumulative realized P&L across the day, one step per closed trade, starting flat.
 *  `drawdown` is the deepest fall from a running peak that starts at zero, so a day that
 *  opens with a loss counts that loss. */
export function pnlCurve(points) {
  const sorted = [...(points ?? [])].sort((a, b) => a.at - b.at);
  let cum = 0;
  let peak = 0;
  let drawdown = 0;
  let low = 0;
  let high = 0;
  const steps = sorted.map((p) => {
    cum += p.net;
    peak = Math.max(peak, cum);
    drawdown = Math.max(drawdown, peak - cum);
    low = Math.min(low, cum);
    high = Math.max(high, cum);
    return { id: p.id, at: p.at, net: p.net, cum, ticker: p.ticker, side: p.side };
  });
  return { steps, drawdown, low, high };
}

/** Headline figures of the day. Money in the display currency. A profit factor with no
 *  losing trade is `Infinity`: the page names it rather than printing a ratio. */
export function dayStats(points) {
  const ps = points ?? [];
  const nets = ps.map((p) => p.net);
  const winners = nets.filter((x) => x > 0);
  const losers = nets.filter((x) => x < 0);
  const grossWin = sum(winners);
  const grossLoss = -sum(losers);
  const fees = sum(ps.map((p) => p.fees ?? 0));
  const net = sum(nets);
  const avgWin = meanOf(winners);
  const avgLoss = losers.length ? -grossLoss / losers.length : null;
  const rs = ps.map((p) => p.r).filter((r) => r != null && Number.isFinite(r));
  const holds = ps.map((p) => p.hold_min).filter((h) => h != null && h >= 0);
  const side = (s) => {
    const xs = ps.filter((p) => p.side === s);
    return { n: xs.length, net: sum(xs.map((p) => p.net)) };
  };
  const decided = winners.length + losers.length;
  return {
    trades: ps.length,
    wins: winners.length,
    losses: losers.length,
    scratch: ps.length - decided,
    net,
    fees,
    gross: net + fees,
    winRate: decided ? (winners.length / decided) * 100 : null,
    avgWin,
    avgLoss,
    payoff: avgWin != null && avgLoss ? avgWin / -avgLoss : null,
    profitFactor: grossLoss > 0 ? grossWin / grossLoss : grossWin > 0 ? Infinity : null,
    expectancy: ps.length ? net / ps.length : null,
    best: nets.length ? Math.max(...nets) : null,
    worst: nets.length ? Math.min(...nets) : null,
    rTotal: rs.length ? sum(rs) : null,
    avgR: meanOf(rs),
    rCount: rs.length,
    avgHold: meanOf(holds),
    long: side('long'),
    short: side('short'),
    drawdown: pnlCurve(ps).drawdown
  };
}

/** Per instrument: trades, wins and net, largest absolute result first. */
export function byTicker(points) {
  const m = new Map();
  for (const p of points ?? []) {
    const row = m.get(p.ticker) ?? { ticker: p.ticker, trades: 0, wins: 0, net: 0 };
    row.trades += 1;
    if (p.net > 0) row.wins += 1;
    row.net += p.net;
    m.set(p.ticker, row);
  }
  return [...m.values()].sort((a, b) => Math.abs(b.net) - Math.abs(a.net));
}

const ms = (v) => (v ? new Date(v).getTime() : null);

/** Every fill executed inside [start, end]: entries and exits of each trade, simple or
 *  legged. A long buys to open and sells to close, a short the other way round. A leg with
 *  no stamp of its own takes its trade's entry or exit time; with neither it is not placed
 *  on the day. Oldest first. */
export function dayFills(trades, start, end) {
  const out = [];
  for (const tr of trades ?? []) {
    const long = tr.side !== 'short';
    const legs = (list, open, fallback) => {
      for (const [i, leg] of (Array.isArray(list) ? list : []).entries()) {
        out.push({
          key: `${tr.id}:${open ? 'e' : 'x'}:${leg.id || i}`,
          tradeId: tr.id,
          at: ms(leg.at) ?? ms(fallback),
          ticker: tr.ticker,
          action: open === long ? 'buy' : 'sell',
          effect: open ? 'open' : 'close',
          qty: leg.qty,
          price: leg.price,
          fees: leg.fees ?? null,
          currency: tr.currency,
          signal: leg.signal ?? null
        });
      }
    };
    if (tr.advanced) {
      legs(tr.entries, true, tr.entry_at);
      legs(tr.exits, false, tr.exit_at);
    } else {
      if (tr.entry_price != null) legs([{ price: tr.entry_price, qty: tr.quantity, at: tr.entry_at }], true, null);
      if (tr.exit_price != null) legs([{ price: tr.exit_price, qty: tr.quantity, at: tr.exit_at }], false, null);
    }
  }
  return out.filter((f) => f.at != null && f.at >= start && f.at <= end).sort((a, b) => a.at - b.at);
}

const PHASES = ['pre', 'live', 'post'];
const answered = (v) => v != null && v !== '' && !(Array.isArray(v) && v.length === 0);

/** The day's check-ins, condensed: each filled template with its score and its answered
 *  prompts in order, the picked templates left unfilled, the day score (same reading as
 *  the Mindset dashboard: mean of every scored answer) and the free note as plain text. */
export function mindsetDigest(hist, date, picked = []) {
  const prompts = [...(hist?.prompts ?? [])].sort((a, b) => (a.position ?? 0) - (b.position ?? 0));
  const tplById = new Map((hist?.templates ?? []).map((t) => [t.id, t]));
  const entries = (hist?.entries ?? []).filter((e) => String(e.entry_date).slice(0, 10) === date);
  const all = [];
  const templates = entries
    .map((e) => {
      const tpl = tplById.get(e.template_id);
      const answers = [];
      const scores = [];
      for (const p of prompts) {
        if (p.template_id !== e.template_id) continue;
        const v = e.answers?.[p.id];
        if (!answered(v)) continue;
        const norm = normalize(p, v);
        if (norm != null) scores.push(norm);
        answers.push({ id: p.id, label: p.label, kind: p.kind, value: v, norm });
      }
      all.push(...scores);
      return {
        id: e.template_id,
        name: tpl?.name ?? '',
        phase: tpl?.phase ?? '',
        score: meanOf(scores),
        answers
      };
    })
    .sort((a, b) => PHASES.indexOf(a.phase) - PHASES.indexOf(b.phase));
  const filled = new Set(templates.map((t) => t.id));
  const missing = (picked ?? [])
    .map((id) => tplById.get(id))
    .filter((t) => t && t.active !== false && !filled.has(t.id))
    .map((t) => ({ id: t.id, name: t.name, phase: t.phase ?? '' }));
  const noteRow = (hist?.notes ?? []).find((n) => String(n.day ?? '').slice(0, 10) === date);
  const note = noteRow ? docText(noteRow.content).trim() : '';
  return { score: meanOf(all), templates, missing, note };
}
