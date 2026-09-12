<script>
  // Two-step picker for adding tiles to a dashboard page.
  //   1. the modules that ship widgets (plus free text, and module links in edit mode)
  //   2. that module's widgets, each drawn as a small visual of what it looks like on the
  //      page; one or many are selected, then Add places them all at once.
  // The backdrop is inert: the picker is left through Cancel (or Escape), never by a stray
  // click while choosing. `onadd` receives the picks: { type, variant } or { moduleId }.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { availableWidgets } from './registry.js';
  import { moduleById, visibleModules } from '$lib/modules/registry';
  import { installedIds } from '$lib/modules/installed.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    onadd = () => {},
    widgetsOnly = false,
    title = ''
  } = $props();

  const LINKS_GROUP = '__links__';

  const widgets = $derived(availableWidgets($installedIds));
  const mods = $derived(visibleModules($installedIds).filter((m) => !m.home));

  /** Step 1: one entry per module that has widgets, free text first, module links last. */
  const groups = $derived.by(() => {
    const out = [];
    const free = widgets.filter((w) => !w.moduleId);
    if (free.length) {
      out.push({
        id: '__free__',
        name: $t('dashboard.widgets.picker.general'),
        icon: 'text-quote',
        widgets: free
      });
    }
    for (const mod of mods) {
      const own = widgets.filter((w) => w.moduleId === mod.id);
      if (own.length) out.push({ id: mod.id, name: mod.name, icon: mod.icon, widgets: own });
    }
    if (!widgetsOnly) {
      out.push({
        id: LINKS_GROUP,
        name: $t('dashboard.widgets.picker.moduleLinks'),
        icon: 'grid',
        blurb: $t('dashboard.widgets.picker.moduleLinksBlurb'),
        widgets: []
      });
    }
    return out;
  });

  let groupId = $state(null); // null = step 1
  let picks = $state([]); // [{ type, variant } | { moduleId }]

  const group = $derived(groups.find((g) => g.id === groupId) ?? null);
  const isLinks = $derived(groupId === LINKS_GROUP);

  /** Step 2 cards: one per data presentation, since a variant is what lands on the page. */
  const cards = $derived.by(() => {
    if (!group || isLinks) return [];
    return group.widgets.flatMap((w) =>
      (w.variants ?? []).map((variant) => ({
        key: `${w.type}:${variant.id}`,
        type: w.type,
        variant: variant.id,
        icon: w.icon,
        label: variant.label ?? w.label,
        subtitle: variant.subtitle ?? w.blurb,
        size: variant.size ?? 'M',
        tone: variant.tone ?? ''
      }))
    );
  });

  // Reset whenever the picker opens, so it never reopens mid-choice.
  $effect(() => {
    if (!open) return;
    groupId = null;
    picks = [];
  });

  function countOf(g) {
    if (g.id === LINKS_GROUP) return mods.length;
    return g.widgets.reduce((sum, w) => sum + (w.variants?.length ?? 1), 0);
  }

  function keyOf(pick) {
    return pick.moduleId ? `mod:${pick.moduleId}` : `${pick.type}:${pick.variant}`;
  }
  function isPicked(pick) {
    return picks.some((p) => keyOf(p) === keyOf(pick));
  }
  function toggle(pick) {
    const key = keyOf(pick);
    picks = isPicked(pick) ? picks.filter((p) => keyOf(p) !== key) : [...picks, pick];
  }

  function back() {
    groupId = null;
    picks = [];
  }
  function cancel() {
    open = false;
  }
  function confirm() {
    if (!picks.length) return;
    onadd(picks);
    open = false;
  }

  /** The visual a size code stands for: a figure, a list, a comparison or a curve. */
  function shapeOf(size) {
    if (size === 'S') return 'stat';
    if (size === 'SR' || size === 'MR') return 'list';
    if (size === 'L' || size === 'LR') return 'chart';
    return 'bars';
  }
</script>

<Modal
  bind:open
  size="lg"
  dismissible={false}
  title={group ? group.name : title || $t('dashboard.widgets.picker.title')}
>
  {#if !group}
    <p class="lead">{$t('dashboard.widgets.picker.chooseModule')}</p>
    <div class="grid mods">
      {#each groups as g (g.id)}
        <button class="mod" onclick={() => (groupId = g.id)}>
          <span class="mi"><Icon name={g.icon} size={18} /></span>
          <span class="mn">{g.name}</span>
          <span class="mc">{$t('dashboard.widgets.picker.available', { count: countOf(g) })}</span>
          <span class="mg"><Icon name="chevron-right" size={14} /></span>
        </button>
      {/each}
    </div>
  {:else if isLinks}
    <p class="lead">{$t('dashboard.widgets.picker.moduleLinksBlurb')}</p>
    <div class="grid links">
      {#each mods as m (m.id)}
        {@const pick = { moduleId: m.id }}
        <button
          class="card link"
          class:on={isPicked(pick)}
          aria-pressed={isPicked(pick)}
          onclick={() => toggle(pick)}
        >
          <span class="ci"><Icon name={m.icon} size={18} /></span>
          <span class="cn">{m.name}</span>
          {#if isPicked(pick)}<span class="tick"><Icon name="check" size={12} /></span>{/if}
        </button>
      {/each}
    </div>
  {:else}
    <div class="grid cards">
      {#each cards as c (c.key)}
        {@const pick = { type: c.type, variant: c.variant }}
        {@const shape = shapeOf(c.size)}
        <button
          class="card"
          class:on={isPicked(pick)}
          aria-pressed={isPicked(pick)}
          style:--pv={c.tone ? `var(--${c.tone})` : 'var(--accent)'}
          onclick={() => toggle(pick)}
        >
          <!-- A drawing of the widget, not a live one: the shape its size code stands for,
               so a page of them is chosen by look rather than by name. -->
          <span class="pv" aria-hidden="true">
            {#if shape === 'stat'}
              <span class="pv-num"></span>
              <span class="pv-sub"></span>
            {:else if shape === 'list'}
              {#each [0, 1, 2, 3] as r (r)}
                <span class="pv-row"><i class="pv-dot"></i><i class="pv-bar"></i><i class="pv-val"></i></span>
              {/each}
            {:else if shape === 'bars'}
              <span class="pv-bars">
                {#each [40, 72, 55, 88, 30] as h (h)}<i style:height={`${h}%`}></i>{/each}
              </span>
            {:else}
              <svg class="pv-chart" viewBox="0 0 100 40" preserveAspectRatio="none">
                <path d="M0 32 L18 24 L34 28 L52 12 L70 18 L86 6 L100 10 L100 40 L0 40 Z" />
                <polyline points="0,32 18,24 34,28 52,12 70,18 86,6 100,10" />
              </svg>
            {/if}
          </span>
          <span class="cinfo">
            <span class="ci"><Icon name={c.icon} size={15} /></span>
            <span class="cn">{c.label}</span>
            <span class="cs">{c.size}</span>
          </span>
          {#if c.subtitle}<span class="cb">{c.subtitle}</span>{/if}
          {#if isPicked(pick)}<span class="tick"><Icon name="check" size={12} /></span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  {#snippet footer()}
    <div class="foot">
      {#if group}
        <Button variant="ghost" size="sm" icon="arrow-left" onclick={back}>
          {$t('common.back')}
        </Button>
        <span class="sel">
          {picks.length ? $t('dashboard.widgets.picker.selected', { count: picks.length }) : ''}
        </span>
      {:else}
        <span class="sel"></span>
      {/if}
      <Button variant="ghost" size="sm" onclick={cancel}>{$t('common.cancel')}</Button>
      {#if group}
        <Button variant="solid" size="sm" icon="plus" onclick={confirm} disabled={!picks.length}>
          {$t('common.add')}
        </Button>
      {/if}
    </div>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .grid {
    display: grid;
    gap: var(--space-3);
  }
  .grid.mods {
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  }
  .grid.cards {
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  }
  .grid.links {
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  }

  .mod {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 2px var(--space-3);
    text-align: left;
    padding: var(--space-3);
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-family: inherit;
  }
  .mod:hover {
    background: var(--surface-2);
    border-color: var(--border-strong);
  }
  .mi {
    grid-row: 1 / 3;
    display: inline-flex;
    color: var(--accent);
  }
  .mn {
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
  }
  .mc {
    grid-column: 2;
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .mg {
    grid-row: 1 / 3;
    grid-column: 3;
    display: inline-flex;
    color: var(--dim);
  }

  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    text-align: left;
    padding: var(--space-3);
    background: var(--bg);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-family: inherit;
  }
  .card:hover {
    background: var(--surface-2);
  }
  .card.on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, var(--bg));
  }
  .card.link {
    flex-direction: row;
    align-items: center;
    gap: var(--space-3);
  }
  .cinfo {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: var(--space-2);
  }
  .ci {
    display: inline-flex;
    color: var(--muted);
  }
  .cn {
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cs {
    font-size: 10px;
    color: var(--dim);
    font-variant-numeric: tabular-nums;
  }
  .cb {
    font-size: var(--text-xs);
    color: var(--dim);
    line-height: 1.35;
  }
  .tick {
    position: absolute;
    top: var(--space-2);
    right: var(--space-2);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--bg);
  }

  /* Widget drawing: a flat sketch on the card's own surface, no frame of its own. */
  .pv {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    height: 66px;
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    overflow: hidden;
  }
  .pv-num {
    width: 46%;
    height: 16px;
    border-radius: 3px;
    background: var(--pv);
    opacity: 0.85;
  }
  .pv-sub {
    width: 28%;
    height: 6px;
    border-radius: 3px;
    background: var(--border-strong);
  }
  .pv-row {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .pv-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--pv);
    opacity: 0.75;
  }
  .pv-bar {
    flex: 1;
    height: 5px;
    border-radius: 3px;
    background: var(--border-strong);
  }
  .pv-val {
    width: 18%;
    height: 5px;
    border-radius: 3px;
    background: var(--pv);
    opacity: 0.55;
  }
  .pv-bars {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    height: 100%;
  }
  .pv-bars i {
    flex: 1;
    border-radius: 2px 2px 0 0;
    background: var(--pv);
    opacity: 0.7;
  }
  .pv-chart {
    width: 100%;
    height: 100%;
  }
  .pv-chart path {
    fill: var(--pv);
    opacity: 0.18;
  }
  .pv-chart polyline {
    fill: none;
    stroke: var(--pv);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }

  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
  }
  .sel {
    flex: 1;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
