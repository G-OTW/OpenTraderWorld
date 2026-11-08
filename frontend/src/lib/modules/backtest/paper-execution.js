/** Display helpers for a paper session that executes on a broker account.
 *
 * The server's sync is the authority (it reads the venue and converges the books); this only
 * shapes what it stored for the page: what the engine intends next to what the account holds,
 * and per trade the simulated price next to the broker's. Decimals arrive as strings and are
 * shown as the broker stated them, never re-rounded here. */

/** A string or number decimal as a number, or null when it is not one. */
export function num(v) {
  if (v === null || v === undefined || v === '') return null;
  const n = typeof v === 'number' ? v : Number(v);
  return Number.isFinite(n) ? n : null;
}

/** Whether the session executes on a broker account (anything else simulates). */
export const onBroker = (session) => session?.execution?.target === 'broker';

/** The badge a session card carries: null (simulated), `live` (real money) or `broker`. */
export function execBadge(session) {
  if (!onBroker(session)) return null;
  return session.exec_level === 'live' ? 'live' : 'broker';
}

/** The one word the panel's status chip shows: disarmed first, then what the last sync found. */
export function execState(view) {
  if (!view || view.target !== 'broker') return 'simulate';
  if (!view.armed_at) return view.state === 'error' ? 'error' : 'disarmed';
  return ['synced', 'degraded', 'paused', 'blocked', 'error'].includes(view.state) ? view.state : 'synced';
}

/** The tone of a state: calm when it trades as intended, amber when it holds back, red when
 *  the user has to act. */
export function stateTone(state) {
  if (state === 'synced') return 'ok';
  if (state === 'degraded' || state === 'paused' || state === 'disarmed') return 'warn';
  if (state === 'blocked' || state === 'error') return 'bad';
  return '';
}

/** What a broker price cost against the simulated one, signed so that positive is worse for
 *  the trader: paying more on a buy, receiving less on a sell. Null when either is unknown. */
export function slippage(sim, real, buy) {
  const s = num(sim);
  const r = num(real);
  if (s === null || r === null) return null;
  return buy ? r - s : s - r;
}

/** Per entry the session sent: the engine's entry and exit beside the broker's, with what
 *  each side cost. `side` is the position's (`long` buys in and sells out). */
export function compareRows(view) {
  return (view?.positions ?? []).map((p) => {
    const long = p.side !== 'short';
    return {
      ...p,
      entrySlip: slippage(p.sim_entry, p.real_entry, long),
      exitSlip: slippage(p.sim_exit, p.real_exit, !long)
    };
  });
}

/** The books side by side, one row per instrument: what the engine holds or rests, and what
 *  the account holds, the session's own share of it and the difference nobody explains. */
export function bookRows(view) {
  return (view?.book?.instruments ?? []).map((i) => {
    const intended = i.intended ?? {};
    const broker = i.broker ?? {};
    const drift = num(broker.drift);
    return {
      key: i.key,
      ticker: i.ticker,
      symbol: i.symbol,
      side: intended.side ?? null,
      stop: num(intended.stop),
      target: num(intended.target),
      entry: intended.entry ?? null,
      limits: intended.limits ?? [],
      held: num(broker.held),
      own: num(broker.own),
      baseline: num(broker.baseline),
      drift,
      drifting: drift !== null && drift !== 0,
      working: broker.working ?? []
    };
  });
}

/** What the server answered a refusal with, as the next step the page offers. */
export function nextStep(error) {
  if (error?.code === 'background_trading_required') return 'sign';
  if (error?.code === 'code_required' || error?.code === 'trading_locked') return 'code';
  return null;
}
