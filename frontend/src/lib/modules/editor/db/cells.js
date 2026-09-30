/** Shared helpers for rendering/coercing database cell values by column type. */

/** Display string for a cell (used by table text mode, gallery, etc.). */
export function displayValue(col, value) {
  if (value == null || value === '') return '';
  switch (col.type) {
    case 'checkbox':
      return value ? '✓' : '';
    case 'select': {
      const opt = (col.options?.choices ?? []).find((o) => o.id === value);
      return opt?.name ?? '';
    }
    case 'multi_select': {
      const choices = col.options?.choices ?? [];
      return (value ?? [])
        .map((id) => choices.find((o) => o.id === id)?.name)
        .filter(Boolean)
        .join(', ');
    }
    case 'number':
      return String(value);
    default:
      return String(value);
  }
}

/** Coerce raw input into the stored shape for a column type. */
export function coerce(col, raw) {
  switch (col.type) {
    case 'number': {
      if (raw === '' || raw == null) return null;
      const n = Number(raw);
      return Number.isNaN(n) ? null : n;
    }
    case 'checkbox':
      return Boolean(raw);
    default:
      return raw;
  }
}

/** The choices array for a select/multi_select column. */
export function choicesOf(col) {
  return col.options?.choices ?? [];
}

/** Find a choice object by id. */
export function choiceById(col, id) {
  return choicesOf(col).find((o) => o.id === id) ?? null;
}
