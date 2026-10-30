import { test } from 'node:test';
import assert from 'node:assert/strict';
import { discoverParams } from '../src/lib/modules/backtest/optimize.js';
import { embedIndicators, usedScriptIds, pickablePlots, defaultPlot } from '../src/lib/modules/backtest/embed.js';
import { strategyInstances } from '../src/lib/modules/backtest/chart.js';

const t = (k) => k;

/** The metadata `compile` reports for `len = input.int(14, "Length", minval = 2, maxval = 20,
 *  step = 2)`, `mult = input.float(2.0, "Mult")`, `src = input.source(close, "Source")`. */
const META = {
  title: 'Band',
  inputs: [
    {
      key: 'len',
      title: 'Length',
      kind: 'Int',
      default: { Int: 14 },
      minval: 2,
      maxval: 20,
      step: 2,
      options: []
    },
    {
      key: 'mult',
      title: 'Mult',
      kind: 'Float',
      default: { Float: 2 },
      minval: null,
      maxval: null,
      step: null,
      options: []
    },
    {
      key: 'src',
      title: 'Source',
      kind: 'Source',
      default: { Source: 'close' },
      minval: null,
      maxval: null,
      step: null,
      options: []
    }
  ],
  plots: [{ index: 0, title: 'mid' }]
};

function settings(operand) {
  const left = operand ? { kind: 'custom_indicator', id: 's1', plot: 'mid' } : { kind: 'price', field: 'close' };
  return {
    kind: 'signals',
    mode: 'long',
    long: {
      entry: {
        logic: 'all',
        conditions: [{ left, op: 'above', right: { kind: 'const', value: 1 } }]
      }
    },
    scripts: { s1: { name: 'Band', source: '', inputs: { mult: 2.5 } } }
  };
}

test('script inputs are params: number inputs only, range inside minval/maxval, input step', () => {
  const ps = discoverParams(settings(true), t, {
    scripts: new Map([['s1', META]])
  }).filter((p) => p.id.startsWith('scripts.'));
  assert.equal(ps.length, 2);
  const [len, mult] = ps;
  assert.deepEqual(len.paths, ['scripts.s1.inputs.len']);
  assert.equal(len.id, 'scripts.s1.inputs.len');
  assert.deepEqual([len.group, len.kind, len.scale, len.label, len.value], ['signals', 'int', 1, 'Band Length', 14]);
  assert.deepEqual([len.from, len.to, len.step], [7, 20, 2]);
  assert.deepEqual([mult.id, mult.kind, mult.value], ['scripts.s1.inputs.mult', 'num', 2.5]);
});

test('a script no operand reads gives no params', () => {
  const ps = discoverParams(settings(false), t, {
    scripts: new Map([['s1', META]])
  });
  assert.ok(!ps.some((p) => p.id.startsWith('scripts.')));
});

// ── Task 8: the strategy editor and the result chart ────────────────────────────────────────

const SRC = '//@version=6\nindicator("Cross")\nlen = input.int(10)\nplot(ta.sma(close, len), "fast")';
const op = (id, plot) => ({ kind: 'custom_indicator', id, plot });
const strat = (left) => ({
  kind: 'signals',
  mode: 'long',
  long: { entry: { logic: 'all', conditions: [{ left, op: 'above', right: { kind: 'const', value: 1 } }] } },
  scripts: { 7: { name: 'old', source: 'old', inputs: { len: 20 } }, 9: { name: 'gone', source: 'x', inputs: {} } }
});
const LIB = [
  {
    id: '7',
    name: 'Cross',
    kind: 'script',
    source: SRC,
    meta: { script: { overlay: true, inputs: [{ key: 'len' }], plots: [{ index: 0, title: 'fast', causal: true }] } }
  },
  { id: '9', name: 'Unused', kind: 'script', source: SRC, meta: { script: { overlay: false, inputs: [], plots: [] } } },
  { id: 'd', name: 'Dag', kind: 'dag', definition: { nodes: [] } }
];

test('embedIndicators keeps script inputs and drops scripts no operand reads', () => {
  const out = embedIndicators(strat(op('7', 'fast')), LIB);
  assert.deepEqual(out.scripts, { 7: { name: 'Cross', source: SRC, inputs: { len: 20 } } });
  assert.deepEqual(out.indicators, {});
  assert.deepEqual(usedScriptIds(out), ['7']);
  const fresh = embedIndicators({ ...strat(op('7')), scripts: undefined }, LIB);
  assert.deepEqual(fresh.scripts[7].inputs, {});
});

test('embedIndicators drops an input the library script no longer declares', () => {
  const s = strat(op('7', 'fast'));
  s.scripts[7].inputs = { len: 20, gone: 3 };
  assert.deepEqual(embedIndicators(s, LIB).scripts[7].inputs, { len: 20 });
});

test('pickablePlots: tainted and duplicated titles are disabled with their reason', () => {
  const meta = {
    plots: [
      { index: 0, title: 'fast', causal: true },
      { index: 1, title: 'x', causal: false, taint: { feature: 'barstate.islast', line: 3 } },
      { index: 2, title: 'fast', causal: true },
      { index: 3, title: 'slow', causal: true },
      { index: 4, title: 'y', causal: false }
    ]
  };
  const p = pickablePlots(meta);
  assert.deepEqual(
    p.map((x) => [x.title, x.disabled, x.reason ?? null]),
    [
      ['fast', true, 'duplicate'],
      ['x', true, 'noncausal'],
      ['fast', true, 'duplicate'],
      ['slow', false, null],
      ['y', true, 'noncausal']
    ]
  );
  assert.deepEqual(p[1].args, { feature: 'barstate.islast', line: 3 });
  assert.equal(p[4].args, undefined);
  assert.equal(defaultPlot(meta), 'slow');
  assert.equal(
    defaultPlot({
      plots: [
        { title: 'a', causal: true },
        { title: 'a', causal: true }
      ]
    }),
    null
  );
});

test('strategyInstances emits a script instance', () => {
  const s = embedIndicators(strat(op('7', 'fast')), LIB);
  const inst = strategyInstances(s, LIB).filter((i) => i.type === 'script');
  assert.equal(inst.length, 1);
  assert.deepEqual(inst[0].script, { id: '7', name: 'Cross', source: SRC, inputs: { len: 20 }, hidden: [] });
  assert.equal(inst[0].pane, 'overlay');
  assert.equal(inst[0].visible, true);
});
