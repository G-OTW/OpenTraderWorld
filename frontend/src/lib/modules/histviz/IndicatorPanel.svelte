<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  // Left-side list of active indicator instances. Each row: name, eye (show/hide),
  // edit, delete. An "+ Add" button opens the modal in add mode. Parent owns the list.
  import { instanceLabel, catalogDef } from './indicators.js';

  let { instances = [], onadd, onedit, ontoggle, onremove } = $props();

  function kindBadge(type) {
    return catalogDef(type)?.kind === 'overlay' ? 'overlay' : 'pane';
  }
</script>

<div class="panel">
  <div class="head">
    <span>{$t('histviz.panel.title')}</span>
    <button class="add" onclick={() => onadd?.()}>
      <Icon name="plus" size={12} strokeWidth={2.5} /> {$t('histviz.panel.add')}
    </button>
  </div>

  {#if !instances.length}
    <p class="empty">{$t('histviz.panel.empty')}</p>
  {/if}

  {#each instances as ind (ind.id)}
    <div class="row {kindBadge(ind.type)}" class:hidden={!ind.visible}>
      <button
        class="act eye"
        title={ind.visible ? $t('histviz.panel.hide') : $t('histviz.panel.show')}
        onclick={() => ontoggle?.(ind.id)}
      >
        <Icon name={ind.visible ? 'eye' : 'eye-off'} size={13} />
      </button>
      <div class="body">
        <span class="name">{instanceLabel(ind.type, ind.params)}</span>
        <span class="badge {kindBadge(ind.type)}">{kindBadge(ind.type) === 'overlay' ? $t('histviz.modal.kindOverlay') : $t('histviz.modal.kindPane')}</span>
      </div>
      <div class="acts">
        <button class="act" title={$t('histviz.panel.edit')} onclick={() => onedit?.(ind)}><Icon name="pencil" size={12} /></button>
        <button class="act danger" title={$t('histviz.panel.remove')} onclick={() => onremove?.(ind.id)}><Icon name="x" size={13} /></button>
      </div>
    </div>
  {/each}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .head > span {
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 var(--space-3);
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--accent);
    font-size: 0.75rem;
    font-weight: 600;
    line-height: 1;
    cursor: pointer;
    transition: background-color 0.12s ease, border-color 0.12s ease;
  }
  .add:hover {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    border-color: color-mix(in srgb, var(--accent) 60%, transparent);
  }
  .empty {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    padding-left: calc(var(--space-2) + 3px);
    overflow: hidden;
    transition: border-color 0.12s ease, background-color 0.12s ease;
  }
  /* Kind rail: overlay = accent, pane = amber (matches the badge hue). */
  .row::before {
    content: '';
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 3px;
    background: var(--accent);
    transition: background-color 0.12s ease;
  }
  .row.pane::before {
    background: var(--amber);
  }
  .row:hover {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .row.hidden::before {
    background: var(--border);
  }
  .row.hidden .name {
    opacity: 0.45;
  }
  .row.hidden .badge {
    opacity: 0.55;
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .name {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: opacity 0.12s ease;
  }
  .badge {
    align-self: flex-start;
    text-transform: uppercase;
    font-size: 0.58rem;
    font-weight: 600;
    letter-spacing: 0.07em;
    line-height: 1;
    padding: 3px 7px;
    border-radius: 999px;
    transition: opacity 0.12s ease;
  }
  .badge.overlay {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
  }
  .badge.pane {
    color: var(--amber);
    background: color-mix(in srgb, var(--amber) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--amber) 25%, transparent);
  }
  .acts {
    display: flex;
    gap: 1px;
  }
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    flex-shrink: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    transition: background-color 0.12s ease, color 0.12s ease;
  }
  .act:hover {
    background: var(--surface);
    color: var(--text);
  }
  .act.danger:hover {
    background: color-mix(in srgb, var(--red) 12%, transparent);
    color: var(--red);
  }
</style>
