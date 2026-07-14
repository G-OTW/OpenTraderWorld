<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Strategy configuration form, split into collapsible steps (Data / Strategy / Sizing /
  // Costs) — each shows a live plain-English summary when collapsed, so the open one gets
  // the pane. Owns the bound `settings` object and the selected dataset; emits `onrun`.
  // Percent SL/TP are shown as percentages but stored as fractions (0.02 = 2%).
  import { defaultSide, conditionText, fmtNum, SIZING_MODES, defaultSizing, sizingNeedsStop, defaultGrid } from './api.js';
  import MultiDatasetSelect from './MultiDatasetSelect.svelte';
  import AlignmentBanner from './AlignmentBanner.svelte';
  import SideEditor from './SideEditor.svelte';
  import { t } from '$lib/i18n';

  let {
    datasets = [],
    settings = $bindable(),
    datasetIds = $bindable([]),
    alignment = null,
    aligning = false,
    running = false,
    customIndicators = [],
    onrun
  } = $props();

  // A run needs at least one dataset; a portfolio run has ≥ 2.
  const multi = $derived(datasetIds.length > 1);
  // Does any enabled side carry a stop-loss? (risk-based sizing needs one).
  const hasStop = $derived(
    ((settings.mode !== 'short' && settings.long?.stop_loss_pct > 0) ||
      (settings.mode !== 'long' && settings.short?.stop_loss_pct > 0))
  );
  const sizingStopMissing = $derived(sizingNeedsStop(settings.sizing) && !hasStop);

  function setSizingMode(mode) {
    settings.sizing = defaultSizing(mode);
  }

  const isGrid = $derived(settings.kind === 'grid');
  function setKind(kind) {
    settings.kind = kind;
    if (kind === 'grid' && !settings.grid) settings.grid = defaultGrid();
  }
  function addTier() {
    const tiers = settings.sizing.tiers ?? [];
    const last = tiers[tiers.length - 1];
    settings.sizing.tiers = [...tiers, { above: (last?.above ?? 0) + 1000, value: last?.value ?? 1 }];
  }
  function removeTier(i) {
    settings.sizing.tiers = settings.sizing.tiers.filter((_, j) => j !== i);
  }

  let open = $state({ data: true, strategy: true, sizing: false, costs: false, advanced: false });
  const toggle = (id) => (open[id] = !open[id]);

  // Number-or-null binding for optional risk/circuit-breaker fields (blank = unset).
  const setOpt = (obj, key) => (e) => (obj[key] = e.target.value === '' ? null : Number(e.target.value));

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
    const sel = datasetIds.map((id) => datasets.find((d) => d.id === id)).filter(Boolean);
    if (!sel.length) return $t('backtest.settings.pickDataset');
    if (sel.length === 1) return `${sel[0].ticker} · ${sel[0].timeframe}`;
    return $t('backtest.settings.dataMulti', { tickers: sel.map((d) => d.ticker).join(', '), tf: sel[0].timeframe });
  });
  const strategySummary = $derived.by(() => {
    if (settings.kind === 'grid') {
      const g = settings.grid ?? {};
      return $t('backtest.grid.summary', { levels: g.levels ?? 0, lower: g.lower ?? 0, upper: g.upper ?? 0 });
    }
    const side = settings.mode === 'short' ? settings.short : settings.long;
    const conds = side?.entry?.conditions ?? [];
    const first = conds.length ? conditionText(conds[0]) : $t('backtest.settings.noEntryRule');
    const more = conds.length > 1 ? ` +${conds.length - 1}` : '';
    return `${settings.mode} · ${first}${more}`;
  });
  const sizingSummary = $derived.by(() => {
    const s = settings.sizing;
    let size;
    switch (s.mode) {
      case 'percent_equity':
        size = $t('backtest.settings.summaryPercentEquity', { percent: s.percent });
        break;
      case 'fixed_qty':
        size = $t('backtest.settings.summaryQty', { qty: s.qty });
        break;
      case 'risk':
        size = $t('backtest.settings.summaryRisk', { pct: s.risk_pct });
        break;
      case 'equity_tiers':
        size = $t('backtest.settings.summaryTiers', { n: s.tiers?.length ?? 0 });
        break;
      case 'kelly':
        size = $t('backtest.settings.summaryKelly', { fraction: s.fraction });
        break;
      default:
        size = s.mode;
    }
    const pyr =
      (settings.pyramiding ?? 1) > 1 ? ` · ${$t('backtest.settings.summaryPyr', { count: settings.pyramiding })}` : '';
    return `${size} · ${settings.leverage}× · ${$t('backtest.settings.summaryStart', { amount: fmtNum(settings.starting_capital, 0) })}${pyr}`;
  });
  const advancedSummary = $derived.by(() => {
    const bits = [];
    const ps = settings.pyramid_steps ?? {};
    if (ps.scale?.length) bits.push($t('backtest.adv.summaryScale', { seq: ps.scale.join('/') }));
    const inst = settings.instrument ?? {};
    if (inst.multiplier && inst.multiplier !== 1) bits.push(`×${inst.multiplier}`);
    if (inst.lot_step) bits.push($t('backtest.adv.summaryLot', { step: inst.lot_step }));
    if (settings.slippage?.value) bits.push($t('backtest.adv.summarySlip'));
    const rk = settings.risk ?? {};
    if (rk.max_drawdown_pct || rk.max_daily_loss_pct) bits.push($t('backtest.adv.summaryBreaker'));
    if (settings.oos_split_pct) bits.push($t('backtest.adv.summaryOos', { pct: Math.round(settings.oos_split_pct * 100) }));
    if (settings.funding?.annual_rate_pct) bits.push($t('backtest.adv.summaryFunding'));
    return bits.length ? bits.join(' · ') : $t('backtest.adv.summaryNone');
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
          <MultiDatasetSelect {datasets} bind:values={datasetIds} />
          {#if multi}
            <AlignmentBanner {alignment} loading={aligning} />
          {/if}
        </div>
      {/if}
    </section>

    <section>
      {@render head('strategy', 'zap', $t('backtest.settings.sectionStrategy'), strategySummary)}
      {#if open.strategy}
        <div class="body">
          <!-- Strategy kind: signal-combination engine vs a grid ladder -->
          <div class="seg kind" role="radiogroup" aria-label={$t('backtest.grid.kindLabel')}>
            <button type="button" class:on={!isGrid} onclick={() => setKind('signals')}>{$t('backtest.grid.signals')}</button>
            <button type="button" class:on={isGrid} onclick={() => setKind('grid')}>{$t('backtest.grid.grid')}</button>
          </div>

          {#if !isGrid}
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
              <SideEditor bind:side={settings.long} sideId="long" {customIndicators} />
            {/if}
            {#if editShort}
              <SideEditor bind:side={settings.short} sideId="short" {customIndicators} />
            {:else if showShort}
              <p class="note">{$t('backtest.settings.shortDerivedNote')}</p>
            {/if}
          {:else}
            <!-- Grid ladder form (compact, separate from the signal builder) -->
            <div class="grid">
              <label>{$t('backtest.grid.lower')}<input type="number" step="any" bind:value={settings.grid.lower} /></label>
              <label>{$t('backtest.grid.upper')}<input type="number" step="any" bind:value={settings.grid.upper} /></label>
              <label>{$t('backtest.grid.levels')}<input type="number" min="2" step="1" bind:value={settings.grid.levels} /></label>
              <label>{$t('backtest.grid.direction')}
                <select bind:value={settings.grid.direction}>
                  <option value="long">{$t('backtest.grid.long')}</option>
                  <option value="short">{$t('backtest.grid.short')}</option>
                  <option value="neutral">{$t('backtest.grid.neutral')}</option>
                </select>
              </label>
              <label>{$t('backtest.grid.qtyPerLevel')}<input type="number" min="0" step="any" bind:value={settings.grid.qty_per_level} /></label>
              <label>{$t('backtest.grid.totalBudget')}<input type="number" min="0" step="any" bind:value={settings.grid.total_budget} /></label>
              <label>{$t('backtest.grid.stopBelow')}<input type="number" min="0" step="any" bind:value={settings.grid.stop_below} /></label>
              <label>{$t('backtest.grid.stopAbove')}<input type="number" min="0" step="any" bind:value={settings.grid.stop_above} /></label>
            </div>
            <p class="note">{$t('backtest.grid.note')}</p>
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
            <select value={settings.sizing.mode} onchange={(e) => setSizingMode(e.target.value)}>
              {#each SIZING_MODES as m (m.id)}
                <option value={m.id}>{m.label}</option>
              {/each}
            </select>
          </label>

          <!-- Mode-specific inputs -->
          {#if settings.sizing.mode === 'percent_equity'}
            <div class="grid">
              <label>{$t('backtest.settings.percentInPct')}<input type="number" min="0" step="1" bind:value={settings.sizing.percent} /></label>
            </div>
          {:else if settings.sizing.mode === 'fixed_qty'}
            <div class="grid">
              <label>{$t('backtest.settings.quantityPerEntry')}<input type="number" min="0" step="any" bind:value={settings.sizing.qty} /></label>
            </div>
          {:else if settings.sizing.mode === 'risk'}
            <div class="grid">
              <label>{$t('backtest.settings.riskPct')}<input type="number" min="0" step="any" bind:value={settings.sizing.risk_pct} /></label>
            </div>
          {:else if settings.sizing.mode === 'equity_tiers'}
            <label class="row">
              <span>{$t('backtest.settings.tierMetric')}</span>
              <select bind:value={settings.sizing.metric}>
                <option value="qty">{$t('backtest.settings.fixedQty')}</option>
                <option value="risk_pct">{$t('backtest.settings.riskPct')}</option>
                <option value="percent_equity">{$t('backtest.settings.percentOfEquity')}</option>
              </select>
            </label>
            <div class="tiers">
              <div class="thead"><span>{$t('backtest.settings.tierAbove')}</span><span>{$t('backtest.settings.tierValue')}</span><span></span></div>
              {#each settings.sizing.tiers as tier, i (i)}
                <div class="trow">
                  <input type="number" min="0" step="any" bind:value={tier.above} />
                  <input type="number" min="0" step="any" bind:value={tier.value} />
                  <button type="button" class="tdel" onclick={() => removeTier(i)} title={$t('common.remove')}>
                    <Icon name="x" size={11} />
                  </button>
                </div>
              {/each}
              <button type="button" class="tadd" onclick={addTier}><Icon name="plus" size={11} /> {$t('backtest.settings.tierAdd')}</button>
            </div>
            <p class="note">{$t('backtest.settings.tierNote')}</p>
          {:else if settings.sizing.mode === 'kelly'}
            <div class="grid">
              <label>{$t('backtest.settings.kellyFraction')}<input type="number" min="0" max="1" step="0.05" bind:value={settings.sizing.fraction} /></label>
              <label>{$t('backtest.settings.kellyWindow')}<input type="number" min="1" step="1" bind:value={settings.sizing.window} /></label>
              <label>{$t('backtest.settings.kellyCap')}<input type="number" min="0" step="any" bind:value={settings.sizing.cap_pct} /></label>
              <label>{$t('backtest.settings.kellyWarmup')}<input type="number" min="0" step="any" bind:value={settings.sizing.warmup.percent} /></label>
            </div>
            <p class="note">{$t('backtest.settings.kellyNote')}</p>
          {/if}

          {#if sizingStopMissing}
            <p class="warn"><Icon name="alert-triangle" size={12} /> {$t('backtest.settings.riskNeedsStop')}</p>
          {/if}

          <div class="grid">
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

          {#if multi}
            <div class="limits">
              <div class="lcap">{$t('backtest.settings.portfolioLimits')}</div>
              <div class="grid">
                <label>{$t('backtest.settings.maxOpen')}
                  <input type="number" min="0" step="1" value={settings.risk.max_open_positions ?? ''}
                    onchange={(e) => (settings.risk.max_open_positions = e.target.value === '' ? null : Number(e.target.value))} />
                </label>
                <label>{$t('backtest.settings.maxExposure')}
                  <input type="number" min="0" step="any" value={settings.risk.max_exposure_pct ?? ''}
                    onchange={(e) => (settings.risk.max_exposure_pct = e.target.value === '' ? null : Number(e.target.value))} />
                </label>
                <label>{$t('backtest.settings.maxExposurePerAsset')}
                  <input type="number" min="0" step="any" value={settings.risk.max_exposure_per_asset_pct ?? ''}
                    onchange={(e) => (settings.risk.max_exposure_per_asset_pct = e.target.value === '' ? null : Number(e.target.value))} />
                </label>
              </div>
              <p class="note">{$t('backtest.settings.limitsNote')}</p>
            </div>
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

    <section>
      {@render head('advanced', 'settings', $t('backtest.settings.sectionAdvanced'), advancedSummary)}
      {#if open.advanced}
        <div class="body">
          <!-- Pyramiding steps -->
          <div class="subcap">{$t('backtest.adv.pyramidSteps')}</div>
          <label class="unit-field">
            <span>{$t('backtest.adv.scale')}</span>
            <input type="text" placeholder="1, 0.5, 0.25"
              value={(settings.pyramid_steps.scale ?? []).join(', ')}
              onchange={(e) => (settings.pyramid_steps.scale = e.target.value.split(',').map((x) => Number(x.trim())).filter((x) => x > 0))} />
          </label>
          <div class="grid">
            <label>{$t('backtest.adv.minDistance')}
              <input type="number" min="0" step="any" value={(settings.pyramid_steps.min_distance_pct ?? 0) * 100}
                onchange={(e) => (settings.pyramid_steps.min_distance_pct = (Number(e.target.value) || 0) / 100)} />
            </label>
            <label>{$t('backtest.adv.afterAddSl')}
              <select bind:value={settings.pyramid_steps.after_add_sl}>
                <option value="none">{$t('backtest.adv.afterNone')}</option>
                <option value="breakeven">{$t('backtest.adv.afterBreakeven')}</option>
                <option value="trail_avg">{$t('backtest.adv.afterTrail')}</option>
              </select>
            </label>
          </div>

          <!-- Instrument profile -->
          <div class="subcap">{$t('backtest.adv.instrument')}</div>
          <div class="grid">
            <label>{$t('backtest.adv.multiplier')}<input type="number" min="0" step="any" bind:value={settings.instrument.multiplier} /></label>
            <label>{$t('backtest.adv.lotStep')}<input type="number" min="0" step="any" bind:value={settings.instrument.lot_step} /></label>
            <label>{$t('backtest.adv.minQty')}<input type="number" min="0" step="any" bind:value={settings.instrument.min_qty} /></label>
          </div>

          <!-- Slippage -->
          <div class="subcap">{$t('backtest.adv.slippage')}</div>
          <div class="grid">
            <label>{$t('backtest.adv.slipKind')}
              <select bind:value={settings.slippage.kind}>
                <option value="pct">{$t('backtest.adv.slipPct')}</option>
                <option value="ticks">{$t('backtest.adv.slipTicks')}</option>
              </select>
            </label>
            {#if settings.slippage.kind === 'pct'}
              <label>{$t('backtest.adv.slipValuePct')}<input type="number" min="0" step="any" value={(settings.slippage.value ?? 0) * 100}
                onchange={(e) => (settings.slippage.value = (Number(e.target.value) || 0) / 100)} /></label>
            {:else}
              <label>{$t('backtest.adv.slipTicksN')}<input type="number" min="0" step="any" bind:value={settings.slippage.value} /></label>
              <label>{$t('backtest.adv.tickSize')}<input type="number" min="0" step="any" bind:value={settings.slippage.tick_size} /></label>
            {/if}
          </div>

          <!-- Circuit breakers -->
          <div class="subcap">{$t('backtest.adv.circuitBreakers')}</div>
          <div class="grid">
            <label>{$t('backtest.adv.maxDailyLoss')}
              <input type="number" min="0" step="any" value={settings.risk.max_daily_loss_pct ?? ''} onchange={setOpt(settings.risk, 'max_daily_loss_pct')} />
            </label>
            <label>{$t('backtest.adv.maxDrawdown')}
              <input type="number" min="0" step="any" value={settings.risk.max_drawdown_pct ?? ''} onchange={setOpt(settings.risk, 'max_drawdown_pct')} />
            </label>
          </div>

          <!-- Out-of-sample split -->
          <div class="subcap">{$t('backtest.adv.oos')}</div>
          <label class="unit-field">
            <span>{$t('backtest.adv.oosSplit')}</span>
            <input type="number" min="0" max="99" step="1" value={Math.round((settings.oos_split_pct ?? 0) * 100)}
              onchange={(e) => (settings.oos_split_pct = Math.min(99, Math.max(0, Number(e.target.value) || 0)) / 100)} />
          </label>
          <p class="note">{$t('backtest.adv.oosNote')}</p>

          <!-- Funding estimate (crypto perps) -->
          <div class="subcap">{$t('backtest.adv.funding')}</div>
          <div class="grid">
            <label>{$t('backtest.adv.fundingRate')}<input type="number" step="any" bind:value={settings.funding.annual_rate_pct} /></label>
            <label>{$t('backtest.adv.fundingInterval')}<input type="number" min="1" step="any" bind:value={settings.funding.interval_hours} /></label>
          </div>
          <p class="note">{$t('backtest.adv.fundingNote')}</p>
        </div>
      {/if}
    </section>
  </div>

  <div class="run-wrap">
    <button class="run" disabled={!datasetIds.length || running || sizingStopMissing} onclick={() => onrun?.()}>
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
    font-size: var(--text-xs);
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    flex-shrink: 0;
  }
  .s-sum {
    color: var(--muted);
    font-size: var(--text-xs);
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
  .seg.kind {
    grid-template-columns: repeat(2, 1fr);
  }
  .seg button {
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    padding: var(--space-1) 0;
    border-radius: 999px;
    cursor: pointer;
  }
  .seg button.on {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    color: var(--text);
  }
  .note {
    font-size: var(--text-xs);
    color: var(--muted);
    font-style: italic;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .row > span {
    font-size: var(--text-sm);
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
    font-size: var(--text-xs);
    color: var(--muted);
    min-width: 0;
  }
  .grid input,
  .grid select {
    width: 100%;
    min-width: 0;
  }
  /* Strip native number spinners so digits aren't clipped in tight cells. */
  input[type='number']::-webkit-outer-spin-button,
  input[type='number']::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  input[type='number'] {
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .check {
    flex-direction: row;
    align-items: center;
    gap: var(--space-2);
  }
  .warn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--amber);
  }
  .tiers {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .thead,
  .trow {
    display: grid;
    grid-template-columns: 1fr 1fr 24px;
    gap: var(--space-2);
    align-items: center;
  }
  .thead span {
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .trow input {
    width: 100%;
    min-width: 0;
  }
  .tdel {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    padding: 3px;
  }
  .tdel:hover {
    color: var(--red);
  }
  .tadd {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    align-self: flex-start;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
    margin-top: 2px;
  }
  .tadd:hover {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }
  .limits {
    border-top: 1px dashed var(--border);
    padding-top: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .lcap {
    font-size: 0.7rem;
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .subcap {
    font-size: 0.68rem;
    font-weight: var(--fw-semibold);
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--accent);
    margin-top: var(--space-1);
  }
  .subcap:not(:first-child) {
    border-top: 1px dashed var(--border);
    padding-top: var(--space-2);
  }
  .unit-field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-xs);
    color: var(--muted);
    min-width: 0;
  }
  .unit-field input {
    width: 100%;
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
    background: linear-gradient(135deg, var(--accent), var(--accent-hover));
    border: none;
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
    color: var(--accent-contrast);
    font-weight: var(--fw-semibold);
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
