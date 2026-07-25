/** Database client — columns + rows for a database document. Single-user. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

export const dbApi = {
  load: (id) => req(`/databases/${id}`), // -> { columns, rows }

  addColumn: (id, name, type, options = {}) =>
    req(`/databases/${id}/columns`, {
      method: 'POST',
      body: JSON.stringify({ name, type, options })
    }).then((r) => r.column),
  updateColumn: (colId, patch) =>
    req(`/databases/columns/${colId}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteColumn: (colId) => req(`/databases/columns/${colId}`, { method: 'DELETE' }),

  addRow: (id, cells = {}) =>
    req(`/databases/${id}/rows`, {
      method: 'POST',
      body: JSON.stringify({ cells })
    }).then((r) => r.row),
  updateRow: (rowId, cells) =>
    req(`/databases/rows/${rowId}`, { method: 'PATCH', body: JSON.stringify({ cells }) }),
  moveRow: (rowId, position) =>
    req(`/databases/rows/${rowId}/move`, {
      method: 'POST',
      body: JSON.stringify({ position })
    }),
  deleteRow: (rowId) => req(`/databases/rows/${rowId}`, { method: 'DELETE' })
};

/** Column type registry — labels and default cell values. */
export const COLUMN_TYPES = [
  { id: 'text', label: 'Text', empty: '' },
  { id: 'number', label: 'Number', empty: null },
  { id: 'select', label: 'Select', empty: null },
  { id: 'multi_select', label: 'Multi-select', empty: [] },
  { id: 'date', label: 'Date', empty: null },
  { id: 'checkbox', label: 'Checkbox', empty: false },
  { id: 'url', label: 'URL', empty: '' }
];

export function typeLabel(id) {
  return COLUMN_TYPES.find((t) => t.id === id)?.label ?? id;
}

/** Generate a short random id for select options (client-side). */
export function optId() {
  return Math.random().toString(36).slice(2, 10);
}

/** Palette for select-option chips. */
export const OPTION_COLORS = ['slate', 'red', 'amber', 'green', 'blue', 'violet', 'pink'];
