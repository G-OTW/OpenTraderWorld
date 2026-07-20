<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { COLUMN_TYPES, OPTION_COLORS, optId } from '../db-api.js';
  import { choicesOf } from './cells.js';
  import { t } from '$lib/i18n';

  // props: col, onPatch(patch), onDelete()
  let { col, onPatch, onDelete } = $props();

  let open = $state(false);
  let name = $state(col.name);

  const isSelect = $derived(col.type === 'select' || col.type === 'multi_select');

  function commitName() {
    if (name !== col.name) onPatch({ name });
  }
  function changeType(type) {
    const options = type === 'select' || type === 'multi_select' ? col.options ?? {} : {};
    onPatch({ type, options });
  }

  // ── Select-option management (stored in col.options.choices) ──
  function addChoice() {
    const choices = [...choicesOf(col)];
    const color = OPTION_COLORS[choices.length % OPTION_COLORS.length];
    choices.push({ id: optId(), name: $t('editor.columnMenu.option'), color });
    onPatch({ options: { ...col.options, choices } });
  }
  function renameChoice(id, value) {
    const choices = choicesOf(col).map((c) => (c.id === id ? { ...c, name: value } : c));
    onPatch({ options: { ...col.options, choices } });
  }
  function recolorChoice(id, color) {
    const choices = choicesOf(col).map((c) => (c.id === id ? { ...c, color } : c));
    onPatch({ options: { ...col.options, choices } });
  }
  function removeChoice(id) {
    const choices = choicesOf(col).filter((c) => c.id !== id);
    onPatch({ options: { ...col.options, choices } });
  }
</script>

<div class="colhead">
  <button class="colname" onclick={() => (open = !open)} title={$t('editor.columnMenu.editColumn')}>
    <span>{col.name || $t('editor.docTree.untitled')}</span>
    <span class="caret"><Icon name="chevron-down" size={12} /></span>
  </button>

  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="backdrop" onclick={() => (open = false)} role="presentation"></div>
    <div class="panel">
      <input
        class="field"
        bind:value={name}
        onblur={commitName}
        onkeydown={(e) => e.key === 'Enter' && commitName()}
        placeholder={$t('editor.columnMenu.columnName')}
      />

      <div class="label">{$t('editor.columnMenu.type')}</div>
      <div class="types">
        {#each COLUMN_TYPES as ct}
          <button class="type" class:active={col.type === ct.id} onclick={() => changeType(ct.id)}>
            {ct.label}
          </button>
        {/each}
      </div>

      {#if isSelect}
        <div class="label">{$t('editor.columnMenu.options')}</div>
        {#each choicesOf(col) as c (c.id)}
          <div class="choice">
            <span class="opt-chip {c.color}">{c.name || '—'}</span>
            <input
              class="field sm"
              value={c.name}
              oninput={(e) => renameChoice(c.id, e.target.value)}
            />
            <div class="swatches">
              {#each OPTION_COLORS as col2}
                <button
                  class="swatch {col2}"
                  class:sel={c.color === col2}
                  onclick={() => recolorChoice(c.id, col2)}
                  aria-label={col2}
                ></button>
              {/each}
            </div>
            <button class="x" onclick={() => removeChoice(c.id)} title={$t('editor.columnMenu.remove')}><Icon name="x" size={13} /></button>
          </div>
        {/each}
        <button class="add-choice" onclick={addChoice}>{$t('editor.columnMenu.addOption')}</button>
      {/if}

      <button class="delete" onclick={() => { open = false; onDelete(); }}>{$t('editor.columnMenu.deleteColumn')}</button>
    </div>
  {/if}
</div>

<style>
  .colhead {
    position: relative;
  }
  .colname {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    width: 100%;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    padding: 6px 8px;
    text-align: left;
  }
  .colname:hover {
    color: var(--text);
  }
  .caret {
    opacity: 0.5;
    font-size: var(--text-xs);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--z-dropdown);
  }
  /* Level 2: it floats over the table, so it carries the shadow and drops the border. */
  .panel {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: calc(var(--z-dropdown) + 1);
    width: 260px;
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .field.sm {
    flex: 1;
    font-size: var(--text-sm);
    padding: 3px 6px;
  }
  .label {
    color: var(--muted);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-top: 2px;
  }
  .types {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
  }
  .type {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 4px 6px;
  }
  .type:hover {
    color: var(--text);
  }
  .type.active {
    color: var(--text);
    border-color: var(--accent);
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .swatches {
    display: flex;
    gap: 2px;
  }
  .swatch {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 1px solid transparent;
    cursor: pointer;
    padding: 0;
  }
  .swatch.sel {
    border-color: var(--text);
  }
  /* Desaturated anchors — mirror db/option-colors.css chip hues. */
  .swatch.slate { background: #6b7280; }
  .swatch.red { background: #b05548; }
  .swatch.amber { background: #a08558; }
  .swatch.green { background: #6f9b7e; }
  .swatch.blue { background: #5f6b7a; }
  .swatch.violet { background: #6d6b74; }
  .swatch.pink { background: #9a6f6a; }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
  }
  .x:hover {
    color: var(--red);
  }
  .add-choice {
    background: transparent;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 4px;
  }
  .add-choice:hover {
    color: var(--text);
  }
  .delete {
    margin-top: 4px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--red);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 5px;
  }
  .delete:hover {
    background: color-mix(in srgb, var(--red) 14%, transparent);
  }
</style>
