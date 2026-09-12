<script>
  // The workflow grid: steps stacked top to bottom, tasks side by side inside a step.
  //
  // No free canvas and no hand-drawn links. A task's place in the grid *is* the order it
  // runs in, so the only thing to get right is where a card lands: every gap between two
  // steps and every gap between two tasks is a drop target, and the one under the pointer
  // is highlighted before the drop so nobody has to guess.
  //
  // Nothing here mutates the graph silently: every change goes through `onchange`, which
  // is what makes autosave and the dirty marker possible upstream.
  import NodeCard from './NodeCard.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { toRows, fromRows, newNode } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let {
    graph = $bindable({ nodes: [], edges: [] }),
    selected = $bindable(''),
    statuses = {},
    onchange = () => {},
    onopen = () => {}
  } = $props();

  const rows = $derived(toRows(graph.nodes));

  // The drop target under the pointer: `{ kind: 'step', at }` inserts a new step, and
  // `{ kind: 'task', row, at }` inserts into an existing one.
  let hover = $state(null);
  let dragging = $state('');

  const isStep = (at) => hover?.kind === 'step' && hover.at === at;
  const isTask = (row, at) => hover?.kind === 'task' && hover.row === row && hover.at === at;

  function commit(next) {
    const g = fromRows(next);
    graph.nodes = g.nodes;
    graph.edges = g.edges;
    onchange();
  }

  /** Put a task at `target`, moving it out of its current place first when it has one. */
  function place(node, target, fromId = '') {
    const next = toRows(graph.nodes).map((row) => [...row]);
    let to = target;
    if (fromId) {
      const row = next.findIndex((r) => r.some((n) => n.id === fromId));
      if (row >= 0) {
        const col = next[row].findIndex((n) => n.id === fromId);
        next[row].splice(col, 1);
        // Taking the card out shifts everything after it: fix the target, not the rows.
        if (to.kind === 'task' && to.row === row && to.at > col) to = { ...to, at: to.at - 1 };
        if (!next[row].length) {
          next.splice(row, 1);
          if (to.kind === 'step' && to.at > row) to = { ...to, at: to.at - 1 };
          else if (to.kind === 'task' && to.row > row) to = { ...to, row: to.row - 1 };
          else if (to.kind === 'task' && to.row === row) to = { kind: 'step', at: row };
        }
      }
    }
    if (to.kind === 'step') next.splice(Math.min(to.at, next.length), 0, [node]);
    else next[to.row].splice(to.at, 0, node);
    commit(next);
  }

  function startMove(ev, node) {
    dragging = node.id;
    ev.dataTransfer.setData('text/otw-node', node.id);
    ev.dataTransfer.effectAllowed = 'move';
  }

  function over(ev, target) {
    ev.preventDefault();
    ev.stopPropagation();
    hover = target;
  }

  /** A card is a target too: its left half means before it, its right half after it. The
   *  10 px gaps alone would be a game of darts. */
  function half(ev, row, x) {
    const r = ev.currentTarget.getBoundingClientRect();
    return { kind: 'task', row, at: ev.clientX - r.left > r.width / 2 ? x + 1 : x };
  }

  function drop(ev, target) {
    ev.preventDefault();
    ev.stopPropagation();
    hover = null;
    const moving = ev.dataTransfer?.getData('text/otw-node');
    if (moving) {
      dragging = '';
      const node = graph.nodes.find((n) => n.id === moving);
      if (node) place(node, target, moving);
      return;
    }
    const kind = ev.dataTransfer?.getData('text/otw-block');
    if (!kind) return;
    // An endpoint dragged from the rail arrives as an `api` task already pointed at it.
    let patch = null;
    const raw = ev.dataTransfer.getData('text/otw-endpoint');
    if (raw) {
      try {
        const e = JSON.parse(raw);
        patch = { method: e.method, path: e.path };
      } catch {
        patch = null;
      }
    }
    const node = newNode(kind, null, graph.nodes);
    if (patch) node.config = { ...node.config, ...patch };
    selected = node.id;
    place(node, target);
  }

  function keydown(ev) {
    if ((ev.key !== 'Delete' && ev.key !== 'Backspace') || !selected) return;
    if (['INPUT', 'TEXTAREA', 'SELECT'].includes(ev.target?.tagName)) return;
    ev.preventDefault();
    removeNode(selected);
  }

  export function removeNode(id) {
    if (selected === id) selected = '';
    commit(toRows(graph.nodes).map((row) => row.filter((n) => n.id !== id)));
  }

  /** Add a task. Without a target it becomes the last step, which is what a click does. */
  export function addNode(kind, target = null, patch = null) {
    const node = newNode(kind, null, graph.nodes);
    if (patch) node.config = { ...node.config, ...patch };
    selected = node.id;
    place(node, target ?? { kind: 'step', at: rows.length });
    return node;
  }
</script>

<svelte:window onkeydown={keydown} ondragend={() => ((hover = null), (dragging = ''))} />

<div
  class="board"
  ondragleave={(e) => e.currentTarget === e.target && (hover = null)}
  role="list"
  aria-label={$t('automator.grid.label')}
>
  {#each rows as row, y (row[0].id)}
    <div
      class="stepgap"
      class:on={isStep(y)}
      ondragover={(e) => over(e, { kind: 'step', at: y })}
      ondrop={(e) => drop(e, { kind: 'step', at: y })}
      role="presentation"
    >
      <span class="line"></span>
      {#if y > 0}<Icon name="chevron-down" size={12} />{/if}
      <span class="tag">{$t('automator.grid.newStep')}</span>
    </div>

    <section class="step" role="listitem">
      <header title={row.length > 1 ? $t('automator.grid.inOrder', { n: row.length }) : ''}>
        <span class="num">{y + 1}</span>
        {#if row.length > 1}<span class="count">{row.length}</span>{/if}
      </header>

      <div class="tasks">
        {#each row as node, x (node.id)}
          <div
            class="taskgap"
            class:on={isTask(y, x)}
            ondragover={(e) => over(e, { kind: 'task', row: y, at: x })}
            ondrop={(e) => drop(e, { kind: 'task', row: y, at: x })}
            role="presentation"
          ></div>
          <div
            class="hold"
            ondragover={(e) => over(e, half(e, y, x))}
            ondrop={(e) => drop(e, half(e, y, x))}
            role="presentation"
          >
            <NodeCard
              {node}
              selected={selected === node.id}
              dragging={dragging === node.id}
              status={statuses[node.id] ?? ''}
              ondragstart={(e) => startMove(e, node)}
              onselect={() => (selected = node.id)}
              onopen={() => onopen(node)}
              onremove={() => removeNode(node.id)}
            />
          </div>
        {/each}
        <div
          class="taskgap end"
          class:on={isTask(y, row.length)}
          ondragover={(e) => over(e, { kind: 'task', row: y, at: row.length })}
          ondrop={(e) => drop(e, { kind: 'task', row: y, at: row.length })}
          role="presentation"
        ></div>
      </div>
    </section>
  {/each}

  <div
    class="stepgap last"
    class:empty={!rows.length}
    class:on={isStep(rows.length)}
    ondragover={(e) => over(e, { kind: 'step', at: rows.length })}
    ondrop={(e) => drop(e, { kind: 'step', at: rows.length })}
    role="presentation"
  >
    <span class="line"></span>
    <span class="tag">{$t('automator.grid.newStep')}</span>
  </div>

  {#if !rows.length}
    <div class="hint">
      <Icon name="zap" size={18} />
      <p>{$t('automator.grid.empty')}</p>
    </div>
  {/if}
</div>

<style>
  .board {
    flex: 1;
    overflow-y: auto;
    background: var(--surface-2);
    padding: var(--space-2) var(--space-4) var(--space-8);
    position: relative;
  }
  /* Between two steps: a thin strip that opens up when a card is held over it. */
  .stepgap {
    height: 14px;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: height 80ms ease;
  }
  .stepgap.last {
    height: 26px;
  }
  .stepgap.empty {
    height: 180px;
  }
  .stepgap :global(svg) {
    position: absolute;
    color: var(--border);
  }
  .stepgap.on :global(svg) {
    display: none;
  }
  .stepgap .line {
    width: 100%;
    height: 0;
    border-top: 0.5px dashed transparent;
  }
  .stepgap .tag {
    position: absolute;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--accent);
    background: var(--surface-2);
    padding: 0 var(--space-2);
    opacity: 0;
  }
  .stepgap.on {
    height: 40px;
  }
  .stepgap.on .line {
    border-top-color: var(--accent);
    border-top-style: solid;
    border-top-width: 1.5px;
  }
  .stepgap.on .tag {
    opacity: 1;
  }
  .step {
    display: flex;
    align-items: stretch;
    gap: var(--space-2);
    background: var(--surface);
    border: 0.5px solid var(--border);
    padding: var(--space-2);
  }
  .step header {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
    width: 34px;
    flex: none;
    border-right: 0.5px solid var(--border);
    padding-right: var(--space-2);
  }
  .num {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .count {
    font-size: 9px;
    color: var(--muted);
    border: 0.5px solid var(--border);
    padding: 0 3px;
  }
  .hold {
    display: flex;
  }
  .tasks {
    display: flex;
    align-items: stretch;
    flex-wrap: wrap;
    row-gap: var(--space-2);
    flex: 1;
    min-width: 0;
  }
  /* Between two tasks of the same step, same idea on the other axis. */
  .taskgap {
    width: 10px;
    flex: none;
    align-self: stretch;
    border-left: 1.5px solid transparent;
    margin: 0 1px;
    transition: width 80ms ease;
  }
  .taskgap.end {
    flex: 1;
    min-width: 10px;
  }
  .taskgap.on {
    width: 26px;
    border-left-color: var(--accent);
  }
  .hint {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    color: var(--dim);
    pointer-events: none;
    font-size: var(--text-sm);
  }
</style>
