import { test } from 'node:test';
import assert from 'node:assert/strict';
import { blankTrade, ticketOf, tradeBody, draftOf, sameTrade, statusOf, tradeAccounts, tradeOptions } from '$lib/modules/histviz/alertTrade.js';

test('a draft writes the relative ticket the server stores, decimals as strings', () => {
  const d = { ...blankTrade('acc'), type: 'limit', limit: { kind: 'pct', value: '-0.5' }, slOn: true, sl: { kind: 'atr', value: 2 }, sizing: 'quote_amount', amount: '250' };
  const { body } = tradeBody(d);
  assert.deepEqual(body, {
    account_id: 'acc',
    intent: 'enter',
    fallback: 'none',
    ticket: {
      side: 'buy',
      order_type: 'limit',
      size: { kind: 'quote_amount', value: '250' },
      limit_offset: { kind: 'pct', value: '-0.5' },
      stop_loss: { kind: 'atr', value: '2' }
    }
  });
});

test('what cannot make an order is named before anything is saved', () => {
  assert.equal(ticketOf({ ...blankTrade(), qty: 0 }).error, 'size');
  assert.equal(ticketOf({ ...blankTrade(), sizing: 'risk_amount', amount: '50' }).error, 'riskNeedsStop');
  assert.equal(ticketOf({ ...blankTrade(), type: 'stop', stop: { kind: 'abs', value: '' } }).error, 'stop');
  assert.equal(ticketOf({ ...blankTrade(), tpOn: true, tp: { kind: 'pct', value: '-1' } }).error, 'tp');
  assert.equal(tradeBody(blankTrade('')).error, 'account');
  // An exit carries no ticket: it closes what is held.
  assert.deepEqual(ticketOf({ ...blankTrade(), intent: 'exit', qty: 0 }), { ticket: {} });
});

test('a stored trade reads back into the same draft, and arming knows it is unchanged', () => {
  const d = { ...blankTrade('acc'), side: 'sell', type: 'stop_limit', stop: { kind: 'abs', value: '10' }, limit: { kind: 'abs', value: '5' }, tpOn: true, tp: { kind: 'ticks', value: '40' }, fallback: 'close_symbol' };
  const stored = { account_id: 'acc', intent: 'enter', fallback: 'close_symbol', ticket: { ...tradeBody(d).body.ticket, tif: 'gtc' }, armed_at: null, status: 'idle' };
  const back = draftOf(stored);
  assert.equal(back.side, 'sell');
  assert.equal(back.type, 'stop_limit');
  assert.equal(back.tpOn, true);
  assert.equal(back.tp.value, '40');
  // The server fills the time in force; the draft says the same thing either way.
  assert.ok(!sameTrade(d, stored), 'a tif the draft does not carry is a change');
  assert.ok(sameTrade({ ...d, tif: 'gtc' }, stored));
  assert.ok(!sameTrade({ ...d, tif: 'gtc', fallback: 'none' }, stored));
});

test('the list chip says armed while armed, else how the last fire went', () => {
  assert.equal(statusOf({ armed_at: '2026-10-28T10:00:00Z', status: 'idle' }), 'armed');
  assert.equal(statusOf({ armed_at: null, status: 'fallback_failed' }), 'fallback_failed');
  assert.equal(statusOf(null), null);
});

test('only accounts granted to the chart with trading on are offered, the paper broker first', () => {
  const list = tradeAccounts([
    { id: 'b', name: 'Binance', broker: 'binance', modules: ['*', 'trading'] },
    { id: 'p', name: 'Paper', broker: 'paper', modules: ['histviz', 'trading'] },
    { id: 'r', name: 'Read only', broker: 'kraken', modules: ['*'] },
    { id: 'j', name: 'Journal only', broker: 'ibkr', modules: ['journal', 'trading'] }
  ]);
  assert.deepEqual(list.map((a) => a.id), ['p', 'b']);
  // Never good till date: a trade armed for later has no date to give it.
  assert.ok(!tradeOptions({ order_caps: { types: ['market', 'limit'], tif: ['day', 'gtd'] } }).tifs.includes('gtd'));
});
