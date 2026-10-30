import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import {
  LAB_TABS,
  resolveView,
  parseList,
  formatList,
  emptyForm,
  buildScenario,
  formOf,
  summaryRows,
  countBy,
  statementParams
} from '../src/lib/modules/journal/lab.js';

const NAV = ['breakdown', 'trades', 'lab', 'fees'];

test('the old exits view opens the lab on its Exits tab', () => {
  assert.deepEqual(resolveView('exits', NAV), { view: 'lab', tab: 'exits' });
  assert.deepEqual(resolveView('lab', NAV), { view: 'lab', tab: null });
  assert.deepEqual(resolveView('trades', NAV), { view: 'trades', tab: null });
  assert.equal(resolveView('nope', NAV), null);
  assert.equal(resolveView(null, NAV), null);
  assert.deepEqual(LAB_TABS, ['exits', 'size', 'filter', 'reverse', 'fees']);
});

test('hour lists parse ranges and refuse what is not an hour', () => {
  assert.deepEqual(parseList('9, 14-16, 9', 0, 23), [9, 14, 15, 16]);
  assert.deepEqual(parseList('', 0, 23), []);
  assert.equal(parseList('24', 0, 23), null);
  assert.equal(parseList('16-14', 0, 23), null);
  assert.equal(parseList('nine', 0, 23), null);
  assert.equal(formatList([16, 9, 10, 11, 14]), '9-11, 14, 16');
  assert.deepEqual(parseList(formatList([0, 1, 2, 23]), 0, 23), [0, 1, 2, 23]);
});

test('a size form becomes a size scenario, and a bad factor is refused', () => {
  const f = { ...emptyForm(), factor: '2', cap: '150' };
  assert.deepEqual(buildScenario('size', f), { scenario: { kind: 'size', factor: 2, daily_loss_cap: 150 } });
  assert.deepEqual(buildScenario('size', { ...f, cap: '' }), { scenario: { kind: 'size', factor: 2 } });
  assert.equal(buildScenario('size', { ...f, factor: 4 }).error, 'journal.lab.error.factor');
  assert.equal(buildScenario('size', { ...f, cap: '-1' }).error, 'journal.lab.error.cap');
});

test('a filter needs a window or a condition, and reverse can carry one', () => {
  const f = { ...emptyForm(), hours: '15', weekdays: [5, 1], exclude: true };
  assert.deepEqual(buildScenario('filter', f).scenario, {
    kind: 'filter',
    window: { hours: [15], weekdays: [1, 5], exclude: true }
  });
  assert.equal(buildScenario('filter', { ...f, hours: '', weekdays: [] }).error, 'journal.lab.error.emptyWindow');
  assert.equal(buildScenario('filter', { ...f, useWindow: false }).error, 'journal.lab.error.emptyFilter');
  assert.equal(buildScenario('filter', { ...f, useSignal: true }).error, 'journal.lab.error.emptySignal');
  assert.deepEqual(buildScenario('reverse', emptyForm()).scenario, { kind: 'reverse' });
  assert.deepEqual(buildScenario('reverse', { ...f, onlyMatching: true }).scenario.only.window.hours, [15]);
});

test('a fees scenario names its schedule, and the family move only when picked', () => {
  const f = { ...emptyForm(), scheduleId: 'abc' };
  assert.deepEqual(buildScenario('fees', f).scenario, { kind: 'fees', schedule_id: 'abc' });
  assert.deepEqual(buildScenario('fees', { ...f, family: 'mnq_nq', to: 'small' }).scenario, {
    kind: 'fees',
    schedule_id: 'abc',
    family: 'mnq_nq',
    to: 'small'
  });
  assert.equal(buildScenario('fees', emptyForm()).error, 'journal.lab.error.schedule');
});

test('a scenario from the strategy card fills the form it came from', () => {
  for (const s of [
    { kind: 'size', factor: 1.5, daily_loss_cap: 200 },
    { kind: 'filter', window: { hours: [14], weekdays: [], exclude: true } },
    { kind: 'reverse', only: { window: { hours: [9, 10], weekdays: [2], exclude: false } } },
    { kind: 'fees', schedule_id: 'x', family: 'mes_es', to: 'large' }
  ]) {
    assert.deepEqual(buildScenario(s.kind, formOf(s)).scenario, s);
  }
});

test('the before and after rows colour the change by which way is good', () => {
  const rows = summaryRows(
    { n: 10, net: 100, fees: 20, max_drawdown: 50, win_rate: 50 },
    { n: 8, net: 140, fees: 25, max_drawdown: 30, win_rate: 50 }
  );
  const by = Object.fromEntries(rows.map((r) => [r.key, r]));
  assert.equal(by.net.delta, 40);
  assert.equal(by.net.tone, 'pos');
  assert.equal(by.fees.tone, 'neg');
  assert.equal(by.max_drawdown.tone, 'pos');
  assert.equal(by.win_rate.tone, '');
  assert.equal(by.n.tone, '');
  assert.equal(by.profit_factor.delta, null);
});

test('reasons are counted and statement numbers formatted by their kind', () => {
  assert.deepEqual(countBy([{ reason: 'fx' }, { reason: 'no_bars' }, { reason: 'no_bars' }], 'reason'), [
    ['no_bars', 2],
    ['fx', 1]
  ]);
  const p = statementParams({ n: 25, delta: -12.5, factor: 1.5, other: 2 }, {
    int: (v) => `i${v}`,
    money: (v) => `m${v}`,
    factor: (v) => `x${v}`,
    num: (v) => `n${v}`
  });
  assert.deepEqual(p, { n: 'i25', delta: 'm-12.5', factor: 'x1.5', other: 'n2' });
});

test('every lab key the components name exists in the English pack', async () => {
  const en = JSON.parse(await readFile(new URL('../src/lib/i18n/en.json', import.meta.url), 'utf8'));
  const fr = JSON.parse(await readFile(new URL('../src/lib/i18n/fr.json', import.meta.url), 'utf8'));
  const keys = [
    ...['size', 'filter', 'reverse', 'fees'].flatMap((k) => [`journal.lab.${k}.hint`, `journal.lab.after.${k}`]),
    ...LAB_TABS.map((k) => `journal.lab.tab.${k}`),
    ...['size', 'filter', 'reverse', 'fees', 'exits'].flatMap((k) => [
      `journal.lab.statement.${k}_better`,
      `journal.lab.statement.${k}_worse`
    ]),
    ...summaryRows({}, {}).map((r) => `journal.lab.metric.${r.key}`),
    ...['stop_mae', 'target_mfe', 'drop_hour', 'size_up', 'size_upCap'].map((k) => `journal.lab.card.scenario.${k}`),
    'journal.page.nav.lab'
  ];
  for (const k of keys) {
    assert.ok(en[k], `en: ${k}`);
    assert.ok(fr[k], `fr: ${k}`);
  }
  for (const [k, v] of Object.entries(en).filter(([k]) => k.startsWith('journal.lab.'))) {
    assert.ok(!v.includes('—') && !v.includes('--'), `no em dash or double dash in ${k}`);
  }
});
