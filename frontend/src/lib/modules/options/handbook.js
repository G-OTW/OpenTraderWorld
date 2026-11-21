/** Strategy handbook: each template is a list of legs relative to the chain, built from the
 * at-the-money strike and the nearest expiries when applied.
 *
 * A leg: `kind` call | put | stock, `off` strike steps from ATM (in the expiry's own strike
 * list), `exp` index into the chain's expiries (0 nearest), `qty` signed (+ long, - short;
 * a stock leg counts shares, 100 covering one contract). */
import { chainStrikes, chainExpiries, findRow, nearestIndex } from './chain.js';
import { legId, legPrice } from './store.svelte.js';

export const OUTLOOKS = ['bullish', 'bearish', 'neutral'];

const C = (off, qty, exp = 0) => ({ kind: 'call', off, qty, exp });
const P = (off, qty, exp = 0) => ({ kind: 'put', off, qty, exp });
const S = (qty) => ({ kind: 'stock', qty });

export const HANDBOOK = [
  { id: 'long_call', outlook: 'bullish', legs: [C(0, 1)] },
  { id: 'long_put', outlook: 'bearish', legs: [P(0, 1)] },
  { id: 'covered_call', outlook: 'bullish', legs: [S(100), C(1, -1)] },
  { id: 'cash_secured_put', outlook: 'bullish', legs: [P(-1, -1)] },
  { id: 'protective_put', outlook: 'bullish', legs: [S(100), P(-1, 1)] },
  { id: 'collar', outlook: 'neutral', legs: [S(100), P(-1, 1), C(1, -1)] },
  { id: 'bull_call_spread', outlook: 'bullish', legs: [C(0, 1), C(2, -1)] },
  { id: 'bear_put_spread', outlook: 'bearish', legs: [P(0, 1), P(-2, -1)] },
  { id: 'bull_put_spread', outlook: 'bullish', legs: [P(0, -1), P(-2, 1)] },
  { id: 'bear_call_spread', outlook: 'bearish', legs: [C(0, -1), C(2, 1)] },
  { id: 'long_straddle', outlook: 'neutral', legs: [C(0, 1), P(0, 1)] },
  { id: 'short_straddle', outlook: 'neutral', legs: [C(0, -1), P(0, -1)] },
  { id: 'long_strangle', outlook: 'neutral', legs: [C(2, 1), P(-2, 1)] },
  { id: 'short_strangle', outlook: 'neutral', legs: [C(2, -1), P(-2, -1)] },
  { id: 'iron_condor', outlook: 'neutral', legs: [P(-4, 1), P(-2, -1), C(2, -1), C(4, 1)] },
  { id: 'iron_butterfly', outlook: 'neutral', legs: [P(-3, 1), P(0, -1), C(0, -1), C(3, 1)] },
  { id: 'long_butterfly', outlook: 'neutral', legs: [C(-2, 1), C(0, -2), C(2, 1)] },
  { id: 'calendar', outlook: 'neutral', legs: [C(0, -1, 0), C(0, 1, 1)] },
  { id: 'diagonal', outlook: 'bullish', legs: [C(2, -1, 0), C(0, 1, 1)] },
  { id: 'ratio_spread', outlook: 'neutral', legs: [C(0, 1), C(2, -2)] }
];

/** Builder legs for a template on the loaded chain, or throws an Error whose message is an
 * i18n key (`options.handbook.err.*`) the caller translates. */
export function applyTemplate(tpl, chain) {
  const rows = chain?.rows ?? [];
  const spot = chain?.spot;
  if (!rows.length || spot == null) throw new Error('options.handbook.err.noChain');
  const expiries = chainExpiries(rows);
  const need = Math.max(0, ...tpl.legs.filter((l) => l.kind !== 'stock').map((l) => l.exp));
  if (need >= expiries.length) throw new Error('options.handbook.err.expiries');
  return tpl.legs.map((l) => {
    if (l.kind === 'stock') return { id: legId(), kind: 'stock', strike: null, expiry: null, qty: l.qty, price: spot, iv: null };
    const expiry = expiries[l.exp];
    const strikes = chainStrikes(rows, expiry);
    const i = nearestIndex(strikes, spot) + l.off;
    if (i < 0 || i >= strikes.length) throw new Error('options.handbook.err.strikes');
    const row = findRow(rows, expiry, strikes[i], l.kind === 'call');
    return {
      id: legId(),
      kind: l.kind,
      strike: strikes[i],
      expiry,
      qty: l.qty,
      price: legPrice(row, l.qty),
      iv: row?.iv_mid ?? row?.provider_iv ?? null
    };
  });
}
