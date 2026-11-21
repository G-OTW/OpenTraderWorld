import { describe, expect, it } from 'vitest';
import { byKey, cleanPreset, exitPrices, savePreset } from './presets.js';

describe('entry presets', () => {
  it('places a long target above and its stop below, in ticks', () => {
    const p = { tp: { value: 8, unit: 'ticks' }, sl: { value: 4, unit: 'ticks' } };
    expect(exitPrices(p, { side: 'buy', ref: 5000, tick: 0.25, digits: 2 })).toEqual({ tp: 5002, sl: 4999 });
  });

  it('mirrors a short', () => {
    const p = { tp: { value: 1, unit: 'pct' }, sl: { value: 5, unit: 'points' } };
    expect(exitPrices(p, { side: 'sell', ref: 200, digits: 2 })).toEqual({ tp: 198, sl: 205 });
  });

  it('turns an amount of money into a distance for the size and point value', () => {
    const p = { tp: { value: 100, unit: 'money' }, sl: null };
    // 2 contracts at 50 a point: 100 is one point.
    expect(exitPrices(p, { side: 'buy', ref: 5000, tick: 0.25, pointValue: 50, qty: 2, digits: 2 })).toEqual({ tp: 5001, sl: null });
    expect(exitPrices(p, { side: 'buy', ref: 5000, qty: null }).error).toBe('qty');
  });

  it('says what is missing', () => {
    expect(exitPrices({ tp: { value: 3, unit: 'ticks' } }, { side: 'buy', ref: 10 }).error).toBe('tick');
    expect(exitPrices({ sl: { value: 20, unit: 'points' } }, { side: 'buy', ref: 10 }).error).toBe('side');
    expect(exitPrices({}, { side: 'buy', ref: 0 }).error).toBe('ref');
  });

  it('keeps one preset per key', () => {
    let list = savePreset([], { name: 'A', key: 1, tp: { value: 1, unit: 'ticks' } });
    list = savePreset(list, { name: 'B', key: 1 });
    expect(list.map((p) => [p.name, p.key])).toEqual([
      ['A', null],
      ['B', 1]
    ]);
    const edited = savePreset(list, { ...list[0], name: 'A2' });
    expect(edited[0].name).toBe('A2');
  });

  it('keeps a preset for both directions, and falls back to long on an unknown side', () => {
    expect(cleanPreset({ name: 'x', side: 'both' }).side).toBe('both');
    expect(cleanPreset({ name: 'x', side: 'up' }).side).toBe('buy');
  });

  it('drops an exit without a distance', () => {
    expect(cleanPreset({ name: 'x', tp: { value: '', unit: 'ticks' }, sl: { value: '2', unit: 'pct' } })).toMatchObject({ tp: null, sl: { value: 2, unit: 'pct' }, qty: null });
  });

  it('lists by key, presets without one last', () => {
    const list = [{ name: 'b', key: null }, { name: 'x', key: 7 }, { name: 'a', key: null }, { name: 'y', key: 3 }];
    expect(byKey(list).map((p) => p.name)).toEqual(['y', 'x', 'a', 'b']);
  });
});
