<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
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
    up: () => themeVar('--green', '#7fb894'),
    down: () => themeVar('--red', '#c9776b'),
    line: () => themeVar('--accent', '#c9a45c')
  };
</script>

<div class="popover">
  <div class="head">
    <span>{$t('histviz.settings.title')}</span>
    <button class="x" title={$t('histviz.settings.close')} onclick={() => onclose?.()}
      ><Icon name="x" size={13} /></button
    >
  </div>

  <p class="grp">{$t('histviz.settings.groupScale')}</p>
  <!-- `float` on both: the popover scrolls its own overflow, and a scrolling ancestor clips
       an absolutely positioned menu down to a sliver. -->
  <div class="field">
    <span>{$t('histviz.settings.priceScale')}</span>
    <Dropdown
      bind:value={settings.scale}
      ariaLabel={$t('histviz.settings.priceScale')}
      float
      options={[
        { value: 'linear', label: $t('histviz.settings.linear') },
        { value: 'log', label: $t('histviz.settings.logarithmic') }
      ]}
    />
  </div>
  <div class="field">
    <span>{$t('histviz.settings.yAxisSide')}</span>
    <Dropdown
      bind:value={settings.yAxisSide}
      ariaLabel={$t('histviz.settings.yAxisSide')}
      float
      options={[
        { value: 'left', label: $t('histviz.settings.left') },
        { value: 'right', label: $t('histviz.settings.right') }
      ]}
    />
  </div>

  <p class="grp">{$t('histviz.settings.groupChart')}</p>
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
  <label class="toggle sub" class:disabled={!settings.crosshair}>
    <input type="checkbox" bind:checked={settings.crosshairTags} disabled={!settings.crosshair} />
    <span>{$t('histviz.settings.crosshairTags')}</span>
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={settings.tooltip} />
    <span>{$t('histviz.settings.tooltip')}</span>
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={settings.lastPrice} />
    <span>{$t('histviz.settings.lastPrice')}</span>
  </label>
  <label class="toggle sub" class:disabled={!settings.lastPrice}>
    <input type="checkbox" bind:checked={settings.countdown} disabled={!settings.lastPrice} />
    <span>{$t('histviz.settings.countdown')}</span>
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={settings.daySeparators} />
    <span>{$t('histviz.settings.daySeparators')}</span>
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={settings.prevClose} />
    <span>{$t('histviz.settings.prevClose')}</span>
  </label>

  <p class="grp">{$t('histviz.settings.groupColors')}</p>
  <!-- The reset keeps its slot whether or not an override exists: a colour row must not
       change width the moment a colour is picked. -->
  <div class="color">
    <span>{$t('histviz.settings.upColor')}</span>
    <span class="ctl">
      <input
        type="color"
        aria-label={$t('histviz.settings.upColor')}
        value={settings.upColor || themed.up()}
        oninput={(e) => (settings.upColor = e.currentTarget.value)}
      />
      <button
        class="reset"
        hidden={!settings.upColor}
        title={$t('histviz.settings.useTheme')}
        aria-label={$t('histviz.settings.useTheme')}
        onclick={() => (settings.upColor = '')}><Icon name="refresh-cw" size={12} /></button
      >
    </span>
  </div>
  <div class="color">
    <span>{$t('histviz.settings.downColor')}</span>
    <span class="ctl">
      <input
        type="color"
        aria-label={$t('histviz.settings.downColor')}
        value={settings.downColor || themed.down()}
        oninput={(e) => (settings.downColor = e.currentTarget.value)}
      />
      <button
        class="reset"
        hidden={!settings.downColor}
        title={$t('histviz.settings.useTheme')}
        aria-label={$t('histviz.settings.useTheme')}
        onclick={() => (settings.downColor = '')}><Icon name="refresh-cw" size={12} /></button
      >
    </span>
  </div>
  <div class="color">
    <span>{$t('histviz.settings.lineColor')}</span>
    <span class="ctl">
      <input
        type="color"
        aria-label={$t('histviz.settings.lineColor')}
        value={settings.lineColor || themed.line()}
        oninput={(e) => (settings.lineColor = e.currentTarget.value)}
      />
      <button
        class="reset"
        hidden={!settings.lineColor}
        title={$t('histviz.settings.useTheme')}
        aria-label={$t('histviz.settings.useTheme')}
        onclick={() => (settings.lineColor = '')}><Icon name="refresh-cw" size={12} /></button
      >
    </span>
  </div>

  <p class="grp">{$t('histviz.settings.groupData')}</p>
  <label class="toggle" title={$t('histviz.settings.autosaveDataHint')}>
    <input type="checkbox" bind:checked={settings.autosaveData} />
    <span>{$t('histviz.settings.autosaveData')}</span>
  </label>
  <p class="hint">{$t('histviz.settings.autosaveDataHint')}</p>

  <div class="sep"></div>
  <button class="reset-all" onclick={() => (settings = { ...DEFAULT_SETTINGS })}
    >{$t('histviz.settings.resetAll')}</button
  >
</div>

<style>
  .popover {
    /* One control height for the whole sheet: the pickers, the swatches and the checkbox
       rows share a rhythm instead of each carrying its own. */
    --control-h: 26px;
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: var(--z-dropdown);
    width: 268px;
    max-height: min(72vh, 640px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .head span {
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .x:hover {
    color: var(--text);
  }
  /* Every row is the same two-column grid: label on the left, control in a fixed right
     column. Rows that size themselves to their own content never line up, which is what
     made this sheet read as a pile of loose boxes. */
  .field,
  .color {
    display: grid;
    grid-template-columns: 1fr 116px;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--fs-body);
    color: var(--text);
  }
  .field > span,
  .color > span:first-child {
    color: var(--dim);
  }
  .ctl {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  /* Section titles: the popover is a settings sheet, not a pile of checkboxes. */
  .grp {
    margin: var(--space-1) 0 0;
    font-family: var(--mono);
    font-size: var(--fs-section);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--dim);
  }
  .grp:first-of-type {
    margin-top: 0;
  }
  .hint {
    margin: -2px 0 0 22px;
    font-size: var(--fs-desc);
    color: var(--dim);
    line-height: 1.35;
  }
  .toggle.sub {
    margin-left: var(--space-3);
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 20px;
    font-size: var(--fs-body);
    color: var(--text);
    cursor: pointer;
  }
  .toggle.disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .color input[type='color'] {
    width: 44px;
    height: 22px;
    padding: 0;
  }
  /* Reserved slot: hidden, never removed, so the row keeps its width. */
  .reset {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .reset[hidden] {
    display: inline-flex;
    visibility: hidden;
    pointer-events: none;
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
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .reset-all:hover {
    color: var(--text);
  }
</style>
