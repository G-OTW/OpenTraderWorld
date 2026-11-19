import { describe, it, expect } from 'vitest';
import { dayBases, pctOf, rebasePct, fmtSignedPct } from './pnlpct.js';

describe('dayBases', () => {
  it('reads each day against the capital carried into it', () => {
    const b = dayBases(
      [
        { date: '2026-01-03', net_pnl: -50 },
        { date: '2026-01-02', net_pnl: 100 }
      ],
      1000
    );
    expect(b.get('2026-01-02')).toBe(1000);
    expect(b.get('2026-01-03')).toBe(1100);
    expect(pctOf(-50, b.get('2026-01-03'))).toBeCloseTo(-4.5454, 3);
  });

  it('without capital, the first result stands in for it', () => {
    const b = dayBases(
      [
        { date: '2026-01-01', net_pnl: 0 },
        { date: '2026-01-02', net_pnl: -200 },
        { date: '2026-01-03', net_pnl: 100 }
      ],
      0
    );
    expect(b.get('2026-01-02')).toBe(200);
    expect(pctOf(100, b.get('2026-01-03'))).toBeNull();
  });
});

describe('rebasePct', () => {
  it('measures change from the first point', () => {
    expect(rebasePct([1000, 1100, 900])).toEqual([0, 10, -10]);
  });
  it('falls back to the first non-zero point when the first is zero', () => {
    expect(rebasePct([0, 50, -25])).toEqual([0, 100, -50]);
  });
  it('has no percent without any non-zero point', () => {
    expect(rebasePct([0, 0])).toEqual([null, null]);
  });
});

it('fmtSignedPct', () => {
  expect(fmtSignedPct(1.234)).toBe('+1.23%');
  expect(fmtSignedPct(-2)).toBe('−2.00%');
  expect(fmtSignedPct(null)).toBe('—');
});
