// Last colours picked from the custom colour picker. Session-only — held in
// module scope so every editor instance shares them, never persisted.
const MAX = 5;

export const recentColors = $state({ text: [], highlight: [] });

export function pushRecent(kind, color) {
  const list = recentColors[kind];
  const next = [color, ...list.filter((c) => c.toLowerCase() !== color.toLowerCase())];
  recentColors[kind] = next.slice(0, MAX);
}
