import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  makeDrawing,
  translate,
  moveAnchor,
  isThreePoint,
  isTwoPoint,
  fibExtPrices,
  drawTool
} from '../src/lib/modules/histviz/drawings.js';

test('new tools take the anchor count their gesture needs', () => {
  assert.equal(drawTool('hray').points, 1);
  assert.ok(isTwoPoint('callout'));
  assert.ok(isThreePoint('channel'));
  assert.ok(isThreePoint('fibext'));
  assert.ok(!isThreePoint('fib'));
});

test('a three-point drawing keeps c through move and anchor drag', () => {
  const d = makeDrawing('channel', { x: 0, y: 100 }, { x: 10, y: 110 }, { c: { x: 5, y: 120 } });
  const moved = translate(d, 2, -5);
  assert.deepEqual(moved.c, { x: 7, y: 115 });
  assert.deepEqual(moved.a, { x: 2, y: 95 });
  const dragged = moveAnchor(d, 'c', { x: 6, y: 130 });
  assert.deepEqual(dragged.c, { x: 6, y: 130 });
  assert.deepEqual(dragged.a, d.a);
});

test('fib extension projects the a to b leg from c', () => {
  const d = makeDrawing('fibext', { x: 0, y: 100 }, { x: 10, y: 120 }, { c: { x: 15, y: 110 } });
  const by = Object.fromEntries(fibExtPrices(d).map((l) => [l.lv, l.price]));
  assert.equal(by[0], 110);
  assert.equal(by[1], 130);
  assert.equal(by[1.618], 110 + 20 * 1.618);
  // A down leg projects down.
  const down = makeDrawing('fibext', { x: 0, y: 120 }, { x: 10, y: 100 }, { c: { x: 15, y: 110 } });
  assert.equal(fibExtPrices(down).find((l) => l.lv === 1).price, 90);
});
