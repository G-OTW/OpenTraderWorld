/** MyWealth display helpers: money that honours the privacy screen and reads an unknown
 *  amount as a word, never as 0 or a dash. Reactive where they are read: `privacy` is a
 *  rune, so a template calling these repaints when the eye is flipped. */
import { fmtMoney, fmtSignedMoney, fmtCompactValue, fmtPct } from '$lib/format.js';
import { privacy } from '$lib/theme/privacy.svelte.js';
import { MASK } from '$lib/ui/charts/palette.js';

const known = (v) => typeof v === 'number' && Number.isFinite(v);

/** Money, or `na` when unknown, or the mask under the privacy screen. */
export function money(v, ccy, na = 'n/a') {
  if (!known(v)) return na;
  return privacy.hidden ? MASK : fmtMoney(v, ccy);
}

/** Money with its sign always printed (+ or U+2212 minus). */
export function signedMoney(v, ccy, na = 'n/a') {
  if (!known(v)) return na;
  return privacy.hidden ? MASK : fmtSignedMoney(v, ccy);
}

/** Compact money for an axis: $2.2M. */
export function compactMoney(v, ccy) {
  if (!known(v)) return '';
  return privacy.hidden ? MASK : fmtCompactValue(v, ccy);
}

/** Percent, or `na` when unknown. A share reveals no amount, so it is never masked. */
export function pct(v, digits = 1, na = 'n/a') {
  return known(v) ? fmtPct(v, digits) : na;
}

/** Linked kinds are read, not typed: a portfolio, a broker account, a wallet. */
export const isLinked = (a) => a?.portfolio_id != null || a?.broker_account_id != null || a?.asset_type === 'wallet';
