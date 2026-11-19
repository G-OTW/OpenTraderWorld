// The Tools section of the navigation rail: tools read from A to Z, and a category is one
// flat level over them. A category holds tools and nothing else, so nesting cannot exist.
// Categories keep the order they are saved in, which the user sets from the Tools editor.
//
// Saved in the dashboard document as `rail.toolCategories`: [{ id, name, tools: [moduleId] }].

/** A to Z on the displayed name, ignoring case and accents. */
export function byName(a, b) {
  return a.name.localeCompare(b.name, undefined, { sensitivity: 'base', numeric: true });
}

/**
 * Split the listed tools into their categories and the ones left outside. Categories come
 * back in their saved order, tools from A to Z. A tool belongs to the first category naming
 * it, and a category with nothing to list is left out.
 */
export function groupTools(tools, categories) {
  const byId = new Map(tools.map((tool) => [tool.id, tool]));
  const placed = new Set();
  const groups = [];
  for (const category of Array.isArray(categories) ? categories : []) {
    const inside = [];
    for (const id of category.tools ?? []) {
      if (!byId.has(id) || placed.has(id)) continue;
      placed.add(id);
      inside.push(byId.get(id));
    }
    if (inside.length) groups.push({ id: category.id, name: category.name, tools: inside.sort(byName) });
  }
  return {
    categories: groups,
    loose: tools.filter((tool) => !placed.has(tool.id)).sort(byName)
  };
}

/** The categories with the one at `index` swapped with its neighbour, `step` being -1 or 1. */
export function moveCategory(cats, index, step) {
  const to = index + step;
  if (to < 0 || to >= cats.length) return cats;
  const next = [...cats];
  [next[index], next[to]] = [next[to], next[index]];
  return next;
}

/** True when `name` is blank or already taken among `names`, whatever the case. */
export function nameTaken(name, names) {
  const wanted = name.trim().toLowerCase();
  return !wanted || names.some((other) => other.trim().toLowerCase() === wanted);
}

/** True when every category has a name of its own. */
export function namesValid(names) {
  return names.every((name, i) => !nameTaken(name, names.slice(0, i)));
}

// The drag zones of the Tools editor, next to one zone per category.
export const LOOSE = 'loose'; // shown in the rail, in no category
export const HIDDEN = 'hidden'; // not shown in the rail

/** The editor's working state: categories in their saved order, and the tools each zone holds. */
export function seedZones(items, selected, categories) {
  const saved = Array.isArray(categories) ? categories : [];
  const { categories: groups, loose } = groupTools(items.filter((item) => selected.includes(item.id)), saved);
  const zones = { [LOOSE]: loose, [HIDDEN]: items.filter((item) => !selected.includes(item.id)).sort(byName) };
  for (const { id } of saved) zones[id] = groups.find((group) => group.id === id)?.tools ?? [];
  return { cats: saved.map(({ id, name }) => ({ id, name })), zones };
}

/**
 * The arrangement to save from the zones. A tool that is not installed right now has no row
 * in the editor, so it stays in its saved category and comes back there when reinstalled.
 */
export function zonesToRail(cats, zones, categories, items) {
  const installed = new Set(items.map((item) => item.id));
  const saved = Array.isArray(categories) ? categories : [];
  const idsIn = (zone) => (zones[zone] ?? []).map((item) => item.id);
  return {
    tools: [...cats.flatMap((cat) => idsIn(cat.id)), ...idsIn(LOOSE)],
    toolCategories: cats.map((cat) => ({
      id: cat.id,
      name: cat.name.trim(),
      tools: [
        ...idsIn(cat.id),
        ...(saved.find((other) => other.id === cat.id)?.tools ?? []).filter((id) => !installed.has(id))
      ]
    }))
  };
}
