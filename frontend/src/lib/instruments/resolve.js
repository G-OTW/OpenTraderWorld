/** Turning a symbol read on a page (FinanceDatabase, Fundamentals, a backtest) into the
 * instrument a chart, a download or a watchlist works on. Such a symbol is not a provider
 * coordinate: it goes through the provider symbol search, and only an exact match counts.
 * Several exact matches are offered to choose from, none is picked by the code. */

/** Histdata asset type of a FinanceDatabase asset type; '' when no provider serves it, so
 * the search runs across every type rather than on a guessed one. */
const FINDB_ASSET = { equity: 'equity', etf: 'etf', index: 'index', currency: 'fx', crypto: 'crypto' };
export const findbAssetType = (t) => FINDB_ASSET[t] ?? '';

/** The search hits that are the instrument itself: the same symbol, case aside. */
export function exactHits(symbol, results) {
  const want = String(symbol ?? '').trim().toUpperCase();
  if (!want) return [];
  return (results ?? []).filter((r) => String(r.symbol ?? '').toUpperCase() === want);
}

/** The chart's deep link for resolved coordinates (`connector_id` optional). */
export function chartHref(c, tf = '1d') {
  const q = new URLSearchParams({ provider: c.provider, asset_type: c.asset_type ?? '', ticker: c.ticker, tf });
  if (c.connector_id) q.set('connector', c.connector_id);
  return `/histviz?${q}`;
}

/** The watchlist search family of an asset type: crypto, or stock (which also finds ETFs). */
export const watchKind = (assetType) => (assetType === 'crypto' ? 'crypto' : 'stock');

/** The timeframe a download or a chart opens on: daily when served, else the provider's first. */
export function defaultTimeframe(timeframes) {
  const list = timeframes ?? [];
  if (!list.length || list.includes('1d')) return '1d';
  return list[0];
}
