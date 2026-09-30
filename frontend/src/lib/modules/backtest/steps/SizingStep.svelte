<script>
  // Step 3 — how big each position is and what it costs. Sizing mode + account (capital,
  // leverage, pyramiding), portfolio limits when several assets are selected, then fees and
  // spread: the two knobs that decide whether a paper edge survives contact with a broker.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { SIZING_MODES, defaultSizing, sizingNeedsStop, sideHasStop } from '../api.js';
  import { t } from '$lib/i18n';
  import './form.css';

  let { settings = $bindable(), multi = false } = $props();

  function setSizingMode(mode) {
    settings.sizing = defaultSizing(mode);
  }

  // Risk-based sizing needs a stop on the side(s) in play — otherwise there is no risk to size.
  const hasStop = $derived(
    (settings.mode !== 'short' && sideHasStop(settings.long)) ||
      (settings.mode !== 'long' && sideHasStop(settings.short))
  );
  const stopMissing = $derived(sizingNeedsStop(settings.sizing) && !hasStop);

  function addTier() {
    const tiers = settings.sizing.tiers ?? [];
    const last = tiers[tiers.length - 1];
    settings.sizing.tiers = [...tiers, { above: (last?.above ?? 0) + 1000, value: last?.value ?? 1 }];
  }
  function removeTier(i) {
    settings.sizing.tiers = settings.sizing.tiers.filter((_, j) => j !== i);
  }

  const setOpt = (obj, key) => (e) => (obj[key] = e.target.value === '' ? null : Number(e.target.value));
</script>

<div class="bt-step">
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.settings.sectionSizing')}</span>
    <div class="bt-grid">
      <div class="bt-field">{$t('backtest.settings.sizeBy')}
        <Dropdown
          value={settings.sizing.mode}
          onpick={setSizingMode}
          ariaLabel={$t('backtest.settings.sizeBy')}
          options={SIZING_MODES.map((m) => ({ value: m.id, label: m.label }))}
        />
      </div>

      {#if settings.sizing.mode === 'percent_equity'}
        <label class="bt-field">{$t('backtest.settings.percentInPct')}<input type="number" min="0" step="1" bind:value={settings.sizing.percent} /></label>
      {:else if settings.sizing.mode === 'fixed_qty'}
        <label class="bt-field">{$t('backtest.settings.quantityPerEntry')}<input type="number" min="0" step="any" bind:value={settings.sizing.qty} /></label>
      {:else if settings.sizing.mode === 'risk'}
        <label class="bt-field">{$t('backtest.settings.riskPct')}<input type="number" min="0" step="any" bind:value={settings.sizing.risk_pct} /></label>
      {:else if settings.sizing.mode === 'equity_tiers'}
        <div class="bt-field">{$t('backtest.settings.tierMetric')}
          <Dropdown
            bind:value={settings.sizing.metric}
            ariaLabel={$t('backtest.settings.tierMetric')}
            options={[
              { value: 'qty', label: $t('backtest.settings.fixedQty') },
              { value: 'risk_pct', label: $t('backtest.settings.riskPct') },
              { value: 'percent_equity', label: $t('backtest.settings.percentOfEquity') }
            ]}
          />
        </div>
      {:else if settings.sizing.mode === 'kelly'}
        <label class="bt-field">{$t('backtest.settings.kellyFraction')}<input type="number" min="0" max="1" step="0.05" bind:value={settings.sizing.fraction} /></label>
        <label class="bt-field">{$t('backtest.settings.kellyWindow')}<input type="number" min="1" step="1" bind:value={settings.sizing.window} /></label>
        <label class="bt-field">{$t('backtest.settings.kellyCap')}<input type="number" min="0" step="any" bind:value={settings.sizing.cap_pct} /></label>
        <label class="bt-field">{$t('backtest.settings.kellyWarmup')}<input type="number" min="0" step="any" bind:value={settings.sizing.warmup.percent} /></label>
      {/if}
    </div>

    {#if settings.sizing.mode === 'fixed_qty' && (settings.leverage ?? 1) > 1}
      <p class="bt-note">{$t('backtest.settings.qtyLeverageNote')}</p>
    {/if}
    {#if settings.sizing.mode === 'kelly'}
      <p class="bt-note">{$t('backtest.settings.kellyNote')}</p>
    {/if}

    {#if settings.sizing.mode === 'equity_tiers'}
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
      <p class="bt-note">{$t('backtest.settings.tierNote')}</p>
    {/if}

    {#if stopMissing}
      <p class="bt-warn"><Icon name="alert-triangle" size={12} /> {$t('backtest.settings.riskNeedsStop')}</p>
    {/if}
  </div>

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.step.accountCap')}</span>
    <div class="bt-grid">
      <label class="bt-field">{$t('backtest.settings.startingCapital')}<input type="number" min="0" step="any" bind:value={settings.starting_capital} /></label>
      <label class="bt-field">{$t('backtest.settings.leverage')}<input type="number" min="1" step="any" bind:value={settings.leverage} /></label>
      <label class="bt-field">{$t('backtest.settings.pyramiding')}
        <input type="number" min="1" max="20" step="1" bind:value={settings.pyramiding} title={$t('backtest.settings.pyramidingTitle')} />
      </label>
    </div>
    {#if (settings.pyramiding ?? 1) > 1}
      <p class="bt-note">{$t('backtest.settings.pyramidingNote', { count: settings.pyramiding })}</p>
    {/if}
  </div>

  {#if multi}
    <div class="bt-block">
      <span class="bt-cap">{$t('backtest.settings.portfolioLimits')}</span>
      <div class="bt-grid">
        <label class="bt-field">{$t('backtest.settings.maxOpen')}
          <input type="number" min="0" step="1" value={settings.risk.max_open_positions ?? ''} onchange={setOpt(settings.risk, 'max_open_positions')} />
        </label>
        <label class="bt-field">{$t('backtest.settings.maxExposure')}
          <input type="number" min="0" step="any" value={settings.risk.max_exposure_pct ?? ''} onchange={setOpt(settings.risk, 'max_exposure_pct')} />
        </label>
        <label class="bt-field">{$t('backtest.settings.maxExposurePerAsset')}
          <input type="number" min="0" step="any" value={settings.risk.max_exposure_per_asset_pct ?? ''} onchange={setOpt(settings.risk, 'max_exposure_per_asset_pct')} />
        </label>
      </div>
      <p class="bt-note">{$t('backtest.settings.limitsNote')}</p>
    </div>
  {/if}

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.settings.sectionCosts')}</span>
    <div class="bt-grid">
      <label class="bt-field">{$t('backtest.settings.feeAmount')}<input type="number" min="0" step="any" bind:value={settings.fees.amount} /></label>
      <div class="bt-field">{$t('backtest.settings.kind')}
        <Dropdown
          bind:value={settings.fees.amount_kind}
          ariaLabel={$t('backtest.settings.kind')}
          options={[
            { value: 'pct', label: $t('backtest.settings.pctOfNotional') },
            { value: 'fixed', label: $t('backtest.settings.fixed') }
          ]}
        />
      </div>
      <div class="bt-field">{$t('backtest.settings.per')}
        <Dropdown
          bind:value={settings.fees.per}
          ariaLabel={$t('backtest.settings.per')}
          options={[
            { value: 'trade', label: $t('backtest.settings.perTrade') },
            { value: 'unit', label: $t('backtest.settings.perUnit') }
          ]}
        />
      </div>
      <label class="bt-field">{$t('backtest.settings.spreadPct')}
        <input type="number" min="0" step="any" value={settings.spread_pct * 100}
          onchange={(e) => (settings.spread_pct = (Number(e.target.value) || 0) / 100)} />
      </label>
    </div>
  </div>
</div>

<style>
  .tiers {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-width: 420px;
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
    border: var(--hairline) solid var(--border);
    border-radius: 0;
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
    background: transparent;
    border: var(--hairline) dashed var(--border-control);
    border-radius: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
    margin-top: 2px;
  }
  .tadd:hover {
    border-style: solid;
    color: var(--text);
  }
</style>
