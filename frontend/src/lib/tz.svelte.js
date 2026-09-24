/**
 * The timezone clocks are displayed in — the one picked in Settings → Defaults.
 *
 * Timestamps travel as instants (RFC3339) and providers stamp them in whatever offset
 * they please: Interactive Brokers sends the exchange's local offset, the live hub sends
 * UTC. Slicing the string therefore printed two different clocks in one axis. Formatting
 * every label through this zone is what makes them one clock again, and it is the user's
 * clock rather than the browser's.
 *
 * Two-layered like the accent: localStorage answers on load, the backend
 * (`default_timezone` via /settings/defaults) is the source of truth across devices.
 */

const STORAGE_KEY = 'otw-tz';

/** Does Intl know this zone? An unknown name throws, and a throwing formatter would take
 *  down every chart axis with it. */
function valid(zone) {
  if (typeof zone !== 'string' || !zone) return false;
  try {
    new Intl.DateTimeFormat('en', { timeZone: zone });
    return true;
  } catch {
    return false;
  }
}

function readCache() {
  if (typeof localStorage === 'undefined') return null;
  const v = localStorage.getItem(STORAGE_KEY);
  return valid(v) ? v : null;
}

class TzStore {
  /** IANA zone name. UTC until the backend answers — the same default it stores. */
  value = $state(readCache() ?? 'UTC');

  /** Adopt the backend value once it loads; keeps the cache in sync. */
  hydrate(zone) {
    this.set(zone);
  }

  /** Apply a zone chosen in settings, so open charts re-label without a reload. */
  set(zone) {
    const next = valid(zone) ? zone : 'UTC';
    this.value = next;
    if (typeof localStorage !== 'undefined') localStorage.setItem(STORAGE_KEY, next);
  }
}

export const tz = new TzStore();

/**
 * A compact chart-axis stamp in `zone`: "MM-DD HH:MM" intraday, "YYYY-MM-DD" otherwise.
 *
 * Deliberately ISO-ordered and not localized — an axis is read as a scale, not as prose,
 * and the digits have to stay the same width whatever the UI language. Built once per
 * (zone, intraday) pair and reused: this runs per tick on every frame.
 */
export function makeTsFmt(zone, intraday) {
  const f = new Intl.DateTimeFormat('en-CA', {
    timeZone: valid(zone) ? zone : 'UTC',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    ...(intraday ? { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' } : {})
  });
  return (ms) => {
    const p = {};
    for (const part of f.formatToParts(ms)) p[part.type] = part.value;
    return intraday ? `${p.month}-${p.day} ${p.hour}:${p.minute}` : `${p.year}-${p.month}-${p.day}`;
  };
}
