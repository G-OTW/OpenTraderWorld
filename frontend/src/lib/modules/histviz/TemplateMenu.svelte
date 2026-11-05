<script>
  // Chart templates menu: apply a saved template to this chart in one click, copy it to other
  // charts, or save this chart's studies and layout under a name (a taken name is replaced).
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  // `at`: viewport position ({ top, right }), drawn fixed so the pane's edge cannot clip it.
  let { templates = [], at = null, onapply, onapplyto, onsave, ondelete } = $props();

  let name = $state('');
  const trimmed = $derived(name.trim());
  const taken = $derived(templates.some((x) => x.name === trimmed));

  function save() {
    if (!trimmed) return;
    onsave?.(trimmed);
    name = '';
  }
  const studies = (tpl) => (tpl.instances ?? []).length;
</script>

<div class="menu" role="menu" style={at ? `position:fixed;top:${at.top}px;right:${at.right}px` : undefined}>
  <p class="head">{$t('histviz.templates.title')}</p>
  {#each templates as tpl (tpl.id)}
    <div class="row">
      <button
        class="go"
        role="menuitem"
        title={$t('histviz.templates.apply')}
        onclick={() => onapply?.(tpl)}
      >
        <span class="name">{tpl.name}</span>
        <span class="meta">{$t('histviz.templates.studies', { n: studies(tpl) })}</span>
      </button>
      <button
        class="act"
        title={$t('histviz.templates.applyTo')}
        aria-label={$t('histviz.templates.applyTo')}
        onclick={() => onapplyto?.(tpl)}
      >
        <Icon name="copy-plus" size={13} />
      </button>
      <button
        class="act danger"
        title={$t('histviz.templates.remove')}
        aria-label={$t('histviz.templates.remove')}
        onclick={() => ondelete?.(tpl)}
      >
        <Icon name="trash-2" size={13} />
      </button>
    </div>
  {:else}
    <p class="empty">{$t('histviz.templates.empty')}</p>
  {/each}
  <div class="sep"></div>
  <form
    class="save"
    onsubmit={(e) => {
      e.preventDefault();
      save();
    }}
  >
    <input bind:value={name} maxlength="60" placeholder={$t('histviz.templates.name')} aria-label={$t('histviz.templates.name')} />
    <button type="submit" class="primary" disabled={!trimmed}>
      {taken ? $t('histviz.templates.replace') : $t('histviz.templates.save')}
    </button>
  </form>
</div>

<style>
  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: var(--z-dropdown);
    width: 280px;
    /* One height whatever the number of templates: more rows scroll, fewer leave room. */
    height: min(78vh, 560px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-lg);
    padding: var(--space-1) 0;
    box-shadow: var(--shadow-3);
  }
  .menu > * {
    flex-shrink: 0;
  }
  .head {
    margin: 0;
    padding: var(--space-2) var(--space-3) var(--space-1);
    color: var(--muted);
    font-size: var(--fs-small, 11px);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .row {
    display: flex;
    align-items: center;
    padding-right: var(--space-2);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .go {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--text);
    font-size: var(--fs-body);
    text-align: left;
    cursor: pointer;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    flex: none;
    color: var(--muted);
    font-size: var(--fs-small, 11px);
  }
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 24px;
    height: 24px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .row:hover .act,
  .act:focus-visible {
    opacity: 1;
  }
  .act:hover {
    color: var(--text);
    background: var(--surface);
  }
  .act.danger:hover {
    color: var(--red);
  }
  .empty {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    font-size: var(--fs-body);
    line-height: 1.4;
  }
  .sep {
    margin: var(--space-1) 0;
    border-top: 1px solid var(--border);
  }
  .save {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
  }
  .save input {
    flex: 1;
    min-width: 0;
  }
  .save button {
    flex: none;
  }
  @media (hover: none) {
    .act {
      opacity: 1;
    }
  }
</style>
