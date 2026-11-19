// The first-run invite counts "customise your dashboard" as done from the stored layout:
// a fresh workspace is not customized, a moved tile or a page of one's own is.
import { expect, test } from 'vitest';
import { normalizeDoc, ensureDashboardPresets, isWorkspaceCustomized, makePage } from './layout.js';

function fresh() {
  const doc = normalizeDoc(null);
  ensureDashboardPresets(doc);
  return doc;
}

test('a freshly seeded workspace is not customized', () => {
  expect(isWorkspaceCustomized(fresh())).toBe(false);
});

test('the same workspace read back from the server (new key order) is still not customized', () => {
  const sorted = (v) =>
    Array.isArray(v)
      ? v.map(sorted)
      : v && typeof v === 'object'
        ? Object.fromEntries(Object.keys(v).sort().reverse().map((k) => [k, sorted(v[k])]))
        : v;
  const doc = normalizeDoc(sorted(JSON.parse(JSON.stringify(fresh()))));
  ensureDashboardPresets(doc);
  expect(isWorkspaceCustomized(doc)).toBe(false);
});

test('a reordered preset tile counts as customized', () => {
  const doc = fresh();
  const items = doc.pages[0].layout.items;
  doc.pages[0].layout.items = [items[1], items[0], ...items.slice(2)];
  expect(isWorkspaceCustomized(doc)).toBe(true);
});

test('a page the user made counts as customized', () => {
  const doc = fresh();
  doc.pages = [...doc.pages, makePage({ name: 'Mine' })];
  expect(isWorkspaceCustomized(doc)).toBe(true);
});
