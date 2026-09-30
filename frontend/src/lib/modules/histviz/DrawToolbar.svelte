<script>
  // The drawing rail, pinned to the left of the chart the way every charting tool puts it.
  // It owns no state: the active tool and the current selection live with the chart, so the
  // rail is a row of radio buttons plus a delete for whatever is selected.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { DRAW_TOOLS } from './drawings.js';

  let {
    tool = $bindable('cursor'),
    magnet = $bindable(false),
    hidden = $bindable(false),
    count = 0,
    hasSelection = false,
    ondelete,
    onclear,
    onundo
  } = $props();
</script>

<div class="rail" role="toolbar" aria-label={$t('histviz.draw.title')}>
  {#each DRAW_TOOLS as d (d.id)}
    <button
      type="button"
      class="tool"
      class:on={tool === d.id}
      title={$t(d.labelKey)}
      aria-label={$t(d.labelKey)}
      aria-pressed={tool === d.id}
      onclick={() => (tool = d.id)}
    >
      <Icon name={d.icon} size={15} />
    </button>
  {/each}

  <span class="sep" aria-hidden="true"></span>

  <!-- Magnet: anchors land on the nearest O/H/L/C instead of wherever the cursor was. -->
  <button
    type="button"
    class="tool"
    class:on={magnet}
    aria-pressed={magnet}
    title={$t('histviz.draw.magnet')}
    aria-label={$t('histviz.draw.magnet')}
    onclick={() => (magnet = !magnet)}
  >
    <Icon name="magnet" size={15} />
  </button>
  <button
    type="button"
    class="tool"
    class:on={hidden}
    aria-pressed={hidden}
    title={hidden ? $t('histviz.draw.show') : $t('histviz.draw.hide')}
    aria-label={hidden ? $t('histviz.draw.show') : $t('histviz.draw.hide')}
    onclick={() => (hidden = !hidden)}
  >
    <Icon name={hidden ? 'eye-off' : 'eye'} size={15} />
  </button>

  <span class="sep" aria-hidden="true"></span>

  <!-- Undo: drops the shape drawn last, the same thing Ctrl/Cmd+Z does on the chart. -->
  <button
    type="button"
    class="tool"
    disabled={!count}
    title={$t('histviz.draw.undo')}
    aria-label={$t('histviz.draw.undo')}
    onclick={() => onundo?.()}
  >
    <Icon name="undo" size={15} />
  </button>
  <button
    type="button"
    class="tool danger"
    disabled={!hasSelection}
    title={$t('histviz.draw.remove')}
    aria-label={$t('histviz.draw.remove')}
    onclick={() => ondelete?.()}
  >
    <Icon name="trash" size={14} />
  </button>
  <button
    type="button"
    class="tool danger"
    disabled={!count}
    title={$t('histviz.draw.clear')}
    aria-label={$t('histviz.draw.clear')}
    onclick={() => onclear?.()}
  >
    <Icon name="x" size={15} />
    {#if count}<span class="badge">{count}</span>{/if}
  </button>
</div>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: var(--space-2) 3px;
    border-right: 1px solid var(--border);
    background: var(--surface);
    flex: none;
  }
  .tool {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    transition: color 0.12s ease, background-color 0.12s ease, border-color 0.12s ease;
  }
  .tool:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .tool.on {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
    background: var(--surface-2);
  }
  .tool:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
  .tool.danger:hover:not(:disabled) {
    color: var(--red);
  }
  .sep {
    width: 18px;
    height: 1px;
    margin: var(--space-1) 0;
    background: var(--border);
  }
  .badge {
    position: absolute;
    bottom: 0;
    right: 0;
    font-size: 9px;
    line-height: 1;
    color: var(--muted);
  }
</style>
