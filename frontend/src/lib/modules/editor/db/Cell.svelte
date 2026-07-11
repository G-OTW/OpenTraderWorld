<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { coerce, choicesOf, choiceById } from './cells.js';
  import { t } from '$lib/i18n';

  // props: col, value, onCommit(newValue)
  let { col, value, onCommit } = $props();

  let open = $state(false); // dropdown for select/multi_select

  function commit(v) {
    onCommit(coerce(col, v));
  }

  function toggleMulti(id) {
    const cur = Array.isArray(value) ? value : [];
    const next = cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id];
    onCommit(next);
  }
</script>

{#if col.type === 'checkbox'}
  <input type="checkbox" checked={!!value} onchange={(e) => commit(e.target.checked)} />
{:else if col.type === 'number'}
  <input
    class="cell-input"
    type="number"
    value={value ?? ''}
    onblur={(e) => commit(e.target.value)}
    onkeydown={(e) => e.key === 'Enter' && e.target.blur()}
  />
{:else if col.type === 'date'}
  <input
    class="cell-input"
    type="date"
    value={value ?? ''}
    onchange={(e) => commit(e.target.value || null)}
  />
{:else if col.type === 'url'}
  <input
    class="cell-input"
    type="url"
    placeholder="https://…"
    value={value ?? ''}
    onblur={(e) => commit(e.target.value)}
    onkeydown={(e) => e.key === 'Enter' && e.target.blur()}
  />
{:else if col.type === 'select'}
  <div class="select">
    <button class="select-trigger" onclick={() => (open = !open)}>
      {#if value && choiceById(col, value)}
        {@const c = choiceById(col, value)}
        <span class="opt-chip {c.color}">{c.name}</span>
      {:else}
        <span class="muted">—</span>
      {/if}
    </button>
    {#if open}
      <div class="menu" role="listbox">
        <button class="opt" onclick={() => { onCommit(null); open = false; }}>
          <span class="muted">{$t('common.clear')}</span>
        </button>
        {#each choicesOf(col) as c}
          <button class="opt" onclick={() => { onCommit(c.id); open = false; }}>
            <span class="opt-chip {c.color}">{c.name}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
{:else if col.type === 'multi_select'}
  <div class="select">
    <button class="select-trigger" onclick={() => (open = !open)}>
      {#if Array.isArray(value) && value.length}
        {#each value as id}
          {@const c = choiceById(col, id)}
          {#if c}<span class="opt-chip {c.color}">{c.name}</span>{/if}
        {/each}
      {:else}
        <span class="muted">—</span>
      {/if}
    </button>
    {#if open}
      <div class="menu" role="listbox">
        {#each choicesOf(col) as c}
          {@const active = Array.isArray(value) && value.includes(c.id)}
          <button class="opt" class:active onclick={() => toggleMulti(c.id)}>
            <span class="opt-chip {c.color}">{c.name}</span>
            {#if active}<span class="check"><Icon name="check" size={12} /></span>{/if}
          </button>
        {/each}
      </div>
    {/if}
  </div>
{:else}
  <!-- text -->
  <input
    class="cell-input"
    value={value ?? ''}
    onblur={(e) => commit(e.target.value)}
    onkeydown={(e) => e.key === 'Enter' && e.target.blur()}
  />
{/if}

<style>
  .cell-input {
    width: 100%;
    background: transparent;
    border: none;
    color: var(--text);
    font-size: var(--text-sm);
    padding: 4px 6px;
    outline: none;
  }
  .cell-input:focus {
    background: var(--surface-2);
    border-radius: 4px;
  }
  .select {
    position: relative;
  }
  .select-trigger {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    width: 100%;
    min-height: 26px;
    align-items: center;
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 3px 6px;
    text-align: left;
  }
  .select-trigger:hover {
    background: var(--surface-2);
    border-radius: 4px;
  }
  .menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: var(--z-dropdown);
    min-width: 160px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    padding: 4px;
  }
  .opt {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 4px 6px;
    border-radius: 4px;
  }
  .opt:hover {
    background: var(--surface-2);
  }
  .check {
    color: var(--accent);
    font-size: var(--text-sm);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
</style>
