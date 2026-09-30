<script>
  // The custom-indicator library, reachable from the Strategy step. Two states in one modal:
  // the list, and the formula-stack builder for one indicator. Saving refreshes the parent's
  // list (`onchange`) so a brand-new indicator is immediately pickable as an operand.
  //
  // The rows are the shared library ($lib/indicators) — the same definitions the chart draws.
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import IndicatorBuilder from '$lib/indicators/IndicatorBuilder.svelte';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { defaultIndicatorDef } from '$lib/indicators/dag.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), indicators = [], onchange } = $props();

  let editing = $state(false); // list ↔ builder
  let id = $state(null);
  let name = $state('');
  let def = $state(defaultIndicatorDef());
  let error = $state('');
  let query = $state('');

  const filtered = $derived(
    indicators.filter((i) => i.name.toLowerCase().includes(query.trim().toLowerCase()))
  );

  // Reopening always lands on the list, never on the last indicator edited.
  $effect(() => {
    if (open) return;
    editing = false;
    error = '';
  });

  function startNew() {
    id = null;
    name = '';
    def = defaultIndicatorDef();
    error = '';
    editing = true;
  }

  async function edit(i) {
    const full = await indicatorLib.get(i.id).catch(() => i);
    id = full.id;
    name = full.name;
    def = full.definition;
    error = '';
    editing = true;
  }

  async function save() {
    const n = name.trim();
    if (!n) {
      error = $t('backtest.indicators.nameRequired');
      return;
    }
    try {
      if (id) await indicatorLib.update(id, n, '', def);
      else await indicatorLib.create(n, '', def);
      editing = false;
      await onchange?.();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove(i) {
    await indicatorLib.remove(i.id).catch(() => {});
    await onchange?.();
  }
</script>

<Modal
  bind:open
  size="lg"
  title={editing
    ? id
      ? $t('backtest.indicators.edit')
      : $t('backtest.indicators.new')
    : $t('backtest.indicators.title')}
>
  {#if !editing}
    <div class="bar">
      <input class="search" bind:value={query} placeholder={$t('backtest.indicators.search')} />
      <button class="new" onclick={startNew}><Icon name="plus" size={12} /> {$t('backtest.indicators.new')}</button>
    </div>
    {#if !indicators.length}
      <p class="empty">{$t('backtest.indicators.none')}</p>
    {:else}
      <div class="rows">
        {#each filtered as i (i.id)}
          <div class="row">
            <button class="main" onclick={() => edit(i)}>
              <span class="nm">{i.name}</span>
              <span class="when">{String(i.updated_at ?? '').slice(0, 10)}</span>
            </button>
            <button class="act danger" title={$t('common.remove')} onclick={() => remove(i)}><Icon name="x" size={12} /></button>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <label class="field">
      <span>{$t('backtest.indicators.name')}</span>
      <input bind:value={name} placeholder={$t('backtest.indicators.namePlaceholder')} />
    </label>
    <IndicatorBuilder bind:def />
    {#if error}<p class="err">{error}</p>{/if}
  {/if}

  {#snippet footer()}
    {#if editing}
      <button class="ghost" onclick={() => (editing = false)}>{$t('common.cancel')}</button>
      <button class="primary" onclick={save}>{$t('common.save')}</button>
    {:else}
      <button class="ghost" onclick={() => (open = false)}>{$t('common.close')}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .bar {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .search {
    flex: 1;
    min-width: 0;
  }
  .new,
  .ghost,
  .primary {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    white-space: nowrap;
  }
  .primary {
    background: color-mix(in srgb, var(--accent) 22%, var(--surface-2));
  }
  .new:hover,
  .ghost:hover,
  .primary:hover {
    background: color-mix(in srgb, var(--accent) 14%, var(--surface-2));
  }
  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-4) 0;
  }
  .rows {
    display: flex;
    flex-direction: column;
    max-height: 46vh;
    overflow-y: auto;
  }
  .row {
    display: flex;
    align-items: center;
    border-bottom: var(--hairline) solid var(--border);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    background: transparent;
    border: none;
    color: var(--text);
    text-align: left;
    padding: var(--space-2) var(--space-2);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .when {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .act {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-2);
    display: inline-flex;
  }
  .act.danger:hover {
    color: var(--red);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: var(--text-xs);
    color: var(--muted);
    margin-bottom: var(--space-3);
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
    margin-top: var(--space-2);
  }
</style>
