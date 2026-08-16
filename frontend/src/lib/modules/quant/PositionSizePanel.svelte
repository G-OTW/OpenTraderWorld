<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Position Size calculator. Risk-based sizing: stack + risk (% or fixed) + entry/stop → qty,
  // notional, margin, R:R. Optionally derives the stop from a downloaded dataset (HV/ATR/swing).
  import { quantApi, fmtRatioPct, fmtNum } from '$lib/modules/quant/api.js';
  import DatasetPicker from '$lib/modules/quant/DatasetPicker.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { datasets = [] } = $props();

  // ── Inputs (persisted to localStorage across refreshes) ─────────────────────
  const STORE_KEY = 'quant.positionSize';
  const DEFAULTS = {
    side: 'long',
    stack: 10000,
    riskMode: 'pct', // 'pct' | 'amount'
    riskPct: 1, // percent
    riskAmount: 100,
    entry: 100,
    stop: 95,
    useLeverage: false,
    leverage: 2,
    useTarget: false,
    target: 110
  };
  function loadSaved() {
    try {
      return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(STORE_KEY) || '{}') };
    } catch {
      return { ...DEFAULTS };
    }
  }
  const saved = loadSaved();

  let side = $state(saved.side);
  let stack = $state(saved.stack);
  let riskMode = $state(saved.riskMode);
  let riskPct = $state(saved.riskPct);
  let riskAmount = $state(saved.riskAmount);
  let entry = $state(saved.entry);
  let stop = $state(saved.stop);
  let useLeverage = $state(saved.useLeverage);
  let leverage = $state(saved.leverage);
  let useTarget = $state(saved.useTarget);
  let target = $state(saved.target);

  // Persist inputs on any change.
  $effect(() => {
    const snap = {
      side,
      stack,
      riskMode,
      riskPct,
      riskAmount,
      entry,
      stop,
      useLeverage,
      leverage,
      useTarget,
      target
    };
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify(snap));
    } catch {
      /* non-fatal */
    }
  });

  let result = $state(null);
  let error = $state('');

  const body = $derived({
    stack: Number(stack) || 0,
    side,
    entry: Number(entry) || 0,
    stop: Number(stop) || 0,
    ...(riskMode === 'amount'
      ? { risk_amount: Number(riskAmount) || 0 }
      : { risk_pct: (Number(riskPct) || 0) / 100 }),
    ...(useLeverage ? { leverage: Number(leverage) || 0 } : {}),
    ...(useTarget ? { target: Number(target) || 0 } : {})
  });

  async function calc() {
    error = '';
    if (body.entry <= 0) {
      result = null;
      return;
    }
    try {
      result = await quantApi.size(body);
    } catch (e) {
      error = e.message;
      result = null;
    }
  }

  // Live recompute as inputs change.
  $effect(() => {
    void body;
    calc();
  });

  // ── Asset-derived stop ────────────────────────────────────────────────────
  let dsId = $state(null);
  let sig = $state(null); // { ticker, timeframe, signals }
  let sigBusy = $state(false);
  let sigError = $state('');

  async function loadSignals() {
    if (!dsId) return;
    sigError = '';
    sigBusy = true;
    try {
      sig = await quantApi.assetSignals(dsId, side, Number(entry) || 0);
    } catch (e) {
      sigError = e.message;
      sig = null;
    } finally {
      sigBusy = false;
    }
  }

  // Re-fetch when dataset, side, or entry change (entry moves the placed stop price).
  $effect(() => {
    void dsId;
    void side;
    void entry;
    if (dsId) loadSignals();
    else sig = null;
  });

  function applyStop(s) {
    stop = Number(s.stop_price.toFixed(6));
  }

  // Fill entry with the asset's latest close so the suggested stops land on real prices.
  function useLastAsEntry() {
    if (sig?.signals?.last_close) entry = Number(sig.signals.last_close.toFixed(6));
  }
</script>

<div class="wrap">
  <section class="calc">
    <div class="side-toggle">
      <button class:active={side === 'long'} onclick={() => (side = 'long')}>{$t('quant.size.long')}</button>
      <button class:active={side === 'short'} onclick={() => (side = 'short')}>{$t('quant.size.short')}</button>
    </div>

    <div class="grid">
      <label>
        <span>{$t('quant.size.stack')}</span>
        <input type="number" min="0" step="100" bind:value={stack} />
      </label>

      <label class="risk">
        <span>
          {$t('quant.size.risk')}
          <span class="risk-modes">
            <button class:active={riskMode === 'pct'} onclick={() => (riskMode = 'pct')}>%</button>
            <button class:active={riskMode === 'amount'} onclick={() => (riskMode = 'amount')}>
              {$t('quant.size.amount')}
            </button>
          </span>
        </span>
        {#if riskMode === 'pct'}
          <input type="number" min="0" step="0.1" bind:value={riskPct} />
        {:else}
          <input type="number" min="0" step="10" bind:value={riskAmount} />
        {/if}
      </label>

      <label>
        <span>{$t('quant.size.entry')}</span>
        <input type="number" min="0" step="0.01" bind:value={entry} />
      </label>
      <label>
        <span>{$t('quant.size.stop')}</span>
        <input type="number" min="0" step="0.01" bind:value={stop} />
      </label>

      <label class="opt">
        <span><input type="checkbox" bind:checked={useLeverage} /> {$t('quant.size.leverage')}</span>
        <input type="number" min="1" step="0.5" bind:value={leverage} disabled={!useLeverage} />
      </label>
      <label class="opt">
        <span><input type="checkbox" bind:checked={useTarget} /> {$t('quant.size.target')}</span>
        <input type="number" min="0" step="0.01" bind:value={target} disabled={!useTarget} />
      </label>
    </div>

    <ErrorText error={error} copyable />

    {#if result}
      <div class="out">
        <div class="big" class:neg={result.quantity <= 0}>
          <span class="lbl">{$t('quant.size.positionSize')}</span>
          <span class="v">{fmtNum(result.quantity, 4)}</span>
          <span class="unit">{$t('quant.size.units')}</span>
        </div>
        <div class="stats">
          <div><span>{$t('quant.size.notional')}</span><b>{fmtNum(result.notional, 2)}</b></div>
          <div><span>{$t('quant.size.riskAmount')}</span><b>{fmtNum(result.risk_amount, 2)}</b></div>
          <div><span>{$t('quant.size.riskOfStack')}</span><b>{fmtRatioPct(result.risk_fraction, 2)}</b></div>
          <div><span>{$t('quant.size.exposure')}</span><b>{fmtRatioPct(result.exposure_fraction, 1)}</b></div>
          <div><span>{$t('quant.size.stopDistance')}</span><b>{fmtNum(result.stop_distance, 4)}</b></div>
          {#if result.margin_required != null}
            <div><span>{$t('quant.size.marginNeeded')}</span><b>{fmtNum(result.margin_required, 2)}</b></div>
          {/if}
          {#if result.reward_risk != null}
            <div><span>{$t('quant.size.rewardRisk')}</span><b>{fmtNum(result.reward_risk, 2)}</b></div>
          {/if}
          {#if result.reward_amount != null}
            <div><span>{$t('quant.size.rewardAmount')}</span><b>{fmtNum(result.reward_amount, 2)}</b></div>
          {/if}
        </div>
        {#each result.warnings as w (w)}
          <p class="warn"><Icon name="alert-triangle" size={12} /> {w}</p>
        {/each}
      </div>
    {/if}
  </section>

  <!-- ── Derive stop from an asset ──────────────────────────────────────── -->
  <section class="asset">
    <h3>{$t('quant.size.suggestStopTitle')}</h3>
    <p class="muted small">
      {$t('quant.size.suggestStopBody')}
    </p>
    {#if datasets.length}
      <DatasetPicker bind:value={dsId} {datasets} />
    {:else}
      <p class="muted small">{$t('quant.size.noDatasets')}</p>
    {/if}

    <ErrorText error={sigError} copyable />

    {#if sigBusy}
      <p class="muted small">{$t('quant.size.analyzing')}</p>
    {:else if sig}
      <div class="sig-head">
        <div class="sig-title-row">
          <span class="title">{sig.ticker} · {sig.timeframe}</span>
          <button class="use-last" onclick={useLastAsEntry} title={$t('quant.size.copyLastCloseTitle')}>
            {$t('quant.size.useLastAsEntry')}
          </button>
        </div>
        <span class="muted small">
          {$t('quant.size.signalsSummary', {
            hv: fmtRatioPct(sig.signals.hv_period, 2),
            atr: fmtNum(sig.signals.atr, 4),
            last: fmtNum(sig.signals.last_close, 4)
          })}
        </span>
        {#if entry > 0 && sig.signals.last_close > 0 && Math.abs(entry / sig.signals.last_close - 1) > 0.2}
          <span class="mismatch small">
            {$t('quant.size.entryMismatch')}
          </span>
        {/if}
      </div>
      <div class="sugg">
        {#each sig.signals.suggestions as s (s.key)}
          <button class="pill" onclick={() => applyStop(s)} title={$t('quant.size.setAsStopTitle')}>
            <span class="pill-lbl">{s.label}</span>
            <span class="pill-val">
              {$t('quant.size.stopSuggestion', { stop: fmtNum(s.stop_price, 4), pct: fmtRatioPct(s.distance_pct, 2) })}
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </section>
</div>

<style>
  .wrap {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
    align-items: start;
  }
  @media (max-width: 900px) {
    .wrap {
      grid-template-columns: 1fr;
    }
  }
  .calc,
  .asset {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .asset {
    border-left: var(--hairline) solid var(--border);
    padding-left: var(--space-4);
  }
  .asset h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .side-toggle {
    display: flex;
    gap: var(--space-1);
  }
  .side-toggle button {
    flex: 1;
    border: var(--hairline) solid var(--border);
    background: transparent;
    color: var(--muted);
    border-radius: var(--radius);
    border-bottom: var(--active-rule) solid transparent;
    padding: var(--space-2);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .side-toggle button.active {
    background: var(--surface-2);
    border-bottom-color: var(--accent);
    color: var(--text);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  label > span {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .risk-modes {
    margin-left: auto;
    display: flex;
    gap: 2px;
  }
  .risk-modes button {
    border: var(--hairline) solid var(--border);
    background: transparent;
    color: var(--muted);
    border-radius: var(--radius);
    border-bottom: var(--active-rule) solid transparent;
    padding: 0 var(--space-1);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .risk-modes button.active {
    background: var(--surface-2);
    border-bottom-color: var(--accent);
    color: var(--text);
  }
  .opt span {
    font-size: var(--text-sm);
  }
  .big {
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
    padding: var(--space-4);
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .big .lbl {
    font-size: var(--text-sm);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    width: 100%;
  }
  .big .v {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xl);
    font-weight: var(--fw-medium);
    color: var(--green);
  }
  .big.neg .v {
    color: var(--red);
  }
  .big .unit {
    color: var(--muted);
    font-size: var(--text-base);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: var(--space-3);
    margin-top: var(--space-3);
  }
  .stats div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .stats b {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-weight: var(--fw-medium);
    font-size: var(--text-md);
    color: var(--text);
  }
  .warn {
    color: var(--amber);
    font-size: var(--text-sm);
  }
  .sig-head {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .sig-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .sig-head .title {
    font-weight: var(--fw-medium);
  }
  .use-last {
    border: var(--hairline) solid var(--border);
    background: var(--surface-2);
    color: var(--text);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .use-last:hover {
    border-color: var(--border-control);
    background: var(--surface-2);
  }
  .mismatch {
    color: var(--amber);
    margin-top: var(--space-1);
    line-height: 1.35;
  }
  .sugg {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .pill {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    border: var(--hairline) solid var(--border);
    background: var(--surface-2);
    color: var(--text);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    text-align: left;
  }
  .pill:hover {
    border-color: var(--border-control);
  }
  .pill-lbl {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  .pill-val {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
</style>
