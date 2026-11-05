import test from 'node:test';
import assert from 'node:assert/strict';
import { groupTools, HIDDEN, LOOSE, nameTaken, namesValid, seedZones, zonesToRail } from '../src/lib/ui/railTools.js';

const tool = (id, name) => ({ id, name });
const tools = [tool('todos', 'ToDo'), tool('news', 'News'), tool('agent', 'agent'), tool('goals', 'Goals')];
const ids = (list) => list.map((item) => item.id);

test('with no category every tool reads from A to Z, whatever the case', () => {
  const { categories, loose } = groupTools(tools, undefined);
  assert.deepEqual(categories, []);
  assert.deepEqual(ids(loose), ['agent', 'goals', 'news', 'todos']);
});

test('categories and the tools inside them read from A to Z, the others stay outside', () => {
  const { categories, loose } = groupTools(tools, [
    { id: 'z', name: 'Zen', tools: ['todos', 'goals'] },
    { id: 'a', name: 'Alpha', tools: ['news'] }
  ]);
  assert.deepEqual(ids(categories), ['a', 'z']);
  assert.deepEqual(ids(categories[1].tools), ['goals', 'todos']);
  assert.deepEqual(ids(loose), ['agent']);
});

test('a category with nothing to list is left out, and a hidden tool does not revive it', () => {
  const { categories } = groupTools(tools, [
    { id: 'e', name: 'Empty', tools: [] },
    { id: 'h', name: 'Hidden', tools: ['detached'] }
  ]);
  assert.deepEqual(categories, []);
});

test('a tool named by two categories sits in the first one only', () => {
  const { categories, loose } = groupTools(tools, [
    { id: 'a', name: 'A', tools: ['news'] },
    { id: 'b', name: 'B', tools: ['news', 'goals'] }
  ]);
  assert.deepEqual(ids(categories[0].tools), ['news']);
  assert.deepEqual(ids(categories[1].tools), ['goals']);
  assert.deepEqual(ids(loose), ['agent', 'todos']);
});

test('a category name must be filled and unique, whatever the case', () => {
  assert.equal(nameTaken('  ', []), true);
  assert.equal(nameTaken(' trading ', ['Trading']), true);
  assert.equal(nameTaken('Research', ['Trading']), false);
  assert.equal(namesValid(['Trading', 'Research']), true);
  assert.equal(namesValid(['Trading', 'trading']), false);
  assert.equal(namesValid(['Trading', ' ']), false);
});

const items = [tool('news', 'News'), tool('goals', 'Goals'), tool('todos', 'ToDo'), tool('time', 'Time Tracker')];

test('the editor seeds one zone per category, empty ones included, plus the loose and hidden tools', () => {
  const { cats, zones } = seedZones(items, ['news', 'goals', 'todos'], [
    { id: 'z', name: 'Zen', tools: ['todos', 'news'] },
    { id: 'a', name: 'Alpha', tools: [] }
  ]);
  assert.deepEqual(ids(cats), ['a', 'z']);
  assert.deepEqual(ids(zones.z), ['news', 'todos']);
  assert.deepEqual(zones.a, []);
  assert.deepEqual(ids(zones[LOOSE]), ['goals']);
  assert.deepEqual(ids(zones[HIDDEN]), ['time']);
});

test('a hidden tool waits in Available, not in its category', () => {
  const { zones } = seedZones(items, ['news'], [{ id: 'a', name: 'A', tools: ['news', 'goals'] }]);
  assert.deepEqual(ids(zones.a), ['news']);
  assert.ok(ids(zones[HIDDEN]).includes('goals'));
});

test('saving lists the zones, leaves Available out and keeps tools that are not installed', () => {
  const saved = [{ id: 'a', name: 'A', tools: ['goals', 'detached'] }];
  const cats = [{ id: 'a', name: ' A ' }, { id: 'n', name: 'New' }];
  const zones = { a: [tool('news', 'News')], n: [], [LOOSE]: [tool('goals', 'Goals')], [HIDDEN]: [tool('todos', 'ToDo')] };
  assert.deepEqual(zonesToRail(cats, zones, saved, items), {
    tools: ['news', 'goals'],
    toolCategories: [
      { id: 'a', name: 'A', tools: ['news', 'detached'] },
      { id: 'n', name: 'New', tools: [] }
    ]
  });
});
