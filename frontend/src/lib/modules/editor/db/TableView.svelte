<script>
  import Icon from '$lib/ui/Icon.svelte';
  import Cell from './Cell.svelte';
  import ColumnMenu from './ColumnMenu.svelte';
  import { t } from '$lib/i18n';
  import { displayValue } from './cells.js';

  // props: columns, rows, callbacks
  let {
    columns,
    rows,
    onCellChange, // (rowId, colId, value)
    onColumnPatch, // (colId, patch)
    onColumnDelete, // (colId)
    onAddColumn, // ()
    onAddRow, // ()
    onDeleteRow, // (rowId)
    onMoveRow, // (dragId, beforeId|null)
    onMoveColumn // (dragId, beforeId|null)
  } = $props();

  // ── Drag state ──
  let dragRowId = $state(null);
  let overRowId = $state(null);
  let dragColId = $state(null);
  let overColId = $state(null);

  function dropRow(beforeId) {
    if (dragRowId && dragRowId !== beforeId) onMoveRow?.(dragRowId, beforeId);
    dragRowId = null;
    overRowId = null;
  }
  // Name used in the touch move buttons: the row's first column, as the table shows it.
  const rowName = (row) =>
    (columns[0] ? displayValue(columns[0], row.cells?.[columns[0].id]) : '') || $t('editor.docTree.untitled');

  function dropCol(beforeId) {
    if (dragColId && dragColId !== beforeId) onMoveColumn?.(dragColId, beforeId);
    dragColId = null;
    overColId = null;
  }
</script>

<div class="table-wrap">
  <table>
    <thead>
      <tr>
        <th class="handle-col"></th>
        {#each columns as col, ci (col.id)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <th
            class:col-over={overColId === col.id}
            class:col-dragging={dragColId === col.id}
            ondragover={(e) => { if (dragColId) { e.preventDefault(); overColId = col.id; } }}
            ondragleave={() => { if (overColId === col.id) overColId = null; }}
            ondrop={() => dropCol(col.id)}
          >
            <div class="th-inner">
              <span
                class="col-grip"
                draggable="true"
                ondragstart={() => (dragColId = col.id)}
                ondragend={() => { dragColId = null; overColId = null; }}
                title={$t('editor.tableView.dragToReorderColumn')}
              ><Icon name="grip-vertical" size={12} /></span>
              <ColumnMenu
                {col}
                onPatch={(patch) => onColumnPatch(col.id, patch)}
                onDelete={() => onColumnDelete(col.id)}
              />
              <!-- Touch alternative to dragging the grip: same move, same saved position. -->
              <span class="tap-move">
                <button
                  disabled={ci === 0}
                  aria-label={`${$t('histviz.chart.panLeft')}: ${col.name || $t('editor.docTree.untitled')}`}
                  onclick={() => onMoveColumn?.(col.id, columns[ci - 1].id)}
                ><Icon name="chevron-left" size={14} /></button>
                <button
                  disabled={ci === columns.length - 1}
                  aria-label={`${$t('histviz.chart.panRight')}: ${col.name || $t('editor.docTree.untitled')}`}
                  onclick={() => onMoveColumn?.(col.id, columns[ci + 2]?.id ?? null)}
                ><Icon name="chevron-right" size={14} /></button>
              </span>
            </div>
          </th>
        {/each}
        <th
          class="add-col"
          ondragover={(e) => { if (dragColId) { e.preventDefault(); overColId = '__end'; } }}
          ondrop={() => dropCol(null)}
        >
          <button onclick={onAddColumn} title={$t('editor.tableView.addColumn')}><Icon name="plus" size={16} /></button>
        </th>
      </tr>
    </thead>
    <tbody>
      {#each rows as row, ri (row.id)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <tr
          class:row-over={overRowId === row.id}
          class:row-dragging={dragRowId === row.id}
          ondragover={(e) => { if (dragRowId) { e.preventDefault(); overRowId = row.id; } }}
          ondragleave={() => { if (overRowId === row.id) overRowId = null; }}
          ondrop={() => dropRow(row.id)}
        >
          <td class="handle">
            <span
              class="grip"
              draggable="true"
              ondragstart={() => (dragRowId = row.id)}
              ondragend={() => { dragRowId = null; overRowId = null; }}
              title={$t('editor.tableView.dragToReorderRow')}
            ><Icon name="grip-vertical" size={12} /></span>
            <span class="tap-move">
              <button
                disabled={ri === 0}
                aria-label={`${$t('dashboard.moveUp')}: ${rowName(row)}`}
                onclick={() => onMoveRow?.(row.id, rows[ri - 1].id)}
              ><Icon name="arrow-up" size={14} /></button>
              <button
                disabled={ri === rows.length - 1}
                aria-label={`${$t('dashboard.moveDown')}: ${rowName(row)}`}
                onclick={() => onMoveRow?.(row.id, rows[ri + 2]?.id ?? null)}
              ><Icon name="arrow-down" size={14} /></button>
            </span>
          </td>
          {#each columns as col (col.id)}
            <td>
              <Cell
                {col}
                value={row.cells?.[col.id]}
                onCommit={(v) => onCellChange(row.id, col.id, v)}
              />
            </td>
          {/each}
          <td class="row-actions">
            <button onclick={() => onDeleteRow(row.id)} title={$t('editor.tableView.deleteRow')}><Icon name="trash" size={14} /></button>
          </td>
        </tr>
      {/each}
      <!-- drop zone to send a row to the very end -->
      <tr class="end-drop" ondragover={(e) => { if (dragRowId) e.preventDefault(); }} ondrop={() => dropRow(null)}>
        <td colspan={columns.length + 2}></td>
      </tr>
    </tbody>
  </table>

  <button class="add-row" onclick={onAddRow}><Icon name="plus" size={13} /> {$t('editor.tableView.newRow')}</button>
</div>

<style>
  .table-wrap {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th,
  td {
    border: 1px solid var(--border);
    text-align: left;
    vertical-align: top;
    min-width: 140px;
  }
  th {
    background: var(--surface);
    padding: 0;
  }
  .th-inner {
    display: flex;
    align-items: center;
  }
  .col-grip,
  .grip {
    color: var(--muted);
    cursor: grab;
    font-size: var(--text-xs);
    line-height: 1;
    padding: 6px 2px 6px 4px;
    letter-spacing: -2px;
    user-select: none;
  }
  .col-grip:hover,
  .grip:hover {
    color: var(--text);
  }
  .handle-col,
  .handle {
    min-width: 22px;
    width: 22px;
    text-align: center;
    padding: 0;
  }
  th.add-col {
    min-width: 40px;
    width: 40px;
    text-align: center;
  }
  th.add-col button,
  .row-actions button {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-base);
    padding: 6px;
  }
  th.add-col button:hover,
  .row-actions button:hover {
    color: var(--text);
  }
  td {
    padding: 0;
  }
  .row-actions {
    min-width: 40px;
    width: 40px;
    text-align: center;
  }
  tr:hover td {
    background: color-mix(in srgb, var(--surface-2) 50%, transparent);
  }
  /* Drag feedback */
  tr.row-dragging td {
    opacity: 0.4;
  }
  tr.row-over td {
    border-top-color: var(--accent);
  }
  th.col-dragging {
    opacity: 0.4;
  }
  th.col-over {
    border-left-color: var(--accent);
  }
  .end-drop td {
    height: 6px;
    border: none;
    min-width: 0;
  }
  .add-row {
    margin-top: 8px;
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 8px 12px;
    width: 100%;
    text-align: left;
  }
  .add-row:hover {
    color: var(--text);
    border-color: var(--accent);
  }
  /* Move buttons replace the drag grips on phones and touch screens. */
  .tap-move {
    display: none;
  }
  @media (max-width: 767px), (pointer: coarse) and (max-width: 1099px) {
    .tap-move {
      display: inline-flex;
    }
    .handle .tap-move {
      flex-direction: column;
    }
    .grip,
    .col-grip {
      display: none;
    }
    .handle-col,
    .handle {
      min-width: 44px;
      width: 44px;
    }
    .tap-move button {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 44px;
      min-height: 44px;
      padding: 0;
      background: transparent;
      border: 0;
      color: var(--muted);
    }
    .tap-move button:disabled {
      opacity: 0.35;
    }
    .th-inner .tap-move button {
      min-width: 36px;
    }
    th.add-col button,
    .row-actions button {
      min-width: 44px;
      min-height: 44px;
    }
  }
</style>
