/**
 * Shared dashboard refresh pulse.
 *
 * Widgets fetch once on mount, so a dashboard left open goes stale: quotes, unread mail
 * and running timers keep moving server-side. `livePulse(secs)` gives a widget a counter
 * to read inside its data effect, so the effect re-runs on a timer.
 *
 * One interval per period, shared by every widget that asks for it, not one per widget:
 * ten widgets on a 60s pulse are one timer and ten refetches, in the same tick.
 *
 * A hidden tab stops its timers entirely (a dashboard left open in a background tab must
 * not poll for hours), and refetches on return only when it has been away long enough to
 * have missed a tick.
 *
 * This refreshes fetched data only. Widget-local state (a half-filled quick-add form, an
 * open picker) lives in its own `$state` and is never touched by a refetch.
 */

const PERIODS = new Map();

class Ticker {
  n = $state(0);
  #secs;
  #refs = 0;
  #handle = null;
  #hiddenAt = 0;

  constructor(secs) {
    this.#secs = secs;
  }

  #start() {
    if (this.#handle || document.hidden) return;
    this.#handle = setInterval(() => (this.n += 1), this.#secs * 1000);
  }

  #stop() {
    if (this.#handle) clearInterval(this.#handle);
    this.#handle = null;
  }

  #vis = () => {
    if (document.hidden) {
      this.#hiddenAt = Date.now();
      this.#stop();
      return;
    }
    if (Date.now() - this.#hiddenAt >= this.#secs * 1000) this.n += 1;
    this.#start();
  };

  /** Register one widget; returns the release function for the effect's cleanup. */
  retain() {
    if (this.#refs++ === 0) {
      document.addEventListener('visibilitychange', this.#vis);
      this.#start();
    }
    return () => {
      if (--this.#refs === 0) {
        this.#stop();
        document.removeEventListener('visibilitychange', this.#vis);
      }
    };
  }
}

/**
 * Call at component init, then read `.n` inside the data effect to make it re-run:
 *
 *   const live = livePulse(LIVE.quotes);
 *   $effect(() => { live.n; if (!editing) load(); });
 */
export function livePulse(secs) {
  if (typeof document === 'undefined') return { n: 0 };
  let tk = PERIODS.get(secs);
  if (!tk) PERIODS.set(secs, (tk = new Ticker(secs)));
  $effect(() => tk.retain());
  return tk;
}

/**
 * Refresh periods, in seconds, ordered by how fast the underlying data moves. These are
 * short on purpose: a self-hosted single-user install talks to a local Postgres, so a tick
 * is a handful of indexed reads, not a broker call. The quote pulse does not make prices
 * fresher than the scheduler writes them, it just shows the newest row sooner.
 */
export const LIVE = {
  runs: 3, // automator runs, running timers
  quotes: 6, // watchlist / portfolio quotes
  inbox: 12, // news items, mail
  agenda: 30 // calendar events
};
