import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildPlan, navTargets, normalize, splitPieces } from '../src/lib/voice/parse.js';

const targets = navTargets(
  [
    { id: 'journal', name: 'Trading Journal', base: '/journal' },
    { id: 'histdata', name: 'Historical Data', base: '/histdata' },
    { id: 'histviz', name: 'Historical Data Visualization', base: '/histviz' },
    { id: 'todos', name: 'ToDo', base: '/todos' }
  ],
  [{ label: 'Settings', path: '/settings', keys: ['settings', 'parametres'] }]
);

const turtle = {
  id: 't',
  phrase: 'Turtle',
  aliases: ['turtles'],
  enabled: true,
  bypass_confirm: false,
  steps: [{ kind: 'navigate', target: '/journal' }]
};

test('normalize strips case, accents and punctuation', () => {
  assert.equal(normalize('  Ouvre le Journal, s’il te plaît! '), 'ouvre le journal s il te plait');
});

test('a chain splits on chain words and commas, never on the first word', () => {
  const p = splitPieces('open journal and then import trades, then go to settings');
  assert.deepEqual(p.map((x) => x.norm), ['open journal', 'import trades', 'go to settings']);
  assert.deepEqual(splitPieces('and open journal').map((x) => x.norm), ['and open journal']);
});

test('a phrase fires on a whole piece only', () => {
  const hit = buildPlan('turtle', { commands: [turtle], targets });
  assert.equal(hit.entries[0].type, 'command');
  const miss = buildPlan('a blue turtle', { commands: [turtle], targets });
  assert.equal(miss.entries[0].type, 'agent');
  assert.equal(miss.steps[0].prompt, 'a blue turtle');
});

test('a phrase that contains a chain word still matches whole', () => {
  const both = { ...turtle, id: 'b', phrase: 'buy and hold', aliases: [] };
  const plan = buildPlan('buy and hold then open settings', { commands: [both], targets });
  assert.deepEqual(plan.entries.map((e) => e.type), ['command', 'open']);
});

test('chained command, built-in open and agent leftovers in order', () => {
  const plan = buildPlan('turtle and open settings and compare AAPL and MSFT', {
    commands: [turtle],
    targets
  });
  assert.deepEqual(plan.entries.map((e) => e.type), ['command', 'open', 'agent']);
  assert.equal(plan.steps[2].prompt, 'compare AAPL and MSFT');
  assert.equal(plan.confirm, true);
});

test('bypass only when every part is a bypassing command', () => {
  const quick = { ...turtle, bypass_confirm: true };
  assert.equal(buildPlan('turtle', { commands: [quick], targets }).confirm, false);
  assert.equal(buildPlan('turtle and open settings', { commands: [quick], targets }).confirm, true);
});

test('an ambiguous page is an error that lists the candidates', () => {
  const plan = buildPlan('open historical', { targets });
  assert.equal(plan.error, 'ambiguous');
  assert.equal(plan.entries[0].candidates.length, 2);
  assert.equal(buildPlan('open historical data', { targets }).steps[0].target, '/histdata');
});

test('french chain words and verbs', () => {
  const plan = buildPlan('ouvre le journal puis va sur les paramètres', { targets, lang: 'fr-FR' });
  assert.deepEqual(plan.steps.map((s) => s.target), ['/journal', '/settings']);
});

test('without the agent fallback, leftovers are reported, not guessed', () => {
  const plan = buildPlan('do something odd', { targets, agentFallback: false });
  assert.equal(plan.error, 'unknown');
  assert.equal(plan.steps.length, 0);
});
