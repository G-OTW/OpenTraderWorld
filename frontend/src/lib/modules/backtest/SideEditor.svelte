<script>
  // Configures one side (long or short): its entry condition group, optional exit condition
  // group, stop-loss, take-profit, and signal-exit. Bound via $bindable so edits flow back
  // into the parent settings object. SL/TP are shown as percentages, stored as fractions.
  import SignalGroupEditor from './SignalGroupEditor.svelte';
  import { t } from '$lib/i18n';

  let { side = $bindable(), sideId = 'long', disabled = false, customIndicators = [] } = $props();

  const isLong = $derived(sideId === 'long');
  const label = $derived(isLong ? $t('backtest.side.long') : $t('backtest.side.short'));

  // SL/TP editors. A stop is either a percent (of avg entry) or an ATR multiple. The engine
  // reads the object form (`side.stop_loss`) when present, falling back to legacy `_pct`.
  // For pct we mirror into `stop_loss_pct` too, so old saved runs keep deserializing.
  const slKind = $derived(side.stop_loss?.kind ?? 'pct');
  const tpKind = $derived(side.take_profit?.kind ?? 'pct');

  // Display value: for pct show as a percentage (0.02 → 2); for atr show the raw multiple.
  // A stop is *enabled* when its object exists (or the legacy `_pct` is > 0). Disabling clears the
  // stored value to null (engine reads that as "no stop") while keeping the last display value in
  // `slVal`/`tpVal` so re-enabling restores it rather than starting from blank.
  let slVal = $state(0);
  let tpVal = $state(0);
  const slOn = $derived(side.stop_loss != null || (side.stop_loss_pct ?? 0) > 0);
  const tpOn = $derived(side.take_profit != null || (side.take_profit_pct ?? 0) > 0);
  $effect(() => {
    const sl = side.stop_loss;
    if (sl) slVal = sl.kind === 'pct' ? (sl.value ?? 0) * 100 : (sl.value ?? 0);
    else if ((side.stop_loss_pct ?? 0) > 0) slVal = side.stop_loss_pct * 100;
    const tp = side.take_profit;
    if (tp) tpVal = tp.kind === 'pct' ? (tp.value ?? 0) * 100 : (tp.value ?? 0);
    else if ((side.take_profit_pct ?? 0) > 0) tpVal = side.take_profit_pct * 100;
  });

  function writeStop(which, kind, displayVal) {
    const v = Number(displayVal) || 0;
    if (kind === 'pct') {
      const frac = v / 100;
      if (which === 'sl') {
        side.stop_loss = frac > 0 ? { kind: 'pct', value: frac, period: 0 } : null;
        side.stop_loss_pct = frac; // keep legacy mirror
      } else {
        side.take_profit = frac > 0 ? { kind: 'pct', value: frac, period: 0 } : null;
        side.take_profit_pct = frac;
      }
    } else {
      // ATR multiple; period default 14 (user can tune below).
      const cur = which === 'sl' ? side.stop_loss : side.take_profit;
      const obj = { kind: 'atr', value: v, period: cur?.period || 14 };
      if (which === 'sl') side.stop_loss = v > 0 ? obj : null;
      else side.take_profit = v > 0 ? obj : null;
    }
  }
  function setKind(which, kind) {
    const displayVal = which === 'sl' ? slVal : tpVal;
    writeStop(which, kind, displayVal);
  }
  // Enable/disable a stop without losing the typed value. Off → null (mirror 0). On → re-apply the
  // remembered display value, defaulting to a sensible non-zero so it actually takes effect.
  function toggleStop(which, on) {
    if (!on) {
      if (which === 'sl') {
        side.stop_loss = null;
        side.stop_loss_pct = 0;
      } else {
        side.take_profit = null;
        side.take_profit_pct = 0;
      }
      return;
    }
    const kind = which === 'sl' ? slKind : tpKind;
    let display = which === 'sl' ? slVal : tpVal;
    if (!(Number(display) > 0)) {
      display = kind === 'pct' ? (which === 'sl' ? 2 : 4) : 2; // defaults: 2%/4% or 2× ATR
      if (which === 'sl') slVal = display;
      else tpVal = display;
    }
    writeStop(which, kind, display);
  }
  function setPeriod(which, period) {
    const cur = which === 'sl' ? side.stop_loss : side.take_profit;
    if (cur?.kind === 'atr') {
      const obj = { ...cur, period: Math.max(1, Number(period) || 14) };
      if (which === 'sl') side.stop_loss = obj;
      else side.take_profit = obj;
    }
  }
</script>

<fieldset class="side" class:short={!isLong} {disabled}>
  <legend><span class="dot"></span>{label}</legend>

  <SignalGroupEditor
    bind:group={side.entry}
    title={$t('backtest.side.entryWhen')}
    defaultOp={isLong ? 'crosses_above' : 'crosses_below'}
    {customIndicators}
  />

  <SignalGroupEditor
    bind:group={side.exit}
    title={$t('backtest.side.exitWhen')}
    defaultOp={isLong ? 'crosses_below' : 'crosses_above'}
    emptyHint={$t('backtest.side.noExitRulesHint')}
    {customIndicators}
  />

  <div class="risk">
    <div class="stop-field" class:off={!slOn}>
      <div class="stop-head">
        <label class="stop-toggle">
          <input type="checkbox" checked={slOn} onchange={(e) => toggleStop('sl', e.target.checked)} />
          <span>{$t('backtest.side.stopLoss')}</span>
        </label>
        <div class="kind-seg">
          <button type="button" class:on={slKind === 'pct'} disabled={!slOn} onclick={() => setKind('sl', 'pct')}>%</button>
          <button type="button" class:on={slKind === 'atr'} disabled={!slOn} onclick={() => setKind('sl', 'atr')}>ATR</button>
        </div>
      </div>
      <span class="unit">
        <input type="number" min="0" step="0.1" bind:value={slVal} disabled={!slOn}
          onchange={() => writeStop('sl', slKind, slVal)} />
        <span class="u">{slKind === 'pct' ? '%' : '×'}</span>
      </span>
      {#if slKind === 'atr' && slOn}
        <input class="period" type="number" min="1" step="1" value={side.stop_loss?.period ?? 14}
          onchange={(e) => setPeriod('sl', e.target.value)} title={$t('backtest.side.atrPeriod')} />
      {/if}
    </div>
    <div class="stop-field" class:off={!tpOn}>
      <div class="stop-head">
        <label class="stop-toggle">
          <input type="checkbox" checked={tpOn} onchange={(e) => toggleStop('tp', e.target.checked)} />
          <span>{$t('backtest.side.takeProfit')}</span>
        </label>
        <div class="kind-seg">
          <button type="button" class:on={tpKind === 'pct'} disabled={!tpOn} onclick={() => setKind('tp', 'pct')}>%</button>
          <button type="button" class:on={tpKind === 'atr'} disabled={!tpOn} onclick={() => setKind('tp', 'atr')}>ATR</button>
        </div>
      </div>
      <span class="unit">
        <input type="number" min="0" step="0.1" bind:value={tpVal} disabled={!tpOn}
          onchange={() => writeStop('tp', tpKind, tpVal)} />
        <span class="u">{tpKind === 'pct' ? '%' : '×'}</span>
      </span>
      {#if tpKind === 'atr' && tpOn}
        <input class="period" type="number" min="1" step="1" value={side.take_profit?.period ?? 14}
          onchange={(e) => setPeriod('tp', e.target.value)} title={$t('backtest.side.atrPeriod')} />
      {/if}
    </div>
  </div>
  <label class="check">
    <input type="checkbox" bind:checked={side.exit_on_reverse} />
    {$t('backtest.side.exitOnReverseHint')}
  </label>
</fieldset>

<style>
  .side {
    --side-color: var(--green);
    border: 1px solid var(--border);
    border-left: 3px solid color-mix(in srgb, var(--side-color) 55%, transparent);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    background: color-mix(in srgb, var(--side-color) 3%, transparent);
  }
  .side.short {
    --side-color: var(--red);
  }
  .side[disabled] {
    opacity: 0.45;
  }
  legend {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--side-color);
    padding: 0 var(--space-1);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--side-color);
  }
  .risk {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-2);
  }
  .stop-field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: var(--text-xs);
    color: var(--muted);
    min-width: 0;
  }
  .stop-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .stop-toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    cursor: pointer;
    user-select: none;
  }
  .stop-toggle input {
    accent-color: var(--accent);
    cursor: pointer;
  }
  /* When a stop is off, dim its value/period inputs but keep the checkbox fully legible. */
  .stop-field.off .unit,
  .stop-field.off .period,
  .stop-field.off .kind-seg {
    opacity: 0.4;
  }
  .kind-seg {
    display: inline-flex;
    background: var(--surface-2);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: 1px;
  }
  .kind-seg button {
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 0.66rem;
    font-weight: var(--fw-medium);
    padding: 1px 7px;
    border-radius: 0;
    cursor: pointer;
  }
  .kind-seg button.on {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    color: var(--text);
  }
  .period {
    width: 100%;
    font-size: var(--text-xs);
  }
  input[type='number']::-webkit-outer-spin-button,
  input[type='number']::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  input[type='number'] {
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .unit {
    display: flex;
    align-items: center;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding-right: var(--space-2);
    transition: border-color var(--dur-fast) var(--ease);
  }
  .unit:focus-within {
    border-color: var(--accent);
  }
  .unit input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
  }
  .u {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .check {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
