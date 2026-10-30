<script>
  // Full chart settings sheet, one category at a time: candles, scales, canvas, trading, data.
  // Edits the shared `settings` object (bindable) live; the parent persists it. A colour left
  // blank follows the theme, and its reset button puts it back there.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { t } from '$lib/i18n';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { DEFAULT_SETTINGS, LINE_STYLES, STYLE_KEYS } from './settings.js';

  // `settings`: one chart's style (bindable). `global` / `onglobal`: the settings every chart
  // shares (the Data tab), written through the page rather than into the chart's own copy.
  let { open = $bindable(false), settings = $bindable(), global = {}, onglobal } = $props();

  const CATEGORIES = [
    { id: 'symbol', icon: 'candlestick', keys: ['upColor', 'downColor', 'bodyOpacity', 'borderUpColor', 'borderDownColor', 'borderWidth', 'lineColor', 'lineWidth', 'volumeUpColor', 'volumeDownColor', 'volumeOpacity'] },
    { id: 'scales', icon: 'ruler', keys: ['scale', 'yAxisSide', 'axisTextColor', 'axisFontSize', 'lastPrice', 'lastPriceLine', 'lastLineStyle', 'lastLineColor', 'lastLineWidth', 'countdown', 'indicatorTags', 'prevClose', 'bidAsk'] },
    { id: 'canvas', icon: 'grid', keys: ['bgColor', 'grid', 'gridV', 'gridColor', 'gridWidth', 'gridStyle', 'crosshair', 'crosshairColor', 'crosshairWidth', 'crosshairStyle', 'crosshairTags', 'tooltip', 'daySeparators'] },
    { id: 'trading', icon: 'briefcase', keys: ['posPnl', 'posLine', 'posSide', 'posQty', 'ordLine', 'ordSide', 'ordQty', 'fills', 'markLongColor', 'markShortColor'] },
    { id: 'data', icon: 'database', keys: ['autosaveData'] }
  ];
  let cat = $state('symbol');

  // A chart saved before a setting existed has no value for it, and a control bound to a
  // missing key refuses to render. On opening, each missing key takes what the chart shows
  // now: the shared setting, else the default.
  $effect(() => {
    if (!open || !settings) return;
    const missing = STYLE_KEYS.filter((k) => settings[k] === undefined);
    if (missing.length) settings = { ...settings, ...Object.fromEntries(missing.map((k) => [k, global[k] ?? DEFAULT_SETTINGS[k]])) };
  });

  // What a blank colour resolves to, so the native swatch (which needs a concrete hex)
  // shows the colour actually drawn.
  const theme = $derived(chartColors());
  const fallback = $derived({
    upColor: theme.green,
    downColor: theme.red,
    borderUpColor: settings.upColor || theme.green,
    borderDownColor: settings.downColor || theme.red,
    lineColor: theme.accent,
    volumeUpColor: theme.green,
    volumeDownColor: theme.red,
    lastLineColor: settings.upColor || theme.green,
    axisTextColor: theme.dim,
    bgColor: theme.bg,
    gridColor: theme.gridLine,
    crosshairColor: theme.muted,
    markLongColor: theme.accent,
    markShortColor: theme.red
  });
  const hex = (v) => (/^#[0-9a-f]{6}$/i.test(v ?? '') ? v : '#888888');

  const styleOpts = $derived(
    LINE_STYLES.map((v) => ({ value: v, label: $t(`histviz.settings.line${v[0].toUpperCase()}${v.slice(1)}`) }))
  );
  const widthOpts = (list) => list.map((w) => ({ value: w, label: `${w} px` }));

  function resetCategory() {
    const keys = CATEGORIES.find((c) => c.id === cat)?.keys ?? [];
    if (cat === 'data') {
      onglobal?.(Object.fromEntries(keys.map((k) => [k, DEFAULT_SETTINGS[k]])));
      return;
    }
    const next = { ...settings };
    for (const k of keys) next[k] = DEFAULT_SETTINGS[k];
    settings = next;
  }
  /** Back to the defaults, this chart's style only. */
  const resetAll = () =>
    (settings = { ...settings, ...Object.fromEntries(STYLE_KEYS.map((k) => [k, DEFAULT_SETTINGS[k]])) });
</script>

{#snippet color(key, label)}
  <div class="row">
    <span class="lbl">{$t(`histviz.settings.${label}`)}</span>
    <span class="ctl">
      <input
        type="color"
        aria-label={$t(`histviz.settings.${label}`)}
        value={hex(settings[key] || fallback[key])}
        oninput={(e) => (settings[key] = e.currentTarget.value)}
      />
      <button
        class="reset"
        hidden={!settings[key]}
        title={$t('histviz.settings.useTheme')}
        aria-label={$t('histviz.settings.useTheme')}
        onclick={() => (settings[key] = '')}><Icon name="refresh-cw" size={12} /></button
      >
    </span>
  </div>
{/snippet}

{#snippet select(key, label, options)}
  <div class="row">
    <span class="lbl">{$t(`histviz.settings.${label}`)}</span>
    <span class="ctl">
      <Dropdown bind:value={settings[key]} ariaLabel={$t(`histviz.settings.${label}`)} float {options} />
    </span>
  </div>
{/snippet}

{#snippet range(key, label, min, max, step, unit)}
  <div class="row">
    <span class="lbl">{$t(`histviz.settings.${label}`)}</span>
    <span class="ctl">
      <input
        type="range"
        {min}
        {max}
        {step}
        value={settings[key]}
        aria-label={$t(`histviz.settings.${label}`)}
        oninput={(e) => (settings[key] = Number(e.currentTarget.value))}
      />
      <span class="num">{settings[key]}{unit}</span>
    </span>
  </div>
{/snippet}

{#snippet toggle(key, label, disabled = false, sub = false)}
  <label class="toggle" class:sub class:disabled>
    <input type="checkbox" bind:checked={settings[key]} {disabled} />
    <span>{$t(`histviz.settings.${label}`)}</span>
  </label>
{/snippet}

<Modal bind:open title={$t('histviz.settings.title')} size="md">
  <div class="sheet">
    <nav class="cats" aria-label={$t('histviz.settings.title')}>
      {#each CATEGORIES as c (c.id)}
        <button class:active={cat === c.id} aria-current={cat === c.id} onclick={() => (cat = c.id)}>
          <Icon name={c.icon} size={14} />
          <span>{$t(`histviz.settings.cat.${c.id}`)}</span>
        </button>
      {/each}
    </nav>

    <div class="body">
      {#if cat === 'symbol'}
        <p class="grp">{$t('histviz.settings.groupBody')}</p>
        {@render color('upColor', 'upColor')}
        {@render color('downColor', 'downColor')}
        {@render range('bodyOpacity', 'opacity', 0, 100, 5, '%')}
        <p class="grp">{$t('histviz.settings.groupBorders')}</p>
        {@render color('borderUpColor', 'upColor')}
        {@render color('borderDownColor', 'downColor')}
        {@render select('borderWidth', 'width', widthOpts([1, 2, 3]))}
        <p class="grp">{$t('histviz.settings.groupLine')}</p>
        {@render color('lineColor', 'color')}
        {@render select('lineWidth', 'width', widthOpts([1, 1.5, 2, 3, 4]))}
        <p class="grp">{$t('histviz.chart.volume')}</p>
        {@render color('volumeUpColor', 'upColor')}
        {@render color('volumeDownColor', 'downColor')}
        {@render range('volumeOpacity', 'opacity', 0, 100, 5, '%')}
      {:else if cat === 'scales'}
        <p class="grp">{$t('histviz.settings.priceScale')}</p>
        {@render select('scale', 'scaleType', [
          { value: 'linear', label: $t('histviz.settings.linear') },
          { value: 'log', label: $t('histviz.settings.logarithmic') }
        ])}
        {@render select('yAxisSide', 'axisSide', [
          { value: 'left', label: $t('histviz.settings.left') },
          { value: 'right', label: $t('histviz.settings.right') }
        ])}
        {@render color('axisTextColor', 'textColor')}
        {@render select('axisFontSize', 'fontSize', widthOpts([9, 10, 11, 12, 13, 14]))}
        <p class="grp">{$t('histviz.settings.groupLastPrice')}</p>
        {@render toggle('lastPrice', 'lastPrice')}
        {@render toggle('lastPriceLine', 'lastPriceLine', !settings.lastPrice, true)}
        {#if settings.lastPrice && settings.lastPriceLine}
          {@render select('lastLineStyle', 'lastLineStyle', styleOpts)}
          {@render select('lastLineWidth', 'lastLineWidth', widthOpts([1, 2, 3, 4]))}
          {@render color('lastLineColor', 'lastLineColor')}
        {/if}
        {@render toggle('countdown', 'countdown', !settings.lastPrice, true)}
        <p class="grp">{$t('histviz.settings.groupLevels')}</p>
        {@render toggle('indicatorTags', 'indicatorTags')}
        {@render toggle('prevClose', 'prevClose')}
        {@render toggle('bidAsk', 'bidAsk')}
        <p class="hint">{$t('histviz.settings.bidAskHint')}</p>
      {:else if cat === 'canvas'}
        <p class="grp">{$t('histviz.settings.groupBackground')}</p>
        {@render color('bgColor', 'color')}
        <p class="grp">{$t('histviz.settings.groupGrid')}</p>
        {@render toggle('grid', 'gridH')}
        {@render toggle('gridV', 'gridV')}
        {@render color('gridColor', 'color')}
        {@render select('gridWidth', 'width', widthOpts([0.5, 1, 2]))}
        {@render select('gridStyle', 'lineStyle', styleOpts)}
        <p class="grp">{$t('histviz.settings.crosshair')}</p>
        {@render toggle('crosshair', 'crosshair')}
        {@render toggle('crosshairTags', 'crosshairTags', !settings.crosshair, true)}
        {@render color('crosshairColor', 'color')}
        {@render select('crosshairWidth', 'width', widthOpts([1, 2, 3]))}
        {@render select('crosshairStyle', 'lineStyle', styleOpts)}
        <p class="grp">{$t('histviz.settings.groupOverlay')}</p>
        {@render toggle('tooltip', 'tooltip')}
        {@render toggle('daySeparators', 'daySeparators')}
      {:else if cat === 'trading'}
        <p class="grp">{$t('histviz.settings.groupPosition')}</p>
        {@render select('posPnl', 'posPnl', [
          { value: 'off', label: $t('histviz.settings.pnlOff') },
          { value: 'pct', label: $t('histviz.settings.pnlPct') },
          { value: 'value', label: $t('histviz.settings.pnlValue') }
        ])}
        {@render select('posLine', 'tradeLine', [
          { value: 'full', label: $t('histviz.settings.lineFull') },
          { value: 'tight', label: $t('histviz.settings.lineTight') },
          { value: 'scale', label: $t('histviz.settings.lineScale') }
        ])}
        {@render toggle('posSide', 'showSide')}
        {@render toggle('posQty', 'showQty')}
        <p class="grp">{$t('histviz.settings.groupOrder')}</p>
        {@render select('ordLine', 'tradeLine', [
          { value: 'full', label: $t('histviz.settings.lineFull') },
          { value: 'tight', label: $t('histviz.settings.lineTight') },
          { value: 'scale', label: $t('histviz.settings.lineScale') }
        ])}
        {@render toggle('ordSide', 'showSide')}
        {@render toggle('ordQty', 'showQty')}
        <p class="grp">{$t('histviz.settings.groupFills')}</p>
        {@render toggle('fills', 'fills')}
        {@render color('markLongColor', 'markLong')}
        {@render color('markShortColor', 'markShort')}
        <p class="hint">{$t('histviz.settings.tradeHint')}</p>
      {:else}
        <p class="grp">{$t('histviz.settings.groupData')}</p>
        <label class="toggle">
          <input
            type="checkbox"
            checked={!!global.autosaveData}
            onchange={(e) => onglobal?.({ autosaveData: e.currentTarget.checked })}
          />
          <span>{$t('histviz.settings.autosaveData')}</span>
        </label>
        <p class="hint">{$t('histviz.settings.autosaveDataHint')}</p>
      {/if}
    </div>
  </div>

  {#snippet footer()}
    <div class="foot">
      <Button variant="ghost" onclick={resetAll}>
        {$t('histviz.settings.resetAll')}
      </Button>
      <Button variant="ghost" onclick={resetCategory}>{$t('histviz.settings.resetCategory')}</Button>
      <span class="grow"></span>
      <Button variant="primary" onclick={() => (open = false)}>{$t('histviz.settings.done')}</Button>
    </div>
  {/snippet}
</Modal>

<style>
  .sheet {
    --control-h: 26px;
    display: grid;
    grid-template-columns: 150px 1fr;
    gap: var(--space-4);
    min-height: 420px;
  }
  .cats {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    border-right: 1px solid var(--border);
    padding-right: var(--space-3);
  }
  .cats button {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-body);
    text-align: left;
    cursor: pointer;
  }
  .cats button:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .cats button.active {
    color: var(--text);
    background: var(--surface-2);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .grp {
    margin: var(--space-3) 0 0;
    font-family: var(--mono);
    font-size: var(--fs-section);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--dim);
  }
  .grp:first-child {
    margin-top: 0;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 180px;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--fs-body);
  }
  .lbl {
    color: var(--dim);
  }
  .ctl {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .ctl :global(.dd) {
    flex: 1;
    min-width: 0;
  }
  .ctl input[type='color'] {
    width: 44px;
    height: 22px;
    padding: 0;
  }
  .ctl input[type='range'] {
    flex: 1;
    min-width: 0;
    accent-color: var(--accent);
  }
  .num {
    min-width: 40px;
    text-align: right;
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 22px;
    font-size: var(--fs-body);
    color: var(--text);
    cursor: pointer;
  }
  .toggle.sub {
    margin-left: var(--space-4);
  }
  .toggle.disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .hint {
    margin: 0 0 0 22px;
    font-size: var(--fs-desc);
    color: var(--dim);
    line-height: 1.35;
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
  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
  }
  .grow {
    flex: 1;
  }
  @media (max-width: 640px) {
    .sheet {
      grid-template-columns: 1fr;
    }
    .cats {
      flex-direction: row;
      overflow-x: auto;
      border-right: none;
      border-bottom: 1px solid var(--border);
      padding: 0 0 var(--space-2);
    }
    .row {
      grid-template-columns: 1fr 150px;
    }
  }
</style>
