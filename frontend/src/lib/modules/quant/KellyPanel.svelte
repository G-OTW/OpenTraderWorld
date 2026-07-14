<script>
  // Kelly Criterion calculator from manual inputs: win rate, average win, average loss.
  // Shows full / half / quarter Kelly. Half-Kelly is the common practical pick.
  import { quantApi, fmtRatioPct, fmtNum } from '$lib/modules/quant/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let winRate = $state(55); // percent
  let avgWin = $state(1.5);
  let avgLoss = $state(1.0);
  let result = $state(null);
  let error = $state('');

  async function calc() {
    error = '';
    try {
      result = await quantApi.kelly(winRate / 100, avgWin, avgLoss);
    } catch (e) {
      error = e.message;
      result = null;
    }
  }

  // Auto-recalc on input change.
  $effect(() => {
    winRate;
    avgWin;
    avgLoss;
    calc();
  });
</script>

<div class="kelly">
  <div class="inputs">
    <label>
      <span>{$t('quant.kelly.winRate')}</span>
      <input type="number" min="0" max="100" step="1" bind:value={winRate} />
    </label>
    <label>
      <span>{$t('quant.kelly.avgWin')}</span>
      <input type="number" min="0" step="0.1" bind:value={avgWin} />
    </label>
    <label>
      <span>{$t('quant.kelly.avgLoss')}</span>
      <input type="number" min="0" step="0.1" bind:value={avgLoss} />
    </label>
  </div>

  <ErrorText error={error} copyable />

  {#if result}
    <div class="out">
      <div class="big" class:neg={result.kelly < 0}>
        <span class="lbl">{$t('quant.kelly.suggestedSize')}</span>
        <span class="v">{fmtRatioPct(result.kelly_clamped, 1)}</span>
        {#if result.kelly < 0}<span class="warn">{$t('quant.kelly.negativeEdge')}</span>{/if}
      </div>
      <div class="frac">
        <div><span>{$t('quant.kelly.halfKelly')}</span><b>{fmtRatioPct(result.half_kelly, 1)}</b></div>
        <div><span>{$t('quant.kelly.quarterKelly')}</span><b>{fmtRatioPct(result.quarter_kelly, 1)}</b></div>
        <div><span>{$t('quant.kelly.payoff')}</span><b>{fmtNum(result.payoff, 2)}</b></div>
      </div>
      <p class="note">
        {$t('quant.kelly.note')}
      </p>
    </div>
  {/if}
</div>

<style>
  .kelly {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .inputs {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--space-3);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .big {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .big .lbl {
    font-size: var(--text-sm);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .big .v {
    font-size: var(--text-xl);
    font-weight: var(--fw-semibold);
    color: var(--green);
  }
  .big.neg .v {
    color: var(--red);
  }
  .warn {
    color: var(--red);
    font-size: var(--text-base);
  }
  .frac {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--space-3);
  }
  .frac div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .frac b {
    font-size: var(--text-md);
    color: var(--text);
  }
  .note {
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: 1.4;
  }
</style>
