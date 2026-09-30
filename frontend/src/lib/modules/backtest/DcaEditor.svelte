<script>
  // The savings plan, in the order it is decided: the basket and its fixed weights, the cash
  // it starts with, the recurring contribution, then the conditional tranches (buy) and the
  // rules that take money back out (sell).
  //
  // Weights are per ticker and come from the datasets picked in step 1, so a plan always names
  // the basket it is about. Editing one asset's weight leaves the others alone; the share each
  // one takes is the weight over their sum, shown live next to the input.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import SignalGroupEditor from './SignalGroupEditor.svelte';
  import {
    DCA_PERIODS,
    DCA_BUY_KINDS,
    DCA_SELL_KINDS,
    defaultContribution,
    defaultDcaBuy,
    defaultDcaSell,
    barsPerSpan,
    fmtNum
  } from './api.js';
  import { t } from '$lib/i18n';
  import './steps/form.css';

  // Starting capital lives in the Sizing step, but a savings plan is entirely read against it,
  // so the DCA editor shows the same field: it is the money the plan starts with.
  let {
    dca = $bindable(),
    capital = $bindable(0),
    assets = [],
    timeframe = '',
    customIndicators = []
  } = $props();

  const tickers = $derived([...new Set(assets.map((a) => a.ticker).filter(Boolean))]);

  // One weight row per selected asset, created on sight and never for an asset that is gone:
  // a stale row would take a share of every tranche without appearing anywhere.
  $effect(() => {
    const want = tickers;
    const have = dca.weights ?? [];
    const kept = have.filter((w) => want.includes(w.ticker));
    const added = want
      .filter((tk) => !kept.some((w) => w.ticker === tk))
      .map((tk) => ({ ticker: tk, weight: 1 }));
    if (added.length || kept.length !== have.length) dca.weights = [...kept, ...added];
  });

  const weightSum = $derived((dca.weights ?? []).reduce((s, w) => s + (Number(w.weight) || 0), 0));
  const share = (w) => (weightSum > 0 ? ((Number(w.weight) || 0) / weightSum) * 100 : 0);
  function equalize() {
    for (const w of dca.weights ?? []) w.weight = 1;
  }

  const hasContribution = $derived(!!dca.contribution);
  function toggleContribution() {
    dca.contribution = dca.contribution ? null : defaultContribution();
  }

  // "1 month" is a number of bars, and only the timeframe knows how many. The presets fill
  // the period of a `change_pct` operand rather than inventing a calendar the engine lacks.
  const SPANS = ['1W', '1M', '3M', '1Y'];
  const spanBars = $derived(Object.fromEntries(SPANS.map((s) => [s, barsPerSpan(timeframe, s)])));
  function applySpan(rule, span) {
    const n = spanBars[span];
    if (!n) return;
    for (const c of rule.condition?.conditions ?? []) {
      for (const side of ['left', 'right']) {
        if (c[side]?.kind === 'metric' && c[side].metric === 'change_pct') c[side].period = n;
      }
    }
  }
  const usesChangePct = (rule) =>
    (rule.condition?.conditions ?? []).some(
      (c) => c.left?.metric === 'change_pct' || c.right?.metric === 'change_pct'
    );

  // The kind decides the unit, so it is picked first and the number's label states what it is:
  // "500" alone reads as euros or as a percent depending on it.
  const buyAmountLabel = (kind) =>
    kind === 'fixed' ? $t('backtest.dca.amountCash') : $t('backtest.dca.percent');
  const sellAmountLabel = (kind) =>
    kind === 'units'
      ? $t('backtest.dca.units')
      : kind === 'amount'
        ? $t('backtest.dca.amountCash')
        : $t('backtest.dca.percent');

  const addBuy = () => (dca.buys = [...(dca.buys ?? []), defaultDcaBuy()]);
  const addSell = () => (dca.sells = [...(dca.sells ?? []), defaultDcaSell()]);
  const removeBuy = (i) => (dca.buys = dca.buys.filter((_, k) => k !== i));
  const removeSell = (i) => (dca.sells = dca.sells.filter((_, k) => k !== i));
</script>

<div class="bt-block">
  <span class="bt-cap">{$t('backtest.dca.basketCap')}</span>
  <p class="bt-note">{$t('backtest.dca.basketHint')}</p>
  {#if !tickers.length}
    <p class="bt-note">{$t('backtest.dca.noAssets')}</p>
  {:else}
    <div class="weights">
      {#each dca.weights ?? [] as w (w.ticker)}
        <label class="wrow">
          <span class="tk">{w.ticker}</span>
          <input type="number" min="0" step="any" bind:value={w.weight} aria-label={w.ticker} />
          <span class="pct">{fmtNum(share(w), 1)}%</span>
        </label>
      {/each}
    </div>
    <button type="button" class="mini" onclick={equalize}>{$t('backtest.dca.equalWeights')}</button>
  {/if}
</div>

<div class="bt-block">
  <span class="bt-cap">{$t('backtest.dca.stackCap')}</span>
  <div class="bt-grid">
    <label class="bt-field"
      >{$t('backtest.settings.startingCapital')}<input
        type="number"
        min="0"
        step="any"
        bind:value={capital} /></label>
  </div>
  <p class="bt-note">{$t('backtest.dca.capitalHint')}</p>
</div>

<div class="bt-block">
  <div class="head">
    <span class="bt-cap">{$t('backtest.dca.contributionCap')}</span>
    <label class="bt-check">
      <input type="checkbox" checked={hasContribution} onchange={toggleContribution} />
      {$t('backtest.dca.enable')}
    </label>
  </div>
  {#if hasContribution}
    <div class="bt-grid">
      <label class="bt-field"
        >{$t('backtest.dca.amount')}<input type="number" min="0" step="any" bind:value={dca.contribution.amount} /></label>
      <label class="bt-field"
        >{$t('backtest.dca.every')}<input type="number" min="1" step="1" bind:value={dca.contribution.every} /></label>
      <div class="bt-field">
        {$t('backtest.dca.period')}
        <Dropdown
          bind:value={dca.contribution.period}
          ariaLabel={$t('backtest.dca.period')}
          options={DCA_PERIODS.map((p) => ({ value: p, label: $t(`backtest.dca.period_${p}`) }))}
        />
      </div>
    </div>
    <label class="bt-check">
      <input type="checkbox" bind:checked={dca.contribution.invest} />
      {$t('backtest.dca.contribInvest')}
    </label>
    <p class="bt-note">{$t('backtest.dca.contributionHint')}</p>
  {:else}
    <p class="bt-note">{$t('backtest.dca.contributionOff')}</p>
  {/if}
</div>

<div class="bt-block">
  <div class="head">
    <span class="bt-cap">{$t('backtest.dca.buysCap')}</span>
    <button type="button" class="mini" onclick={addBuy}><Icon name="plus" size={11} /> {$t('backtest.dca.addBuy')}</button>
  </div>
  <p class="bt-note">{$t('backtest.dca.buysHint')}</p>
  {#each dca.buys ?? [] as rule, i (rule)}
    <div class="rule">
      <div class="rhead">
        <input
          class="name"
          type="text"
          bind:value={rule.name}
          placeholder={$t('backtest.dca.rulePlaceholder', { n: i + 1 })}
          aria-label={$t('backtest.dca.ruleName')} />
        <button type="button" class="rm" onclick={() => removeBuy(i)} title={$t('backtest.dca.removeRule')}>
          <Icon name="x" size={12} />
        </button>
      </div>
      <div class="bt-grid">
        <div class="bt-field">
          {$t('backtest.dca.buyWhat')}
          <Dropdown
            bind:value={rule.amount_kind}
            ariaLabel={$t('backtest.dca.buyWhat')}
            options={DCA_BUY_KINDS.map((k) => ({ value: k, label: $t(`backtest.dca.buyKind_${k}`) }))}
          />
        </div>
        <label class="bt-field"
          >{buyAmountLabel(rule.amount_kind)}<input
            type="number"
            min="0"
            step="any"
            bind:value={rule.amount} /></label>
        <label class="bt-field"
          >{$t('backtest.dca.maxFires')}<input type="number" min="0" step="1" bind:value={rule.max_fires} /></label>
        <label class="bt-field"
          >{$t('backtest.dca.cooldown')}<input type="number" min="0" step="1" bind:value={rule.cooldown_bars} /></label>
      </div>
      <label class="bt-check">
        <input type="checkbox" bind:checked={rule.per_asset} />
        {$t('backtest.dca.perAsset')}
      </label>
      <SignalGroupEditor
        bind:group={rule.condition}
        title={$t('backtest.dca.when')}
        defaultOp="below"
        positions
        {customIndicators}
        emptyHint={$t('backtest.dca.noCondition')} />
      {#if usesChangePct(rule)}
        <div class="spans">
          <span class="lbl">{$t('backtest.dca.spanPreset')}</span>
          {#each SPANS as s (s)}
            <button type="button" class="mini" disabled={!spanBars[s]} onclick={() => applySpan(rule, s)}>{s}</button>
          {/each}
          {#if !spanBars['1M']}<span class="bt-note">{$t('backtest.dca.spanUnknown')}</span>{/if}
        </div>
      {/if}
    </div>
  {/each}
</div>

<div class="bt-block">
  <div class="head">
    <span class="bt-cap">{$t('backtest.dca.sellsCap')}</span>
    <button type="button" class="mini" onclick={addSell}><Icon name="plus" size={11} /> {$t('backtest.dca.addSell')}</button>
  </div>
  <p class="bt-note">{$t('backtest.dca.sellsHint')}</p>
  {#each dca.sells ?? [] as rule, i (rule)}
    <div class="rule">
      <div class="rhead">
        <input
          class="name"
          type="text"
          bind:value={rule.name}
          placeholder={$t('backtest.dca.sellPlaceholder', { n: i + 1 })}
          aria-label={$t('backtest.dca.ruleName')} />
        <button type="button" class="rm" onclick={() => removeSell(i)} title={$t('backtest.dca.removeRule')}>
          <Icon name="x" size={12} />
        </button>
      </div>
      <div class="bt-grid">
        <div class="bt-field">
          {$t('backtest.dca.sellWhat')}
          <Dropdown
            bind:value={rule.amount_kind}
            ariaLabel={$t('backtest.dca.sellWhat')}
            options={DCA_SELL_KINDS.map((k) => ({ value: k, label: $t(`backtest.dca.sellKind_${k}`) }))}
          />
        </div>
        {#if rule.amount_kind !== 'all'}
          <label class="bt-field"
            >{sellAmountLabel(rule.amount_kind)}<input
              type="number"
              min="0"
              step="any"
              bind:value={rule.amount} /></label>
        {/if}
        <label class="bt-field"
          >{$t('backtest.dca.targetGain')}<input type="number" min="0" step="any" bind:value={rule.target_gain_pct} /></label>
        <label class="bt-field"
          >{$t('backtest.dca.maxFires')}<input type="number" min="0" step="1" bind:value={rule.max_fires} /></label>
        <label class="bt-field"
          >{$t('backtest.dca.cooldown')}<input type="number" min="0" step="1" bind:value={rule.cooldown_bars} /></label>
      </div>
      <label class="bt-check">
        <input type="checkbox" bind:checked={rule.per_asset} />
        {$t('backtest.dca.perAssetSell')}
      </label>
      <label class="bt-check">
        <input type="checkbox" bind:checked={rule.withdraw} />
        {$t('backtest.dca.withdraw')}
      </label>
      <SignalGroupEditor
        bind:group={rule.condition}
        title={$t('backtest.dca.when')}
        defaultOp="above"
        positions
        {customIndicators}
        emptyHint={$t('backtest.dca.sellNoCondition')} />
    </div>
  {/each}
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
  .weights {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-2);
  }
  .wrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    border: var(--hairline) solid var(--border-control);
    padding: var(--space-1) var(--space-2);
    background: var(--surface);
  }
  .wrow .tk {
    flex: 1;
    font-size: var(--text-sm);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The frame is the row's; the input inside it carries none. */
  .wrow input {
    width: 76px;
    text-align: right;
    border: 0;
    background: transparent;
    height: var(--control-h, 28px);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .wrow .pct {
    width: 52px;
    text-align: right;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .mini {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    align-self: flex-start;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    color: var(--text);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
  }
  .mini:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-2));
  }
  .mini:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .rule {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: var(--hairline) solid var(--border);
    padding: var(--space-3);
    background: var(--surface);
  }
  .rhead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .rhead .name {
    flex: 1;
    height: var(--control-h, 28px);
    font-size: var(--text-sm);
    background-color: var(--surface-2);
  }
  .rm {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-1);
    display: inline-flex;
  }
  .rm:hover {
    color: var(--red);
  }
  .spans {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .spans .lbl {
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
