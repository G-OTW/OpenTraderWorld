<script>
  // Frame for a single dashboard widget: a titled card that lazy-loads the widget's own
  // component from the registry. Gives every widget a consistent header (icon + title +
  // an optional redirect link to the owning module) and, in edit mode, a config gear and
  // remove button. The body scrolls internally so a widget never breaks the grid.
  import Icon from '$lib/ui/Icon.svelte';
  import { moduleById } from '$lib/modules/registry';
  import { widgetByType } from './registry.js';
  import { t } from '$lib/i18n';

  let {
    item,          // { id, type, span, config } — config is bindable via item
    editing = false,
    onconfig = null, // () => void, open config editor (edit mode)
    onremove = null  // () => void, remove this widget (edit mode)
  } = $props();

  const def = $derived(widgetByType(item.type));
  const mod = $derived(def?.moduleId ? moduleById(def.moduleId) : null);
  // Title: widget's own configured title (free text) falls back to the def label.
  const title = $derived(item.config?.title?.trim() || def?.label || $t('dashboard.widgets.shell.widget'));


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

<div class="widget" class:editing>
  <header class="whead">
    <span class="wtitle">
      <Icon name={def?.icon ?? 'grid'} size={15} />
      <span class="wtxt">{title}</span>
    </span>
    <span class="wtools">
      {#if editing}
        {#if onconfig}
          <button class="wbtn" title={$t('dashboard.widgets.shell.configure')} onclick={onconfig}><Icon name="settings" size={14} /></button>
        {/if}
        {#if onremove}
          <button class="wbtn danger" title={$t('dashboard.widgets.shell.remove')} onclick={onremove}><Icon name="x" size={14} /></button>
        {/if}
      {:else}
        {#if onconfig}
          <button class="wbtn" title={$t('dashboard.widgets.shell.configure')} onclick={onconfig}><Icon name="settings" size={14} /></button>
        {/if}
        {#if mod}
          <a class="wbtn" href={mod.base} title={$t('dashboard.widgets.shell.open', { name: mod.name })}><Icon name="external-link" size={14} /></a>
        {/if}
      {/if}
    </span>
  </header>

  <div class="wbody" class:noscroll={editing}>
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
    background: var(--bg);
    border: 0.5px solid var(--border);
    border-radius: 0;
    height: 100%;
    overflow: hidden;
  }
  .widget.editing {
    cursor: grab;
  }
  .whead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-bottom: 0.5px solid var(--border);
    flex-shrink: 0;
  }
  .wtitle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    color: var(--text);
    font-weight: var(--fw-medium);
    font-size: 13.5px;
    letter-spacing: 0.02em;
  }
  .wtitle :global(svg) {
    color: var(--muted);
    flex-shrink: 0;
  }
  .wtxt {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .wtools {
    display: flex;
    align-items: center;
    gap: var(--space-1, 4px);
    flex-shrink: 0;
  }
  .wbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 0;
    border: 0.5px solid transparent;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    text-decoration: none;
  }
  .wbtn:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .wbtn.danger:hover {
    color: var(--red);
  }
  .wbody {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--space-4);
  }
  .wbody.noscroll {
    overflow: hidden;
    pointer-events: none;
  }
  .werr {
    color: var(--red);
    font-size: var(--text-sm);
  }
  .wmuted {
    color: var(--muted);
    font-size: var(--text-base);
  }
</style>
