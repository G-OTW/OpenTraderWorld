import { describe, expect, it } from 'vitest';
import { blankDraft, draftOf, guardOf, ruleText } from './guards.js';

const t = (k, p = {}) => `${k}${Object.keys(p).length ? JSON.stringify(p) : ''}`;

describe('chart guards', () => {
  it('writes a window in minutes from hours', () => {
    const g = guardOf({ ...blankDraft(), kind: 'trades', n: 10, windowValue: 2, windowUnit: 'h', rolling: false });
    expect(g.rule).toEqual({ kind: 'trades', n: 10, minutes: 120, rolling: false });
    expect(g.account_id).toBe(null);
  });

  it('keeps closing positions only with a stop', () => {
    expect(guardOf({ ...blankDraft(), action: 'alert', close_positions: true }).close_positions).toBe(false);
    expect(guardOf({ ...blankDraft(), action: 'stop', close_positions: true }).close_positions).toBe(true);
  });

  it('opens a stored guard as the draft that wrote it', () => {
    const d = { ...blankDraft(), account_id: 'a1', kind: 'trade_loss', value: '1.5', pct: true, of: 'equity', action: 'stop', close_positions: true };
    const back = draftOf({ ...guardOf(d), id: 1 });
    expect(guardOf(back)).toEqual(guardOf(d));
    const w = draftOf({ rule: { kind: 'trades', n: 5, minutes: 45, rolling: true }, action: 'alert' });
    expect([w.windowValue, w.windowUnit]).toEqual([45, 'min']);
  });

  it('words a rule', () => {
    expect(ruleText({ kind: 'trades', n: 10, minutes: 60, rolling: true }, t)).toContain('tradesSliding');
    expect(ruleText({ kind: 'trade_loss', value: '2', pct: true, of: 'equity' }, t)).toContain('tradeLossPct.equity');
  });
});
