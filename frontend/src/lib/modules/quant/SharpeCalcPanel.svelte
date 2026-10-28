<script>
  // Sharpe significance from summary numbers, for a figure quoted without its data: is it
  // distinguishable from zero (or from a benchmark), how long a record it needs, and what is
  // left of it once the number of strategies tried is accounted for.
  import './panel.css';
  import { quantApi, fmtNum } from './api.js';
  import { fmtP, fmtX, fmtPc, stars } from './charts.js';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let sharpe = $state(1.2);
  let observations = $state(756);
  let ppy = $state(252);
  let skew = $state(0);
  let kurtosis = $state(3);
  let benchmark = $state(0);
  let trials = $state('');
  let trialsStd = $state('');
  let data = $state(null);
  let error = $state('');

  const PPY = [
    [252, 'daily'],
    [365, 'daily247'],
    [52, 'weekly'],
    [12, 'monthly']
  ];

  async function run() {
    error = '';
    try {
      data = await quantApi.sharpe({
        sharpe: Number(sharpe),
        observations: Math.round(Number(observations)),
        periods_per_year: Number(ppy),
        skew: Number(skew) || 0,
        kurtosis: Number(kurtosis) || 3,
        benchmark: Number(benchmark) || 0,
        trials: Number(trials) >= 2 ? Math.round(Number(trials)) : null,
        trials_sharpe_std: Number(trialsStd) > 0 ? Number(trialsStd) : null
      });
    } catch (e) {
      error = e.message;
      data = null;
    }
  }

  const s = $derived(data?.inference);
</script>

<div class="qp">
  <div class="qp-params">
    <label class="qp-field"><span class="qp-label">{$t('quant.sharpe.sharpe')}</span><input type="number" step="0.05" bind:value={sharpe} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.sharpe.observations')}</span><input type="number" min="2" step="1" bind:value={observations} /></label>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.sharpe.frequency')}</span>
      <Dropdown
        bind:value={ppy}
        ariaLabel={$t('quant.sharpe.frequency')}
        options={PPY.map(([v, k]) => ({ value: v, label: $t(`quant.sharpe.freq.${k}`) }))}
      />
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.sharpe.skew')}</span><input type="number" step="0.1" bind:value={skew} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.sharpe.kurtosis')}</span><input type="number" min="1" step="0.1" bind:value={kurtosis} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.sharpe.benchmark')}</span><input type="number" step="0.1" bind:value={benchmark} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.sharpe.trials')}</span><input type="number" min="1" step="1" bind:value={trials} placeholder="1" /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.sharpe.trialsStd')}</span><input type="number" min="0" step="0.05" bind:value={trialsStd} /></label>
    <button class="primary" onclick={run}>{$t('quant.page.compute')}</button>
  </div>

  <ErrorText {error} />

  {#if s}
    <section class="qp-block">
      <p class="qp-hint">{$t('quant.sharpe.summary', { y: fmtNum(s.observations / s.periods_per_year, 1) })}</p>
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.sharpe.t')}</span><span class="v">{fmtX(s.t_stat)}</span><span class="s">p {fmtP(s.p_value)} {stars(s.p_value)}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.sharpe.ci')}</span><span class="v">{fmtX(s.ci_low)} … {fmtX(s.ci_high)}</span><span class="s">{$t('quant.sharpe.ciHint', { se: fmtX(s.se_nonnormal, 3), iid: fmtX(s.se_iid, 3) })}</span></div>
        <div class="qp-card"><span class="k">PSR</span><span class="v" class:qp-good={s.psr >= 0.95}>{fmtPc(s.psr, 1)}</span><span class="s">{$t('quant.sharpe.psrHint', { b: fmtX(s.benchmark) })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.sharpe.minTrl')}</span><span class="v">{s.min_trl_years == null ? '−' : `${fmtNum(s.min_trl_years, 1)} ${$t('quant.sharpe.years')}`}</span><span class="s">{s.min_trl_periods == null ? $t('quant.sharpe.minTrlNone') : $t('quant.sharpe.minTrlHint', { n: fmtNum(s.min_trl_periods, 0) })}</span></div>
        {#if data.deflated}
          <div class="qp-card"><span class="k">{$t('quant.compare.bar')}</span><span class="v">{fmtX(data.deflated.expected_max_sharpe)}</span><span class="s">{$t('quant.compare.barHint', { n: data.deflated.trials })}</span></div>
          <div class="qp-card"><span class="k">DSR</span><span class="v" class:qp-good={data.deflated.deflated_sharpe >= 0.95} class:qp-bad={data.deflated.deflated_sharpe < 0.5}>{fmtPc(data.deflated.deflated_sharpe, 1)}</span><span class="s">{$t('quant.compare.dsrHint')}</span></div>
        {/if}
      </div>
      <p class="qp-hint">{$t('quant.sharpe.hint')}</p>
    </section>
  {/if}
</div>
