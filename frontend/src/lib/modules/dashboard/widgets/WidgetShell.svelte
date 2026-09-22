<script>
  // Frame for a single dashboard widget: a titled card that lazy-loads the widget's own
  // component from the registry. Every widget gets the same two corner affordances — the
  // overflow control that opens its configuration, and a link to the module it previews —
  // plus an icon and a title. The pair is always present but drawn a rank below the data,
  // brightening on approach, so a grid of widgets still reads as figures rather than as a
  // field of buttons. The body scrolls internally so a widget never breaks the grid.
  import Icon from '$lib/ui/Icon.svelte';
  import { moduleById } from '$lib/modules/registry';
  import { widgetByType, widgetVariant } from './registry.js';
  import { t } from '$lib/i18n';
  import { privacy } from '$lib/theme/privacy.svelte.js';

  let {
    item,          // { id, type, span, config } — config is bindable via item
    editing = false,
    onconfig = null, // () => void, open config editor (edit mode)
    onremove = null  // () => void, remove this widget (edit mode)
  } = $props();

  const def = $derived(widgetByType(item.type));
  const mod = $derived(def?.moduleId ? moduleById(def.moduleId) : null);
  const referenceTypes = new Set([
    'wealth', 'subscriptions', 'watchlists', 'goals', 'todos',
    'time', 'routines', 'mindset', 'remindme-stats', 'mailbox',
    'resources', 'agent', 'agent-stats', 'backtest', 'automator', 'editor',
    'calendar', 'findb', 'histdata', 'quant', 'webhooks',
    'news', 'economics', 'prompts', 'community-docs', 'histviz', 'mportfolios'
  ]);
  const isReferenceWidget = $derived(referenceTypes.has(item.type));
  const activeVariant = $derived(widgetVariant(def, item.config));
  // Every widget names the data presentation it's showing (e.g. "Cash vs invested"),
  // not just its owning module. Only the reference cards additionally get the
  // card-specific subtitle/tone below.
  const title = $derived(
    item.config?.title?.trim() ||
      activeVariant?.label ||
      def?.label ||
      $t('dashboard.widgets.shell.widget')
  );
  const subtitle = $derived(isReferenceWidget ? activeVariant?.subtitle : '');
  const tone = $derived(isReferenceWidget ? activeVariant?.tone ?? 'blue' : '');
  // The global privacy eye blurs the figures, never the chrome: the widget keeps its
  // title and tools so the dashboard stays navigable with the data screened off.
  // A section heading carries no data, so it is left alone.
  const masked = $derived(privacy.hidden && item.type !== 'text');


  // Lazily resolve the widget component.
  let Comp = $state(null);
  let loadErr = $state('');
  $effect(() => {
    if (!def) {
      loadErr = $t('dashboard.widgets.shell.unknown', { type: item.type });
      return;
    }
    let alive = true;
    def
      .loader()
      .then((m) => {
        if (alive) Comp = m.default;
      })
      .catch((e) => {
        if (alive) loadErr = e.message;
      });
    return () => (alive = false);
  });
</script>

<div
  class="widget otw-widget"
  class:editing
  class:reference-widget={isReferenceWidget}
  class:section-heading={item.type === 'text' && item.config?.height === 'title'}
  data-widget-type={isReferenceWidget ? item.type : undefined}
  data-widget-tone={tone || undefined}
>
  <header class="whead">
    <span class="wtitle">
      <span class="wicon"><Icon name={def?.icon ?? 'grid'} size={isReferenceWidget ? 17 : 15} /></span>
      <span class="wtitle-copy">
        <span class="wtxt">{title}</span>
        {#if subtitle}<span class="wsubtitle">{subtitle}</span>{/if}
      </span>
    </span>
    <span class="wtools">
      {#if editing}
        {#if onconfig}
          <button class="wbtn configure" title={$t('dashboard.widgets.shell.configure')} aria-label={$t('dashboard.widgets.shell.configure')} aria-haspopup="dialog" onclick={onconfig}><Icon name="more-horizontal" size={16} /></button>
        {/if}
        {#if onremove}
          <button class="wbtn danger" title={$t('dashboard.widgets.shell.remove')} onclick={onremove}><Icon name="x" size={14} /></button>
        {/if}
      {:else}
        {#if onconfig}
          <button class="wbtn configure" title={$t('dashboard.widgets.shell.configure')} aria-label={$t('dashboard.widgets.shell.configure')} aria-haspopup="dialog" onclick={onconfig}><Icon name="more-horizontal" size={16} /></button>
        {/if}
        {#if mod}
          <a class="wbtn" href={mod.base} title={$t('dashboard.widgets.shell.open', { name: mod.name })}><Icon name="external-link" size={14} /></a>
        {/if}
      {/if}
    </span>
  </header>

  <div class="wbody" class:noscroll={editing} class:masked aria-hidden={masked || undefined}>
    {#if loadErr}
      <p class="werr">{loadErr}</p>
    {:else if Comp}
      <Comp {item} {editing} />
    {:else}
      <p class="wmuted">…</p>
    {/if}
  </div>
</div>

<style>
  .widget {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: none;
    height: 100%;
    min-width: 0;
    max-width: 100%;
    overflow: hidden;
    transition: border-color var(--dur-fast) var(--ease);
  }
  .widget.editing {
    cursor: grab;
  }
  /* Privacy screen: the body stays laid out (nothing reloads when it comes back) but is
     blurred out and inert, so no figure is legible and nothing inside can be clicked. */
  .wbody.masked {
    filter: blur(9px);
    opacity: 0.5;
    pointer-events: none;
    user-select: none;
  }
  .widget:hover {
    border-color: var(--border-strong);
  }
  /* Dashboard layout uses title-height text widgets as section dividers. They retain the
     same lazy widget contract, but should read as hierarchy rather than as an empty card. */
  .widget.section-heading {
    min-height: 34px;
    background: transparent;
    border-color: transparent;
    border-radius: 0;
    box-shadow: none;
    overflow: visible;
  }
  .whead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 13px var(--space-4) 0;
    border-bottom: 0;
    flex-shrink: 0;
    min-width: 0;
  }
  .section-heading .whead {
    min-height: 34px;
    padding: 0 2px;
    border-bottom: 0;
  }
  /* A section heading is a label for the row beneath it, not a widget with a name. */
  .section-heading .wtitle {
    color: var(--dim);
    font-size: 10px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .section-heading .wtitle :global(svg) {
    color: var(--faint);
  }
  .section-heading .wbody {
    display: none;
  }
  .wtitle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    color: var(--text);
    font-weight: var(--fw-medium);
    font-size: var(--fs-item-title);
    letter-spacing: 0.01em;
    line-height: var(--lh-tight);
  }
  .wicon {
    display: inline-flex;
    flex-shrink: 0;
  }
  .wtitle-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  /* The icon identifies the owning module; it is orientation, not decoration, so it
     stays a rank below the title it labels. */
  .wtitle :global(svg) {
    color: var(--faint);
    flex-shrink: 0;
  }
  /* Widgets 01–27 use the supplied reference language: a small coloured orientation
     tile, precise title, and a quiet explanatory second line. Other modules deliberately
     keep the neutral shell until their own reference pass. */
  .reference-widget .whead {
    padding: var(--space-4) var(--space-4) 0;
  }
  /* The supplied compact cards are deliberately taller than OTW's old 150px utility
     tile: the second header line and a meaningful chart/donut need room to breathe.
     This is scoped to the reference ports, leaving the remaining legacy widgets
     at their current density until they receive their own visual pass. */
  .reference-widget {
    min-height: 184px;
  }
  .reference-widget .wtitle {
    gap: 11px;
    font-size: 14px;
    font-weight: 650;
    letter-spacing: -0.02em;
  }
  .reference-widget .wtxt {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    white-space: normal;
  }
  .reference-widget .wicon {
    width: 36px;
    height: 36px;
    align-items: center;
    justify-content: center;
    border-radius: 9px;
    color: var(--accent);
    background: var(--accent-soft);
  }
  .reference-widget[data-widget-tone='green'] .wicon {
    color: var(--green);
    background: var(--positive-soft);
  }
  .reference-widget[data-widget-tone='amber'] .wicon {
    color: var(--amber);
    background: var(--warning-soft);
  }
  .reference-widget[data-widget-tone='red'] .wicon {
    color: var(--red);
    background: var(--negative-soft);
  }
  .reference-widget .wtitle :global(svg),
  .reference-widget:hover .wtitle :global(svg) {
    color: currentColor;
  }
  .wsubtitle {
    overflow: hidden;
    color: var(--dim);
    font-size: 10px;
    font-weight: var(--fw-normal);
    letter-spacing: 0;
    line-height: 1.15;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .widget:hover .wtitle :global(svg) {
    color: var(--dim);
  }
  .wtxt {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .wtools {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
    /* Present at rest, quiet enough not to compete with the figures beside them. */
    opacity: 0.5;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .widget:hover .wtools,
  .widget.editing .wtools,
  .wtools:focus-within {
    opacity: 1;
  }
  @media (hover: none) {
    .wtools {
      opacity: 1;
    }
  }
  .wbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-sm);
    border: 0;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    text-decoration: none;
    transition: color var(--dur-fast) var(--ease), background-color var(--dur-fast) var(--ease);
  }
  .wbtn:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  /* The config affordance reads as accent only once you reach for it: a permanent blue
     chip on every widget turns the grid into a field of buttons. */
  .wbtn.configure:hover {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .wbtn:focus-visible {
    outline: none;
    box-shadow: var(--ring);
  }
  .wbtn.danger:hover {
    color: var(--red-ink);
    background: var(--negative-soft);
  }
  .wbody {
    flex: 1;
    min-height: 0;
    /* Widget content adapts to the card it was given, not to the viewport: a body that
       stacks two halves at a third of the page puts them side by side at full width.
       Named so a widget's own rule can query `widget` without matching some other
       container that happens to sit between them. */
    container-type: inline-size;
    container-name: widget;
    overflow-y: auto;
    overflow-x: hidden;
    /* Scroll the widget first, then chain to the page once it hits its end. */
    overscroll-behavior: auto;
    scrollbar-gutter: stable;
    padding: var(--w-pad-top) var(--space-4) var(--space-4);
    /* Widget primitives bleed their hairlines and hover bands to this padding edge. */
    --w-bleed: var(--space-4);
    --w-pad-top: 12px;
  }
  .reference-widget .wbody {
    --w-pad-top: 14px;
  }
  .wbody.noscroll {
    overflow: hidden;
    pointer-events: none;
  }
  .werr {
    color: var(--red-ink);
    font-size: var(--text-sm);
  }
  .wmuted {
    color: var(--faint);
    font-size: var(--text-sm);
  }
</style>
