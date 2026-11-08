/**
 * Client-side FX conversion for display-currency views.
 *
 * Mirrors the Rust `journal_fx::convert` cross-rate math: rates are USD-based
 * (1 USD = `rate` units of the quote), so `from -> to` is `amount / from_rate * to_rate`.
 * A missing quote yields `null` — callers fall back to the native amount rather than
 * rendering a blank cell, matching the backend's `unconverted` behaviour.
 *
 * The row's own currency stays the reference; this only affects what is displayed.
 */
import { redirectIfUnauthorized } from '$lib/auth.js';

/** Today's date as `YYYY-MM-DD` in local time (matches the backend's `today`). */
export function todayIso() {
  const d = new Date();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${m}-${day}`;
}

/** Shift an ISO date by `days` (negative = earlier). */
function shiftIso(iso, days) {
  const d = new Date(iso + 'T00:00:00');
  d.setDate(d.getDate() + days);
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${m}-${day}`;
}

/** Fetch the USD-based rate map for one exact date. Returns {} when unavailable. */
async function fetchRatesExact(date) {
  try {
    const res = await fetch(`/api/journal/fx/rates/${date}`, {
      headers: { 'content-type': 'application/json' }
    });
    redirectIfUnauthorized(res);
    if (!res.ok) return {};
    const body = await res.json();
    const rows = body?.rates ?? [];
    // The endpoint returns rows ({ quote, rate, ... }); collapse to a { quote: rate } map.
    if (Array.isArray(rows)) {
      const map = {};
      for (const r of rows) if (r?.quote && Number.isFinite(r.rate)) map[r.quote] = r.rate;
      return map;
    }
    return rows;
  } catch {
    return {};
  }
}

/**
 * Fetch rates with carry-forward, mirroring the backend's `rate_date <= $date` lookup.
 * `/journal/fx/rates/{date}` is a strict exact-date query, so it is empty on weekends and
 * holidays — walk back up to `maxBack` days to the last business close, as the server does.
 */
export async function fetchRates(date = todayIso(), maxBack = 10) {
  let d = date;
  for (let i = 0; i <= maxBack; i++) {
    const map = await fetchRatesExact(d);
    if (Object.keys(map).length > 0) return map;
    d = shiftIso(d, -1);
  }
  return {};
}

/**
 * Convert `amount` from one currency to another using a USD-based rate map.
 * Returns null when either side has no rate (never a silently wrong number).
 */
export function convert(amount, from, to, rates) {
  if (amount == null || !Number.isFinite(amount)) return null;
  if (!from || !to || from === to) return amount;
  const fromRate = from === 'USD' ? 1 : rates?.[from];
  const toRate = to === 'USD' ? 1 : rates?.[to];
  if (!Number.isFinite(fromRate) || !Number.isFinite(toRate) || fromRate === 0) return null;
  return (amount / fromRate) * toRate;
}

/**
 * Reactive rate store: refetches on demand, exposes a `convert` bound to the loaded map.
 * Rates are date-keyed and change rarely, so one load per page mount is enough — switching
 * the display currency re-derives from the same map without a refetch.
 */
export function createFxRates() {
  let rates = $state({});
  let loaded = $state(false);

  return {
    get rates() {
      return rates;
    },
    get loaded() {
      return loaded;
    },
    async load(date = todayIso()) {
      rates = await fetchRates(date);
      loaded = true;
      return rates;
    },
    /** Convert into `to`, or null if no rate is available. */
    to(amount, from, to) {
      return convert(amount, from, to, rates);
    }
  };
}
