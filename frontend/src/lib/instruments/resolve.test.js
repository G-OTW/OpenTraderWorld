import { describe, it, expect } from 'vitest';
import { exactHits, chartHref, findbAssetType, watchKind, defaultTimeframe } from './resolve.js';

describe('instrument resolve', () => {
  const hits = [
    { symbol: 'AAPL', provider: 'yahoo' },
    { symbol: 'AAPL.MX', provider: 'yahoo' },
    { symbol: 'aapl', provider: 'ibkr' },
    { symbol: 'APP', provider: 'yahoo' }
  ];

  it('keeps only exact symbols, case aside', () => {
    expect(exactHits(' aapl ', hits).map((h) => h.provider)).toEqual(['yahoo', 'ibkr']);
  });

  it('never matches on a prefix or a blank', () => {
    expect(exactHits('AP', hits)).toEqual([]);
    expect(exactHits('', hits)).toEqual([]);
    expect(exactHits('AAPL', null)).toEqual([]);
  });

  it('builds the chart deep link from coordinates', () => {
    const url = new URL(chartHref({ provider: 'yahoo', asset_type: 'equity', ticker: 'AAPL', connector_id: 'c1' }), 'http://x');
    expect(url.pathname).toBe('/histviz');
    expect(Object.fromEntries(url.searchParams)).toEqual({ provider: 'yahoo', asset_type: 'equity', ticker: 'AAPL', tf: '1d', connector: 'c1' });
    expect(chartHref({ provider: 'yahoo', asset_type: 'etf', ticker: 'SPY' })).not.toContain('connector');
  });

  it('maps FinanceDatabase types without inventing one', () => {
    expect(findbAssetType('currency')).toBe('fx');
    expect(findbAssetType('fund')).toBe('');
    expect(watchKind('crypto')).toBe('crypto');
    expect(watchKind('etf')).toBe('stock');
  });

  it('opens daily when served', () => {
    expect(defaultTimeframe(['1h', '1d'])).toBe('1d');
    expect(defaultTimeframe(['1m', '5m'])).toBe('1m');
    expect(defaultTimeframe([])).toBe('1d');
  });
});
