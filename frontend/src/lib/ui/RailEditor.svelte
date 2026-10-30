<script>
  // Arranges one navigation-rail section: which entries it shows, and in which order.
  // Drag to reorder, click to include or drop an entry. The list is capped and scrolls,
  // so a section can never grow taller than the rail can hold.
  //
  //   <RailEditor bind:open title="Workspace" {items} selected={ids} onsave={apply} />
  //
  // props: open (bindable), title, items ([{ id, name, icon }] — every candidate),
  //        selected (ids currently shown, in order), max, onsave(ids)
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import { dndzone } from 'svelte-dnd-action';
  import { flip } from 'svelte/animate';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), title = '', items = [], selected = [], max = 12, onsave = () => {} } =
    $props();

  const byId = $derived(new Map(items.map((it) => [it.id, it])));
  let shown = $state([]); // working copy: [{ id, name, icon }]

  // Re-seed each time the modal opens, so a cancelled edit leaves nothing behind.
  let wasOpen = false;
  $effect(() => {
    if (open === wasOpen) return;
    wasOpen = open;
    if (open) shown = selected.map((id) => byId.get(id)).filter(Boolean);
  });

  const hidden = $derived(items.filter((it) => !shown.some((s) => s.id === it.id)));
  const full = $derived(shown.length >= max);

  function add(item) {
    if (full) return;
    shown = [...shown, item];
  }
  function drop(id) {
    shown = shown.filter((it) => it.id !== id);
  }
  function handleDnd(e) {
    shown = e.detail.items;
  }
  function apply() {
    onsave(shown.map((it) => it.id));
    open = false;
  }
</script>

<Modal bind:open size="sm" {title}>
  <p class="hint">{$t('rail.editor.hint', { max })}</p>

  <div
    class="list"
    use:dndzone={{ items: shown, flipDurationMs: 140, type: 'rail' }}
    onconsider={handleDnd}
    onfinalize={handleDnd}
  >
    {#each shown as item (item.id)}
      <div class="row" animate:flip={{ duration: 140 }}>
        <span class="grip" aria-hidden="true"><Icon name="grip-vertical" size={14} /></span>
        <span class="ic"><Icon name={item.icon} size={15} strokeWidth={1.8} /></span>
        <span class="nm">{item.name}</span>
        <button
          class="drop"
          title={$t('rail.editor.remove')}
          aria-label={$t('rail.editor.remove')}
          onclick={() => drop(item.id)}
        >
          <Icon name="x" size={13} />
        </button>
      </div>
    {/each}
  </div>

  {#if hidden.length}
    <h3 class="sec">{$t('rail.editor.available')}</h3>
    <div class="list pool">
      {#each hidden as item (item.id)}
        <button class="row add" disabled={full} onclick={() => add(item)}>
          <span class="ic"><Icon name={item.icon} size={15} strokeWidth={1.8} /></span>
          <span class="nm">{item.name}</span>
          <span class="plus"><Icon name="plus" size={13} /></span>
        </button>
      {/each}
    </div>
  {/if}

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={apply}>{$t('common.save')}</button>
  {/snippet}
</Modal>

<style>
  .hint {
    margin: 0 0 var(--space-3);
    color: var(--dim);
    font-size: var(--text-xs);
    line-height: var(--lh-base);
  }
  .sec {
    margin: var(--space-4) 0 var(--space-2);
    color: var(--dim);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  /* Both lists are capped and scroll: the rail itself is capped too. */
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-height: 240px;
    overflow-y: auto;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: 38px;
    padding: 0 var(--space-2) 0 var(--space-1);
    border: none;
    border-radius: var(--radius);
    background: var(--pill-bg);
    color: var(--pill-fg);
    font-family: inherit;
    font-size: var(--text-sm);
    text-align: left;
  }
  .row.add {
    padding-left: var(--space-3);
    background: transparent;
    cursor: pointer;
  }
  .row.add:hover:not(:disabled) {
    background: var(--pill-bg);
  }
  .row.add:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .grip {
    display: inline-flex;
    color: var(--faint);
    cursor: grab;
  }
  .ic {
    display: inline-flex;
    color: var(--muted);
  }
  .nm {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .plus {
    display: inline-flex;
    color: var(--muted);
  }
  .drop {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .drop:hover {
    color: var(--red);
    background: var(--surface-2);
  }
</style>
