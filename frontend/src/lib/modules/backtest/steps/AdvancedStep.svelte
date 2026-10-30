<script>
  // Step 4 — everything that refines an already-working strategy: how adds are scaled, the
  // instrument's own units, execution friction, circuit breakers, the out-of-sample split and
  // perp funding. Defaults here are inert, so an untouched Advanced step changes nothing.
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';
  import { backtestApi, defaultExecution } from '../api.js';
  import './form.css';

  let { settings = $bindable(), datasetIds = [] } = $props();

  // A draft from before execution options existed carries no block: give it the inert one.
  if (!settings.execution) settings.execution = defaultExecution();
  const ex = $derived(settings.execution);

  // What the selected datasets can feed the options: bid/ask coverage and stored lower
  // timeframes. Re-read whenever the selection changes.
  let exData = $state([]);
  $effect(() => {
    const ids = datasetIds.slice();
    if (!ids.length) {
      exData = [];
      return;
    }
    backtestApi
      .executionData(ids)
      .then((d) => (exData = d ?? []))
      .catch(() => (exData = []));
  });
  const quotesPossible = $derived(exData.some((d) => d.quotes_supported) || ex.use_quotes);



  const ORDER_KINDS = $derived([
    { value: 'market', label: $t('backtest.exec.market') },
    { value: 'limit', label: $t('backtest.exec.limit') }
  ]);
  const TIMEFRAMES = $derived([
    { value: '', label: $t('backtest.exec.auto') },
    ...['1m', '5m', '15m', '1h', '4h'].map((tf) => ({ value: tf, label: tf }))
  ]);

  const setOpt = (obj, key) => (e) => (obj[key] = e.target.value === '' ? null : Number(e.target.value));

  // Grid and DCA engines read only the instrument (grid: its multiplier) and, for DCA, the
  // slippage model. Pyramid steps, breakers, OOS and funding apply to the signal engine alone.
  const isSignals = $derived((settings.kind ?? 'signals') === 'signals');
  const isGrid = $derived(settings.kind === 'grid');
</script>

{#snippet limitFields(o, exit)}
  <div class="bt-grid">
    <span class="bt-field sub">{exit ? $t('backtest.exec.exitLimit') : $t('backtest.exec.entryLimit')}</span>
    <div class="bt-field">{$t('backtest.exec.offsetKind')}
      <Dropdown
        bind:value={o.offset_kind}
        ariaLabel={$t('backtest.exec.offsetKind')}
        options={[
          { value: 'pct', label: $t('backtest.exec.offsetPct') },
          { value: 'abs', label: $t('backtest.exec.offsetAbs') },
          { value: 'atr', label: $t('backtest.exec.offsetAtr') }
        ]}
      />
    </div>
    {#if o.offset_kind === 'pct'}
      <label class="bt-field">{$t('backtest.exec.offsetValue')}
        <input type="number" min="0" step="any" value={(o.offset ?? 0) * 100}
          onchange={(e) => (o.offset = Math.max(0, Number(e.target.value) || 0) / 100)} />
      </label>
    {:else}
      <label class="bt-field">{$t('backtest.exec.offsetValue')}<input type="number" min="0" step="any" bind:value={o.offset} /></label>
    {/if}
    {#if o.offset_kind === 'atr'}
      <label class="bt-field">{$t('backtest.exec.atrPeriod')}<input type="number" min="1" step="1" bind:value={o.atr_period} /></label>
    {/if}
    <div class="bt-field">{$t('backtest.exec.reference')}
      <Dropdown
        bind:value={o.reference}
        ariaLabel={$t('backtest.exec.reference')}
        options={[
          { value: 'close', label: $t('backtest.exec.refClose') },
          { value: 'open', label: $t('backtest.exec.refOpen') }
        ]}
      />
    </div>
    <label class="bt-field">{$t('backtest.exec.validBars')}
      <input type="number" min="1" step="1" value={o.valid_bars}
        onchange={(e) => (o.valid_bars = Math.max(1, Math.round(Number(e.target.value) || 1)))} />
    </label>
    <div class="bt-field">{$t('backtest.exec.fillRule')}
      <Dropdown
        bind:value={o.fill}
        ariaLabel={$t('backtest.exec.fillRule')}
        options={[
          { value: 'touch', label: $t('backtest.exec.fillTouch') },
          { value: 'through', label: $t('backtest.exec.fillThrough') }
        ]}
      />
    </div>
    {#if exit}
      <div class="bt-field">{$t('backtest.exec.onExpiry')}
        <Dropdown
          bind:value={o.on_expiry}
          ariaLabel={$t('backtest.exec.onExpiry')}
          options={[
            { value: 'market', label: $t('backtest.exec.expiryMarket') },
            { value: 'cancel', label: $t('backtest.exec.expiryCancel') }
          ]}
        />
      </div>
    {/if}
  </div>
{/snippet}

<div class="bt-step">
  {#if isSignals}
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.adv.pyramidSteps')}</span>
    <div class="bt-grid">
      <label class="bt-field">{$t('backtest.adv.scale')}
        <input type="text" placeholder="1, 0.5, 0.25"
          value={(settings.pyramid_steps.scale ?? []).join(', ')}
          onchange={(e) => (settings.pyramid_steps.scale = e.target.value.split(',').map((x) => Number(x.trim())).filter((x) => x > 0))} />
      </label>
      <label class="bt-field">{$t('backtest.adv.minDistance')}
        <input type="number" min="0" step="any" value={(settings.pyramid_steps.min_distance_pct ?? 0) * 100}
          onchange={(e) => (settings.pyramid_steps.min_distance_pct = (Number(e.target.value) || 0) / 100)} />
      </label>
      <div class="bt-field">{$t('backtest.adv.afterAddSl')}
        <Dropdown
          bind:value={settings.pyramid_steps.after_add_sl}
          ariaLabel={$t('backtest.adv.afterAddSl')}
          options={[
            { value: 'none', label: $t('backtest.adv.afterNone') },
            { value: 'breakeven', label: $t('backtest.adv.afterBreakeven') },
            { value: 'trail_avg', label: $t('backtest.adv.afterTrail') }
          ]}
        />
      </div>
    </div>
  </div>
  {/if}

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.adv.instrument')}</span>
    <div class="bt-grid">
      <label class="bt-field">{$t('backtest.adv.multiplier')}<input type="number" min="0" step="any" bind:value={settings.instrument.multiplier} /></label>
      <label class="bt-field">{$t('backtest.adv.priceTick')}<input type="number" min="0" step="any" bind:value={settings.instrument.tick_size} /></label>
      {#if !isGrid}
        <label class="bt-field">{$t('backtest.adv.lotStep')}<input type="number" min="0" step="any" bind:value={settings.instrument.lot_step} /></label>
        <label class="bt-field">{$t('backtest.adv.minQty')}<input type="number" min="0" step="any" bind:value={settings.instrument.min_qty} /></label>
      {/if}
    </div>
    <p class="bt-note">{$t('backtest.adv.priceTickNote')}</p>
  </div>

  {#if !isGrid}
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.adv.slippage')}</span>
    <div class="bt-grid">
      <div class="bt-field">{$t('backtest.adv.slipKind')}
        <Dropdown
          bind:value={settings.slippage.kind}
          ariaLabel={$t('backtest.adv.slipKind')}
          options={[
            { value: 'pct', label: $t('backtest.adv.slipPct') },
            { value: 'ticks', label: $t('backtest.adv.slipTicks') }
          ]}
        />
      </div>
      {#if settings.slippage.kind === 'pct'}
        <label class="bt-field">{$t('backtest.adv.slipValuePct')}
          <input type="number" min="0" step="any" value={(settings.slippage.value ?? 0) * 100}
            onchange={(e) => (settings.slippage.value = (Number(e.target.value) || 0) / 100)} />
        </label>
      {:else}
        <label class="bt-field">{$t('backtest.adv.slipTicksN')}<input type="number" min="0" step="any" bind:value={settings.slippage.value} /></label>
        <label class="bt-field">{$t('backtest.adv.tickSize')}<input type="number" min="0" step="any" bind:value={settings.slippage.tick_size} /></label>
      {/if}
    </div>
  </div>
  {/if}

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.adv.prices')}</span>
    <label class="bt-check">
      <input type="checkbox" bind:checked={settings.execution.raw_prices} />
      {$t('backtest.adv.rawPrices')}
    </label>
    <p class="bt-note">{$t('backtest.adv.rawPricesNote')}</p>
  </div>

  {#if isSignals}
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.exec.title')}</span>
    <div class="bt-grid">
      <div class="bt-field">{$t('backtest.exec.entryOrder')}
        <Dropdown bind:value={ex.entry_order.kind} ariaLabel={$t('backtest.exec.entryOrder')} options={ORDER_KINDS} />
      </div>
      <div class="bt-field">{$t('backtest.exec.exitOrder')}
        <Dropdown bind:value={ex.exit_order.kind} ariaLabel={$t('backtest.exec.exitOrder')} options={ORDER_KINDS} />
      </div>
    </div>
    {#if ex.entry_order.kind === 'limit'}
      {@render limitFields(ex.entry_order, false)}
    {/if}
    {#if ex.exit_order.kind === 'limit'}
      {@render limitFields(ex.exit_order, true)}
    {/if}
    {#if ex.entry_order.kind === 'limit' || ex.exit_order.kind === 'limit'}
      <p class="bt-note">{$t('backtest.exec.limitNote')}</p>
    {/if}
    <div class="bt-grid">
      <div class="bt-field">{$t('backtest.exec.tpFill')}
        <Dropdown
          bind:value={ex.take_profit_fill}
          ariaLabel={$t('backtest.exec.tpFill')}
          options={[
            { value: 'touch', label: $t('backtest.exec.fillTouch') },
            { value: 'through', label: $t('backtest.exec.fillThrough') }
          ]}
        />
      </div>
    </div>
    <p class="bt-note">{$t('backtest.exec.tpNote')}</p>

    <label class="bt-check">
      <input type="checkbox" bind:checked={ex.intrabar} />
      {$t('backtest.exec.intrabar')}
    </label>
    {#if ex.intrabar}
      <div class="bt-grid">
        <div class="bt-field">{$t('backtest.exec.intrabarTf')}
          <Dropdown bind:value={ex.intrabar_timeframe} ariaLabel={$t('backtest.exec.intrabarTf')} options={TIMEFRAMES} />
        </div>
      </div>
      {#each exData as d (d.dataset_id)}
        {#if d.lower?.length}
          <p class="bt-note">{$t('backtest.exec.lowerStored', { ticker: d.ticker, tfs: d.lower.map((x) => x.timeframe).join(', ') })}</p>
        {:else if d.lower_possible?.length}
          <p class="bt-note">{$t('backtest.exec.lowerMissing', { ticker: d.ticker })}</p>
        {/if}
      {/each}
      <label class="bt-check">
        <input type="checkbox" bind:checked={ex.intrabar_fetch} />
        {$t('backtest.exec.intrabarFetch')}
      </label>
      <p class="bt-note">{$t('backtest.exec.intrabarFetchNote')}</p>
    {/if}
    <p class="bt-note">{$t('backtest.exec.intrabarNote')}</p>

    <label class="bt-check" class:off={!quotesPossible}>
      <input type="checkbox" bind:checked={ex.use_quotes} disabled={!quotesPossible} />
      {$t('backtest.exec.quotes')}
    </label>
    {#each exData as d (d.dataset_id)}
      {#if d.quote_kind === 'none'}
        <div class="bt-warn">{$t('backtest.exec.quotesUnsupported', { ticker: d.ticker, provider: d.provider })}</div>
      {:else}
        <p class="bt-note">
          {$t(d.quote_kind === 'ticks' ? 'backtest.exec.quotesTicks' : 'backtest.exec.quotesCandles', { ticker: d.ticker, provider: d.provider })}
          {#if d.quoted_bars}{$t('backtest.exec.quotesStored', { n: d.quoted_bars, total: d.bars })}{/if}
        </p>
      {/if}
    {/each}
    <p class="bt-note">{$t('backtest.exec.quotesNote')}</p>
  </div>

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.adv.circuitBreakers')}</span>
    <div class="bt-grid">
      <label class="bt-field">{$t('backtest.adv.maxDailyLoss')}
        <input type="number" min="0" step="any" value={settings.risk.max_daily_loss_pct ?? ''} onchange={setOpt(settings.risk, 'max_daily_loss_pct')} />
      </label>
      <label class="bt-field">{$t('backtest.adv.maxDrawdown')}
        <input type="number" min="0" step="any" value={settings.risk.max_drawdown_pct ?? ''} onchange={setOpt(settings.risk, 'max_drawdown_pct')} />
      </label>
    </div>
  </div>

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.adv.oos')}</span>
    <div class="bt-grid">
      <label class="bt-field">{$t('backtest.adv.oosSplit')}
        <input type="number" min="0" max="99" step="1" value={Math.round((settings.oos_split_pct ?? 0) * 100)}
          onchange={(e) => (settings.oos_split_pct = Math.min(99, Math.max(0, Number(e.target.value) || 0)) / 100)} />
      </label>
    </div>
    <p class="bt-note">{$t('backtest.adv.oosNote')}</p>
  </div>

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.adv.funding')}</span>
    <div class="bt-grid">
      <label class="bt-field">{$t('backtest.adv.fundingRate')}<input type="number" step="any" bind:value={settings.funding.annual_rate_pct} /></label>
      <label class="bt-field">{$t('backtest.adv.fundingInterval')}<input type="number" min="1" step="any" bind:value={settings.funding.interval_hours} /></label>
    </div>
    <p class="bt-note">{$t('backtest.adv.fundingNote')}</p>
  </div>
  {/if}
</div>

<style>
  .sub {
    grid-column: 1 / -1;
    color: var(--text);
  }
  .off {
    color: var(--muted);
    cursor: default;
  }
</style>
