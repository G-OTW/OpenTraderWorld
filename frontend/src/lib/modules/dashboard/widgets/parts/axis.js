// Labels for the chart parts. A widget's series is usually a bare array of numbers built
// from a "last N days" loop; these rebuild the matching x labels so a hovered column can
// name its day instead of falling back to the series name.

// The last `n` days ending today, oldest first: the same order the `perDay` loops build.
export function dayLabels(n, end = new Date()) {
  const base = new Date(end);
  base.setHours(0, 0, 0, 0);
  const out = [];
  for (let i = n - 1; i >= 0; i--) {
    const d = new Date(base);
    d.setDate(base.getDate() - i);
    out.push(d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' }));
  }
  return out;
}

// The last `n` months ending this month, oldest first.
export function monthLabels(n, end = new Date()) {
  const base = new Date(end.getFullYear(), end.getMonth(), 1);
  const out = [];
  for (let i = n - 1; i >= 0; i--) {
    const d = new Date(base.getFullYear(), base.getMonth() - i, 1);
    out.push(d.toLocaleDateString(undefined, { month: 'short', year: '2-digit' }));
  }
  return out;
}

// A full date for one ISO key, for the tooltip title of a dated point.
export function dateLabel(v) {
  const d = new Date(v);
  if (Number.isNaN(d.getTime())) return String(v);
  return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
}
