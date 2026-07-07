<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Strategy configuration form, split into collapsible steps (Data / Strategy / Sizing /
  // Costs) — each shows a live plain-English summary when collapsed, so the open one gets
  // the pane. Owns the bound `settings` object and the selected dataset; emits `onrun`.
  // Percent SL/TP are shown as percentages but stored as fractions (0.02 = 2%).
  import { defaultSide, conditionText, fmtNum } from './api.js';
  import DatasetSelect from './DatasetSelect.svelte';
  import SideEditor from './SideEditor.svelte';
  import { t } from '$lib/i18n';

  let {
    datasets = [],
    settings = $bindable(),
    datasetId = $bindable(),
    running = false,
    onrun
  } = $props();

  let open = $state({ data: true, strategy: true, sizing: false, costs: false });
  const toggle = (id) => (open[id] = !open[id]);

  const showLong = $derived(settings.mode === 'long' || settings.mode === 'both');
  const showShort = $derived(settings.mode === 'short' || settings.mode === 'both');
  // The short side is hidden when "reverse side" derives it from long.
  const editShort = $derived(showShort && !(settings.reverse_side && showLong));

  // Ensure side objects exist when the mode switches them on.
  $effect(() => {
    if (showLong && !settings.long) settings.long = defaultSide('crosses_above');
    if (showShort && !settings.short) settings.short = defaultSide('crosses_below');
  });

  const MODES = $derived([
    { id: 'long', label: $t('backtest.settings.modeLong') },
    { id: 'short', label: $t('backtest.settings.modeShort') },
    { id: 'both', label: $t('backtest.settings.modeBoth') }
  ]);

  // ── Collapsed-header summaries ──
  const dataSummary = $derived.by(() => {
    const d = datasets.find((d) => d.id === datasetId);
    return d ? `${d.ticker} · ${d.timeframe}` : $t('backtest.settings.pickDataset');
  });
  const strategySummary = $derived.by(() => {
    const side = settings.mode === 'short' ? settings.short : settings.long;
    const conds = side?.entry?.conditions ?? [];
    const first = conds.length ? conditionText(conds[0]) : $t('backtest.settings.noEntryRule');
    const more = conds.length > 1 ? ` +${conds.length - 1}` : '';
    return `${settings.mode} · ${first}${more}`;
  });
  const sizingSummary = $derived.by(() => {
    const s = settings.sizing;
    const size =
      s.mode === 'percent_equity'
        ? $t('backtest.settings.summaryPercentEquity', { percent: s.percent })
        : $t('backtest.settings.summaryQty', { qty: s.qty });
    const pyr =
      (settings.pyramiding ?? 1) > 1 ? ` · ${$t('backtest.settings.summaryPyr', { count: settings.pyramiding })}` : '';
    return `${size} · ${settings.leverage}× · ${$t('backtest.settings.summaryStart', { amount: fmtNum(settings.starting_capital, 0) })}${pyr}`;
  });
  const costsSummary = $derived.by(() => {
    const f = settings.fees;
    const fee = f.amount
      ? `${f.amount}${f.amount_kind === 'pct' ? '%' : ''} / ${f.per}`
      : $t('backtest.settings.noFees');
    const spread = settings.spread_pct
      ? ` · ${$t('backtest.settings.summarySpread', { pct: fmtNum(settings.spread_pct * 100) })}`
      : '';
    return fee + spread;
  });
</script>

{#snippet head(id, icon, title, summary)}
  <button type="button" class="sect-head" onclick={() => toggle(id)} aria-expanded={open[id]}>
    <span class="s-icon"><Icon name={icon} size={13} /></span>
    <span class="s-title">{title}</span>
    {#if !open[id]}<span class="s-sum">{summary}</span>{/if}
    <span class="s-caret" class:open={open[id]}><Icon name="chevron-right" size={14} /></span>
  </button>
{/snippet}

<div class="settings">
  <div class="scroll">
    <section>
      {@render head('data', 'database', $t('backtest.settings.sectionData'), dataSummary)}
      {#if open.data}
        <div class="body">
          <DatasetSelect {datasets} bind:value={datasetId} />
        </div>
      {/if}
    </section>

    <section>
      {@render head('strategy', 'zap', $t('backtest.settings.sectionStrategy'), strategySummary)}
      {#if open.strategy}
        <div class="body">
          <div class="seg" role="radiogroup" aria-label={$t('backtest.settings.tradeDirection')}>
            {#each MODES as m (m.id)}
              <button
                type="button"
                class:on={settings.mode === m.id}
                onclick={() => (settings.mode = m.id)}>{m.label}</button>
            {/each}
          </div>

          {#if settings.mode === 'both'}
            <label class="check">
              <input type="checkbox" bind:checked={settings.reverse_side} />
              {$t('backtest.settings.reverseSideHint')}
            </label>
            <label class="check">
              <input type="checkbox" bind:checked={settings.stop_and_reverse} />
              {$t('backtest.settings.stopAndReverseHint')}
            </label>
          {/if}

          {#if showLong}
            <SideEditor bind:side={settings.long} sideId="long" />
          {/if}
          {#if editShort}
            <SideEditor bind:side={settings.short} sideId="short" />
          {:else if showShort}
            <p class="note">{$t('backtest.settings.shortDerivedNote')}</p>
          {/if}
        </div>
      {/if}
    </section>

    <section>
      {@render head('sizing', 'wallet', $t('backtest.settings.sectionSizing'), sizingSummary)}
      {#if open.sizing}
        <div class="body">
          <label class="row">
            <span>{$t('backtest.settings.sizeBy')}</span>
            <select
              value={settings.sizing.mode}
              onchange={(e) =>
                (settings.sizing =
                  e.target.value === 'percent_equity'
                    ? { mode: 'percent_equity', percent: 100 }
                    : { mode: 'fixed_qty', qty: 1 })}
            >
              <option value="percent_equity">{$t('backtest.settings.percentOfEquity')}</option>
              <option value="fixed_qty">{$t('backtest.settings.fixedQty')}</option>
            </select>
          </label>
          <div class="grid">
            {#if settings.sizing.mode === 'percent_equity'}
              <label>{$t('backtest.settings.percentInPct')}<input type="number" min="0" step="1" bind:value={settings.sizing.percent} /></label>
            {:else}
              <label>{$t('backtest.settings.quantityPerEntry')}<input type="number" min="0" step="any" bind:value={settings.sizing.qty} /></label>
            {/if}
            <label>{$t('backtest.settings.pyramiding')}
              <input type="number" min="1" max="20" step="1" bind:value={settings.pyramiding}
                title={$t('backtest.settings.pyramidingTitle')} />
            </label>
            <label>{$t('backtest.settings.startingCapital')}<input type="number" min="0" step="any" bind:value={settings.starting_capital} /></label>
            <label>{$t('backtest.settings.leverage')}<input type="number" min="1" step="any" bind:value={settings.leverage} /></label>
          </div>
          {#if (settings.pyramiding ?? 1) > 1}
            <p class="note">{$t('backtest.settings.pyramidingNote', { count: settings.pyramiding })}</p>
          {/if}
        </div>
      {/if}
    </section>

    <section>
      {@render head('costs', 'coins', $t('backtest.settings.sectionCosts'), costsSummary)}
      {#if open.costs}
        <div class="body">
          <div class="grid">
            <label>{$t('backtest.settings.feeAmount')}<input type="number" min="0" step="any" bind:value={settings.fees.amount} /></label>
            <label>{$t('backtest.settings.kind')}
              <select bind:value={settings.fees.amount_kind}>
                <option value="pct">{$t('backtest.settings.pctOfNotional')}</option>
                <option value="fixed">{$t('backtest.settings.fixed')}</option>
              </select>
            </label>
            <label>{$t('backtest.settings.per')}
              <select bind:value={settings.fees.per}>
                <option value="trade">{$t('backtest.settings.perTrade')}</option>
                <option value="unit">{$t('backtest.settings.perUnit')}</option>
              </select>
            </label>
            <label>{$t('backtest.settings.spreadPct')}<input type="number" min="0" step="any" value={settings.spread_pct * 100}
              onchange={(e) => (settings.spread_pct = (Number(e.target.value) || 0) / 100)} /></label>
          </div>
        </div>
      {/if}
    </section>
  </div>

  <div class="run-wrap">
    <button class="run" disabled={!datasetId || running} onclick={() => onrun?.()}>
      {#if running}{$t('backtest.settings.running')}{:else}<Icon name="play" size={13} /> {$t('backtest.settings.runBacktest')}{/if}
    </button>
  </div>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-3) var(--space-4) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  section {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface-2) 35%, transparent);
  }
  .sect-head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    color: var(--text);
    text-align: left;
    min-width: 0;
  }
  .s-icon {
    display: inline-flex;
    color: var(--accent);
    flex-shrink: 0;
  }
  .s-title {
    font-size: 0.72rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    flex-shrink: 0;
  }
  .s-sum {
    color: var(--muted);
    font-size: 0.75rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .s-caret {
    margin-left: auto;
    display: inline-flex;
    color: var(--muted);
    transition: transform 0.15s ease;
    flex-shrink: 0;
  }
  .s-caret.open {
    transform: rotate(90deg);
  }
  .body {
    padding: 0 var(--space-3) var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .seg {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 3px;
    gap: 3px;
  }
  .seg button {
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 0.8rem;
    font-weight: 600;
    padding: var(--space-1) 0;
    border-radius: 999px;
    cursor: pointer;
  }
  .seg button.on {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    color: var(--text);
  }
  .note {
    font-size: 0.78rem;
    color: var(--muted);
    font-style: italic;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .row > span {
    font-size: 0.82rem;
    color: var(--muted);
    min-width: 64px;
  }
  .row select {
    flex: 1;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 0.78rem;
    color: var(--muted);
    min-width: 0;
  }
  .grid input,
  .grid select {
    width: 100%;
    min-width: 0;
  }
  .check {
    flex-direction: row;
    align-items: center;
    gap: var(--space-2);
  }
  .run-wrap {
    flex-shrink: 0;
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--border);
    background: var(--surface);
  }
  .run {
    width: 100%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    background: linear-gradient(135deg, var(--accent), color-mix(in srgb, var(--accent) 55%, #8b5cf6));
    border: none;
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
    color: #fff;
    font-weight: 700;
    font-size: 0.92rem;
    letter-spacing: 0.02em;
    cursor: pointer;
    box-shadow: 0 4px 16px color-mix(in srgb, var(--accent) 35%, transparent);
    transition: transform 0.12s ease, box-shadow 0.12s ease, opacity 0.12s ease;
  }
  .run:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 6px 20px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .run:disabled {
    opacity: 0.5;
    cursor: not-allowed;
    box-shadow: none;
  }
</style>
