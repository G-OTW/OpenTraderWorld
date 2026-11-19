import { describe, expect, test } from 'vitest';
import { fleetPnl, fleetEnvironments, fleetRanking, fleetExposure, periodStart } from './fleet.js';

const robots = [
  { environment: 'simulated', start_equity: 1000, started_at: '2026-01-01T00:00:00Z', unrealized: 5, closed: [['2026-01-02T10:00:00Z', 10], ['2026-01-05T10:00:00Z', -4]] },
  { environment: 'live', start_equity: 500, started_at: '2026-01-03T00:00:00Z', unrealized: null, closed: [['2026-01-04T10:00:00Z', 20]] }
];
const NOW = Date.parse('2026-01-10T00:00:00Z');

describe('fleetPnl', () => {
  test('sums every robot when no filter is set', () => {
    const r = fleetPnl(robots, {}, NOW);
    expect(r.realized).toBe(26);
    expect(r.unrealized).toBe(5);
    expect(r.total).toBe(31);
    expect(r.points.map((p) => p.equity)).toEqual([1500, 1510, 1530, 1526, 1531]);
  });

  test('keeps one environment', () => {
    const r = fleetPnl(robots, { env: 'live' }, NOW);
    expect(r.realized).toBe(20);
    expect(r.unrealized).toBeNull();
    expect(r.points.map((p) => p.equity)).toEqual([500, 520, 520]);
  });

  test('a date range carries what closed before it and stops at its end', () => {
    const r = fleetPnl(robots, { from: '2026-01-03', to: '2026-01-04' }, NOW);
    expect(r.realized).toBe(20);
    expect(r.points.map((p) => p.equity)).toEqual([1510, 1530]);
  });

  test('lists each environment once', () => {
    expect(fleetEnvironments([...robots, { environment: 'live' }])).toEqual(['simulated', 'live']);
  });
});

describe('fleetRanking', () => {
  const fleet = [
    { id: 'a', name: 'A', start_equity: 1000, closed: [['2026-01-02T10:00:00Z', 10], ['2026-01-09T10:00:00Z', -4]] },
    { id: 'b', name: 'B', start_equity: 500, closed: [['2026-01-04T10:00:00Z', 20]] },
    { id: 'c', name: 'C', start_equity: 0, closed: [] }
  ];

  test('ranks on everything closed since the start', () => {
    const r = fleetRanking(fleet, 'all', NOW);
    expect(r.map((x) => [x.id, x.pnl])).toEqual([['b', 20], ['a', 6], ['c', 0]]);
    expect(r[0].pct).toBe(4);
    expect(r[2].pct).toBeNull();
  });

  test('keeps only the trades closed in the period', () => {
    const r = fleetRanking(fleet, 'week', NOW);
    expect(r.map((x) => [x.id, x.pnl, x.trades])).toEqual([['b', 20, 1], ['c', 0, 0], ['a', -4, 1]]);
  });

  test('today starts at local midnight', () => {
    const d = new Date(periodStart('today', NOW));
    expect([d.getHours(), d.getMinutes(), d.getSeconds()]).toEqual([0, 0, 0]);
  });
});

describe('fleetExposure', () => {
  test('sums the open size by asset type, largest first', () => {
    const r = fleetExposure([{ exposure: { crypto: 300, futures: 100 } }, { exposure: { crypto: 600 } }, { exposure: {} }]);
    expect(r).toEqual([
      { kind: 'crypto', value: 900, robots: 2, pct: 90 },
      { kind: 'futures', value: 100, robots: 1, pct: 10 }
    ]);
  });

  test('is empty when nothing is open', () => {
    expect(fleetExposure([{ exposure: {} }, {}])).toEqual([]);
  });
});
