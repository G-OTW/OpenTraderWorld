<script>
  // Every object drawn on this instrument, in one list: what it is, where it sits, and the
  // three things a drawing needs once there are more than five of them, hide, lock and
  // delete. Reordering is the paint order, which is what decides who is on top.
  //
  // The list is the same array the chart draws, so anything done here is on screen before
  // the modal closes.
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';
  import { drawTool } from './drawings.js';

  let {
    open = $bindable(false),
    drawings = $bindable([]),
    selected = $bindable(null),
    /** Formats a price the way this pane's axis does. */
    fmt = (v) => String(v)
  } = $props();

  const label = (d) => d.text?.trim() || $t(drawTool(d.tool).labelKey);
  const where = (d) => {
    const parts = [];
    if (d.a?.y != null) parts.push(fmt(d.a.y));
    if (d.b?.y != null && d.b.y !== d.a?.y) parts.push(fmt(d.b.y));
    return parts.join(' → ');
  };

  const patch = (id, fields) =>
    (drawings = drawings.map((d) => (d.id === id ? { ...d, ...fields } : d)));

  function move(id, delta) {
    const i = drawings.findIndex((d) => d.id === id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= drawings.length) return;
    const next = drawings.slice();
    [next[i], next[j]] = [next[j], next[i]];
    drawings = next;
  }

  function remove(id) {
    drawings = drawings.filter((d) => d.id !== id);
    if (selected === id) selected = null;
  }
</script>

<Modal bind:open title={$t('histviz.objects.title')} size="md">
  <ul class="tree">
    {#each drawings as d, i (d.id)}
      <li class:sel={selected === d.id} class:off={d.hidden}>
        <button class="pick" onclick={() => (selected = d.id)} title={$t('histviz.objects.select')}>
          <span class="swatch" style:background={d.style?.color || 'var(--accent)'}></span>
          <span class="name">{label(d)}</span>
          <span class="at">{where(d)}</span>
        </button>
        <button
          class="icon"
          title={$t(d.hidden ? 'histviz.panel.show' : 'histviz.panel.hide')}
          onclick={() => patch(d.id, { hidden: !d.hidden })}
        >
          <Icon name={d.hidden ? 'eye-off' : 'eye'} size={12} />
        </button>
        <button
          class="icon"
          class:on={d.locked}
          title={$t(d.locked ? 'histviz.objects.unlock' : 'histviz.objects.lock')}
          onclick={() => patch(d.id, { locked: !d.locked })}
        >
          <Icon name={d.locked ? 'lock' : 'unlock'} size={12} />
        </button>
        <button
          class="icon"
          disabled={i === 0}
          title={$t('histviz.objects.up')}
          onclick={() => move(d.id, -1)}
        >
          <Icon name="chevron-up" size={12} />
        </button>
        <button
          class="icon"
          disabled={i === drawings.length - 1}
          title={$t('histviz.objects.down')}
          onclick={() => move(d.id, 1)}
        >
          <Icon name="chevron-down" size={12} />
        </button>
        <button class="icon danger" title={$t('common.remove')} onclick={() => remove(d.id)}>
          <Icon name="trash-2" size={12} />
        </button>
      </li>
    {:else}
      <li class="empty">{$t('histviz.objects.empty')}</li>
    {/each}
  </ul>
</Modal>

<style>
  .tree {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .tree li {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px 0;
    border-bottom: 1px solid var(--border);
  }
  .tree li.sel {
    background: var(--surface-2);
  }
  .tree li.off {
    opacity: 0.55;
  }
  .tree li.empty {
    color: var(--muted);
    font-size: 0.76rem;
    border: 0;
    padding: var(--space-2) 0;
  }
  .pick {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 3px var(--space-1);
    background: none;
    border: 0;
    color: var(--text);
    cursor: pointer;
    text-align: left;
    font-size: 0.78rem;
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    flex: none;
  }
  .name {
    font-weight: 600;
  }
  .at {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 0.72rem;
  }
  .icon {
    display: inline-flex;
    padding: 3px;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border);
  }
  .icon:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .icon.on {
    color: var(--amber);
  }
  .icon.danger:hover {
    color: var(--red);
    border-color: var(--red);
  }
</style>
