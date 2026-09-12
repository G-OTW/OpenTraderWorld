<script>
  // Step 4 — everything that refines an already-working strategy: how adds are scaled, the
  // instrument's own units, execution friction, circuit breakers, the out-of-sample split and
  // perp funding. Defaults here are inert, so an untouched Advanced step changes nothing.
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';
  import './form.css';

  let { settings = $bindable() } = $props();

  const setOpt = (obj, key) => (e) => (obj[key] = e.target.value === '' ? null : Number(e.target.value));
</script>

<div class="bt-step">
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

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.adv.instrument')}</span>
    <div class="bt-grid">
      <label class="bt-field">{$t('backtest.adv.multiplier')}<input type="number" min="0" step="any" bind:value={settings.instrument.multiplier} /></label>
      <label class="bt-field">{$t('backtest.adv.lotStep')}<input type="number" min="0" step="any" bind:value={settings.instrument.lot_step} /></label>
      <label class="bt-field">{$t('backtest.adv.minQty')}<input type="number" min="0" step="any" bind:value={settings.instrument.min_qty} /></label>
    </div>
  </div>

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
</div>
