import { test } from 'node:test';
import assert from 'node:assert/strict';
import { alertMarks } from '../src/lib/modules/histviz/alertMarks.js';

const coords = { provider: 'binance', asset_type: 'crypto', ticker: 'BTCUSDT', timeframe: '1h' };
const alert = (o = {}) => ({
  id: 'a',
  enabled: true,
  provider: 'BINANCE',
  asset_type: 'crypto',
  ticker: 'btcusdt',
  timeframe: '1h',
  op: 'above',
  lhs: { kind: 'price', field: 'close' },
  rhs: { kind: 'const', value: 100 },
  ...o
});
const rsi = { kind: 'indicator', indicator: 'rsi', period: 14 };
const study = (o = {}) => ({ id: 1, type: 'rsi', visible: true, params: { period: 14 }, ...o });

test('a price level shows on its instrument at every timeframe, and nowhere else', () => {
  const m = alertMarks([alert({ timeframe: '1d' })], coords, []);
  assert.equal(m.levels.length, 1);
  assert.equal(m.levels[0].value, 100);
  assert.equal(m.levels[0].study, null);
  assert.equal(alertMarks([alert({ ticker: 'ETHUSDT' })], coords, []).levels.length, 0);
  assert.equal(alertMarks([alert({ provider: 'kraken' })], coords, []).levels.length, 0);
});

test('a time alert shows on every chart, whatever its instrument and timeframe', () => {
  const at = '2030-01-01T10:00:00Z';
  const m = alertMarks([alert({ op: 'at_time', ticker: 'AAPL', timeframe: '1d', fire_at: at })], coords, []);
  assert.deepEqual(m.times.map((x) => x.at), [Date.parse(at)]);
  assert.equal(m.levels.length, 0);
});

test('a paused or expired alert is not on the scales', () => {
  assert.equal(alertMarks([alert({ enabled: false })], coords, []).levels.length, 0);
  assert.equal(alertMarks([alert({ expires_at: '2020-01-01T00:00:00Z' })], coords, []).levels.length, 0);
});

test('an indicator in price units sits on the price scale, at its own timeframe only', () => {
  const lhs = { kind: 'indicator', indicator: 'bb_upper', period: 20, mult: 2 };
  assert.equal(alertMarks([alert({ lhs })], coords, []).levels[0].study, null);
  assert.equal(alertMarks([alert({ lhs, timeframe: '1d' })], coords, []).levels.length, 0);
});

test('an oscillator level needs the same study on the chart, at its own timeframe', () => {
  const s = study();
  assert.equal(alertMarks([alert({ lhs: rsi })], coords, [s]).levels[0].study, s);
  assert.equal(alertMarks([alert({ lhs: rsi })], coords, []).levels.length, 0);
  assert.equal(alertMarks([alert({ lhs: rsi, timeframe: '5m' })], coords, [s]).levels.length, 0);
  assert.equal(alertMarks([alert({ lhs: rsi })], coords, [study({ visible: false })]).levels.length, 0);
  assert.equal(alertMarks([alert({ lhs: rsi })], coords, [study({ params: { period: 21 } })]).levels.length, 0);
  assert.equal(alertMarks([alert({ lhs: rsi })], coords, [study({ params: { period: 14, source: 'hl2' } })]).levels.length, 0);
});

test('operand fields map onto the study params', () => {
  const s = { id: 2, type: 'stochastic', visible: true, params: { k: 14, d: 3 } };
  const lhs = { kind: 'indicator', indicator: 'stoch_k', period: 14, signal_period: 3 };
  assert.equal(alertMarks([alert({ lhs })], coords, [s]).levels[0].study, s);
  const m = { id: 3, type: 'macd', visible: true, params: { fast: 12, slow: 26, signal: 5 } };
  const line = { kind: 'indicator', indicator: 'macd', fast: 12, slow: 26 };
  assert.equal(alertMarks([alert({ lhs: line })], coords, [m]).levels[0].study, m);
  const signal = { ...line, indicator: 'macd_signal', signal_period: 9 };
  assert.equal(alertMarks([alert({ lhs: signal })], coords, [m]).levels.length, 0);
});

test('a library indicator reads the study drawn from its row', () => {
  const s = { id: 4, type: 'script', visible: true, pane: 'pane', script: { id: 'row' } };
  const lhs = { kind: 'custom_indicator', id: 'row', plot: 'x' };
  assert.equal(alertMarks([alert({ lhs })], coords, [s]).levels[0].study, s);
  assert.equal(alertMarks([alert({ lhs: { ...lhs, id: 'other' } })], coords, [s]).levels.length, 0);
});

test('nothing fixed to read on a scale, nothing drawn', () => {
  const none = (o) => assert.equal(alertMarks([alert(o)], coords, []).levels.length, 0);
  none({ rhs: { kind: 'indicator', indicator: 'sma', period: 20 } });
  none({ lhs: { kind: 'metric', metric: 'change_pct', period: 5 } });
  none({ lhs: { kind: 'price', field: 'volume' } });
});
