<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { choicesOf, displayValue } from './cells.js';
  import { t } from '$lib/i18n';

  // props: columns, rows, groupColId, onGroupColChange, onCellChange, callbacks
  let {
    columns,
    rows,
    groupColId,
    onGroupColChange, // (colId)
    onCellChange, // (rowId, colId, value) — used to move a card between groups
    onAddRow, // (groupValue) — create row pre-filled with the group value
    onOpenRow // (rowId) — focus/inspect (here: noop-friendly)
  } = $props();

  // Only single-select columns can group a kanban.
  const selectCols = $derived(columns.filter((c) => c.type === 'select'));
  const groupCol = $derived(columns.find((c) => c.id === groupColId) ?? selectCols[0] ?? null);

  // Build columns: one per choice + an "Empty" lane.
  const lanes = $derived(buildLanes(groupCol, rows));

  function buildLanes(col, allRows) {
    if (!col) return [];
    const lanes = choicesOf(col).map((ch) => ({
      key: ch.id,
      title: ch.name,
      color: ch.color,
      rows: []
    }));
    const empty = { key: null, title: $t('editor.kanbanView.noValue', { name: col.name || $t('editor.kanbanView.value') }), color: 'slate', rows: [] };
    for (const r of allRows) {
      const v = r.cells?.[col.id];
      const lane = lanes.find((l) => l.key === v) ?? empty;
      lane.rows.push(r);
    }
    return [...lanes, empty];
  }

  // Title-ish fields to show on a card: first text/url column.
  const titleCol = $derived(columns.find((c) => c.type === 'text') ?? columns[0] ?? null);
  const detailCols = $derived(columns.filter((c) => c.id !== groupCol?.id && c.id !== titleCol?.id));

  // ── Drag a card to another lane ──
  let dragRowId = $state(null);
  let overLane = $state(undefined);

  function onDrop(laneKey) {
    if (dragRowId && groupCol) onCellChange(dragRowId, groupCol.id, laneKey);
    dragRowId = null;
    overLane = undefined;
  }
</script>

{#if selectCols.length === 0}
  <div class="hint">{@html $t('editor.kanbanView.addSelectHint')}</div>
{:else}
  <div class="kanban-bar">
    <label>
      {$t('editor.kanbanView.groupBy')}
      <select value={groupCol?.id} onchange={(e) => onGroupColChange(e.target.value)}>
        {#each selectCols as c}
          <option value={c.id}>{c.name || $t('editor.docTree.untitled')}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="board">
    {#each lanes as lane (lane.key ?? '∅')}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="lane"
        class:over={overLane === lane.key}
        ondragover={(e) => { e.preventDefault(); overLane = lane.key; }}
        ondragleave={() => (overLane = undefined)}
        ondrop={() => onDrop(lane.key)}
      >
        <div class="lane-head">
          <span class="opt-chip {lane.color}">{lane.title}</span>
          <span class="count">{lane.rows.length}</span>
        </div>

        <div class="cards">
          {#each lane.rows as row (row.id)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="card"
              class:dragging={dragRowId === row.id}
              draggable="true"
              ondragstart={() => (dragRowId = row.id)}
              ondragend={() => (dragRowId = null)}
              onclick={() => onOpenRow?.(row.id)}
              role="button"
              tabindex="0"
            >
              <div class="card-title">
                {titleCol ? displayValue(titleCol, row.cells?.[titleCol.id]) || $t('editor.docTree.untitled') : $t('editor.docTree.untitled')}
              </div>
              {#each detailCols as col}
                {@const txt = displayValue(col, row.cells?.[col.id])}
                {#if txt}
                  <div class="card-field"><span class="k">{col.name}</span> {txt}</div>
                {/if}
              {/each}
            </div>
          {/each}

          <button class="add-card" onclick={() => onAddRow(lane.key)}><Icon name="plus" size={13} /> {$t('editor.kanbanView.addCard')}</button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .kanban-bar {
    margin-bottom: 10px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .kanban-bar select {
    margin-left: 6px;
  }
  .board {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    overflow-x: auto;
    padding-bottom: 8px;
  }
  .lane {
    flex: 0 0 260px;
    background: color-mix(in srgb, var(--surface) 60%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px;
    min-height: 80px;
  }
  .lane.over {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .lane-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .count {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .cards {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    cursor: grab;
  }
  .card:hover {
    border-color: var(--accent);
  }
  .card.dragging {
    opacity: 0.4;
  }
  .card-title {
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    margin-bottom: 4px;
  }
  .card-field {
    color: var(--muted);
    font-size: var(--text-xs);
    margin-top: 2px;
  }
  .card-field .k {
    color: var(--text);
    opacity: 0.6;
  }
  .add-card {
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: 6px;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 6px;
    text-align: left;
  }
  .add-card:hover {
    color: var(--text);
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-base);
    padding: 20px;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
  }
</style>
