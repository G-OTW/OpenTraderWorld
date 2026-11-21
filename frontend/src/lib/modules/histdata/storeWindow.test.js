// A ticker is checked against the connector's own symbol lookup before a download is
// queued: an exact listing passes, a case-only difference takes the provider's spelling,
// anything else is an error with suggestions, never the closest hit.
import { describe, expect, test } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../../../mocks/node.js';
import { resolveTicker, defaultFrom } from './storeWindow.js';

const C = '11111111-1111-1111-1111-111111111111';
const hit = (symbol, connector_id = C) => ({ connector_id, symbol, provider: 'p', name: '' });

function answer(body) {
  let asked = null;
  server.use(
    http.get('*/api/histdata/symbols', ({ request }) => {
      asked = new URL(request.url).searchParams;
      return HttpResponse.json(body);
    })
  );
  return () => asked;
}

describe('resolveTicker', () => {
  test('an exact listing resolves, asking only this connector under the histdata grant', async () => {
    const asked = answer({ results: [hit('AAPL'), hit('AAPL.MX')], notes: [] });
    await expect(resolveTicker({ connectorId: C, assetType: 'equity', ticker: 'AAPL' })).resolves.toEqual({ status: 'ok', symbol: 'AAPL' });
    expect(asked().get('connectors')).toBe(C);
    expect(asked().get('module')).toBe('histdata');
    expect(asked().get('asset_type')).toBe('equity');
  });

  test('a case-only difference takes the provider spelling', async () => {
    answer({ results: [hit('BTCUSDT')], notes: [] });
    await expect(resolveTicker({ connectorId: C, assetType: 'crypto', ticker: 'btcusdt' })).resolves.toEqual({ status: 'ok', symbol: 'BTCUSDT' });
  });

  test('a near miss is never picked: it is offered', async () => {
    answer({ results: [hit('AAPL'), hit('AAPL.MX'), hit('AAPL', '22222222-2222-2222-2222-222222222222')], notes: [] });
    await expect(resolveTicker({ connectorId: C, assetType: '', ticker: 'AAP' })).resolves.toEqual({ status: 'missing', suggestions: ['AAPL', 'AAPL.MX'] });
  });

  test('a provider without search is left to its first fetch', async () => {
    answer({ results: [], notes: [{ connector_id: C, code: 'unsupported', message: 'no symbol search' }] });
    await expect(resolveTicker({ connectorId: C, assetType: '', ticker: 'X' })).resolves.toEqual({ status: 'unchecked' });
  });

  test('a failed lookup carries the provider reason', async () => {
    answer({ results: [], notes: [{ connector_id: C, code: 'network', message: 'timed out' }] });
    await expect(resolveTicker({ connectorId: C, assetType: '', ticker: 'X' })).resolves.toEqual({ status: 'failed', message: 'timed out' });
  });
});

test('defaultFrom: five years of daily bars, thirty days of intraday', () => {
  const now = new Date('2026-11-11T12:00:00Z');
  expect(defaultFrom('1d', now)).toBe('2021-11-11');
  expect(defaultFrom('1h', now)).toBe('2026-10-12');
});
