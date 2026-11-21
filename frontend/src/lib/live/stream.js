/** Supervised live stream: the one way the UI keeps a live feed open.
 *
 *   const s = liveStream({
 *     url: () => api.streamsUrl(currentRequests()),   // re-read on every (re)subscribe
 *     events: { bar: onBar, status: onStatus },        // named events → handler(parsed data, { type, data })
 *     onstate: (state, info) => …                      // 'connecting' | 'open' | 'reconnecting' | 'refused'
 *   });
 *   s.close();
 *
 * **One socket per tab.** Every stream of the tab is a subscription on one WebSocket
 * (`/api/live/ws`, protocol in `core/otw-core/src/live_ws.rs`), opened with the first
 * subscription and closed with the last. `url` is the stream's SSE route (path and query);
 * the server serves the same stream on the socket. Over plain HTTP/1.1 (the `local` and
 * `lan` modes) a browser allows six connections to one host across every tab, and one
 * EventSource per stream used to spend them: two chart windows were enough to make a form
 * save wait. One socket per tab leaves the other five to requests, so a hidden tab keeps
 * receiving its data.
 *
 * What this fixes for every stream at once:
 *  - **A dropped socket comes back.** Closed, failed or refused at the upgrade (a
 *    restarting server, an expired session), the socket is reopened on our own schedule,
 *    with backoff, forever, while one stream is open, and every live subscription is sent
 *    again.
 *  - **Each resubscribe re-reads `url`.** A live chart puts its newest bar in the URL
 *    (`since`) so the server replays what closed meanwhile; the replay starts from what the
 *    page holds at that moment.
 *  - **A silent socket looks healthy.** The server pings every 15 s
 *    (`core/otw-core/src/sse.rs`); when no frame at all has arrived for `idleMs`, the socket
 *    is treated as dead and reconnected.
 *  - **A stream that ends is resubscribed** with backoff, as a closed SSE response was.
 *  - **A refused subscription is final.** The route said no (a bad query, a connector not
 *    granted, an unknown path): state `refused` with `{ status, message }`, and it is not
 *    retried until `reconnect()`. A refusal that may pass later (429: the demo's live
 *    budget is full; 5xx) is retried with backoff instead, as `reconnecting`.
 *
 * `onstate` lets each page say so where its user is looking (a pane's status line, a dot);
 * the stream itself never shows UI. `open` means the server accepted this subscription. */

const BACKOFF_MS = [1000, 2000, 4000, 8000, 15000, 30000];
const SOCKET_PATH = '/api/live/ws';

const backoff = (attempt) => BACKOFF_MS[Math.min(attempt, BACKOFF_MS.length - 1)];
/** A refusal that may pass later: the demo's live budget, a server error. */
const transient = (status) => status === 429 || status >= 500;

// ── The tab's socket (module singleton) ──────────────────────────────────────

/** Every open `liveStream`, refused ones included (they wait for `reconnect()`). */
const subs = new Set();
/** Wire id → subscription. A fresh wire id per subscribe, so a frame of an old attempt
 *  never reaches the new one. */
const byWire = new Map();
let socket = null;
let ready = false;
let attempt = 0;
let retryTimer = null;
let watchdog = null;
let lastFrame = 0;
let wireSeq = 0;

function socketUrl() {
  const { protocol, host } = window.location;
  return `${protocol === 'https:' ? 'wss:' : 'ws:'}//${host}${SOCKET_PATH}`;
}

function touch() {
  lastFrame = Date.now();
}

function openSocket() {
  retryTimer = null;
  if (socket || !subs.size) return;
  const ws = new WebSocket(socketUrl());
  socket = ws;
  ready = false;
  touch();
  if (!watchdog) watchdog = setInterval(checkIdle, 5000);
  ws.onopen = () => {
    if (socket !== ws) return;
    ready = true;
    attempt = 0;
    touch();
    for (const sub of subs) if (!sub.refused && !sub.timer) subscribe(sub);
  };
  ws.onmessage = (e) => {
    if (socket !== ws) return;
    touch();
    let f;
    try {
      f = JSON.parse(e.data);
    } catch {
      return; // a malformed frame is skipped, the next one restores the stream
    }
    if (f?.op === 'ping') return;
    const sub = byWire.get(f?.id);
    if (sub) receive(sub, f);
  };
  ws.onclose = () => {
    if (socket === ws) drop('closed');
  };
  ws.onerror = () => {
    if (socket === ws) drop('error');
  };
}

/** Close the socket and schedule the next one; every subscription goes with it. */
function drop(reason) {
  const ws = socket;
  socket = null;
  ready = false;
  if (ws) {
    ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null;
    try {
      ws.close();
    } catch {
      /* already closing */
    }
  }
  byWire.clear();
  const live = [...subs].filter((s) => !s.refused);
  for (const sub of subs) {
    sub.wire = null;
    clearTimeout(sub.timer);
    sub.timer = null;
  }
  if (!live.length) return stopIfIdle();
  const wait = backoff(attempt);
  attempt += 1;
  for (const sub of live) state(sub, 'reconnecting', { attempt, reason, retryInMs: wait });
  clearTimeout(retryTimer);
  retryTimer = setTimeout(openSocket, wait);
}

function checkIdle() {
  if (!socket) return;
  const idle = Math.min(...[...subs].map((s) => s.idleMs));
  if (Date.now() - lastFrame > idle) drop('idle');
}

/** No live subscription left: let go of the socket and the timers. */
function stopIfIdle() {
  if ([...subs].some((s) => !s.refused)) return;
  clearTimeout(retryTimer);
  retryTimer = null;
  clearInterval(watchdog);
  watchdog = null;
  attempt = 0;
  const ws = socket;
  socket = null;
  ready = false;
  byWire.clear();
  if (ws) {
    ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null;
    try {
      ws.close(1000);
    } catch {
      /* already closing */
    }
  }
}

/** Ask for a socket now, unless one is open, opening, or already scheduled. */
function ensureSocket() {
  if (!socket && !retryTimer) openSocket();
}

// ── One subscription ─────────────────────────────────────────────────────────

function state(sub, s, info = {}) {
  try {
    sub.onstate?.(s, info);
  } catch {
    /* a page's state handler must not take the stream down */
  }
}

/** The stream's route as a path with its query, whatever form `url()` returned. */
function routeOf(sub) {
  try {
    const href = sub.url();
    if (!href) return null;
    const u = new URL(href, window.location.href);
    return u.pathname + u.search;
  } catch {
    return null;
  }
}

function subscribe(sub) {
  if (!ready || sub.closed || sub.refused) return;
  const path = routeOf(sub);
  if (!path) return;
  if (sub.wire) unsubscribe(sub);
  wireSeq += 1;
  sub.wire = `s${wireSeq}`;
  byWire.set(sub.wire, sub);
  socket.send(JSON.stringify({ op: 'subscribe', id: sub.wire, path }));
}

function unsubscribe(sub) {
  if (!sub.wire) return;
  byWire.delete(sub.wire);
  if (ready) socket.send(JSON.stringify({ op: 'unsubscribe', id: sub.wire }));
  sub.wire = null;
}

/** Subscribe again after a backoff: the stream ended, or a refusal that may pass. */
function retryLater(sub, reason, info = {}) {
  byWire.delete(sub.wire);
  sub.wire = null;
  const wait = backoff(sub.attempt);
  sub.attempt += 1;
  state(sub, 'reconnecting', { ...info, attempt: sub.attempt, reason, retryInMs: wait });
  clearTimeout(sub.timer);
  sub.timer = setTimeout(() => {
    sub.timer = null;
    subscribe(sub);
  }, wait);
}

function receive(sub, f) {
  const name = f.event;
  if (name === 'open') {
    sub.attempt = 0;
    state(sub, 'open');
    return;
  }
  if (name === 'end') return retryLater(sub, 'end');
  if (name === 'error') {
    const status = f.data?.status ?? 0;
    const message = f.data?.message ?? '';
    if (transient(status)) return retryLater(sub, 'refused', { status, message });
    byWire.delete(sub.wire);
    sub.wire = null;
    sub.refused = true;
    state(sub, 'refused', { status, message });
    stopIfIdle();
    return;
  }
  const handler = sub.events[name];
  if (!handler) return;
  try {
    handler(f.data ?? null, { type: name, data: f.data ?? null });
  } catch {
    /* one bad handler call must not stop the next frame */
  }
}

export function liveStream({ url, events = {}, onstate, idleMs = 45000 }) {
  const sub = { url, events, onstate, idleMs, wire: null, attempt: 0, timer: null, refused: false, closed: false };
  subs.add(sub);
  state(sub, 'connecting');
  if (ready) subscribe(sub);
  else ensureSocket();

  return {
    close() {
      if (sub.closed) return;
      sub.closed = true;
      clearTimeout(sub.timer);
      sub.timer = null;
      unsubscribe(sub);
      subs.delete(sub);
      stopIfIdle();
    },
    /** Subscribe again now (a manual retry), a refused stream included. */
    reconnect() {
      if (sub.closed) return;
      clearTimeout(sub.timer);
      sub.timer = null;
      sub.refused = false;
      sub.attempt = 0;
      if (ready) return subscribe(sub);
      clearTimeout(retryTimer);
      retryTimer = null;
      attempt = 0;
      ensureSocket();
    }
  };
}
