import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  age,
  capitalBody,
  countdown,
  envTone,
  filterFleet,
  fleetAccounts,
  robotState,
  setThenKind,
  setWhenKind,
  shortDuration,
  stateTone
} from '../src/lib/modules/robots/fleet.js';

const NOW = Date.parse('2026-11-08T12:00:00Z');

test('a robot state reads running, error, stopped, held, then the sync', () => {
  assert.equal(robotState({ running: true, status: 'active' }), 'running');
  assert.equal(robotState({ status: 'error' }), 'error');
  assert.equal(robotState({ status: 'paused' }), 'stopped');
  assert.equal(robotState({ status: 'active', guard_holds: 'paused by its guard' }), 'held');
  assert.equal(robotState({ status: 'active', exec_state: 'degraded' }), 'degraded');
  assert.equal(robotState({ status: 'active', exec_state: 'synced' }), 'active');
  // Disarmed by an act at the broker: the user arms it again, before any guard hold.
  assert.equal(robotState({ status: 'active', exec_state: 'stopped', guard_holds: 'paused' }), 'interrupted');
  assert.equal(robotState({ status: 'active', exec_state: 'unreachable' }), 'unreachable');
  assert.equal(stateTone('interrupted'), 'bad');
  assert.equal(stateTone('unreachable'), 'bad');
  assert.equal(stateTone('held'), 'warn');
  assert.equal(stateTone('blocked'), 'bad');
  assert.equal(stateTone('active'), 'ok');
  assert.equal(envTone('live'), 'danger');
  assert.equal(envTone('testnet'), 'warn');
  assert.equal(envTone('simulated'), 'neutral');
});

test('the countdown and the age are short and never negative', () => {
  assert.equal(shortDuration(45), '45s');
  assert.equal(shortDuration(185), '3m 05s');
  assert.equal(shortDuration(7800), '2h 10m');
  assert.equal(shortDuration(273600), '3d 4h');
  assert.equal(countdown('2026-11-08T12:01:05Z', NOW), '1m 05s');
  assert.equal(countdown('2026-11-08T11:59:00Z', NOW), 'due');
  assert.equal(countdown(null, NOW), null);
  assert.equal(age('2026-11-08T11:59:30Z', NOW), '30s');
});

test('the fleet filters by state and account', () => {
  const robots = [
    { id: 'a', status: 'active', account_id: 'x', account: 'Paper' },
    { id: 'b', status: 'paused', account_id: null },
    { id: 'c', status: 'active', running: true, account_id: 'x', account: 'Paper' },
    { id: 'd', status: 'active', guard_holds: 'halted', account_id: 'y', account: 'Testnet' }
  ];
  assert.deepEqual(filterFleet(robots, { status: 'active' }).map((r) => r.id), ['a', 'c']);
  assert.deepEqual(filterFleet(robots, { status: 'held' }).map((r) => r.id), ['d']);
  assert.deepEqual(filterFleet(robots, { account: 'simulated' }).map((r) => r.id), ['b']);
  assert.deepEqual(filterFleet(robots, { account: 'x' }).map((r) => r.id), ['a', 'c']);
  assert.deepEqual(fleetAccounts(robots), [{ id: 'x', name: 'Paper' }, { id: 'y', name: 'Testnet' }]);
});

test('a guard rule keeps its number across kinds and takes the fields its kind needs', () => {
  assert.deepEqual(setWhenKind({ kind: 'consecutive_losses', n: 4 }, 'losses_in_window'), { kind: 'losses_in_window', n: 4, window: 'day' });
  assert.deepEqual(setWhenKind({ kind: 'consecutive_losses', n: 4 }, 'drawdown_pct'), { kind: 'drawdown_pct', pct: 4, from: 'peak' });
  assert.deepEqual(setWhenKind({ kind: 'loss_pct', pct: 0 }, 'loss_amount'), { kind: 'loss_amount', amount: 5, window: 'day' });
  assert.deepEqual(setThenKind('scale'), { kind: 'scale', divisor: 2, restore: { kind: 'after_trades', n: 5 } });
  assert.deepEqual(setThenKind('halt'), { kind: 'halt' });
});

test('a capital policy is sent with only what was set', () => {
  assert.deepEqual(capitalBody({ allocation: ' 1000 ', currency: 'usdt', percent_in: 50, max_positions: '', max_orders_per_day: '4' }), {
    allocation: '1000', currency: 'USDT', percent_in: '50', max_orders_per_day: 4
  });
  assert.deepEqual(capitalBody({}), {});
});
