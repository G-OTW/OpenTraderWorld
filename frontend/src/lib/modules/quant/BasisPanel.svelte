<script>
  // Futures basis and carry from quoted prices: the basis, the carry it implies per year, the
  // fair value at a financing rate, and the roll yield to the next contract.
  import './panel.css';
  import { quantApi, fmtNum } from './api.js';
  import { fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let spot = $state(100);
  let future = $state(101);
  let days = $state(90);
  let rate = $state(4);
  let yieldRate = $state(0);
  let nextFuture = $state('');
  let nextDays = $state('');
  let data = $state(null);
  let error = $state('');

  async function run() {
    error = '';
    try {
      data = await quantApi.basis({
        spot: Number(spot),
        future: Number(future),
        days: Number(days),
        rate: rate === '' || rate == null ? null : Number(rate) / 100,
        yield_rate: Number(yieldRate) / 100,
        next_future: Number(nextFuture) > 0 ? Number(nextFuture) : null,
        next_days: Number(nextDays) > 0 ? Number(nextDays) : null
      });
    } catch (e) {
      error = e.message;
      data = null;
    }
  }
</script>

<div class="qp">
  <div class="qp-params">
    <label class="qp-field"><span class="qp-label">{$t('quant.basis.spot')}</span><input type="number" min="0" step="any" bind:value={spot} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.basis.future')}</span><input type="number" min="0" step="any" bind:value={future} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.days')}</span><input type="number" min="1" step="1" bind:value={days} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.rate')}</span><input type="number" step="0.1" bind:value={rate} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.basis.yield')}</span><input type="number" step="0.1" bind:value={yieldRate} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.basis.nextFuture')}</span><input type="number" min="0" step="any" bind:value={nextFuture} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.basis.nextDays')}</span><input type="number" min="1" step="1" bind:value={nextDays} /></label>
    <button class="primary" onclick={run}>{$t('quant.page.compute')}</button>
  </div>

  <ErrorText {error} />

  {#if data}
    <section class="qp-block">
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.basis.basis')}</span><span class="v">{fmtNum(data.basis, 4)}</span><span class="s">{fmtPc(data.basis_pct, 3)} · {data.contango ? $t('quant.basis.contango') : $t('quant.basis.backwardation')}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.basis.carry')}</span><span class="v">{fmtPc(data.carry_continuous, 2)}</span><span class="s">{$t('quant.basis.carrySimple', { v: fmtPc(data.carry_simple, 2) })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.basis.repo')}</span><span class="v">{fmtPc(data.implied_repo, 2)}</span><span class="s">{$t('quant.basis.repoHint')}</span></div>
        {#if data.fair_value != null}
          <div class="qp-card"><span class="k">{$t('quant.basis.fair')}</span><span class="v">{fmtNum(data.fair_value, 4)}</span><span class="s" class:qp-bad={Math.abs(data.mispricing) > 1e-9}>{$t('quant.basis.mispricing', { v: fmtNum(data.mispricing, 4) })}</span></div>
        {/if}
        {#if data.roll_yield != null}
          <div class="qp-card"><span class="k">{$t('quant.basis.roll')}</span><span class="v" class:qp-good={data.roll_yield > 0} class:qp-bad={data.roll_yield < 0}>{fmtPc(data.roll_yield, 2)}</span><span class="s">{$t('quant.basis.rollHint')}</span></div>
        {/if}
      </div>
      <p class="qp-hint">{$t('quant.basis.hint')}</p>
    </section>
  {:else}
    <p class="qp-hint">{$t('quant.basis.pickHint')}</p>
  {/if}
</div>
