/**
 * The bookkeeping shared by the two ways a notification reaches the app: the push stream
 * (one row per event, the moment it is inserted) and the unread poll (a batch, the
 * reconciliation path). Both feed `takeFresh`, so a row toasts once whichever path brings it
 * first, and `ringGate`, so alerts arriving together ring once.
 */

/** Ids remembered for dedupe. Far more than ever toast between two polls. */
const SEEN_MAX = 500;

const at = (iso) => {
  const t = Date.parse(iso ?? '');
  return Number.isNaN(t) ? -Infinity : t;
};

/**
 * The rows of `incoming` not toasted yet, in order, and the `since` to poll from next.
 *
 * `seen` (a Set of ids) is updated in place and kept under `SEEN_MAX`, oldest first out.
 * A row already read elsewhere is remembered but not returned: the poll never returns one,
 * and the stream must not toast what the poll would not. `since` only moves forward.
 */
export function takeFresh(incoming, seen, since) {
  const fresh = [];
  let next = since;
  for (const n of incoming ?? []) {
    if (!n?.id) continue;
    if (at(n.created_at) > at(next)) next = n.created_at;
    if (seen.has(n.id)) continue;
    seen.add(n.id);
    if (!n.read_at) fresh.push(n);
  }
  while (seen.size > SEEN_MAX) seen.delete(seen.values().next().value);
  return { fresh, since: next };
}

/**
 * A leading-edge throttle for the alert sound: the first call rings, calls within
 * `windowMs` after it stay silent (the sounds last a second or less), so alerts fired
 * together ring together, not over each other, whichever path delivered them.
 */
export function ringGate(windowMs = 1000) {
  let last = -Infinity;
  return (now = Date.now()) => {
    if (now - last < windowMs) return false;
    last = now;
    return true;
  };
}
