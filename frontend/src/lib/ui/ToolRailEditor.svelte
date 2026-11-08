<script>
  // Arranges the Tools section of the navigation rail. Every tool sits in one drag zone: a
  // category, "no category", or Available (not shown in the rail). Dragging between zones
  // files, unfiles or hides a tool. A zone only holds tools, so a category can never hold
  // another one. A dropped tool settles in its A to Z place, the order the rail reads in.
  // Categories face the other zones in two columns that scroll on their own, so the usual
  // drag crosses the modal without having to scroll to its target. A folded zone stays a
  // drop target: it hands the drag library an empty list, and a tool dropped there joins the
  // rows it keeps out of sight. The arrows of a category set its place in the rail.
  //
  //   <ToolRailEditor bind:open {items} selected={ids} {categories} onsave={apply} />
  //
  // props: open (bindable), items ([{ id, name, icon }]: every candidate), selected (ids
  //        currently shown), categories ([{ id, name, tools }]), onsave({ tools, toolCategories })
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import { dndzone } from 'svelte-dnd-action';
  import { flip } from 'svelte/animate';
  import { byName, HIDDEN, LOOSE, moveCategory, nameTaken, namesValid, seedZones, zonesToRail } from './railTools.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), items = [], selected = [], categories = [], onsave = () => {} } =
    $props();

  const FLIP_MS = 140;
  let cats = $state([]); // working copy: [{ id, name }]
  let zones = $state({}); // zone id -> [{ id, name, icon }]
  let folded = $state({}); // zone id -> rows hidden
  let passing = $state({}); // folded zone id -> what the drag library holds there mid-drag
  let draft = $state('');

  // Re-seed each time the modal opens, so a cancelled edit leaves nothing behind.
  let wasOpen = false;
  $effect(() => {
    if (open === wasOpen) return;
    wasOpen = open;
    if (!open) return;
    ({ cats, zones } = seedZones(items, selected, categories));
    folded = {};
    passing = {};
    draft = '';
  });

  const names = $derived(cats.map((cat) => cat.name));
  const canAdd = $derived(!nameTaken(draft, names));
  const canSave = $derived(namesValid(names));
  // The modal paints its body before the seeding effect has run.
  const inZone = (zone) => zones[zone] ?? [];
  const onScreen = (zone) => (folded[zone] ? (passing[zone] ?? []) : inZone(zone));

  function addCategory() {
    if (!canAdd) return;
    const id = `cat_${Math.random().toString(36).slice(2, 10)}`;
    cats = [...cats, { id, name: draft.trim() }];
    zones[id] = [];
    draft = '';
  }
  function dropCategory(id) {
    zones[LOOSE] = [...inZone(LOOSE), ...inZone(id)].sort(byName);
    delete zones[id];
    cats = cats.filter((cat) => cat.id !== id);
  }
  function move(item, from, to) {
    zones[from] = inZone(from).filter((other) => other.id !== item.id);
    zones[to] = [...inZone(to), item].sort(byName);
  }
  function onConsider(id, e) {
    if (folded[id]) passing[id] = e.detail.items;
    else zones[id] = e.detail.items;
  }
  function onFinalize(id, e) {
    zones[id] = [...(folded[id] ? inZone(id) : []), ...e.detail.items].sort(byName);
    passing[id] = [];
  }
  function onDraftKey(e) {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    addCategory();
  }
  function apply() {
    onsave(zonesToRail(cats, zones, categories, items));
    open = false;
  }
</script>

{#snippet chevron(id)}
  <Icon name={folded[id] ? 'chevron-right' : 'chevron-down'} size={12} />
{/snippet}

{#snippet section(id, label)}
  <button class="sec" aria-expanded={!folded[id]} onclick={() => (folded[id] = !folded[id])}>
    {@render chevron(id)}
    {label}
    <span class="count">{inZone(id).length}</span>
  </button>
  {@render zone(id)}
{/snippet}

{#snippet zone(id)}
  <div class="zone-box">
    {#if !onScreen(id).length}
      <span class="drop-hint">{$t('rail.tools.dropHere')}</span>
    {/if}
    <div
      class="zone"
      use:dndzone={{
        items: onScreen(id),
        flipDurationMs: FLIP_MS,
        type: 'rail-tools',
        dropTargetStyle: { outline: '1px dashed var(--accent)' }
      }}
      onconsider={(e) => onConsider(id, e)}
      onfinalize={(e) => onFinalize(id, e)}
    >
      {#each onScreen(id) as item (item.id)}
        <div class="row" animate:flip={{ duration: FLIP_MS }}>
          <span class="grip" aria-hidden="true"><Icon name="grip-vertical" size={14} /></span>
          <span class="ic"><Icon name={item.icon} size={15} strokeWidth={1.8} /></span>
          <span class="nm">{item.name}</span>
          {#if id === HIDDEN}
            <button
              class="act"
              title={$t('rail.tools.addTool')}
              aria-label={$t('rail.tools.addTool')}
              onclick={() => move(item, HIDDEN, LOOSE)}
            >
              <Icon name="plus" size={13} />
            </button>
          {:else}
            <button
              class="act remove"
              title={$t('rail.editor.remove')}
              aria-label={$t('rail.editor.remove')}
              onclick={() => move(item, id, HIDDEN)}
            >
              <Icon name="x" size={13} />
            </button>
          {/if}
        </div>
      {/each}
    </div>
  </div>
{/snippet}

<Modal bind:open size="lg" title={$t('rail.arrangeTools')}>
  <p class="hint">{$t('rail.tools.hint')}</p>

  <div class="cols">
    <div class="col">
      <div class="new">
        <input
          bind:value={draft}
          maxlength="40"
          placeholder={$t('rail.tools.newCategory')}
          aria-label={$t('rail.tools.newCategory')}
          onkeydown={onDraftKey}
        />
        <button class="ghost" disabled={!canAdd} onclick={addCategory}>{$t('common.add')}</button>
      </div>

      {#each cats as cat, index (cat.id)}
        <section class="cat">
          <div class="cat-head">
            <button
              class="act"
              title={$t('rail.tools.fold')}
              aria-label={$t('rail.tools.fold')}
              aria-expanded={!folded[cat.id]}
              onclick={() => (folded[cat.id] = !folded[cat.id])}
            >
              {@render chevron(cat.id)}
            </button>
            <input bind:value={cat.name} maxlength="40" aria-label={$t('rail.tools.categoryName')} />
            <span class="count">{inZone(cat.id).length}</span>
            <button
              class="act"
              title={$t('dashboard.moveUp')}
              aria-label={$t('dashboard.moveUp')}
              disabled={index === 0}
              onclick={() => (cats = moveCategory(cats, index, -1))}
            >
              <Icon name="chevron-up" size={13} />
            </button>
            <button
              class="act"
              title={$t('dashboard.moveDown')}
              aria-label={$t('dashboard.moveDown')}
              disabled={index === cats.length - 1}
              onclick={() => (cats = moveCategory(cats, index, 1))}
            >
              <Icon name="chevron-down" size={13} />
            </button>
            <button
              class="act remove"
              title={$t('rail.tools.deleteCategory')}
              aria-label={$t('rail.tools.deleteCategory')}
              onclick={() => dropCategory(cat.id)}
            >
              <Icon name="trash" size={13} />
            </button>
          </div>
          {@render zone(cat.id)}
        </section>
      {/each}
    </div>

    <div class="col">
      {@render section(LOOSE, $t('rail.tools.noCategory'))}
      {@render section(HIDDEN, $t('rail.editor.available'))}
    </div>
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={!canSave} onclick={apply}>{$t('common.save')}</button>
  {/snippet}
</Modal>

<style>
  .hint {
    margin: 0 0 var(--space-3);
    color: var(--dim);
    font-size: var(--text-xs);
    line-height: var(--lh-base);
  }
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: var(--space-4);
  }
  .col {
    min-width: 0;
    max-height: min(520px, calc(100dvh - 240px));
    padding: 1px var(--space-1) 1px 1px;
    overflow-y: auto;
  }
  @media (max-width: 640px) {
    .cols {
      grid-template-columns: minmax(0, 1fr);
    }
    .col {
      max-height: none;
      overflow: visible;
    }
  }
  .sec {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    width: 100%;
    margin: 0 0 var(--space-2);
    padding: 0;
    border: none;
    background: transparent;
    color: var(--dim);
    font-family: inherit;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-align: left;
    text-transform: uppercase;
    cursor: pointer;
  }
  .sec:hover {
    color: var(--text);
  }
  .count {
    color: var(--faint);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .zone-box + .sec {
    margin-top: var(--space-4);
  }
  .new {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .new input {
    flex: 1;
    min-width: 0;
  }
  /* One frame per category: the name field inside it stays borderless. */
  .cat {
    margin-bottom: var(--space-2);
    padding: var(--space-1) var(--space-2) var(--space-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    transition: border-color var(--dur-fast) var(--ease);
  }
  .cat:focus-within {
    border-color: var(--accent);
  }
  .cat-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .cat-head input,
  .cat-head input:focus {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: none;
    background: transparent;
    box-shadow: none;
    font-weight: var(--fw-medium);
  }
  /* The drop hint sits behind the zone: a child of the zone would count as a dragged row. */
  .zone-box {
    position: relative;
  }
  .zone {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-height: 38px;
    border-radius: var(--radius);
  }
  .drop-hint {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--faint);
    font-size: var(--text-xs);
    pointer-events: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: 38px;
    padding: 0 var(--space-2) 0 var(--space-1);
    border-radius: var(--radius);
    background: var(--pill-bg);
    color: var(--pill-fg);
    font-size: var(--text-sm);
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
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .act:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .act.remove:hover {
    color: var(--red);
  }
  .act:disabled {
    color: var(--faint);
    background: transparent;
    cursor: default;
  }
</style>
