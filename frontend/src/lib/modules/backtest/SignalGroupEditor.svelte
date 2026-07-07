<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Edits a signal-condition group: each condition reads as one sentence line
  // (`left op right`), rows joined by a clickable AND/OR pill that toggles the group
  // logic. Bound via $bindable so edits flow to the parent.
  import { OPS, opIsBinary, defaultCondition } from './api.js';
  import OperandEditor from './OperandEditor.svelte';
  import { t } from '$lib/i18n';

  let {
    group = $bindable(),
    title = undefined,
    defaultOp = 'crosses_above',
    emptyHint = ''
  } = $props();

  const titleText = $derived(title ?? $t('backtest.signalGroup.entry'));

  function add() {
    group.conditions.push(defaultCondition(defaultOp));
  }
  function remove(i) {
    group.conditions.splice(i, 1);
  }
  function toggleLogic() {
    group.logic = group.logic === 'any' ? 'all' : 'any';
  }
  // Switching to a binary op needs a right operand (unary-op conditions drop it).
  function setOp(c, op) {
    c.op = op;
    if (opIsBinary(op) && !c.right) c.right = { kind: 'price', field: 'close' };
  }
</script>

<div class="group">
  <div class="head">
    <span class="title">{titleText}</span>
    <button type="button" class="add" onclick={add}><Icon name="plus" size={11} /> {$t('backtest.signalGroup.rule')}</button>
  </div>

  {#if !group.conditions.length && emptyHint}
    <p class="hint">{emptyHint}</p>
  {/if}

  {#each group.conditions as c, i (c)}
    {#if i > 0}
      <button
        type="button"
        class="joiner"
        onclick={toggleLogic}
        title={$t('backtest.signalGroup.toggleLogicTitle')}
        >{group.logic === 'any' ? 'OR' : 'AND'}</button>
    {/if}
    <div class="cond">
      <OperandEditor bind:operand={c.left} />
      <select class="op" value={c.op} onchange={(e) => setOp(c, e.target.value)}>
        {#each OPS as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
      </select>
      {#if opIsBinary(c.op) && c.right}
        <OperandEditor bind:operand={c.right} />
      {/if}
      <button type="button" class="rm" title={$t('backtest.signalGroup.removeRule')} onclick={() => remove(i)}>
        <Icon name="x" size={12} />
      </button>
    </div>
  {/each}
</div>

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: 2px;
  }
  .title {
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.72rem;
    padding: 2px 10px;
    cursor: pointer;
  }
  .add:hover {
    color: var(--accent);
    border-color: var(--accent);
  }
  .hint {
    font-size: 0.78rem;
    color: var(--muted);
    font-style: italic;
  }
  .joiner {
    align-self: center;
    border: none;
    font-size: 0.62rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border-radius: 999px;
    padding: 2px 12px;
    cursor: pointer;
  }
  .joiner:hover {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
  }
  .cond {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-1);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
  }
  .op {
    height: 28px;
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--accent);
    background-color: var(--surface);
    border-radius: 999px;
    max-width: 132px;
  }
  .rm {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-1);
    border-radius: var(--radius);
  }
  .rm:hover {
    color: var(--red);
  }
</style>
