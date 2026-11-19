/** Page state shared by the Chain and Builder tabs. Module level, so a tab switch keeps the
 * loaded chain, the picked underlying and the legs being built. */

export const opt = $state({
  /** Connector id and its provider (`ibkr` | `massive` | `alpaca`). */
  source: null,
  provider: null,
  /** Asset type of the underlying search: `etf` | `equity` | `index`. */
  assetType: 'etf',
  /** The picked symbol-search hit, never free text: `{ symbol, name, asset_type, exchange }`. */
  underlying: null,
  /** Rate and dividend yield as fractions. */
  rate: 0.04,
  dividend: 0,
  /** Answer of GET /options/dividends, and whether the model prices with it. */
  dividends: null,
  useDividends: true,
  /** Answer of GET /options/expiries. */
  expiries: null,
  /** Result of the last chain task, plus where it came from (`source`, `picked` symbol) and
   * what was asked (`asked: { expiries, lo, hi, rate, dividend, strike }`, `strike` the
   * strike filter key). */
  chain: null,
  /** Chain filters and view, kept across tab switches. */
  expiryFilter: { mode: 'month', from: '', to: '', dates: [] },
  strikeFilter: { mode: 'near', n: 10, lo: '', hi: '', list: '' },
  spreadFilter: { mode: 'any', value: 10 },
  view: 'both',
  /** How volatility moves with spot in the Builder: `sticky_strike` | `sticky_delta`. */
  volRegime: 'sticky_strike',
  /** Builder legs: `{ id, kind, strike, expiry, qty, price, iv }` (iv a fraction). */
  legs: [],
  /** Saved strategy currently loaded in the builder, if any. */
  strategyId: null,
  strategyName: ''
});

/** The cash dividends the model prices with, as the API takes them. */
export function cashDividends() {
  if (!opt.useDividends) return [];
  return (opt.dividends?.dividends ?? []).map(({ ex_date, amount }) => ({ ex_date, amount }));
}

/** The yield sent with them: cash dividends replace it, or the same income counts twice. */
export const dividendYield = () => (cashDividends().length ? 0 : opt.dividend);

/** Exercise style of the current underlying: listed index options read as European. */
export const optionStyle = () => opt.chain?.style ?? (opt.assetType === 'index' ? 'european' : 'american');

let seq = 0;
/** A fresh local id for a leg row. */
export const legId = () => `leg${Date.now().toString(36)}${(seq++).toString(36)}`;

/** Price a leg pays per unit from a chain row: the ask when buying, the bid when selling,
 * the mid or last trade when that side is missing. */
export function legPrice(row, qty) {
  if (!row) return null;
  const side = qty >= 0 ? row.ask : row.bid;
  return side ?? row.mid ?? row.last ?? null;
}

/** Add a chain contract to the builder at the right side of the quote. */
export function addContract(row, qty) {
  opt.legs.push({
    id: legId(),
    kind: row.call ? 'call' : 'put',
    strike: row.strike,
    expiry: row.expiry,
    qty,
    price: legPrice(row, qty),
    iv: row.iv_mid ?? row.provider_iv ?? null
  });
}

/** The first field a leg still needs before it can be evaluated or saved (`price`, `strike`,
 * `expiry`), or null when it is complete. */
export function legMissing(leg) {
  const p = leg.price;
  if (p == null || p === '' || !Number.isFinite(Number(p))) return 'price';
  if (leg.kind === 'stock') return null;
  if (leg.strike == null) return 'strike';
  if (!leg.expiry) return 'expiry';
  return null;
}
