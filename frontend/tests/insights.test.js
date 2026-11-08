import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import {
  SOURCE_MODULE,
  sourceFromQuery,
  hrefOf,
  bodyOf,
  insightParams,
  journalParams,
  evidenceParts,
  actionOf,
  labHref,
  parseScenario,
  parseWindow,
  applyWindow,
  weekdayName,
  duration
} from '../src/lib/insights/insights.js';

const RUN = '11111111-1111-4111-8111-111111111111';
const SESSION = '22222222-2222-4222-8222-222222222222';
const CAT = '33333333-3333-4333-8333-333333333333';

const q = (s) => new URLSearchParams(s);

test('the page address names one source, and an incomplete one is refused', () => {
  assert.deepEqual(sourceFromQuery(q(`source=journal&category_id=${CAT}&bogus=1`)), {
    source: { kind: 'journal', filters: { category_id: CAT } }
  });
  assert.deepEqual(sourceFromQuery(q('')), { source: { kind: 'journal', filters: {} } });
  assert.deepEqual(sourceFromQuery(q(`source=backtest&run=${RUN}`)), { source: { kind: 'backtest', run: RUN } });
  assert.deepEqual(sourceFromQuery(q('source=backtest')), { error: 'insights.error.noRun' });
  assert.deepEqual(sourceFromQuery(q('source=paper&session=nope')), { error: 'insights.error.noSession' });
  assert.deepEqual(sourceFromQuery(q('source=excel')), { error: 'insights.error.source' });
  assert.equal(SOURCE_MODULE.paper, 'backtest');
});

test('an address and a body round-trip the source', () => {
  for (const src of [
    { kind: 'journal', filters: { category_id: CAT } },
    { kind: 'backtest', run: RUN },
    { kind: 'paper', session: SESSION }
  ]) {
    const back = sourceFromQuery(new URL(hrefOf(src), 'http://x').searchParams).source;
    assert.deepEqual(back, src);
  }
  assert.deepEqual(bodyOf({ kind: 'backtest', run: RUN }), { source: { backtest: { run_id: RUN } } });
  assert.deepEqual(bodyOf({ kind: 'paper', session: SESSION }), { source: { paper: { session_id: SESSION } } });
  assert.deepEqual(bodyOf({ kind: 'journal', filters: { category_id: CAT } }, -60), {
    source: { journal: { filters: { category_id: CAT } } },
    tz_offset: -60
  });
});

const fmt = {
  money: (v) => `$${v}`,
  signedMoney: (v) => `${v > 0 ? '+' : ''}${v}$`,
  int: (v) => String(v),
  num: (v) => `~${v}`,
  pct: (v) => `${v}%`,
  signedPct: (v) => `${v > 0 ? '+' : ''}${v}%`,
  factor: (v) => `x${v}`,
  ratio: (v) => `${v}x`,
  hour: (v) => `${v}:00`,
  weekday: (v) => `day${v}`,
  minutes: (v) => `${v}m`
};

test('statement values are formatted by name and labels are positional', () => {
  const i = { key: 'hour_worst', values: { hour: 15, trades: 10, net: -150 }, labels: [] };
  assert.deepEqual(insightParams(i, fmt), { hour: '15:00', trades: '10', net: '$-150' });
  const side = { key: 'side_drag', values: { weak: -6, strong: 10 }, labels: ['short', 'long'] };
  const p = insightParams(side, fmt, (l, key) => (key === 'side_drag' ? l.toUpperCase() : l));
  assert.deepEqual(p, { weak: '$-6', strong: '$10', 0: 'SHORT', 1: 'LONG' });
  // An unknown value name falls back to a plain number, never to a guess of its unit.
  assert.deepEqual(insightParams({ values: { mystery: 2 } }, fmt), { mystery: '~2' });
  // The journal's own statements keep the journal's formats.
  assert.deepEqual(journalParams({ values: { net: 5, share: 60, winners: 12, gini: 0.3 } }, fmt), {
    net: '+5$',
    share: '60%',
    winners: '12',
    gini: '~0.3'
  });
});

test('the evidence line reads sample, result, drawdown and risk, and skips what is absent', () => {
  assert.deepEqual(evidenceParts({ n: 84, delta_pnl: 1240, delta_dd: -21, delta_risk: null }, fmt), [
    ['insights.evidence.n', { n: '84' }],
    ['insights.evidence.pnl', { v: '+1240$' }],
    ['insights.evidence.dd', { v: '-21$' }]
  ]);
  assert.deepEqual(evidenceParts({ n: 30, delta_pnl: null, delta_dd: 0, delta_risk: -40 }, fmt), [
    ['insights.evidence.n', { n: '30' }],
    ['insights.evidence.risk', { v: '-40%' }]
  ]);
  assert.deepEqual(evidenceParts(null, fmt), []);
});

const windowFilter = { kind: 'filter', window: { hours: [15], weekdays: [], exclude: true } };

test('a journal finding opens the lab, a run finding adds a filter, anything else has no button', () => {
  const journal = { kind: 'journal', filters: { category_id: CAT } };
  const a = actionOf({ apply: windowFilter }, journal);
  assert.equal(a.kind, 'lab');
  const url = new URL(a.href, 'http://x');
  assert.equal(url.pathname, '/journal');
  assert.equal(url.searchParams.get('view'), 'lab');
  assert.equal(url.searchParams.get('category'), CAT);
  assert.deepEqual(parseScenario(url.searchParams.get('scenario')), windowFilter);

  const exits = { kind: 'exits', signals: [{ id: 's', rule: 'sl_pct', params: { pct: 1.1 } }] };
  assert.equal(actionOf({ apply: exits }, journal).kind, 'lab');
  assert.equal(actionOf({ apply: exits }, { kind: 'backtest', run: RUN }), null, 'a stop is not a filter');

  const f = actionOf({ apply: windowFilter }, { kind: 'backtest', run: RUN });
  assert.equal(f.kind, 'filter');
  const fu = new URL(f.href, 'http://x');
  assert.equal(fu.searchParams.get('run'), RUN);
  assert.deepEqual(parseWindow(fu.searchParams.get('window')), windowFilter.window);
  const p = actionOf({ apply: windowFilter }, { kind: 'paper', session: SESSION });
  assert.equal(new URL(p.href, 'http://x').searchParams.get('paper_session'), SESSION);

  const withSignal = { kind: 'filter', signal: { logic: 'all', conditions: [] }, window: windowFilter.window };
  assert.equal(actionOf({ apply: withSignal }, { kind: 'backtest', run: RUN }), null);
  assert.equal(actionOf({ apply: null }, journal), null);
  assert.equal(labHref({ kind: 'size', factor: 1.5 }).includes('category'), false);
});

test('scenarios and windows from an address are checked, never guessed', () => {
  assert.equal(parseScenario('{"kind":"martingale"}'), null);
  assert.equal(parseScenario('not json'), null);
  assert.deepEqual(parseScenario('{"kind":"size","factor":1.5}'), { kind: 'size', factor: 1.5 });
  assert.equal(parseWindow('{"hours":[24]}'), null);
  assert.equal(parseWindow('{"weekdays":[0]}'), null);
  assert.equal(parseWindow('{"hours":[],"weekdays":[]}'), null);
  assert.deepEqual(parseWindow('{"weekdays":[5],"exclude":1}'), { hours: [], weekdays: [5], exclude: true });
});

const blank = { tz_offset_min: 0, timezone: '', weekdays: [], sessions: [], include_dates: [], exclude_dates: [] };

test('a dropped hour becomes one session wrapping over midnight', () => {
  const r = applyWindow(blank, { hours: [15], weekdays: [], exclude: true });
  assert.deepEqual(r.filters.sessions, [{ from: '16:00', to: '15:00' }]);
  assert.deepEqual(r.filters.weekdays, []);
  // The input is not touched.
  assert.deepEqual(blank.sessions, []);
});

test('a found window narrows the sessions a strategy already has, never widens them', () => {
  const f = { ...blank, sessions: [{ from: '09:30', to: '16:00' }] };
  assert.deepEqual(applyWindow(f, { hours: [15], weekdays: [], exclude: true }).filters.sessions, [
    { from: '09:30', to: '15:00' }
  ]);
  assert.deepEqual(applyWindow(f, { hours: [10], weekdays: [], exclude: true }).filters.sessions, [
    { from: '09:30', to: '10:00' },
    { from: '11:00', to: '16:00' }
  ]);
  assert.deepEqual(applyWindow(f, { hours: [9], weekdays: [], exclude: false }).filters.sessions, [
    { from: '09:30', to: '10:00' }
  ]);
  assert.deepEqual(applyWindow(f, { hours: [20], weekdays: [], exclude: false }), { error: 'insights.error.emptyWindow' });
  // An overnight session keeps wrapping.
  const night = { ...blank, sessions: [{ from: '22:00', to: '02:00' }] };
  assert.deepEqual(applyWindow(night, { hours: [23], weekdays: [], exclude: true }).filters.sessions, [
    { from: '00:00', to: '02:00' },
    { from: '22:00', to: '23:00' }
  ]);
});

test('a dropped weekday comes off the strategy weekdays', () => {
  assert.deepEqual(applyWindow(blank, { hours: [], weekdays: [5], exclude: true }).filters.weekdays, [1, 2, 3, 4, 6, 7]);
  const weekdays = { ...blank, weekdays: [1, 2, 3, 4, 5] };
  assert.deepEqual(applyWindow(weekdays, { hours: [], weekdays: [5], exclude: true }).filters.weekdays, [1, 2, 3, 4]);
  assert.deepEqual(applyWindow(weekdays, { hours: [], weekdays: [2], exclude: false }).filters.weekdays, [2]);
  assert.deepEqual(applyWindow({ ...blank, weekdays: [5] }, { hours: [], weekdays: [5], exclude: true }), {
    error: 'insights.error.emptyWindow'
  });
});

test('weekday names and durations read in the reader language', () => {
  assert.equal(weekdayName(1, 'en'), 'Monday');
  assert.equal(weekdayName(5, 'en', 'short'), 'Fri');
  const units = { min: (n) => `${n} min`, hour: (n) => `${n} h`, day: (n) => `${n} d` };
  assert.equal(duration(30, units), '30 min');
  assert.equal(duration(150, units, (x) => x.toFixed(1)), '2.5 h');
  assert.equal(duration(4320, units, (x) => x.toFixed(1)), '3.0 d');
});

test('every statement and every page string exists in English and French, without a dash', async () => {
  const en = JSON.parse(await readFile(new URL('../src/lib/i18n/en.json', import.meta.url), 'utf8'));
  const fr = JSON.parse(await readFile(new URL('../src/lib/i18n/fr.json', import.meta.url), 'utf8'));
  const keys = Object.keys(en).filter((k) => k.startsWith('insights.'));
  const statements = [
    'hour_worst', 'hour_best', 'weekday_worst', 'weekday_best', 'hold_losers_longer', 'hold_winners_longer',
    'stop_tight', 'stop_wide', 'ticker_drag', 'ticker_share', 'class_drag', 'sector_share', 'fees_no_schedule',
    'fees_saving', 'fees_family_saving', 'size_after_losses', 'side_drag'
  ];
  for (const s of statements) assert.ok(en[`insights.statement.${s}`], `en ${s}`);
  for (const k of keys) {
    assert.ok(fr[k], `fr ${k}`);
    for (const v of [en[k], fr[k]]) assert.ok(!/[\u2014\u2013]|-{2}/.test(v), `${k}: ${v}`);
  }
});
