// The tab's live socket: every stream is a subscription on one WebSocket, resent on
// reconnect from a re-read URL, and the socket goes with the last stream.
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

class MockSocket {
  static all = [];
  constructor(url) {
    this.url = url;
    this.sent = [];
    this.closed = false;
    MockSocket.all.push(this);
  }
  send(text) {
    this.sent.push(JSON.parse(text));
  }
  close() {
    this.closed = true;
  }
  // Server side.
  accept() {
    this.onopen?.();
  }
  frame(f) {
    this.onmessage?.({ data: JSON.stringify(f) });
  }
  drop() {
    this.onclose?.();
  }
  subscribed() {
    return this.sent.filter((m) => m.op === 'subscribe');
  }
}

const last = () => MockSocket.all.at(-1);
let liveStream;

beforeEach(async () => {
  vi.useFakeTimers();
  MockSocket.all = [];
  vi.stubGlobal('WebSocket', MockSocket);
  vi.resetModules();
  ({ liveStream } = await import('./stream.js'));
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('liveStream', () => {
  test('two streams share one socket, each event routed to its own handlers', () => {
    const bars = [];
    const notes = [];
    const states = [];
    liveStream({ url: () => '/api/histviz/streams?instruments=%5B%5D', events: { bar: (d, e) => bars.push([d, e.type]) }, onstate: (s) => states.push(s) });
    liveStream({ url: () => '/api/notifications/stream', events: { notification: (d) => notes.push(d) } });

    expect(MockSocket.all).toHaveLength(1);
    expect(last().url).toBe(`ws://${window.location.host}/api/live/ws`);
    last().accept();
    const [a, b] = last().subscribed();
    expect(a.path).toBe('/api/histviz/streams?instruments=%5B%5D');
    expect(b.path).toBe('/api/notifications/stream');
    expect(a.id).not.toBe(b.id);

    last().frame({ id: a.id, event: 'open', data: null });
    last().frame({ id: a.id, event: 'bar', data: { c: 1, i: 0 } });
    last().frame({ id: b.id, event: 'notification', data: { unread: 2 } });
    last().frame({ op: 'ping' });
    expect(bars).toEqual([[{ c: 1, i: 0 }, 'bar']]);
    expect(notes).toEqual([{ unread: 2 }]);
    expect(states).toEqual(['connecting', 'open']);
  });

  test('a dropped socket comes back with backoff and resubscribes from a re-read url', () => {
    let since = 'a';
    const states = [];
    liveStream({ url: () => `/api/histviz/stream?since=${since}`, onstate: (s, info) => states.push([s, info.retryInMs]) });
    last().accept();
    expect(last().subscribed()[0].path).toBe('/api/histviz/stream?since=a');

    since = 'b';
    last().drop();
    expect(states.at(-1)).toEqual(['reconnecting', 1000]);
    expect(MockSocket.all).toHaveLength(1);
    vi.advanceTimersByTime(1000);
    expect(MockSocket.all).toHaveLength(2);
    last().accept();
    const [again] = last().subscribed();
    expect(again.path).toBe('/api/histviz/stream?since=b');
    last().frame({ id: again.id, event: 'open', data: null });
    expect(states.at(-1)).toEqual(['open', undefined]);
  });

  test('closing unsubscribes, and the last close lets go of the socket', () => {
    const one = liveStream({ url: () => '/api/feeds/stream' });
    const two = liveStream({ url: () => '/api/notifications/stream' });
    last().accept();
    const [a] = last().subscribed();
    one.close();
    expect(last().sent.at(-1)).toEqual({ op: 'unsubscribe', id: a.id });
    expect(last().closed).toBe(false);
    two.close();
    expect(last().closed).toBe(true);
    vi.advanceTimersByTime(120000);
    expect(MockSocket.all).toHaveLength(1);
  });

  test('a refused subscription is final, a full budget is retried', () => {
    const refused = [];
    const busy = [];
    liveStream({ url: () => '/api/nowhere', onstate: (s, info) => refused.push([s, info.status]) });
    liveStream({ url: () => '/api/histviz/stream?x=1', onstate: (s, info) => busy.push([s, info.reason]) });
    last().accept();
    const [a, b] = last().subscribed();
    last().frame({ id: a.id, event: 'error', data: { status: 404, message: 'not a live stream' } });
    last().frame({ id: b.id, event: 'error', data: { status: 429, message: 'demo full' } });
    expect(refused.at(-1)).toEqual(['refused', 404]);
    expect(busy.at(-1)).toEqual(['reconnecting', 'refused']);

    vi.advanceTimersByTime(1000);
    expect(last().subscribed().map((m) => m.path)).toEqual(['/api/nowhere', '/api/histviz/stream?x=1', '/api/histviz/stream?x=1']);

    // A new socket resends only what was not refused.
    last().drop();
    vi.advanceTimersByTime(1000);
    last().accept();
    expect(last().subscribed().map((m) => m.path)).toEqual(['/api/histviz/stream?x=1']);
    expect(refused.filter(([s]) => s === 'reconnecting')).toEqual([]);
  });

  test('a stream the server ends is subscribed again', () => {
    const states = [];
    liveStream({ url: () => '/api/robots/streams?instruments=%5B%5D', onstate: (s, info) => states.push([s, info.reason]) });
    last().accept();
    const [a] = last().subscribed();
    last().frame({ id: a.id, event: 'end', data: null });
    expect(states.at(-1)).toEqual(['reconnecting', 'end']);
    vi.advanceTimersByTime(1000);
    const subs = last().subscribed();
    expect(subs).toHaveLength(2);
    expect(subs[1].id).not.toBe(a.id);
    // A late frame of the ended attempt reaches nobody.
    const got = [];
    liveStream({ url: () => '/api/feeds/stream', events: { bar: (d) => got.push(d) } });
    last().frame({ id: a.id, event: 'bar', data: { c: 1 } });
    expect(got).toEqual([]);
  });

  test('a silent socket is treated as dead and reconnected', () => {
    const states = [];
    liveStream({ url: () => '/api/notifications/stream', onstate: (s, info) => states.push([s, info.reason]) });
    last().accept();
    vi.advanceTimersByTime(30000);
    last().frame({ op: 'ping' });
    vi.advanceTimersByTime(30000);
    expect(last().closed).toBe(false);
    vi.advanceTimersByTime(20000);
    expect(MockSocket.all[0].closed).toBe(true);
    expect(states.at(-1)).toEqual(['reconnecting', 'idle']);
    vi.advanceTimersByTime(1000);
    expect(MockSocket.all).toHaveLength(2);
  });
});
