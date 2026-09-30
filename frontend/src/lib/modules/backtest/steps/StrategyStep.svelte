<script>
  // Step 2, the rules. One of three engines: the signal engine (entry/exit condition groups
  // per side, with SL/TP), a grid ladder, or a DCA savings plan (a weighted basket, an initial
  // stack and conditional tranches on top). Custom indicators are built from here: the library
  // modal is one click away, and anything saved in it becomes an operand in every editor.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import SideEditor from '../SideEditor.svelte';
  import DcaEditor from '../DcaEditor.svelte';
  import { defaultSide, defaultGrid, defaultDca, GRID_ANCHORS } from '../api.js';
  import { t } from '$lib/i18n';
  import './form.css';

  let {
    settings = $bindable(),
    customIndicators = [],
    assets = [],
    timeframe = '',
    onindicators
  } = $props();

  const isGrid = $derived(settings.kind === 'grid');
  const isDca = $derived(settings.kind === 'dca');
  const isSignals = $derived(!isGrid && !isDca);
  // Anchored: the ladder is centred on a moving line, so the fixed bounds no longer apply.
  const anchored = $derived(isGrid && settings.grid?.anchor && settings.grid.anchor !== 'none');
  function setKind(kind) {
    settings.kind = kind;
    if (kind === 'grid') settings.grid = { ...defaultGrid(), ...(settings.grid ?? {}) };
    if (kind === 'dca') settings.dca = { ...defaultDca(), ...(settings.dca ?? {}) };
  }

  const MODES = $derived([
    { id: 'long', label: $t('backtest.settings.modeLong') },
    { id: 'short', label: $t('backtest.settings.modeShort') },
    { id: 'both', label: $t('backtest.settings.modeBoth') }
  ]);

  const showLong = $derived(settings.mode === 'long' || settings.mode === 'both');
  const showShort = $derived(settings.mode === 'short' || settings.mode === 'both');
  // The short side is hidden when "reverse side" derives it from long.
  const editShort = $derived(showShort && !(settings.reverse_side && showLong));

  // Ensure side objects exist when the mode switches them on.
  $effect(() => {
    if (!isSignals) return;
    if (showLong && !settings.long) settings.long = defaultSide('crosses_above');
    if (showShort && !settings.short) settings.short = defaultSide('crosses_below');
  });
</script>

<div class="bt-step">
  <div class="bt-block">
    <div class="head">
      <span class="bt-cap">{$t('backtest.step.engineCap')}</span>
      <button type="button" class="lib" onclick={() => onindicators?.()}>
        <Icon name="plus" size={12} /> {$t('backtest.step.customIndicators')}
        {#if customIndicators.length}<span class="n">{customIndicators.length}</span>{/if}
      </button>
    </div>
    <div class="bt-seg" role="radiogroup" aria-label={$t('backtest.grid.kindLabel')}>
      <button type="button" class:on={isSignals} onclick={() => setKind('signals')}>{$t('backtest.grid.signals')}</button>
      <button type="button" class:on={isGrid} onclick={() => setKind('grid')}>{$t('backtest.grid.grid')}</button>
      <button type="button" class:on={isDca} onclick={() => setKind('dca')}>{$t('backtest.dca.mode')}</button>
    </div>
  </div>

  {#if isDca}
    <DcaEditor bind:dca={settings.dca} bind:capital={settings.starting_capital} {assets} {timeframe} {customIndicators} />
  {:else if isSignals}
    <div class="bt-block">
      <span class="bt-cap">{$t('backtest.settings.tradeDirection')}</span>
      <div class="bt-seg" role="radiogroup" aria-label={$t('backtest.settings.tradeDirection')}>
        {#each MODES as m (m.id)}
          <button type="button" class:on={settings.mode === m.id} onclick={() => (settings.mode = m.id)}>{m.label}</button>
        {/each}
      </div>
      {#if settings.mode === 'both'}
        <label class="bt-check">
          <input type="checkbox" bind:checked={settings.reverse_side} />
          {$t('backtest.settings.reverseSideHint')}
        </label>
        <label class="bt-check">
          <input type="checkbox" bind:checked={settings.stop_and_reverse} />
          {$t('backtest.settings.stopAndReverseHint')}
        </label>
      {/if}
    </div>

    <div class="sides" class:pair={showLong && editShort}>
      {#if showLong}
        <SideEditor bind:side={settings.long} sideId="long" {customIndicators} />
      {/if}
      {#if editShort}
        <SideEditor bind:side={settings.short} sideId="short" {customIndicators} />
      {:else if showShort}
        <p class="bt-note">{$t('backtest.settings.shortDerivedNote')}</p>
      {/if}
    </div>
  {:else}
    <div class="bt-block">
      <span class="bt-cap">{$t('backtest.grid.anchor')}</span>
      <div class="bt-grid">
        <div class="bt-field">{$t('backtest.grid.anchor')}
          <Dropdown
            bind:value={settings.grid.anchor}
            ariaLabel={$t('backtest.grid.anchor')}
            options={GRID_ANCHORS.map((a) => ({
              value: a,
              label: a === 'none' ? $t('backtest.grid.anchorNone') : a.toUpperCase()
            }))}
          />
        </div>
        {#if anchored}
          <label class="bt-field">{$t('backtest.grid.anchorPeriod')}<input type="number" min="1" step="1" bind:value={settings.grid.anchor_period} /></label>
          <div class="bt-field">{$t('backtest.grid.widthKind')}
            <Dropdown
              bind:value={settings.grid.width_kind}
              ariaLabel={$t('backtest.grid.widthKind')}
              options={[
                { value: 'pct', label: $t('backtest.grid.widthPct') },
                { value: 'atr', label: $t('backtest.grid.widthAtr') }
              ]}
            />
          </div>
          <label class="bt-field">{$t('backtest.grid.widthValue')}<input type="number" min="0" step="any" bind:value={settings.grid.width_value} /></label>
          {#if settings.grid.width_kind === 'atr'}
            <label class="bt-field">{$t('backtest.grid.atrPeriod')}<input type="number" min="1" step="1" bind:value={settings.grid.width_period} /></label>
          {/if}
        {/if}
      </div>
      <p class="bt-note">{anchored ? $t('backtest.grid.anchorHint') : $t('backtest.grid.fixedHint')}</p>
      <label class="bt-check">
        <input type="checkbox" bind:checked={settings.grid.reset_on_close} />
        {$t('backtest.grid.resetOnClose')}
      </label>
      <p class="bt-note">{$t('backtest.grid.resetHint')}</p>
    </div>

    <div class="bt-block">
      <span class="bt-cap">{$t('backtest.grid.grid')}</span>
      <div class="bt-grid">
        {#if !anchored}
          <label class="bt-field">{$t('backtest.grid.lower')}<input type="number" step="any" bind:value={settings.grid.lower} /></label>
          <label class="bt-field">{$t('backtest.grid.upper')}<input type="number" step="any" bind:value={settings.grid.upper} /></label>
        {/if}
        <label class="bt-field">{$t('backtest.grid.levels')}<input type="number" min="2" step="1" bind:value={settings.grid.levels} /></label>
        <div class="bt-field">{$t('backtest.grid.direction')}
          <Dropdown
            bind:value={settings.grid.direction}
            ariaLabel={$t('backtest.grid.direction')}
            options={[
              { value: 'long', label: $t('backtest.grid.long') },
              { value: 'short', label: $t('backtest.grid.short') },
              { value: 'neutral', label: $t('backtest.grid.neutral') }
            ]}
          />
        </div>
        <label class="bt-field">{$t('backtest.grid.qtyPerLevel')}<input type="number" min="0" step="any" bind:value={settings.grid.qty_per_level} /></label>
        <label class="bt-field">{$t('backtest.grid.totalBudget')}<input type="number" min="0" step="any" bind:value={settings.grid.total_budget} /></label>
        <label class="bt-field">{$t('backtest.grid.stopBelow')}<input type="number" min="0" step="any" bind:value={settings.grid.stop_below} /></label>
        <label class="bt-field">{$t('backtest.grid.stopAbove')}<input type="number" min="0" step="any" bind:value={settings.grid.stop_above} /></label>
      </div>
      <p class="bt-note">{$t('backtest.grid.note')}</p>
    </div>
  {/if}
</div>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .head :global(.bt-cap) {
    flex: 1;
  }
  .lib {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    white-space: nowrap;
  }
  .lib:hover {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-2));
  }
  .n {
    color: var(--muted);
  }
  /* Both sides side by side once there is room; one column on a narrow window. */
  .sides {
    display: grid;
    gap: var(--space-4);
  }
  @media (min-width: 1180px) {
    .sides.pair {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
