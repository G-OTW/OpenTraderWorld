/**
 * The chart's time scale: which bars carry a label, and what each one says.
 *
 * Every label used to be a full "MM-DD HH:MM" stamp picked every N bars, so the axis read
 * as a wall of near-identical digits. Here a label says only what changed at that bar,
 * and the bars that get one sit on round calendar steps:
 *
 * - The step is the finest one (1m, 5m, 15m, 1h, 6h, 1D, 7D, 1M, 3M, 1Y, ...) whose
 *   boundaries, counted on the bars actually in view, still leave room for a label each.
 *   Counting bars instead of dividing the time span is what keeps a session chart honest:
 *   nights and weekends have no bars, so they take no room.
 * - A label names the largest unit that rolled over: "2026" > "Oct" > "Mon 27" > "06:00".
 * - When two labels would collide, the larger unit wins, so a day or a month change is
 *   never hidden behind an hour.
 *
 * Intraday stamps are instants shown in the zone picked in Settings → Defaults; daily and
 * coarser stamps are calendar dates and are read as such (no zone shift).
 */

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const HOUR = 3600000;
const DAY = 86400000;

// Rank of the unit a label names. A collision keeps the higher rank.
const R_MINUTE = 1;
const R_HOUR = 2;
const R_DAY = 3;
const R_MONTH = 4;
const R_YEAR = 5;

const pad2 = (x) => String(x).padStart(2, '0');

/** Zone offset (ms) at an instant, cached per hour: DST moves on whole hours. */
const offsetCache = new Map();
function offsetFn(zone) {
  let fn = offsetCache.get(zone);
  if (fn) return fn;
  let f;
  try {
    f = new Intl.DateTimeFormat('en-US', {
      timeZone: zone,
      hourCycle: 'h23',
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: 'numeric',
      minute: 'numeric'
    });
  } catch {
    f = null;
  }
  const memo = new Map();
  fn = (ms) => {
    if (!f) return 0;
    const h = Math.floor(ms / HOUR);
    let off = memo.get(h);
    if (off === undefined) {
      const p = {};
      for (const x of f.formatToParts(h * HOUR)) p[x.type] = x.value;
      off = Date.UTC(+p.year, +p.month - 1, +p.day, +p.hour, +p.minute) - h * HOUR;
      memo.set(h, off);
    }
    return off;
  };
  offsetCache.set(zone, fn);
  return fn;
}

/**
 * Wall-clock fields of one axis stamp, or null when it is not a date (a "+3" pad slot).
 * `t` is the wall clock as a UTC-epoch number, so plain arithmetic gives day numbers.
 */
export function stampParts(stamp, intraday, zone) {
  if (typeof stamp !== 'string' || stamp.length < 10) return null;
  let t;
  if (intraday) {
    const ms = Date.parse(stamp);
    if (!Number.isFinite(ms)) return null;
    t = ms + offsetFn(zone)(ms);
  } else {
    t = Date.parse(stamp.slice(0, 10) + 'T00:00:00Z');
    if (!Number.isFinite(t)) return null;
  }
  const d = new Date(t);
  return {
    y: d.getUTCFullYear(),
    mo: d.getUTCMonth(),
    d: d.getUTCDate(),
    dow: d.getUTCDay(),
    min: d.getUTCHours() * 60 + d.getUTCMinutes(),
    day: Math.floor(t / DAY)
  };
}

/**
 * Crosshair / tooltip / legend stamp: "Mon 27 Oct '26 06:56" intraday, "Mon 27 Oct 2026"
 * otherwise. Falls back to the raw string when it is not a date.
 */
export function makeStampFmt(zone, intraday) {
  return (stamp) => {
    const p = stampParts(stamp, intraday, zone);
    if (!p) return stamp ?? '';
    const date = `${DAYS[p.dow]} ${p.d} ${MONTHS[p.mo]}`;
    if (!intraday) return `${date} ${p.y}`;
    return `${date} '${pad2(p.y % 100)} ${pad2(Math.floor(p.min / 60))}:${pad2(p.min % 60)}`;
  };
}

// Candidate steps, finest first. Each maps a bar to a bucket key; a label goes where the
// key changes. Every key folds in the larger units, so a day or month change always ends
// a bucket, whatever the step.
const minuteStep = (k) => (p) => p.day * 1440 + Math.floor(p.min / k);
const dayStep = (k) => (p) => (p.y * 12 + p.mo) * 32 + Math.floor((p.d - 1) / k);
const monthStep = (k) => (p) => p.y * 12 + Math.floor(p.mo / k);
const yearStep = (k) => (p) => Math.floor(p.y / k);
const INTRADAY_STEPS = [1, 2, 5, 10, 15, 30, 60, 120, 180, 240, 360, 720].map(minuteStep);
const DATE_STEPS = [
  ...[1, 2, 3, 5, 7, 10, 15].map(dayStep),
  ...[1, 2, 3, 6].map(monthStep),
  ...[1, 2, 5, 10, 20, 50, 100].map(yearStep)
];

/** What a label at bar `p` says, given the bar before it. */
function labelFor(p, prev, intraday) {
  if (p.y !== prev.y) return { text: String(p.y), rank: R_YEAR };
  if (p.mo !== prev.mo) return { text: MONTHS[p.mo], rank: R_MONTH };
  if (p.day !== prev.day) return { text: intraday ? `${DAYS[p.dow]} ${p.d}` : String(p.d), rank: R_DAY };
  const text = `${pad2(Math.floor(p.min / 60))}:${pad2(p.min % 60)}`;
  return { text, rank: p.min % 60 === 0 ? R_HOUR : R_MINUTE };
}

/**
 * Labels for the bars `s..e` (inclusive indices into `cats`) on an axis `width` px wide.
 * Returns a Map from category string to `{ text, strong }`; `strong` marks a date change
 * (a day on an intraday chart, a month or year on a daily one).
 */
export function timeTicks(cats, s, e, width, fontPx, intraday, zone) {
  const out = new Map();
  const lo = Math.max(0, s);
  const hi = Math.min(cats.length - 1, e);
  if (hi <= lo || width <= 0) return out;
  const bars = hi - lo + 1;
  const pxPerBar = width / bars;

  // Wall-clock fields for the window plus the bar just before it (the first label in view
  // needs to know what changed).
  const from = Math.max(0, lo - 1);
  const parts = [];
  for (let i = from; i <= hi; i++) parts.push(stampParts(cats[i], intraday, zone));

  // Room per label: a 6-character label plus a gap that keeps the scale airy.
  const charPx = fontPx * 0.62;
  const budget = Math.max(1, Math.floor(width / (charPx * 6 + fontPx * 4)));

  const steps = intraday ? [...INTRADAY_STEPS, ...DATE_STEPS] : DATE_STEPS;
  let marks = [];
  for (const key of steps) {
    marks = [];
    for (let i = Math.max(lo, 1); i <= hi; i++) {
      const p = parts[i - from];
      const q = parts[i - from - 1];
      if (p && q && key(p) !== key(q)) marks.push(i);
    }
    if (marks.length <= budget) break;
  }

  // Collisions: larger units first, then left to right; a label is kept only when it
  // clears every label already kept.
  const cand = marks.map((i) => {
    const l = labelFor(parts[i - from], parts[i - from - 1], intraday);
    return { i, ...l, x: (i - lo + 0.5) * pxPerBar, half: (l.text.length * charPx) / 2 };
  });
  cand.sort((a, b) => b.rank - a.rank || a.i - b.i);
  const kept = [];
  for (const c of cand) {
    if (kept.every((k) => Math.abs(k.x - c.x) >= k.half + c.half + fontPx)) kept.push(c);
  }
  const strongAt = intraday ? R_DAY : R_MONTH;
  kept.sort((a, b) => a.i - b.i);
  for (const k of kept) out.set(cats[k.i], { text: k.text, strong: k.rank >= strongAt });
  return out;
}
