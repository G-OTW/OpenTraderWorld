<script>
  // Assigned color picker: the chart palette's eight slots (they follow the light and dark
  // themes) plus "inherit". A slot is stored as `chart-N`, never as a hex, so a theme switch
  // repaints every hive, signal and item with it.
  import { t } from '$lib/i18n';
  import { cssColor } from './board.js';

  let { value = $bindable(''), inherit = true, label = '' } = $props();
  const SLOTS = Array.from({ length: 8 }, (_, i) => `chart-${i + 1}`);
</script>

<div class="sw" role="radiogroup" aria-label={label || $t('scanner.color')}>
  {#if inherit}
    <button
      type="button"
      role="radio"
      aria-checked={!value}
      class="dot none"
      class:on={!value}
      title={$t('scanner.colorInherit')}
      onclick={() => (value = '')}
    ></button>
  {/if}
  {#each SLOTS as s (s)}
    <button
      type="button"
      role="radio"
      aria-checked={value === s}
      class="dot"
      class:on={value === s}
      style="--c:{cssColor(s)}"
      title={s}
      aria-label={s}
      onclick={() => (value = s)}
    ></button>
  {/each}
</div>

<style>
  .sw {
    display: inline-flex;
    gap: var(--space-1);
    flex-wrap: wrap;
    align-items: center;
  }
  .dot {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid var(--surface);
    background: var(--c);
    box-shadow: 0 0 0 var(--hairline) var(--border-control);
    padding: 0;
    cursor: pointer;
  }
  .dot.none {
    background: repeating-linear-gradient(45deg, var(--surface-2) 0 3px, var(--surface) 3px 6px);
  }
  .dot.on {
    box-shadow: 0 0 0 2px var(--text);
  }
  .dot:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
