import { test } from 'node:test';
import assert from 'node:assert/strict';
import { splitPair, tradeUnitsFor } from '../src/lib/modules/histviz/orders.js';

test('splitPair reads the pair spellings the venues use, and nothing else', () => {
  assert.deepEqual(splitPair('BTCUSDT'), { base: 'BTC', quote: 'USDT' });
  assert.deepEqual(splitPair('ETH-USDC'), { base: 'ETH', quote: 'USDC' });
  assert.deepEqual(splitPair('sol_usdt'), { base: 'SOL', quote: 'USDT' });
  assert.deepEqual(splitPair('BTCFDUSD'), { base: 'BTC', quote: 'FDUSD' }, 'the longest quote wins');
  assert.deepEqual(splitPair('ETHUSDT_250926'), { base: 'ETH', quote: 'USDT' });
  assert.equal(splitPair('BTC-PERPETUAL'), null);
  assert.equal(splitPair('USDT'), null);
});

test('tradeUnitsFor: crypto spot sizes in the base asset or an amount of the quote', () => {
  assert.deepEqual(tradeUnitsFor({ assetType: 'crypto', broker: 'binance', ticker: 'BTCUSDT' }), [
    { value: 'units', unit: 'units', code: 'BTC' },
    { value: 'notional', unit: 'notional', code: 'USDT' }
  ]);
});

test('tradeUnitsFor: Binance USDⓈ-M takes contracts or an amount, other crypto contracts only contracts', () => {
  const usdm = tradeUnitsFor({ assetType: 'crypto', broker: 'binance_futures', ticker: 'BTCUSDT' });
  assert.deepEqual(usdm.map((u) => u.unit), ['contracts', 'notional']);
  assert.equal(usdm[1].code, 'USDT');
  for (const broker of ['kucoin_futures', 'deribit']) {
    assert.deepEqual(tradeUnitsFor({ assetType: 'crypto', broker, ticker: 'XBTUSDTM' }).map((u) => u.unit), ['contracts']);
  }
});

test('tradeUnitsFor: shares or an amount, a listed future contracts only', () => {
  const eq = tradeUnitsFor({ assetType: 'equity', broker: 'ibkr', ticker: 'AAPL', quote: 'USD' });
  assert.deepEqual(eq, [
    { value: 'units', unit: 'shares', code: null },
    { value: 'notional', unit: 'notional', code: 'USD' }
  ]);
  assert.deepEqual(tradeUnitsFor({ assetType: 'future', broker: 'ibkr', ticker: 'MNQ' }).map((u) => u.unit), ['contracts']);
  assert.deepEqual(tradeUnitsFor({ kind: 'option', assetType: 'option', ticker: 'X' }).map((u) => u.unit), ['contracts']);
});
