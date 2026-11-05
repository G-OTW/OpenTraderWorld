/** Extra clocks of a workspace: a list of IANA zones shown in the chart's bottom bar. */

/** Whether the browser knows `tz` as a time zone. */
export function validZone(tz) {
  if (typeof tz !== 'string' || !tz) return false;
  try {
    new Intl.DateTimeFormat('en-GB', { timeZone: tz });
    return true;
  } catch {
    return false;
  }
}

/** The short name a clock is labelled with: the city of `Area/City`, or the zone itself. */
export function cityOf(tz) {
  const last = String(tz ?? '').split('/').pop();
  return (last || String(tz ?? '')).replaceAll('_', ' ');
}

/** The stored list, cleaned: known zones only, each once. */
export function clocksIn(list) {
  if (!Array.isArray(list)) return [];
  const seen = new Set();
  return list
    .map((c) => (typeof c === 'string' ? { tz: c } : c))
    .filter((c) => validZone(c?.tz) && !seen.has(c.tz) && seen.add(c.tz))
    .map((c) => ({ tz: c.tz }));
}
