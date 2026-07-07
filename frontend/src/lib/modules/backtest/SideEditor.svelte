<script>
  // Configures one side (long or short): its entry condition group, optional exit condition
  // group, stop-loss, take-profit, and signal-exit. Bound via $bindable so edits flow back
  // into the parent settings object. SL/TP are shown as percentages, stored as fractions.
  import SignalGroupEditor from './SignalGroupEditor.svelte';
  import { t } from '$lib/i18n';

  let { side = $bindable(), sideId = 'long', disabled = false } = $props();

  const isLong = $derived(sideId === 'long');
  const label = $derived(isLong ? $t('backtest.side.long') : $t('backtest.side.short'));

  let slPct = $state(0);
  let tpPct = $state(0);
  $effect(() => {
    slPct = (side.stop_loss_pct ?? 0) * 100;
    tpPct = (side.take_profit_pct ?? 0) * 100;
  });
</script>

<fieldset class="side" class:short={!isLong} {disabled}>
  <legend><span class="dot"></span>{label}</legend>

  <SignalGroupEditor
    bind:group={side.entry}
    title={$t('backtest.side.entryWhen')}
    defaultOp={isLong ? 'crosses_above' : 'crosses_below'}
  />

  <SignalGroupEditor
    bind:group={side.exit}
    title={$t('backtest.side.exitWhen')}
    defaultOp={isLong ? 'crosses_below' : 'crosses_above'}
    emptyHint={$t('backtest.side.noExitRulesHint')}
  />

  <div class="risk">
    <label class="unit-field">
      <span>{$t('backtest.side.stopLoss')}</span>
      <span class="unit">
        <input type="number" min="0" step="0.1" bind:value={slPct}
          onchange={() => (side.stop_loss_pct = (Number(slPct) || 0) / 100)} />
        <span class="u">%</span>
      </span>
    </label>
    <label class="unit-field">
      <span>{$t('backtest.side.takeProfit')}</span>
      <span class="unit">
        <input type="number" min="0" step="0.1" bind:value={tpPct}
          onchange={() => (side.take_profit_pct = (Number(tpPct) || 0) / 100)} />
        <span class="u">%</span>
      </span>
    </label>
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
    font-size: 0.72rem;
    font-weight: 700;
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
  .unit-field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 0.75rem;
    color: var(--muted);
    min-width: 0;
  }
  .unit {
    display: flex;
    align-items: center;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding-right: var(--space-2);
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .unit:focus-within {
    border-color: var(--accent);
    box-shadow: var(--ring);
  }
  .unit input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
  }
  .unit input:focus {
    box-shadow: none;
  }
  .u {
    font-size: 0.75rem;
    color: var(--muted);
  }
  .check {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.78rem;
    color: var(--muted);
  }
</style>
