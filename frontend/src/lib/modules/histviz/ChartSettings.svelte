<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  // Chart display settings popover. Edits the shared `settings` object (bindable); the
  // parent persists it server-side (fixed global config). Colors default to the theme
  // when left blank — a "reset" clears the override back to theme.
  import { DEFAULT_SETTINGS } from './settings.js';

  let { settings = $bindable(), onclose } = $props();

  // Native <input type=color> needs a concrete hex; resolve the theme value for the swatch
  // while keeping the stored value '' (meaning "use theme").
  function themeVar(name, fallback) {
    if (typeof window === 'undefined') return fallback;
    const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return v || fallback;
  }
  const themed = {
    up: () => themeVar('--green', '#26a69a'),
    down: () => themeVar('--red', '#ef5350'),
    line: () => themeVar('--accent', '#4a90d9')
  };
</script>

<div class="popover">
  <div class="head">
    <span>{$t('histviz.settings.title')}</span>
    <button class="x" title={$t('histviz.settings.close')} onclick={() => onclose?.()}><Icon name="x" size={13} /></button>
  </div>

  <label class="field">
    <span>{$t('histviz.settings.priceScale')}</span>
    <select bind:value={settings.scale}>
      <option value="linear">{$t('histviz.settings.linear')}</option>
      <option value="log">{$t('histviz.settings.logarithmic')}</option>
    </select>
  </label>

  <label class="toggle">
    <input type="checkbox" bind:checked={settings.grid} />
    <span>{$t('histviz.settings.gridH')}</span>
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={settings.gridV} />
    <span>{$t('histviz.settings.gridV')}</span>
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={settings.crosshair} />
    <span>{$t('histviz.settings.crosshair')}</span>
  </label>
  <label class="toggle" class:disabled={!settings.crosshair}>
    <input type="checkbox" bind:checked={settings.crosshairTags} disabled={!settings.crosshair} />
    <span>{$t('histviz.settings.crosshairTags')}</span>
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={settings.tooltip} />
    <span>{$t('histviz.settings.tooltip')}</span>
  </label>

  <div class="sep"></div>

  <div class="color">
    <span>{$t('histviz.settings.upColor')}</span>
    <input
      type="color"
      value={settings.upColor || themed.up()}
      oninput={(e) => (settings.upColor = e.currentTarget.value)}
    />
    {#if settings.upColor}<button class="reset" title={$t('histviz.settings.useTheme')} onclick={() => (settings.upColor = '')}><Icon name="refresh-cw" size={12} /></button>{/if}
  </div>
  <div class="color">
    <span>{$t('histviz.settings.downColor')}</span>
    <input
      type="color"
      value={settings.downColor || themed.down()}
      oninput={(e) => (settings.downColor = e.currentTarget.value)}
    />
    {#if settings.downColor}<button class="reset" title={$t('histviz.settings.useTheme')} onclick={() => (settings.downColor = '')}><Icon name="refresh-cw" size={12} /></button>{/if}
  </div>
  <div class="color">
    <span>{$t('histviz.settings.lineColor')}</span>
    <input
      type="color"
      value={settings.lineColor || themed.line()}
      oninput={(e) => (settings.lineColor = e.currentTarget.value)}
    />
    {#if settings.lineColor}<button class="reset" title={$t('histviz.settings.useTheme')} onclick={() => (settings.lineColor = '')}><Icon name="refresh-cw" size={12} /></button>{/if}
  </div>

  <div class="sep"></div>
  <button class="reset-all" onclick={() => (settings = { ...DEFAULT_SETTINGS })}>{$t('histviz.settings.resetAll')}</button>
</div>

<style>
  .popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    width: 240px;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .head span {
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.8rem;
  }
  .x:hover {
    color: var(--text);
  }
  .field select {
    flex: 1;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.8rem;
    color: var(--text);
    cursor: pointer;
  }
  .toggle.disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .color {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.8rem;
    color: var(--text);
  }
  .color span {
    flex: 1;
  }
  .color input[type='color'] {
    width: 32px;
  }
  .reset {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.85rem;
  }
  .reset:hover {
    color: var(--text);
  }
  .sep {
    border-top: 1px solid var(--border);
  }
  .reset-all {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    color: var(--muted);
    font-size: 0.75rem;
    cursor: pointer;
  }
  .reset-all:hover {
    color: var(--text);
  }
</style>
