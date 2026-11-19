import { describe, it, expect } from 'vitest';
import { dayStats, pnlCurve, byTicker, dayFills, mindsetDigest, shiftDate, dayBounds } from './dayReport.js';

const T = (h, m = 0) => new Date(`2026-11-02T${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:00`).getTime();

const points = [
  { id: 'b', at: T(11), net: 50, fees: 2, r: 1, hold_min: 30, ticker: 'ES', side: 'long' },
  { id: 'a', at: T(10), net: -100, fees: 2, r: -1, hold_min: 10, ticker: 'NQ', side: 'short' },
  { id: 'c', at: T(12), net: 150, fees: 1, r: null, hold_min: 20, ticker: 'ES', side: 'long' },
  { id: 'd', at: T(13), net: 0, fees: 1, r: null, hold_min: null, ticker: 'CL', side: 'long' }
];

describe('day figures', () => {
  const s = dayStats(points);

  it('sums net, fees and gross', () => {
    expect(s).toMatchObject({ trades: 4, wins: 2, losses: 1, scratch: 1, net: 100, fees: 6, gross: 106 });
  });

  it('leaves scratches out of the win rate', () => {
    expect(s.winRate).toBeCloseTo(66.667, 2);
  });

  it('reads profit factor, payoff and expectancy', () => {
    expect(s.profitFactor).toBe(2);
    expect(s.avgWin).toBe(100);
    expect(s.avgLoss).toBe(-100);
    expect(s.payoff).toBe(1);
    expect(s.expectancy).toBe(25);
  });

  it('counts R only where a stop was logged', () => {
    expect(s).toMatchObject({ rTotal: 0, avgR: 0, rCount: 2, avgHold: 20 });
  });

  it('splits long and short', () => {
    expect(s.long).toEqual({ n: 3, net: 200 });
    expect(s.short).toEqual({ n: 1, net: -100 });
  });

  it('names a day with no loser as an infinite profit factor', () => {
    expect(dayStats([points[0]]).profitFactor).toBe(Infinity);
    expect(dayStats([]).profitFactor).toBeNull();
  });
});

describe('intraday curve', () => {
  it('steps in time order from zero and measures the fall from the running peak', () => {
    const c = pnlCurve(points);
    expect(c.steps.map((s) => s.cum)).toEqual([-100, -50, 100, 100]);
    // An opening loss is a drawdown from flat.
    expect(c.drawdown).toBe(100);
    expect(c.low).toBe(-100);
    expect(c.high).toBe(100);
  });
});

describe('per instrument', () => {
  it('groups and orders by size of the result', () => {
    expect(byTicker(points).map((r) => [r.ticker, r.trades, r.wins, r.net])).toEqual([
      ['ES', 2, 2, 200],
      ['NQ', 1, 0, -100],
      ['CL', 1, 0, 0]
    ]);
  });
});

describe('fills of the day', () => {
  const iso = (h, m = 0) => new Date(T(h, m)).toISOString();
  const trades = [
    // Simple short opened and closed in the day: sell to open, buy to close.
    { id: 's', side: 'short', ticker: 'NQ', currency: 'USD', advanced: false, entry_price: 100, exit_price: 90, quantity: 2, entry_at: iso(9), exit_at: iso(10) },
    // Legged long opened the day before, scaled out today; one exit leg has no stamp.
    {
      id: 'l', side: 'long', ticker: 'ES', currency: 'USD', advanced: true,
      entry_at: new Date(T(9) - 86400000).toISOString(), exit_at: iso(15),
      entries: [{ id: 'e1', price: 10, qty: 3, at: new Date(T(9) - 86400000).toISOString() }],
      exits: [{ id: 'x1', price: 11, qty: 1, at: iso(11), fees: 0.5 }, { id: 'x2', price: 12, qty: 2 }]
    }
  ];
  const f = dayFills(trades, T(0), T(23, 59));

  it('keeps only fills inside the day, oldest first', () => {
    expect(f.map((x) => x.key)).toEqual(['s:e:0', 's:x:0', 'l:x:x1', 'l:x:x2']);
  });

  it('reads buy or sell from the side and the leg', () => {
    expect(f.map((x) => `${x.action}/${x.effect}`)).toEqual(['sell/open', 'buy/close', 'sell/close', 'sell/close']);
  });

  it('places an unstamped leg at its trade exit', () => {
    expect(f[3].at).toBe(T(15));
    expect(f[2].fees).toBe(0.5);
  });
});

describe('mindset of the day', () => {
  const hist = {
    templates: [
      { id: 'post', name: 'Review', phase: 'post', active: true },
      { id: 'pre', name: 'Morning', phase: 'pre', active: true },
      { id: 'live', name: 'Midday', phase: 'live', active: true }
    ],
    prompts: [
      { id: 'sleep', template_id: 'pre', kind: 'scale', label: 'Sleep', position: 1, config: {} },
      { id: 'mood', template_id: 'pre', kind: 'choice', label: 'Mood', position: 0, config: { options: ['bad', 'ok', 'good'], score: 'asc' } },
      { id: 'tilt', template_id: 'post', kind: 'scale', label: 'Tilt', position: 0, config: { score: 'desc' } },
      { id: 'tags', template_id: 'post', kind: 'tags', label: 'Feelings', position: 1, config: {} },
      { id: 'txt', template_id: 'post', kind: 'text', label: 'Notes', position: 2, config: {} }
    ],
    entries: [
      { entry_date: '2026-11-02', template_id: 'post', answers: { tilt: 1, tags: ['calm'], txt: '' } },
      { entry_date: '2026-11-02', template_id: 'pre', answers: { sleep: 3, mood: 'good' } },
      { entry_date: '2026-11-01', template_id: 'pre', answers: { sleep: 1 } }
    ],
    notes: [{ day: '2026-11-02', content: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Patient day.' }] }] } }]
  };
  const d = mindsetDigest(hist, '2026-11-02', ['pre', 'live']);

  it('orders filled check-ins by phase and their answers by prompt position', () => {
    expect(d.templates.map((t) => t.id)).toEqual(['pre', 'post']);
    expect(d.templates[0].answers.map((a) => a.id)).toEqual(['mood', 'sleep']);
  });

  it('drops unanswered prompts', () => {
    expect(d.templates[1].answers.map((a) => a.id)).toEqual(['tilt', 'tags']);
  });

  it('scores each check-in and the day with the mindset normalization', () => {
    // mood good = 100, sleep 3 = 50, tilt 1 desc = 100
    expect(d.templates[0].score).toBe(75);
    expect(d.templates[1].score).toBe(100);
    expect(d.score).toBeCloseTo(83.33, 2);
  });

  it('lists the picked check-ins left unfilled', () => {
    expect(d.missing.map((m) => m.id)).toEqual(['live']);
  });

  it('flattens the day note', () => {
    expect(d.note).toBe('Patient day.');
  });

  it('reads an empty day as nothing', () => {
    expect(mindsetDigest(hist, '2026-11-05', [])).toEqual({ score: null, templates: [], missing: [], note: '' });
  });
});

describe('dates', () => {
  it('steps across a month end', () => {
    expect(shiftDate('2026-10-31', 1)).toBe('2026-11-01');
    expect(shiftDate('2026-03-01', -1)).toBe('2026-02-28');
  });

  it('bounds a local day', () => {
    const b = dayBounds('2026-11-02');
    expect(b.end - b.start).toBe(86400000 - 1);
  });
});
