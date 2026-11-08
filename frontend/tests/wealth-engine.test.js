import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  pairMath,
  runway,
  liquidTotal,
  cpiAt,
  deflate,
  compareSnapshots,
  monthsBack,
  walletAddressOk,
  annuity,
  taxLink
} from '../src/lib/modules/wealth/engine.js';

test('LTV and equity of a house and its mortgage', () => {
  assert.deepEqual(pairMath(400_000, 300_000), { equity: 100_000, ltv: 75 });
  // The debt may arrive signed: the balance is what counts.
  assert.deepEqual(pairMath(400_000, -100_000), { equity: 300_000, ltv: 25 });
  // Under water: equity negative, LTV above 100.
  assert.equal(pairMath(200_000, 250_000).ltv, 125);
  // No value, no ratio.
  assert.equal(pairMath(0, 1000).ltv, null);
  assert.deepEqual(pairMath(null, 1000), { equity: null, ltv: null });
});

test('runway is liquid money over the monthly cost', () => {
  assert.equal(runway(12_000, 1_500, 500).months, 6);
  assert.equal(runway(12_000, 2_000, null).months, 6);
  assert.equal(runway(-500, 1_000, 0).months, 0);
  assert.equal(runway(12_000, 0, 0).reason, 'no_cost');
  assert.equal(runway(null, 1_000, 0).reason, 'no_liquid');
  assert.equal(liquidTotal({ Cash: 10, Broker: 5, House: 100 }, ['Cash', 'Broker']), 15);
  assert.equal(liquidTotal({ Cash: 10 }, []), null);
});

test('deflation reads the index in force at each date', () => {
  const obs = [
    ['2024-01-01', 100],
    ['2025-01-01', 110],
    ['2026-01-01', 121]
  ];
  assert.equal(cpiAt(obs, '2023-12-31'), null);
  assert.equal(cpiAt(obs, '2025-06-30'), 110);
  const real = deflate(
    [
      { at: '2023-12-31', net_worth: 50 },
      { at: '2024-12-31', net_worth: 100 },
      { at: '2026-02-01', net_worth: 121, estimated: 0 }
    ],
    obs
  );
  // Before the series: a gap, not the nominal value.
  assert.equal(real[0].net_worth, null);
  // 100 in 2024 money is 121 in 2026 money.
  assert.equal(real[1].net_worth, 121);
  // The latest point is the base: unchanged.
  assert.equal(real[2].net_worth, 121);
  assert.deepEqual(deflate([{ at: '2026-01-01', net_worth: 1 }], []), []);
});

test('then versus now lines up every category with its change', () => {
  const { rows, total } = compareSnapshots(
    { net_worth: 100, by_category: { Cash: 40, House: 60 } },
    { net_worth: 150, by_category: { Cash: 50, House: 60, Crypto: 40 } }
  );
  assert.deepEqual(rows.map((r) => r.name), ['Crypto', 'Cash', 'House']);
  const crypto = rows[0];
  assert.equal(crypto.then, null);
  assert.equal(crypto.delta, 40);
  assert.equal(crypto.pct, null);
  assert.equal(rows[1].pct, 25);
  assert.deepEqual(total, { then: 100, now: 150, delta: 50, pct: 50 });
});

test('months back clamps the day', () => {
  assert.equal(monthsBack('2026-03-31', 1), '2026-02-28');
  assert.equal(monthsBack('2026-11-06', 12), '2025-11-06');
  assert.equal(monthsBack('2026-11-06', 60), '2021-11-06');
});

test('wallet address format', () => {
  assert.ok(walletAddressOk('btc', 'bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh'));
  assert.ok(walletAddressOk('btc', '1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa'));
  assert.ok(!walletAddressOk('btc', 'bc1Qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh'));
  assert.ok(!walletAddressOk('btc', '0x52908400098527886E0F7030069857D2E4169EE7'));
  assert.ok(walletAddressOk('eth', '0x52908400098527886E0F7030069857D2E4169EE7'));
  assert.ok(!walletAddressOk('eth', '0x5290'));
});

test('annuity preview matches the textbook mortgage', () => {
  assert.ok(Math.abs(annuity(200_000, 6, 360) - 1199.1) < 0.01);
  assert.equal(annuity(1200, 0, 12), 100);
  assert.equal(annuity(0, 5, 12), null);
});

test('tax link sums only the picked categories', () => {
  const { base, href } = taxLink({ Cash: 1000.123, House: 5000, Debt: -2000 }, new Set(['Cash', 'Debt']), 'EUR');
  assert.ok(Math.abs(base - -999.877) < 1e-9);
  assert.equal(href, '/taxcalc?wealth_base=-999.88&wealth_currency=EUR');
});

import { periodQuery, periodPoints, curveChange, revisionChange } from '../src/lib/modules/wealth/engine.js';

test('headline periods map to breakdown samples', () => {
  assert.deepEqual(periodQuery('1M', '2026-11-06'), { granularity: 'month', points_back: 2 });
  // YTD in November: eleven month ends from the last day of last year, plus today.
  assert.equal(periodQuery('YTD', '2026-11-06').points_back, 12);
  assert.equal(periodQuery('YTD', '2026-01-15').points_back, 2);
  assert.equal(periodQuery('1Y', '2026-11-06').points_back, 13);
  assert.equal(periodQuery('ALL', '2026-11-06').points_back, 60);
});

test('ALL starts at the first valuation, other periods keep their samples', () => {
  const pts = [
    { at: '2025-08-31', net_worth: 0 },
    { at: '2025-09-30', net_worth: 100 },
    { at: '2025-10-31', net_worth: 150 }
  ];
  assert.equal(periodPoints(pts, 'ALL').length, 2);
  assert.equal(periodPoints(pts, '1Y').length, 3);
  assert.deepEqual(periodPoints([], 'ALL'), []);
});

test('curve change reads first to last, no percent from zero', () => {
  assert.deepEqual(curveChange([{ at: 'a', net_worth: 100 }, { at: 'b', net_worth: 150 }]), { abs: 50, pct: 50, from: 'a' });
  assert.equal(curveChange([{ at: 'a', net_worth: 0 }, { at: 'b', net_worth: 10 }]).pct, null);
  // A real curve with a gap still measures between the points it has.
  assert.equal(curveChange([{ at: 'a', net_worth: null }, { at: 'b', net_worth: 80 }, { at: 'c', net_worth: 100 }]).abs, 20);
  assert.equal(curveChange([{ at: 'a', net_worth: 5 }]).abs, null);
});

test('revision change compares the two newest values', () => {
  const r = revisionChange([{ value: 279_000, valued_at: '2026-09-05' }, { value: 268_000, valued_at: '2025-09-15' }]);
  assert.equal(r.abs, 11_000);
  assert.equal(r.since, '2025-09-15');
  assert.equal(revisionChange([{ value: 1 }]).abs, null);
});
