import { test } from 'node:test';
import assert from 'node:assert/strict';
import { inkOn, DEFAULT_SETTINGS, STYLE_KEYS } from '$lib/modules/histviz/settings.js';

test('inkOn keeps white on the theme greens and reds', () => {
  assert.equal(inkOn('#15866a'), '#fff');
  assert.equal(inkOn('#c94b54'), '#fff');
});

test('inkOn switches to black on a light tag', () => {
  assert.equal(inkOn('#ffffff'), '#000');
  assert.equal(inkOn('#fff'), '#000');
  assert.equal(inkOn('rgb(250, 250, 250)'), '#000');
  assert.equal(inkOn('#f5d000'), '#000');
});

test('inkOn reads the darker shade separately', () => {
  assert.equal(inkOn('#ffffff', 0.55), '#fff');
});

test('inkOn falls back to white on an unparsable color', () => {
  assert.equal(inkOn('var(--green)'), '#fff');
  assert.equal(inkOn(undefined), '#fff');
});

test('trading display settings default to everything shown, no result, full lines', () => {
  const d = DEFAULT_SETTINGS;
  assert.deepEqual([d.posPnl, d.posLine, d.posSide, d.posQty, d.ordLine, d.ordSide, d.ordQty], ['off', 'full', true, true, 'full', true, true]);
  // Per chart, like the rest of the style.
  for (const k of ['posPnl', 'posLine', 'posSide', 'posQty', 'ordLine', 'ordSide', 'ordQty']) assert.ok(STYLE_KEYS.includes(k), k);
});
