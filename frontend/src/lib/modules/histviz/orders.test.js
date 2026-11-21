import { describe, expect, it } from 'vitest';
import { buildOrders, legsOf, positionExitId, positionExitsFor } from './orders.js';

describe('a position exit', () => {
  it('is changed under the id the orders panel lists it by', () => {
    expect(positionExitId('linear:BTCUSDT:long', 'sl')).toBe('linear:BTCUSDT:tpsl::sl:buy');
    expect(positionExitId('usdt-futures:ETHUSDT:short', 'tp')).toBe('usdt-futures:ETHUSDT:tpsl::tp:sell');
    expect(positionExitId('265598', 'sl')).toBeNull();
    expect(positionExitId('linear:BTCUSDT:long', 'trail')).toBeNull();
  });

  it('is offered only where the broker says so', () => {
    expect(positionExitsFor({ order_caps: { position_exits: true } })).toBe(true);
    expect(positionExitsFor({ order_caps: { brackets: true } })).toBe(false);
    expect(positionExitsFor({})).toBe(false);
  });
});

describe('the size unit of a book sized in the quote currency', () => {
  const ctx = { instrument: 'deribit|crypto|BTC-PERPETUAL', symbol: 'BTC-PERPETUAL', account: 'a' };

  it('goes with every leg when the user picks the quote currency', () => {
    const { orders } = buildOrders({ side: 'buy', type: 'limit', qty: 25, limit: 50000, sl: 49000, qtyUnit: 'quote' }, ctx);
    expect(orders.map((o) => o.qtyUnit)).toEqual(['quote', 'quote']);
    const t = legsOf(orders);
    expect(t.legs.map((l) => l.qty_unit)).toEqual(['quote', 'quote']);
    expect(t.legs[0].qty).toBe('25');
  });

  it('is left out for a size in the coin, the default', () => {
    const { orders } = buildOrders({ side: 'buy', type: 'market', qty: 0.01 }, ctx);
    expect(orders[0].qtyUnit).toBeNull();
    expect('qty_unit' in legsOf(orders).legs[0]).toBe(false);
  });
});
