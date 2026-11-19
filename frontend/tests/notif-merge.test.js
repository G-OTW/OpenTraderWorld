import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ringGate, takeFresh } from '../src/lib/modules/remindme/notifMerge.js';

const row = (id, created_at, extra = {}) => ({ id, kind: 'chart', created_at, read_at: null, ...extra });

test('a row pushed by the stream is not toasted again by the poll', () => {
  const seen = new Set();
  const pushed = takeFresh([row('a', '2026-11-05T10:00:01Z')], seen, '2026-11-05T10:00:00Z');
  assert.deepEqual(pushed.fresh.map((n) => n.id), ['a']);
  assert.equal(pushed.since, '2026-11-05T10:00:01Z');

  const polled = takeFresh(
    [row('a', '2026-11-05T10:00:01Z'), row('b', '2026-11-05T10:00:02Z')],
    seen,
    pushed.since
  );
  assert.deepEqual(polled.fresh.map((n) => n.id), ['b']);
  assert.equal(polled.since, '2026-11-05T10:00:02Z');
});

test('since only moves forward, also for a row committed out of order', () => {
  const seen = new Set();
  const late = takeFresh([row('old', '2026-11-05T09:59:00Z')], seen, '2026-11-05T10:00:00Z');
  // Still toasted (the stream brought it), but the poll keeps reading from the newer mark.
  assert.deepEqual(late.fresh.map((n) => n.id), ['old']);
  assert.equal(late.since, '2026-11-05T10:00:00Z');
});

test('since compares instants, not strings of different precision', () => {
  const r = takeFresh([row('a', '2026-11-05T10:00:00.5Z')], new Set(), '2026-11-05T10:00:00.123456Z');
  assert.equal(r.since, '2026-11-05T10:00:00.5Z');
  const s = takeFresh([row('b', '2026-11-05T10:00:00Z')], new Set(), '2026-11-05T10:00:00.5Z');
  assert.equal(s.since, '2026-11-05T10:00:00.5Z');
});

test('a row already read is not toasted, and a null since takes the first row', () => {
  const seen = new Set();
  const r = takeFresh(
    [row('r', '2026-11-05T10:00:00Z', { read_at: '2026-11-05T10:00:01Z' }), row('u', '2026-11-05T10:00:02Z')],
    seen,
    null
  );
  assert.deepEqual(r.fresh.map((n) => n.id), ['u']);
  assert.equal(r.since, '2026-11-05T10:00:02Z');
  assert.ok(seen.has('r'));
});

test('the seen set stays bounded, oldest ids out first', () => {
  const seen = new Set();
  const rows = Array.from({ length: 600 }, (_, i) => row(`n${i}`, '2026-11-05T10:00:00Z'));
  takeFresh(rows, seen, null);
  assert.equal(seen.size, 500);
  assert.ok(!seen.has('n0'));
  assert.ok(seen.has('n599'));
});

test('alerts arriving together ring once', () => {
  const ring = ringGate(1000);
  assert.equal(ring(10_000), true);
  assert.equal(ring(10_020), false);
  assert.equal(ring(10_900), false);
  assert.equal(ring(11_000), true);
});
