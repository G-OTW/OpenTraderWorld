<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  // props: docs (flat list of metadata), selectedId, callbacks
  let {
    docs = [],
    selectedId = null,
    onselect = () => {},
    oncreate = () => {}, // (parentId, kind)
    onrename = () => {}, // (id, title)
    ondelete = () => {}, // (id)
    onmove = () => {} // (id, parentId, position)
  } = $props();

  // Build a parent → children map.
  const tree = $derived(buildTree(docs));
  let expanded = $state(new Set());

  function buildTree(list) {
    const byParent = new Map();
    for (const d of list) {
      const key = d.parent_id ?? 'root';
      if (!byParent.has(key)) byParent.set(key, []);
      byParent.get(key).push(d);
    }
    return byParent;
  }

  function childrenOf(id) {
    return tree.get(id ?? 'root') ?? [];
  }

  function toggle(id) {
    const next = new Set(expanded);
    next.has(id) ? next.delete(id) : next.add(id);
    expanded = next;
  }

  // Expand every folder / collapse all.
  function expandAll() {
    expanded = new Set(docs.filter((d) => d.kind === 'folder').map((d) => d.id));
  }
  function collapseAll() {
    expanded = new Set();
  }

  let renamingId = $state(null);
  let renameValue = $state('');

  // Inline SVG icon paths (16x16, currentColor) — robust across fonts/OSes.
  const ICONS = {
    folder: 'M2 4a1 1 0 0 1 1-1h3.6a1 1 0 0 1 .7.3L8.7 4.7H13a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z',
    page: 'M4 2h5l3 3v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1zM9 2.5V5h2.5',
    database: 'M2.5 4h11v8h-11zM2.5 6.7h11M2.5 9.3h11M6 4v8',
    plus: 'M8 4v8M4 8h8',
    pencil: 'M11.5 2.5l2 2L6 12l-2.6.6L4 10z',
    trash: 'M3 4h10M6 4V2.8a.8.8 0 0 1 .8-.8h2.4a.8.8 0 0 1 .8.8V4M5 4l.5 9h5L11 4'
  };

  function startRename(d) {
    renamingId = d.id;
    renameValue = d.title;
  }
  function commitRename() {
    if (renamingId) onrename(renamingId, renameValue.trim() || $t('editor.docTree.untitled'));
    renamingId = null;
  }

  // ── Drag & drop ──
  // dragId = item being dragged. dropTarget = { id, mode } where mode is
  // 'inside' (drop onto a folder) or 'before' (drop above a row, same level).
  let dragId = $state(null);
  let dropTarget = $state(null);

  const byId = $derived(new Map(docs.map((d) => [d.id, d])));

  /** True if `maybeChildId` is `ancestorId` or sits under it (prevents cycles). */
  function isDescendant(ancestorId, maybeChildId) {
    let cur = byId.get(maybeChildId);
    while (cur) {
      if (cur.id === ancestorId) return true;
      cur = cur.parent_id ? byId.get(cur.parent_id) : null;
    }
    return false;
  }

  function onDragStart(e, d) {
    dragId = d.id;
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', d.id);
  }

  function onDragOver(e, target, mode) {
    // Row/before handlers own this event — don't let it bubble to the
    // body-level (root) handler, which would otherwise light up the whole tree.
    if (target) e.stopPropagation();
    if (!dragId || dragId === target?.id) {
      // Hovering the dragged item itself: clear any stale highlight.
      if (target && dragId === target.id) dropTarget = null;
      return;
    }
    // Cannot drop a folder into itself or its own descendants.
    if (target && isDescendant(dragId, target.id)) {
      dropTarget = null;
      return;
    }
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    dropTarget = target ? { id: target.id, mode } : { id: 'root', mode: 'inside' };
  }

  function clearDrag() {
    dragId = null;
    dropTarget = null;
  }

  /** Compute a position that places the dragged item before `target` among siblings. */
  function positionBefore(siblings, targetId) {
    const idx = siblings.findIndex((s) => s.id === targetId);
    if (idx <= 0) {
      const first = siblings[0];
      return first ? first.position - 1 : 1;
    }
    const prev = siblings[idx - 1];
    const target = siblings[idx];
    return (prev.position + target.position) / 2;
  }

  function onDrop(e, target, mode) {
    e.preventDefault();
    e.stopPropagation();
    const id = dragId;
    if (!id) return clearDrag();

    if (mode === 'inside') {
      // Drop into a folder (or root). Append to the end of that parent.
      const parentId = target === 'root' ? null : target.id;
      if (parentId && isDescendant(id, parentId)) return clearDrag();
      const siblings = childrenOf(parentId).filter((s) => s.id !== id);
      const lastPos = siblings.length ? siblings[siblings.length - 1].position : 0;
      onmove(id, parentId, lastPos + 1);
    } else {
      // Drop before a row: same parent as the target, positioned before it.
      const parentId = target.parent_id ?? null;
      if (parentId && isDescendant(id, parentId)) return clearDrag();
      const siblings = childrenOf(parentId).filter((s) => s.id !== id);
      onmove(id, parentId, positionBefore(siblings.length ? childrenOf(parentId) : [target], target.id));
    }
    clearDrag();
  }
</script>

{#snippet icon(name, cls)}
  <svg class={cls} viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d={ICONS[name]} />
  </svg>
{/snippet}

{#snippet node(d, depth)}
  {@const kids = childrenOf(d.id)}
  {@const isFolder = d.kind === 'folder'}
  {@const isOpen = expanded.has(d.id)}

  <!-- "before" drop zone: reorder above this row at the same level -->
  <div
    class="drop-before"
    class:active={dropTarget?.id === d.id && dropTarget?.mode === 'before'}
    style="margin-left:{depth * 14 + 8}px"
    role="presentation"
    ondragover={(e) => onDragOver(e, d, 'before')}
    ondrop={(e) => onDrop(e, d, 'before')}
  ></div>

  <div
    class="row"
    class:selected={d.id === selectedId}
    class:drop-inside={isFolder && dropTarget?.id === d.id && dropTarget?.mode === 'inside'}
    class:dragging={dragId === d.id}
    style="padding-left:{depth * 14 + 8}px"
    draggable="true"
    role="treeitem"
    aria-selected={d.id === selectedId}
    tabindex="-1"
    ondragstart={(e) => onDragStart(e, d)}
    ondragend={clearDrag}
    ondragover={(e) => (isFolder ? onDragOver(e, d, 'inside') : (e.stopPropagation(), onDragOver(e, d, 'before')))}
    ondrop={(e) => (isFolder ? onDrop(e, d, 'inside') : (e.stopPropagation(), onDrop(e, d, 'before')))}
  >
    {#if isFolder}
      <button class="twisty" onclick={() => toggle(d.id)} aria-label={$t('editor.docTree.toggle')}><Icon name={isOpen ? 'chevron-down' : 'chevron-right'} size={12} /></button>
    {:else}
      <span class="twisty leaf"></span>
    {/if}

    {#if renamingId === d.id}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="rename"
        bind:value={renameValue}
        autofocus
        onblur={commitRename}
        onkeydown={(e) => e.key === 'Enter' && commitRename()}
      />
    {:else}
      <button class="label" onclick={() => (isFolder ? toggle(d.id) : onselect(d.id))} ondblclick={() => startRename(d)}>
        {@render icon(isFolder ? 'folder' : d.kind === 'database' ? 'database' : 'page', 'ico')}
        <span class="title">{d.title || $t('editor.docTree.untitled')}</span>
      </button>
    {/if}

    {#if renamingId !== d.id}
    <span class="actions">
      {#if isFolder}
        <button class="act" title={$t('editor.docTree.newSubfolder')} onclick={() => { expanded = new Set(expanded).add(d.id); oncreate(d.id, 'folder'); }}>
          {@render icon('folder', '')}
        </button>
        <button class="act" title={$t('editor.docTree.newPage')} onclick={() => { expanded = new Set(expanded).add(d.id); oncreate(d.id, 'page'); }}>{@render icon('plus', '')}</button>
        <button class="act" title={$t('editor.docTree.newDatabase')} onclick={() => { expanded = new Set(expanded).add(d.id); oncreate(d.id, 'database'); }}>{@render icon('database', '')}</button>
      {/if}
      <button class="act" title={$t('editor.docTree.rename')} onclick={() => startRename(d)}>{@render icon('pencil', '')}</button>
      <button class="act" title={$t('editor.docTree.delete')} onclick={() => ondelete(d.id)}>{@render icon('trash', '')}</button>
    </span>
    {/if}
  </div>

  {#if isFolder && isOpen}
    {#each kids as child}
      {@render node(child, depth + 1)}
    {/each}
  {/if}
{/snippet}

<div class="doctree">
  <div class="tree-head">
    <span>{$t('editor.docTree.documents')}</span>
    <span class="head-actions">
      <button class="act" title={$t('editor.docTree.expandAll')} onclick={expandAll}><Icon name="chevrons-up-down" size={13} /></button>
      <button class="act" title={$t('editor.docTree.collapseAll')} onclick={collapseAll}><Icon name="chevrons-down-up" size={13} /></button>
      <button class="act" title={$t('editor.docTree.newFolder')} onclick={() => oncreate(null, 'folder')}>
        {@render icon('folder', '')}
      </button>
      <button class="act" title={$t('editor.docTree.newPage')} onclick={() => oncreate(null, 'page')}>{@render icon('plus', '')}</button>
      <button class="act" title={$t('editor.docTree.newDatabase')} onclick={() => oncreate(null, 'database')}>{@render icon('database', '')}</button>
    </span>
  </div>

  <!-- Drop onto empty body area = move to root level -->
  <div
    class="tree-body"
    class:drop-root={dropTarget?.id === 'root'}
    role="tree"
    tabindex="-1"
    ondragover={(e) => onDragOver(e, null, 'inside')}
    ondrop={(e) => onDrop(e, 'root', 'inside')}
  >
    {#each childrenOf(null) as d}
      {@render node(d, 0)}
    {/each}
    {#if childrenOf(null).length === 0}
      <p class="empty">{$t('editor.docTree.emptyTree')}</p>
    {/if}
  </div>
</div>

<style>
  .doctree {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .tree-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-3);
    font-size: var(--text-xs);
    font-family: var(--mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    border-bottom: 1px solid var(--border);
  }
  .act {
    display: inline-flex;
    align-items: center;
    position: relative;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 3px 5px;
    border-radius: var(--radius);
  }
  .act:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .tree-body {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-2) 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 2px;
    padding-right: var(--space-2);
    height: 30px;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.selected {
    background: var(--surface-2);
  }
  .row.selected .title {
    color: var(--accent);
  }
  .row.dragging {
    opacity: 0.4;
  }
  .row.drop-inside {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }

  /* Thin reorder indicator between rows */
  .drop-before {
    height: 4px;
    margin-top: -2px;
    margin-bottom: -2px;
  }
  .drop-before.active {
    height: 4px;
    background: var(--accent);
    border-radius: var(--radius);
  }

  .tree-body.drop-root {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .twisty {
    width: 16px;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
  }
  .twisty.leaf {
    display: inline-block;
  }

  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text);
    cursor: pointer;
    text-align: left;
    font-size: var(--text-sm);
    overflow: hidden;
  }
  .label .title {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ico {
    flex-shrink: 0;
    color: var(--muted);
  }
  .row.selected .ico {
    color: var(--accent);
  }

  .actions {
    display: none;
    gap: 0;
  }
  .row:hover .actions {
    display: flex;
  }

  .rename {
    flex: 1;
    background: var(--bg);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    padding: 2px 6px;
  }

  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-4) var(--space-3);
    line-height: 1.5;
  }
</style>
