import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dataOutputs, upsertPoint, seriesIdsOf, searchSeries } from '$lib/modules/histviz/dataSeries.js';
import { computeAll, paneOf, paneKey } from '$lib/modules/histviz/indicators.js';
import { stepFill, exactFill, evalDef } from '$lib/indicators/evaluate.js';
import { modelToDef, defToModel } from '$lib/indicators/dag.js';

const DAY = 86_400_000;
const day = (i) => new Date(Date.UTC(2024, 0, 1) + i * DAY).toISOString();
const bars = (n) => {
  const ts = Array.from({ length: n }, (_, i) => day(i));
  const c = ts.map((_, i) => 100 + i);
  return { ts, o: c, h: c, l: c, c, v: c };
};
const t0 = Date.UTC(2024, 0, 1);
// [period_start_ms, known_ms|null, value]
const OBS = [
  [t0, t0 + 2 * DAY + 3_600_000, 3.1],
  [t0 + 3 * DAY, null, 3.2]
];

test('a step holds each value until the next point and never reads ahead', () => {
  const times = [0, 1, 2, 3, 4].map((i) => t0 + i * DAY);
  assert.deepEqual(stepFill(times, [[t0 + DAY + 1, 10], [t0 + 3 * DAY, 20]]), [null, null, 10, 20, 20]);
  // Two points on one stamp: the later in input wins, as a revision does.
  assert.deepEqual(stepFill(times, [[t0, 1], [t0, 2]]), [2, 2, 2, 2, 2]);
});

test('an exact fill reads a point only on the bar it lands on', () => {
  const times = [0, 1, 2, 3, 4].map((i) => t0 + i * DAY);
  assert.deepEqual(exactFill(times, [[t0 + DAY + 1, 10], [t0 + 3 * DAY, 20]]), [null, null, 10, 20, null]);
  // Before the first bar: lands nowhere. Two between two bars: the later wins.
  assert.deepEqual(exactFill(times, [[t0 - DAY, 9], [t0 + DAY + 1, 1], [t0 + DAY + 2, 2]]), [null, null, 2, null, null]);
});

test('a live point is placed where the server says it can be read', () => {
  const [row] = upsertPoint([], { period_start: t0, released_at: null, known_at: t0 + 31 * DAY, value: 1 });
  assert.equal(row[1], t0 + 31 * DAY);
});

test('a series step keeps its fill through the builder', () => {
  const { model } = defToModel({ nodes: [{ op: 'series', series: 's', fill: 'exact' }], output: 0 });
  assert.equal(model.steps[0].fill, 'exact');
  assert.equal(modelToDef(model).nodes[0].fill, 'exact');
  const { model: held } = defToModel({ nodes: [{ op: 'series', series: 's' }], output: 0 });
  assert.equal(held.steps[0].fill, 'step');
  assert.equal(modelToDef(held).nodes[0].fill, undefined, 'the default stays out of the saved definition');
});

test('a data pane is placed at the release when known, else at the period start', () => {
  const b = bars(5);
  const at = (where) => dataOutputs({ type: 'data', data: { id: 's', at: where } }, b, { s: OBS })[0];
  const released = at('released');
  assert.equal(released.step, true);
  // Released one hour into day 2: first read on day 3; February has no release, so day 3.
  assert.deepEqual(released.data, [null, null, null, 3.2, 3.2]);
  assert.deepEqual(at('period').data, [3.1, 3.1, 3.1, 3.2, 3.2]);
  // Not loaded yet: nothing drawn, never a guess.
  assert.deepEqual(dataOutputs({ type: 'data', data: { id: 'x' } }, b, {})[0].data, [null, null, null, null, null]);
});

test('a data pane draws in its own pane unless set on the price', () => {
  const inst = { id: 7, type: 'data', pane: 'pane', data: { id: 's' } };
  assert.equal(paneOf(inst), 'oscillator');
  assert.equal(paneKey(inst), 'data:7');
  assert.equal(paneOf({ ...inst, pane: 'overlay' }), 'overlay');
  const out = computeAll([{ ...inst, visible: true }], bars(5), new Map(), { series: { s: OBS } });
  assert.deepEqual(out.get(7)[0].data, [null, null, null, 3.2, 3.2]);
});

test('a live point appends a period or revises it, latest vintage wins', () => {
  const one = upsertPoint(OBS, { period_start: t0 + 3 * DAY, released_at: null, value: 3.3 });
  assert.equal(one.length, 2);
  assert.deepEqual(one[1], [t0 + 3 * DAY, null, 3.3]);
  const two = upsertPoint(one, { period_start: t0 + 5 * DAY, released_at: t0 + 6 * DAY, value: 3.4 });
  assert.equal(two.length, 3);
  assert.equal(upsertPoint(two, { period_start: t0 + 5 * DAY, released_at: null, value: 3.4 }), two, 'nothing new, same array');
  // A missing release never erases the one held.
  assert.equal(upsertPoint(OBS, { period_start: t0, released_at: null, value: 3.0 })[0][1], OBS[0][1]);
});

test('the series a chart needs come from its data panes and its series nodes', () => {
  const ids = seriesIdsOf([
    { type: 'data', data: { id: 'a' } },
    { type: 'custom', custom: { def: { nodes: [{ op: 'series', series: 'b' }, { op: 'price', field: 'close' }] } } },
    { type: 'rsi' }
  ]);
  assert.deepEqual(ids.sort(), ['a', 'b']);
  const list = [{ title: 'US CPI', code: 'CPIAUCSL' }, { title: 'Unemployment', code: 'UNRATE' }];
  assert.equal(searchSeries(list, 'cpi').length, 1);
  assert.equal(searchSeries(list, 'unr')[0].code, 'UNRATE');
});

test('a series step compiles to a series node and back', () => {
  const model = {
    steps: [
      { id: 'a', name: 'cpi', kind: 'series', series: 'abc', at: 'period' },
      { id: 'b', name: 'f1', kind: 'formula', expr: '@close / @cpi' }
    ],
    outputs: ['f1']
  };
  const def = modelToDef(model);
  assert.deepEqual(def.nodes[0], { op: 'series', series: 'abc', at: 'period' });
  const back = defToModel(def).model;
  assert.equal(back.steps[0].kind, 'series');
  assert.equal(back.steps[0].series, 'abc');
  assert.equal(back.steps[0].at, 'period');
  // Evaluated over bars with stamps and the loaded observations.
  const b = bars(5);
  const [out] = evalDef(def, { ...b, t: b.ts.map(Date.parse) }, { series: { abc: OBS } });
  assert.equal(out[0], 100 / 3.1);
  assert.throws(() => modelToDef({ steps: [{ id: 'x', name: 's1', kind: 'series', series: '' }], outputs: ['s1'] }));
});
