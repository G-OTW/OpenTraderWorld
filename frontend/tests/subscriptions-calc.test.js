import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  addMonthsClamped,
  billingDates,
  nextBillingDate,
  switchSavings,
  cardExpiryState,
  trialsFirst,
  needsRating,
  worthRanking,
  monthGrid,
  monthEvents,
  monthlyOf,
  dayKey
} from '../src/lib/modules/subscriptions/calc.js';

const sub = (extra = {}) => ({
  id: 'x',
  name: 'Sub',
  price: 10,
  currency: 'USD',
  frequency: 'monthly',
  started_on: '2026-01-31',
  active: true,
  my_share_pct: 100,
  ...extra
});
const keys = (ds) => ds.map(dayKey);
const d = (s) => new Date(`${s}T00:00:00`);

test('months clamp to the month end without drifting', () => {
  assert.equal(dayKey(addMonthsClamped(d('2026-01-31'), 1)), '2026-02-28');
  assert.equal(dayKey(addMonthsClamped(d('2026-01-31'), 2)), '2026-03-31');
  assert.equal(dayKey(addMonthsClamped(d('2024-02-29'), 12)), '2025-02-28');
  assert.equal(dayKey(addMonthsClamped(d('2026-01-15'), -1)), '2025-12-15');
});

test('billing dates expand for each frequency over 12 months', () => {
  const from = d('2026-01-01');
  const to = d('2026-12-31');
  const m = billingDates(sub({ started_on: '2025-11-10' }), from, to);
  assert.equal(m.length, 12);
  assert.equal(dayKey(m[0]), '2026-01-10');
  assert.deepEqual(keys(billingDates(sub({ started_on: '2025-11-10', frequency: 'quarterly' }), from, to)), [
    '2026-02-10',
    '2026-05-10',
    '2026-08-10',
    '2026-11-10'
  ]);
  assert.deepEqual(keys(billingDates(sub({ started_on: '2025-11-10', frequency: 'yearly' }), from, to)), ['2026-11-10']);
  const w = billingDates(sub({ started_on: '2026-01-01', frequency: 'weekly' }), from, to);
  assert.equal(w.length, 53);
});

test('trial end anchors billing; paused, cancelled and undated subs bill nothing past their state', () => {
  const from = d('2026-01-01');
  const to = d('2026-06-30');
  assert.equal(dayKey(billingDates(sub({ started_on: '2025-12-05', trial_end: '2026-03-20' }), from, to)[0]), '2026-03-20');
  assert.deepEqual(keys(billingDates(sub({ started_on: '2025-12-05', ended_at: '2026-03-05' }), from, to)), [
    '2026-01-05',
    '2026-02-05'
  ]);
  assert.deepEqual(billingDates(sub({ active: false }), from, to), []);
  assert.deepEqual(billingDates(sub({ started_on: null }), from, to), []);
  assert.equal(dayKey(nextBillingDate(sub(), d('2026-02-01'))), '2026-02-28');
});

test('yearly switch savings: 12 x monthly minus yearly, at my share, null when unknown', () => {
  assert.equal(switchSavings(sub({ yearly_price: 100 })), 20);
  assert.equal(switchSavings(sub({ yearly_price: 100, my_share_pct: 50 })), 10);
  assert.equal(switchSavings(sub({ yearly_price: 130 })), -10);
  assert.equal(switchSavings(sub({ yearly_price: null })), null);
  assert.equal(switchSavings(sub({ yearly_price: '' })), null);
  assert.equal(switchSavings(sub({ frequency: 'yearly', yearly_price: 100 })), null);
});

test('shared plans count my share in the monthly equivalent', () => {
  assert.equal(monthlyOf(sub({ price: 120, frequency: 'yearly', my_share_pct: 50 })), 5);
  assert.equal(monthlyOf(sub({ my_share_pct: 0 })), 10);
});

test('card expiry flags expired and within 60 days', () => {
  const today = d('2026-11-06');
  assert.equal(cardExpiryState('2026-10', today), 'expired');
  assert.equal(cardExpiryState('2026-11', today), 'soon');
  assert.equal(cardExpiryState('2026-12', today), 'soon');
  assert.equal(cardExpiryState('2027-03', today), null);
  assert.equal(cardExpiryState('', today), null);
});

test('trials about to convert are listed first, soonest first', () => {
  const today = d('2026-11-06');
  const rows = [
    sub({ id: 'a', name: 'A' }),
    sub({ id: 'b', name: 'B', trial_end: '2026-11-20' }),
    sub({ id: 'c', name: 'C', trial_end: '2026-11-08' }),
    sub({ id: 'd', name: 'D', trial_end: '2026-10-01' })
  ];
  assert.deepEqual(trialsFirst(rows, today).map((r) => r.id), ['c', 'b', 'a', 'd']);
});

test('rating prompt and "Worth it?" ranking by monthly cost per point', () => {
  const today = d('2026-11-06');
  assert.equal(needsRating(sub({ last_rated_at: null }), today), true);
  assert.equal(needsRating(sub({ last_rated_at: '2026-09-01' }), today), false);
  assert.equal(needsRating(sub({ last_rated_at: '2026-07-01' }), today), true);
  assert.equal(needsRating(sub({ ended_at: '2026-10-01' }), today), false);
  const ranked = worthRanking(
    [
      sub({ id: 'cheap-loved', name: 'A', price: 5, last_score: 5 }),
      sub({ id: 'pricey-unused', name: 'B', price: 50, last_score: 1 }),
      sub({ id: 'mid', name: 'C', price: 30, last_score: 3 }),
      sub({ id: 'unrated', name: 'D', price: 99, last_score: null }),
      sub({ id: 'no-rate', name: 'E', price: 10, currency: 'XXX', last_score: 2 })
    ],
    (a, c) => (c === 'USD' ? a : null),
    today
  );
  assert.deepEqual(ranked.map((r) => r.sub.id), ['pricey-unused', 'mid', 'cheap-loved']);
  assert.equal(ranked[0].costPerPoint, 50);
});

test('month grid starts Monday and holds every day; events land on their billing day', () => {
  const weeks = monthGrid(2026, 1); // February 2026
  assert.equal(dayKey(weeks[0][0]), '2026-01-26');
  assert.ok(weeks.every((w) => w.length === 7));
  const days = weeks.flat().filter((x) => x.getMonth() === 1);
  assert.equal(days.length, 28);
  const ev = monthEvents([sub({ my_share_pct: 50 })], 2026, 1);
  assert.deepEqual(Object.keys(ev), ['2026-02-28']);
  assert.equal(ev['2026-02-28'][0].amount, 5);
});

import { stackByCategory, upcomingBillings, reviewIds, categoryOf } from '../src/lib/modules/subscriptions/calc.js';

test('stackByCategory keeps ranked categories in order and folds the rest', () => {
  const subs = [
    { id: 'a', category: 'Video' },
    { id: 'b', category: 'Music' },
    { id: 'c', category: 'News' },
    { id: 'd', category: '' }
  ];
  const points = [{ by_sub: { a: 10, b: 5, c: 2, d: 1 } }, { by_sub: { a: 10 } }];
  const out = stackByCategory(points, subs, ['Video', 'Music'], 2);
  assert.deepEqual(out.named, ['Video', 'Music']);
  assert.deepEqual(out.rows, [[10, 10], [5, 0]]);
  assert.deepEqual(out.other, [3, 0]);
  assert.equal(categoryOf(subs[3]), 'Uncategorized');
  assert.equal(stackByCategory(points.slice(1), subs, ['Video'], 4).other, null);
});

test('upcomingBillings lists the next 30 days, today included', () => {
  const today = d('2026-03-10');
  const rows = upcomingBillings([sub({ started_on: '2026-01-10' }), sub({ id: 'y', frequency: 'yearly', started_on: '2025-06-01' })], 30, today);
  assert.deepEqual(rows.map((r) => dayKey(r.date)), ['2026-03-10']);
  assert.equal(rows[0].amount, 10);
});

test('reviewIds flags low scores and costly points', () => {
  const r = (id, score, costPerPoint) => ({ sub: { id }, score, costPerPoint });
  const ids = reviewIds([r('a', 5, 1), r('b', 4, 1.2), r('c', 3, 5), r('d', 2, 0.5)]);
  assert.deepEqual([...ids].sort(), ['c', 'd']);
  assert.equal(reviewIds([]).size, 0);
});
