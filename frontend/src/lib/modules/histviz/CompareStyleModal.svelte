<script>
  // Style of one compared series: its color, width and line style. The source series keeps
  // the chart's own style; only the comparison line is edited here. Edits apply live
  // (`onpatch`), so the chart behind the modal is the preview.
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';
  import { LINE_STYLES } from './settings.js';

  let { open = $bindable(false), series = null, color = '', onpatch, onclose } = $props();
</script>

<Modal bind:open title={series?.ticker ?? ''} size="sm" {onclose}>
  {#if series}
    <div class="style-grid">
      <label class="field">
        <span>{$t('histviz.modal.line')}</span>
        <input
          type="color"
          value={series.color || color || '#c9a45c'}
          oninput={(e) => onpatch?.({ color: e.currentTarget.value })}
        />
      </label>
      <label class="field">
        <span>{$t('histviz.modal.width')}</span>
        <input
          type="number"
          min="0.5"
          max="8"
          step="0.5"
          placeholder={$t('histviz.modal.auto')}
          value={series.width ?? ''}
          oninput={(e) => {
            const w = Number(e.currentTarget.value);
            onpatch?.({ width: w > 0 ? Math.min(8, w) : null });
          }}
        />
      </label>
      <div class="field">
        <span>{$t('histviz.modal.lineType')}</span>
        <div class="seg">
          {#each LINE_STYLES as ls (ls)}
            <button
              type="button"
              class:on={(series.lineType || 'solid') === ls}
              onclick={() => onpatch?.({ lineType: ls })}
            >
              {$t(`histviz.settings.line${ls[0].toUpperCase()}${ls.slice(1)}`)}
            </button>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</Modal>

<style>
  .style-grid {
    display: flex;
    gap: var(--space-4);
    align-items: flex-start;
    flex-wrap: wrap;
    --style-h: 32px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  input[type='color'] {
    width: 44px;
    height: var(--style-h);
    padding: 2px;
    box-sizing: border-box;
  }
  input[type='number'] {
    width: 70px;
    height: var(--style-h);
    box-sizing: border-box;
  }
  .seg {
    display: flex;
    height: var(--style-h);
  }
  .seg button {
    height: var(--style-h);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .seg button + button {
    border-left: none;
  }
  .seg button.on {
    color: var(--text);
    border-color: var(--border-control);
    background: var(--surface);
  }
</style>
