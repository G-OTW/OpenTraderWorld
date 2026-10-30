import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bookRows, compareRows, execBadge, execState, nextStep, slippage, stateTone } from '../src/lib/modules/backtest/paper-execution.js';

test('a simulated session carries no badge; a broker one says whether money is real', () => {
  assert.equal(execBadge({ execution: null }), null);
  assert.equal(execBadge({ execution: { target: 'simulate' } }), null);
  assert.equal(execBadge({ execution: { target: 'broker' }, exec_level: 'paper' }), 'broker');
  assert.equal(execBadge({ execution: { target: 'broker' }, exec_level: 'sandbox' }), 'broker');
  assert.equal(execBadge({ execution: { target: 'broker' }, exec_level: 'live' }), 'live');
});

test('the state chip says disarmed before anything the last sync found', () => {
  assert.equal(execState({ target: 'simulate' }), 'simulate');
  assert.equal(execState({ target: 'broker', armed_at: null, state: 'idle' }), 'disarmed');
  assert.equal(execState({ target: 'broker', armed_at: null, state: 'error' }), 'error');
  assert.equal(execState({ target: 'broker', armed_at: 'x', state: 'degraded' }), 'degraded');
  assert.equal(execState({ target: 'broker', armed_at: 'x', state: 'whatever' }), 'synced');
  assert.equal(stateTone('synced'), 'ok');
  assert.equal(stateTone('degraded'), 'warn');
  assert.equal(stateTone('blocked'), 'bad');
});

test('slippage is signed against the trader, from decimal strings', () => {
  assert.equal(slippage(100, '100.4', true), 100.4 - 100);
  assert.equal(slippage('110', '109.5', false), 0.5);
  assert.equal(slippage(null, '1', true), null);
  assert.equal(slippage(1, undefined, true), null);
  const rows = compareRows({ positions: [{ side: 'short', sim_entry: 100, real_entry: '99', sim_exit: 90, real_exit: '91' }] });
  // A short sells in (receiving 1 less is worse) and buys out (paying 1 more is worse).
  assert.equal(rows[0].entrySlip, 1);
  assert.equal(rows[0].exitSlip, 1);
});

test('the books read side by side, a difference nobody explains flagged', () => {
  const view = { book: { instruments: [
    { key: 'binance:BTCUSDT', ticker: 'BTCUSDT', symbol: 'BTCUSDT', intended: { side: 'long', stop: 97, target: 110 },
      broker: { held: '0.5', own: '0.123', baseline: '0', drift: '0.377', working: [{ role: 'stop' }] } },
    { key: 'binance:ETHUSDT', ticker: 'ETHUSDT', symbol: 'ETHUSDT', intended: {}, broker: { held: '0', own: '0', drift: '0' } }
  ] } };
  const rows = bookRows(view);
  assert.equal(rows[0].drifting, true);
  assert.equal(rows[0].drift, 0.377);
  assert.equal(rows[0].working.length, 1);
  assert.equal(rows[1].drifting, false);
  assert.equal(rows[1].side, null);
  assert.deepEqual(bookRows(null), []);
});

test('a refusal names the step the page offers next', () => {
  assert.equal(nextStep({ code: 'background_trading_required' }), 'sign');
  assert.equal(nextStep({ code: 'code_required' }), 'code');
  assert.equal(nextStep({ code: 'trading_locked' }), 'code');
  assert.equal(nextStep({ code: 'live_off' }), null);
  assert.equal(nextStep(new Error('x')), null);
});
