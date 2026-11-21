// Per-trade PnL histogram. Zero is always a bucket edge, so no bar mixes a winner with a
// loser, and every trade lands in exactly one bucket.
import { describe, expect, test } from 'vitest';
import { autoStep, pnlBuckets } from './pnldist.js';

const counts = (rows) => rows.map((b) => b.trades);

describe('autoStep', () => {
  test('snaps to a 1, 2, 2.5 or 5 graduation', () => {
    // 2nd..98th percentile of 0..2400 spans 2304, /24 = 96: next graduation is 100.
    const v = Array.from({ length: 2401 }, (_, i) => i);
    expect(autoStep(v)).toBe(100);
    // span 48 / 24 = 2.
    expect(autoStep([0, 48])).toBeCloseTo(2, 12);
    // span 60 / 24 = 2.5.
    expect(autoStep([0, 60])).toBeCloseTo(2.5, 12);
  });

  test('empty, single and constant series fall back to a unit step', () => {
    expect(autoStep([])).toBe(1);
    expect(autoStep([42])).toBe(1);
    expect(autoStep([5, 5, 5])).toBe(1);
  });
});

describe('pnlBuckets', () => {
  test('a hand-counted book: losers left of zero, winners right, net and win rate per bar', () => {
    const rows = pnlBuckets([-150, -20, -5, 0, 10, 95, 120], 100);
    expect(rows.map((b) => [b.lo, b.hi])).toEqual([
      [-200, -100],
      [-100, 0],
      [0, 100],
      [100, 200]
    ]);
    expect(counts(rows)).toEqual([1, 2, 3, 1]);
    expect(rows.map((b) => b.net)).toEqual([-150, -25, 105, 120]);
    expect(rows.map((b) => b.wins)).toEqual([0, 0, 2, 1]);
    // A zero trade is not a win: 2 of 3 in [0, 100).
    expect(rows[2].win_rate).toBeCloseTo((2 / 3) * 100, 10);
    expect(rows[1].avg).toBe(-12.5);
  });

  test('every trade is counted once, whatever the step', () => {
    const v = [-73.2, -1, 0, 0.5, 3, 3, 44.4, 900];
    for (const step of [1, 2.5, 10, 50, 1000]) {
      expect(counts(pnlBuckets(v, step)).reduce((a, b) => a + b, 0)).toBe(v.length);
    }
  });

  test('an empty book, or a non-positive step, gives no bars', () => {
    expect(pnlBuckets([], 10)).toEqual([]);
    expect(pnlBuckets([1, 2], 0)).toEqual([]);
    expect(pnlBuckets([1, 2], -5)).toEqual([]);
  });

  test('past the bin cap the outliers fold into open-ended edge buckets', () => {
    // 1..100 plus two freak trades: a unit step would need ~20000 bars.
    const v = [-10000, ...Array.from({ length: 100 }, (_, i) => i + 1), 10000];
    const rows = pnlBuckets(v, 1, 121);
    // The window shrinks to the 1st..99th percentile: 1.01 to 99.99, bins [1, 2) to [99, 100).
    expect(rows.length).toBe(99);
    expect(rows[0].lo).toBeNull();
    expect(rows.at(-1).hi).toBeNull();
    // The first bar holds its own trade (1) and the freak loss; the last holds 99, 100, 10000.
    expect([rows[0].trades, rows[0].net]).toEqual([2, -9999]);
    expect([rows.at(-1).trades, rows.at(-1).net]).toEqual([3, 10199]);
    expect(counts(rows).reduce((a, b) => a + b, 0)).toBe(v.length);
  });

  test('empty bars between trades are kept so the axis has no hole', () => {
    const rows = pnlBuckets([-30, 30], 10);
    expect(counts(rows)).toEqual([1, 0, 0, 0, 0, 0, 1]);
    expect(rows[3].win_rate).toBeNull();
    expect(rows[3].avg).toBe(0);
  });
});
