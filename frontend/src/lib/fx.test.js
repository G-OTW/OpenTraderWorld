// Display-currency conversion: the rate lookup walks back over weekends like the backend,
// and a missing rate is null (the caller shows the native amount), never a wrong number.
import { describe, expect, test, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../mocks/node.js';
import { convert, createFxRates, fetchRates } from '$lib/fx.svelte.js';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));

const RATES = { EUR: 0.8, JPY: 150 };

describe('convert', () => {
  test('USD-based cross rates: 100 EUR is 125 USD and 18750 JPY', () => {
    expect(convert(100, 'EUR', 'USD', RATES)).toBe(125);
    expect(convert(100, 'EUR', 'JPY', RATES)).toBe(18750);
    expect(convert(150, 'USD', 'JPY', RATES)).toBe(22500);
  });

  test('A to B and back is the identity, within float rounding', () => {
    for (const amount of [0.01, 1, 1234.56, 1e9]) {
      const there = convert(amount, 'EUR', 'JPY', RATES);
      expect(convert(there, 'JPY', 'EUR', RATES)).toBeCloseTo(amount, 6);
    }
  });

  test('the same currency, or no currency, returns the amount untouched', () => {
    expect(convert(42, 'EUR', 'EUR', {})).toBe(42);
    expect(convert(42, '', 'EUR', {})).toBe(42);
  });

  test('a side without a rate gives null, never a silently wrong number', () => {
    expect(convert(100, 'GBP', 'USD', RATES)).toBeNull();
    expect(convert(100, 'USD', 'GBP', RATES)).toBeNull();
    expect(convert(100, 'EUR', 'USD', { EUR: 0 })).toBeNull();
    expect(convert(null, 'EUR', 'USD', RATES)).toBeNull();
    expect(convert(NaN, 'EUR', 'USD', RATES)).toBeNull();
  });
});

describe('fetchRates', () => {
  test('a weekend date carries the last business close forward', async () => {
    const asked = [];
    server.use(
      http.get('*/api/journal/fx/rates/:date', ({ params }) => {
        asked.push(params.date);
        return params.date === '2026-03-13'
          ? HttpResponse.json({ rates: [{ quote: 'EUR', rate: 0.8 }, { quote: 'BAD', rate: null }] })
          : HttpResponse.json({ rates: [] });
      })
    );
    // 2026-03-15 is a Sunday: Sunday and Saturday are empty, Friday answers.
    expect(await fetchRates('2026-03-15')).toEqual({ EUR: 0.8 });
    expect(asked).toEqual(['2026-03-15', '2026-03-14', '2026-03-13']);
  });

  test('a failing endpoint gives an empty map after maxBack days, not an exception', async () => {
    const asked = [];
    server.use(
      http.get('*/api/journal/fx/rates/:date', ({ params }) => {
        asked.push(params.date);
        return HttpResponse.json({ error: 'boom' }, { status: 500 });
      })
    );
    expect(await fetchRates('2026-03-01', 2)).toEqual({});
    // Month boundary on the way back.
    expect(asked).toEqual(['2026-03-01', '2026-02-28', '2026-02-27']);
  });

  test('a network failure is an empty map too', async () => {
    server.use(http.get('*/api/journal/fx/rates/:date', () => HttpResponse.error()));
    expect(await fetchRates('2026-03-13', 0)).toEqual({});
  });
});

describe('createFxRates', () => {
  test('converts with the loaded map only once it is loaded', async () => {
    const fx = createFxRates();
    expect(fx.loaded).toBe(false);
    expect(fx.to(100, 'EUR', 'USD')).toBeNull();
    await fx.load('2026-03-13');
    expect(fx.loaded).toBe(true);
    expect(fx.rates).toEqual({ EUR: 0.8, JPY: 150 });
    expect(fx.to(100, 'EUR', 'USD')).toBe(125);
  });
});
