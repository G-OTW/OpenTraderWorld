/** Supervised server-sent event stream: the one way the UI keeps a live connection open.
 *
 *   const s = liveStream({
 *     url: () => api.streamsUrl(currentRequests()),   // re-read on every (re)connect
 *     events: { bar: onBar, status: onStatus },        // named events → handler(parsed data, raw event)
 *     onstate: (state, info) => …                      // 'connecting' | 'open' | 'reconnecting' | 'paused'
 *   });
 *   s.close();
 *
 * What `EventSource` alone gets wrong, and this fixes for every stream at once:
 *  - **A refused reconnect is final.** The browser retries a dropped connection by itself,
 *    but gives up for good on any non-200 answer (a restarting server, an expired session, a
 *    full slot) and says nothing. Here every error closes the source and reconnects on our
 *    own schedule, with backoff, forever, until `close()`.
 *  - **The native retry reuses the old URL.** A live chart puts its newest bar in the URL
 *    (`since`) so the server replays what closed meanwhile; `url` is a function, re-read on
 *    each attempt, so the replay starts from what the page actually holds.
 *  - **A silent connection looks healthy.** The server sends a named `ping` every 15 s
 *    (`core/otw-core/src/sse.rs`); when no event at all has arrived for `idleMs`, the stream
 *    is treated as dead and reconnected.
 *  - **A hidden tab keeps its slot.** Over plain HTTP/1.1 (the `local` and `lan` modes) a
 *    browser allows six connections to one host across every tab, and each open stream is
 *    one of them. With a few tabs open the slots run out and every other request, a form
 *    save included, waits until some stream closes. So a tab hidden for `HIDDEN_CLOSE_MS`
 *    closes its stream (state `paused`) and reconnects the moment it is shown again; `url`
 *    is re-read then, so a chart replays what it missed from `since`.
 *
 * `onstate` lets each page say so where its user is looking (a pane's status line, a dot);
 * the stream itself never shows UI. */

const BACKOFF_MS = [1000, 2000, 4000, 8000, 15000, 30000];
/** How long a tab stays hidden before its stream lets go of the connection: a glance at
 *  another tab costs no reconnect. */
const HIDDEN_CLOSE_MS = 10000;

const hidden = () => typeof document !== 'undefined' && document.visibilityState === 'hidden';

export function liveStream({ url, events = {}, onstate, idleMs = 45000 }) {
  let es = null;
  let closed = false;
  let attempt = 0;
  let retryTimer = null;
  let lastEvent = 0;
  let watchdog = null;
  let paused = false;
  let hideTimer = null;

  const state = (s, info = {}) => {
    try {
      onstate?.(s, info);
    } catch {
      /* a page's state handler must not take the stream down */
    }
  };

  function touch() {
    lastEvent = Date.now();
  }

  function open() {
    if (closed || paused) return;
    let href;
    try {
      href = url();
    } catch {
      href = null;
    }
    if (!href) return;
    if (!attempt) state('connecting');
    const src = new EventSource(href);
    es = src;
    touch();
    src.addEventListener('open', () => {
      if (es !== src) return;
      attempt = 0;
      touch();
      state('open');
    });
    src.addEventListener('ping', touch);
    for (const [name, handler] of Object.entries(events)) {
      src.addEventListener(name, (e) => {
        if (es !== src) return;
        touch();
        let data = null;
        try {
          data = e.data ? JSON.parse(e.data) : null;
        } catch {
          return; // a malformed frame is skipped, the next one restores the stream
        }
        try {
          handler(data, e);
        } catch {
          /* one bad handler call must not stop the next frame */
        }
      });
    }
    src.addEventListener('error', () => {
      if (es !== src) return;
      drop('error');
    });
  }

  /** Close the current connection and schedule the next attempt. */
  function drop(reason) {
    es?.close();
    es = null;
    if (closed || paused) return;
    const wait = BACKOFF_MS[Math.min(attempt, BACKOFF_MS.length - 1)];
    attempt += 1;
    state('reconnecting', { attempt, reason, retryInMs: wait });
    clearTimeout(retryTimer);
    retryTimer = setTimeout(open, wait);
  }

  watchdog = setInterval(() => {
    if (es && Date.now() - lastEvent > idleMs) drop('idle');
  }, 5000);

  function onVisibility() {
    if (closed) return;
    clearTimeout(hideTimer);
    if (hidden()) {
      hideTimer = setTimeout(() => {
        if (closed || !hidden()) return;
        paused = true;
        clearTimeout(retryTimer);
        es?.close();
        es = null;
        state('paused');
      }, HIDDEN_CLOSE_MS);
    } else if (paused) {
      paused = false;
      attempt = 0;
      open();
    }
  }
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibility);

  open();
  onVisibility();

  return {
    close() {
      closed = true;
      clearTimeout(retryTimer);
      clearTimeout(hideTimer);
      clearInterval(watchdog);
      if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibility);
      es?.close();
      es = null;
    },
    /** Reconnect now (a page coming back to the foreground, a manual retry). */
    reconnect() {
      if (closed || paused) return;
      es?.close();
      es = null;
      clearTimeout(retryTimer);
      attempt = 0;
      open();
    }
  };
}
