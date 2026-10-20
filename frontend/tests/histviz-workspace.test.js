import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fromRow, toBody, savedKey, newWorkspace, migrateLegacy } from '../src/lib/modules/histviz/workspace.js';
import { templateOf, applyTemplate, templatesIn } from '../src/lib/modules/histviz/templates.js';

const coords = (ticker, timeframe = '1d') => ({ provider: 'p', asset_type: 'equity', ticker, timeframe });
const rsi = { id: 1, type: 'rsi', params: { period: 14 }, visible: true };

test('a workspace round-trips its studies, pane heights and per-symbol drawings', () => {
  const ws = newWorkspace('W');
  ws.panes[0].coords = coords('AAPL');
  ws.panes[0].instances = [rsi];
  ws.panes[0].paneHeights = { rsi: 20 };
  ws.drawings = { 'p|equity|AAPL': [{ id: 'd1', tool: 'hline' }], 'p|equity|MSFT': [] };
  const body = toBody(ws);
  assert.equal(body.settings.v, 2);
  assert.deepEqual(Object.keys(body.settings.drawings), ['p|equity|AAPL'], 'empty boards are dropped');
  const back = fromRow({ id: 'x', name: 'W', grid_rows: 1, grid_cols: 1, ...body });
  assert.equal(back.legacy, false);
  assert.deepEqual(back.panes[0].paneHeights, { rsi: 20 });
  assert.deepEqual(back.drawings['p|equity|AAPL'], [{ id: 'd1', tool: 'hline' }]);
});

test('focusing another chart is not an unsaved change; any other edit is', () => {
  const ws = newWorkspace('W');
  ws.panes[0].coords = coords('AAPL');
  const saved = savedKey(ws);
  ws.active = 'other';
  assert.equal(savedKey(ws), saved);
  ws.panes[0].instances = [rsi];
  assert.notEqual(savedKey(ws), saved);
});

test('a workspace saved before v2 takes its charts and drawings from the instrument layouts', async () => {
  const row = { id: 'x', name: 'Old', grid_rows: 1, grid_cols: 2, panes: [{ id: 'a', coords: coords('AAPL') }, { id: 'b', coords: coords('MSFT') }], settings: {} };
  const ws = fromRow(row);
  assert.equal(ws.legacy, true);
  const layouts = { AAPL: { type: 'line', instances: [rsi], paneHeights: { rsi: 30 }, drawings: [{ id: 'd' }] } };
  await migrateLegacy(ws, async (c) => layouts[c.ticker] ?? null, (key) => (key.endsWith('MSFT') ? [{ id: 'local' }] : []));
  assert.equal(ws.legacy, false);
  assert.equal(ws.panes[0].type, 'line');
  assert.deepEqual(ws.panes[0].instances, [rsi]);
  assert.deepEqual(ws.panes[0].paneHeights, { rsi: 30 });
  assert.deepEqual(ws.drawings['p|equity|AAPL'], [{ id: 'd' }]);
  assert.deepEqual(ws.drawings['p|equity|MSFT'], [{ id: 'local' }], 'this browser copy when the layout has none');
  assert.deepEqual(ws.panes[1].instances, [], 'no layout: the chart keeps what it had');
});

test('a template carries studies and layout to another chart, never its symbol or brick', () => {
  const src = { coords: coords('AAPL'), type: 'renko', brick: 5, instances: [rsi], paneHeights: { rsi: 25 }, settings: { up: '#0f0' } };
  const tpl = templateOf(src, '  Momentum  ');
  assert.equal(tpl.name, 'Momentum');
  src.instances[0].params.period = 99; // the template is a copy
  const dst = { coords: coords('MSFT', '1h'), type: 'candlestick', brick: 0, instances: [], paneHeights: {}, settings: null, link: 'red' };
  applyTemplate(dst, tpl);
  assert.deepEqual(dst.coords, coords('MSFT', '1h'));
  assert.equal(dst.link, 'red');
  assert.equal(dst.type, 'renko');
  assert.equal(dst.brick, 0);
  assert.equal(dst.instances[0].params.period, 14);
  assert.deepEqual(dst.paneHeights, { rsi: 25 });
  assert.deepEqual(dst.settings, { up: '#0f0' });
  dst.instances[0].params.period = 7;
  assert.equal(tpl.instances[0].params.period, 14, 'applying copies too');
  assert.deepEqual(templatesIn({ chartTemplates: [tpl, { name: 'no id' }] }).map((t) => t.name), ['Momentum']);
  assert.deepEqual(templatesIn({}), []);
});
