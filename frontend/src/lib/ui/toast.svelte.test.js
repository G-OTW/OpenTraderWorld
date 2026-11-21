// The toast queue is a runes store: each message expires on its own timer, an error
// stays longer than a confirmation, and a ttl of 0 keeps it until dismissed.
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { toast } from '$lib/ui/toast.svelte.js';

beforeEach(() => vi.useFakeTimers({ now: new Date('2026-03-14T12:00:00Z') }));
afterEach(() => {
  for (const t of toast.items) toast.dismiss(t.id);
  vi.useRealTimers();
});

test('ok, warn and err queue in order with their kind and text', () => {
  toast.ok('Saved');
  toast.warn('Partial');
  toast.err('Refused');
  expect(toast.items.map(({ kind, text }) => ({ kind, text }))).toEqual([
    { kind: 'ok', text: 'Saved' },
    { kind: 'warn', text: 'Partial' },
    { kind: 'err', text: 'Refused' }
  ]);
});

test('a confirmation expires after 3.2 s, an error only after 6 s', () => {
  toast.ok('Saved');
  toast.err('Refused');
  vi.advanceTimersByTime(3199);
  expect(toast.items.map((t) => t.text)).toEqual(['Saved', 'Refused']);
  vi.advanceTimersByTime(1);
  expect(toast.items.map((t) => t.text)).toEqual(['Refused']);
  vi.advanceTimersByTime(6000 - 3200);
  expect(toast.items).toEqual([]);
});

test('a ttl of 0 keeps the toast until it is dismissed by id', () => {
  const id = toast.push('warn', 'Sticky', 0);
  const other = toast.ok('Saved', 0);
  vi.advanceTimersByTime(60_000);
  expect(toast.items.map((t) => t.text)).toEqual(['Sticky', 'Saved']);
  expect(other).toBeGreaterThan(id);
  toast.dismiss(id);
  expect(toast.items.map((t) => t.text)).toEqual(['Saved']);
  // Dismissing an unknown id changes nothing.
  toast.dismiss(-1);
  expect(toast.items.map((t) => t.text)).toEqual(['Saved']);
});
