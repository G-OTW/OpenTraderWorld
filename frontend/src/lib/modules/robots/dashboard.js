/** Display projection only: the engine remains the authority for fills and fees, and the
 *  server for the robot's size on each trade. */
export const finite = (v) => typeof v === 'number' && Number.isFinite(v);
export function sessionState(session) {
  if (session?.running) return 'running';
  if (session?.status === 'error') return 'error';
  if (session?.status !== 'active') return 'stopped';
  return session?.stats ? 'watching' : 'pending';
}
const keyOf = (p) => `${p.ticker}|${p.direction}|${p.entry_ts}`;
const timeframeMs = (tf) => {
  const match = /^(\d+)(s|m|h|d|w)$/.exec(tf ?? '');
  return match ? Number(match[1]) * ({ s: 1000, m: 60000, h: 3600000, d: 86400000, w: 604800000 })[match[2]] : 0;
};

export function projectBook(data, quotes = {}) {
  const session = data?.session ?? {};
  const stats = session.stats ?? {};
  const fills = data?.live_fills ?? [];
  const lastRun = Date.parse(session.last_run_at ?? session.updated_at) || 0;
  const fresh = fills.filter((f) => Date.parse(f.at) >= lastRun);
  const exited = new Set(fills.filter((f) => f.kind === 'exit').map((f) => f.trade_key));
  const rows = data?.trades ?? [];
  const fallbackTicker = data?.instruments?.length === 1 ? data.instruments[0].ticker : '';
  const positions = (stats.open_positions ?? []).map((p) => ({ ...p, ticker: p.ticker || fallbackTicker }));
  // Grid / DCA books may expose positions only through their open trade rows.
  for (const row of rows.filter((r) => r.open && !r.trade?.pending)) {
    if (!positions.some((p) => keyOf(p) === row.trade_key)) {
      positions.push({ ...row.trade, ticker: row.ticker, direction: row.direction,
        entry_ts: row.entry_ts, avg_price: row.trade?.entry_price, fees: row.fees });
    }
  }
  const pending = (stats.pending_orders ?? []).map((o) => ({ ...o, ticker: o.ticker || fallbackTicker }));
  const filledOrders = new Set();
  for (const fill of fills.filter((f) => f.kind === 'entry')) {
    const order = pending.find((o) => o.kind === 'entry' && o.ticker === fill.ticker &&
      o.direction === fill.direction && o.order === fill.reason && !filledOrders.has(o) &&
      (o.signal_ts ? Date.parse(o.signal_ts) <= Date.parse(fill.at) : fresh.includes(fill)));
    // A running/failed evaluation updates last_run_at before replacing the snapshot.
    // Its still-pending signal is stronger evidence than that evaluation timestamp.
    if (!order && !fresh.includes(fill)) continue;
    if (order) filledOrders.add(order);
    if (exited.has(fill.trade_key) || positions.some((p) => keyOf(p) === fill.trade_key)) continue;
    const event = data.events?.find((e) => e.kind === 'open' && e.payload?.live &&
      e.payload.ticker === fill.ticker && e.payload.entry_ts === fill.at);
    positions.push({ ticker: fill.ticker, direction: fill.direction, entry_ts: fill.at,
      avg_price: fill.price, qty: event?.payload?.qty ?? order?.qty, live: true,
      // Fees and protection levels await engine reconciliation; never invent them.
      fees: null, trade_key: fill.trade_key });
  }
  const multiplier = finite(session.settings?.instrument?.multiplier) && session.settings.instrument.multiplier > 0
    ? session.settings.instrument.multiplier : 1;
  const open = positions.filter((p) => !exited.has(p.trade_key ?? keyOf(p))).map((p) => {
    // Do not silently choose between different providers for an ambiguous symbol.
    const instruments = (data.instruments ?? []).filter((i) => i.ticker === p.ticker);
    const instrument = instruments.length === 1 ? instruments[0] : null;
    const quote = instrument ? quotes[instrument.id] ?? { price: instrument.price, at: instrument.price_at, source: 'stored' } : null;
    const mark = quote?.price;
    const gross = finite(mark) && finite(p.avg_price) && finite(p.qty)
      ? (mark - p.avg_price) * Math.abs(p.qty) * multiplier * (p.direction === 'short' ? -1 : 1) : null;
    return { ...p, quote, mark, unrealized: gross,
      exposure: finite(mark) && finite(p.qty) ? Math.abs(mark * p.qty * multiplier) : null };
  });
  // The robot's share of each trade: its own size over the engine's (`robot_qty`, a decimal
  // string; absent on a trade recorded before robots, which counts whole). A trade the robot
  // did not take counts nothing and is listed apart.
  const share = (r) => {
    if (r.robot_qty === null || r.robot_qty === undefined) return 1;
    const q = Math.abs(Number(r.trade?.qty ?? 0));
    const taken = Number(r.robot_qty);
    return Number.isFinite(taken) && q > 0 ? taken / q : 0;
  };
  const skipped = rows.filter((r) => share(r) === 0).map((r) => ({ ...r.trade, ticker: r.ticker,
    direction: r.direction, entry_ts: r.entry_ts, exit_ts: r.exit_ts, note: r.robot_note ?? '' }));
  const closed = rows.filter((r) => !r.open && share(r) > 0).map((r) => {
    const k = share(r);
    return { ...r.trade, ticker: r.ticker, direction: r.direction, entry_ts: r.entry_ts, exit_ts: r.exit_ts,
      qty: finite(r.trade?.qty) ? r.trade.qty * k : r.trade?.qty, pnl: r.pnl * k, fees: r.fees * k,
      scale: r.scale ?? '1' };
  });
  // Live exits are recorded as events before the next candle rebuilds the trade book.
  for (const e of data?.events ?? []) {
    const p = e.payload;
    if (e.kind !== 'close' || !p?.live || !finite(p.pnl)) continue;
    const barMs = Math.max(0, ...(data.instruments ?? []).filter((i) => i.ticker === p.ticker).map((i) => timeframeMs(i.timeframe)));
    // Replayed live entries use the candle timestamp rather than the original tick time.
    // The engine also stamps the exit at that candle's start; its price and reason stay exact.
    const reconciled = closed.some((c) => c.ticker === p.ticker && c.direction === p.direction && (
      c.entry_ts === p.entry_ts || (barMs > 0 && c.exit_price === p.exit_price && c.exit_reason === p.exit_reason &&
        c.qty === p.qty && Date.parse(p.exit_ts) >= Date.parse(c.exit_ts) &&
        Date.parse(p.exit_ts) - Date.parse(c.exit_ts) < barMs)
    ));
    if (!reconciled) closed.push(p);
  }
  closed.sort((a, b) => Date.parse(a.exit_ts) - Date.parse(b.exit_ts));
  const realized = closed.reduce((sum, c) => sum + (finite(c.pnl) ? c.pnl : 0), 0);
  const wins = closed.filter((c) => c.pnl > 0).length;
  let cumulative = 0;
  const curve = closed.map((c) => ({ ts: c.exit_ts, value: cumulative += c.pnl }));
  return { open, pending: pending.filter((o) => !filledOrders.has(o)), closed, skipped, realized, curve,
    unrealized: open.every((p) => finite(p.unrealized)) ? open.reduce((n, p) => n + p.unrealized, 0) : null,
    exposure: open.every((p) => finite(p.exposure)) ? open.reduce((n, p) => n + p.exposure, 0) : null,
    winRate: closed.length ? wins / closed.length * 100 : null,
    fees: closed.reduce((n, p) => n + (p.fees ?? 0), 0) };
}

/** Adapt the paper book to the backtest chart's existing tradeMarks vocabulary.
 * Open positions have entries only: the engine's end-of-window mark is not an exit. */
export function paperChartTrades(book, ticker) {
  if (!ticker) return [];
  return [
    ...(book?.closed ?? []),
    ...(book?.open ?? []).map((p) => ({ ...p, entry_price: p.avg_price, exit_ts: null, exit_price: null }))
  ].filter((trade) => trade.ticker === ticker && finite(trade.entry_price));
}
