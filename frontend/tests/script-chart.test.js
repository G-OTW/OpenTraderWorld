import { test } from 'node:test';
import assert from 'node:assert/strict';
import { refreshScriptInstances, isScript, paneOf, paneKey, computeAll, SCRIPT_TYPE } from '../src/lib/modules/histviz/indicators.js';

const inst = (extra = {}) => ({
  type: SCRIPT_TYPE,
  id: 7,
  visible: true,
  pane: 'pane',
  script: { id: 'row-1', name: 'Old', source: 'old', inputs: { len: 3 }, hidden: [] },
  ...extra
});

test('refreshScriptInstances takes the library row, keeps the source when the row is gone', () => {
  const dag = { type: 'custom', id: 1, custom: { id: 'row-1', name: 'D', def: {} } };
  const rows = [{ id: 'row-1', kind: 'script', name: 'New', source: 'new' }];
  const [s, d] = refreshScriptInstances([inst(), dag], rows);
  assert.equal(s.script.name, 'New');
  assert.equal(s.script.source, 'new');
  assert.deepEqual(s.script.inputs, { len: 3 });
  assert.equal(d, dag);
  const [gone] = refreshScriptInstances([inst()], []);
  assert.equal(gone.script.id, null);
  assert.equal(gone.script.source, 'old');
});

test('a script instance owns its pane, or draws on the price pane', () => {
  assert.ok(isScript(inst()));
  assert.equal(paneOf(inst()), 'oscillator');
  assert.equal(paneOf(inst({ pane: 'overlay' })), 'overlay');
  assert.equal(paneKey(inst()), 'script:7');
});

test('computeAll publishes script outputs under the instance id, usable as a source', () => {
  const bars = { ts: ['a', 'b', 'c'], o: [1, 2, 3], h: [1, 2, 3], l: [1, 2, 3], c: [1, 2, 3], v: [1, 1, 1] };
  const sma = { id: 8, type: 'sma', visible: true, params: { period: 2, source: 'ind:7:0' } };
  const out = computeAll([inst(), sma], bars, new Map([[7, [{ name: 'p', kind: 'line', data: [10, 20, 30], color: null }]]]));
  assert.deepEqual(out.get(7)[0].data, [10, 20, 30]);
  assert.deepEqual(out.get(8)[0].data.slice(1), [15, 25]);
});
