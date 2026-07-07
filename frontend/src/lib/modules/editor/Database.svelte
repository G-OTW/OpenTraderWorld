<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { dbApi } from './db-api.js';
  import './db/option-colors.css';
  import TableView from './db/TableView.svelte';
  import KanbanView from './db/KanbanView.svelte';
  import GalleryView from './db/GalleryView.svelte';
  import { t } from '$lib/i18n';

  // props: docId, content (view config saved on the document), onConfigChange(content)
  let { docId, content = null, onConfigChange = () => {} } = $props();

  let columns = $state([]);
  let rows = $state([]);
  let loading = $state(true);

  // View config persisted in documents.content.
  let view = $state('table'); // table | kanban | gallery
  let groupColId = $state(null); // kanban
  let coverColId = $state(null); // gallery

  // Reload whenever the active database document changes.
  $effect(() => {
    const id = docId;
    loading = true;
    // Hydrate view config from the document content.
    const cfg = content?.db ?? {};
    view = cfg.view ?? 'table';
    groupColId = cfg.groupColId ?? null;
    coverColId = cfg.coverColId ?? null;
    load(id);
  });

  async function load(id) {
    const data = await dbApi.load(id);
    if (id !== docId) return; // raced past a doc switch
    columns = data.columns;
    rows = data.rows;
    loading = false;
  }

  function persistConfig() {
    onConfigChange({ ...(content ?? {}), db: { view, groupColId, coverColId } });
  }

  function setView(v) {
    view = v;
    persistConfig();
  }

  // ── Columns ──
  async function addColumn() {
    const col = await dbApi.addColumn(docId, $t('editor.database.column'), 'text', {});
    columns = [...columns, col];
  }
  async function patchColumn(colId, patch) {
    await dbApi.updateColumn(colId, patch);
    columns = columns.map((c) => (c.id === colId ? { ...c, ...rename(patch) } : c));
  }
  // Map API patch (uses `type`) onto local column shape.
  function rename(patch) {
    const out = {};
    if (patch.name !== undefined) out.name = patch.name;
    if (patch.type !== undefined) out.type = patch.type;
    if (patch.options !== undefined) out.options = patch.options;
    if (patch.position !== undefined) out.position = patch.position;
    return out;
  }
  async function deleteColumn(colId) {
    await dbApi.deleteColumn(colId);
    columns = columns.filter((c) => c.id !== colId);
    if (groupColId === colId) groupColId = null;
    if (coverColId === colId) coverColId = null;
    persistConfig();
  }

  // ── Rows ──
  async function addRow(prefill = {}) {
    const row = await dbApi.addRow(docId, prefill);
    rows = [...rows, row];
  }
  async function addRowForGroup(groupValue) {
    const cells = groupColId && groupValue != null ? { [groupColId]: groupValue } : {};
    // Kanban groups by groupCol; KanbanView passes the lane key.
    const gid = currentGroupColId();
    await addRow(gid && groupValue != null ? { [gid]: groupValue } : cells);
  }
  function currentGroupColId() {
    const selectCols = columns.filter((c) => c.type === 'select');
    return groupColId ?? selectCols[0]?.id ?? null;
  }
  async function setCell(rowId, colId, value) {
    const row = rows.find((r) => r.id === rowId);
    if (!row) return;
    const cells = { ...(row.cells ?? {}), [colId]: value };
    rows = rows.map((r) => (r.id === rowId ? { ...r, cells } : r));
    await dbApi.updateRow(rowId, cells);
  }
  async function deleteRow(rowId) {
    await dbApi.deleteRow(rowId);
    rows = rows.filter((r) => r.id !== rowId);
  }

  // ── Reordering (table drag) ──
  // Compute a position midway so the dragged item lands just before `beforeId`
  // (or at the end if beforeId is null). `items` is the current ordered array.
  function positionFor(items, dragId, beforeId) {
    const rest = items.filter((i) => i.id !== dragId);
    const idx = beforeId ? rest.findIndex((i) => i.id === beforeId) : rest.length;
    const prev = rest[idx - 1];
    const next = rest[idx];
    if (!prev && !next) return 1;
    if (!prev) return next.position - 1;
    if (!next) return prev.position + 1;
    return (prev.position + next.position) / 2;
  }

  async function moveRow(dragId, beforeId) {
    const pos = positionFor(rows, dragId, beforeId);
    rows = rows
      .map((r) => (r.id === dragId ? { ...r, position: pos } : r))
      .sort((a, b) => a.position - b.position);
    await dbApi.moveRow(dragId, pos);
  }

  async function moveColumn(dragId, beforeId) {
    const pos = positionFor(columns, dragId, beforeId);
    columns = columns
      .map((c) => (c.id === dragId ? { ...c, position: pos } : c))
      .sort((a, b) => a.position - b.position);
    await dbApi.updateColumn(dragId, { position: pos });
  }

  function setGroupCol(colId) {
    groupColId = colId;
    persistConfig();
  }
  function setCoverCol(colId) {
    coverColId = colId || null;
    persistConfig();
  }
</script>

<div class="db">
  <div class="view-tabs">
    <button class:active={view === 'table'} onclick={() => setView('table')}><Icon name="grid" size={13} /> {$t('editor.database.table')}</button>
    <button class:active={view === 'kanban'} onclick={() => setView('kanban')}><Icon name="layers" size={13} /> {$t('editor.database.kanban')}</button>
    <button class:active={view === 'gallery'} onclick={() => setView('gallery')}><Icon name="image" size={13} /> {$t('editor.database.gallery')}</button>
  </div>

  {#if loading}
    <div class="loading">{$t('common.loading')}</div>
  {:else if columns.length === 0}
    <div class="empty">
      <p>{$t('editor.database.noColumns')}</p>
      <button class="primary" onclick={addColumn}><Icon name="plus" size={14} /> {$t('editor.database.addFirstColumn')}</button>
    </div>
  {:else if view === 'table'}
    <TableView
      {columns}
      {rows}
      onCellChange={setCell}
      onColumnPatch={patchColumn}
      onColumnDelete={deleteColumn}
      onAddColumn={addColumn}
      onAddRow={() => addRow({})}
      onDeleteRow={deleteRow}
      onMoveRow={moveRow}
      onMoveColumn={moveColumn}
    />
  {:else if view === 'kanban'}
    <KanbanView
      {columns}
      {rows}
      {groupColId}
      onGroupColChange={setGroupCol}
      onCellChange={setCell}
      onAddRow={addRowForGroup}
    />
  {:else}
    <GalleryView
      {columns}
      {rows}
      {coverColId}
      onCoverColChange={setCoverCol}
      onAddRow={() => addRow({})}
      onSetCover={setCell}
    />
  {/if}
</div>

<style>
  .db {
    padding: var(--space-6) var(--space-6) 25vh;
    max-width: 1100px;
    margin: 0 auto;
    width: 100%;
  }
  .view-tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 16px;
    border-bottom: 1px solid var(--border);
    padding-bottom: 8px;
  }
  .view-tabs button {
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: 0.85rem;
    padding: 5px 12px;
  }
  .view-tabs button:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .view-tabs button.active {
    color: var(--accent);
    background: var(--surface-2);
  }
  .loading,
  .empty {
    color: var(--muted);
    padding: 30px;
    text-align: center;
  }
  .empty p {
    margin-bottom: 12px;
  }
</style>
