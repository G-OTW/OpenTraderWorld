<script>
  // Options calculator: Black-Scholes-Merton price and greeks, a binomial tree for American
  // exercise, and the implied volatility of a quoted price on the same contract.
  import './panel.css';
  import QChart from './QChart.svelte';
  import { quantApi, fmtNum } from './api.js';
  import { valueAxis, tooltip, title, legend, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let kind = $state('call');
  let style = $state('european');
  let spot = $state(100);
  let strike = $state(100);
  let days = $state(30);
  let vol = $state(20);
  let rate = $state(4);
  let dividend = $state(0);
  let market = $state('');
  let data = $state(null);
  let ivRes = $state(null);
  let busy = $state(false);
  let error = $state('');
  let ivError = $state('');

  const contract = () => ({
    kind,
    spot: Number(spot),
    strike: Number(strike),
    days: Number(days),
    rate: Number(rate) / 100,
    dividend: Number(dividend) / 100
  });

  async function run() {
    busy = true;
    error = '';
    try {
      data = await quantApi.options({ ...contract(), style, vol: Number(vol) / 100 });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  async function solve() {
    ivError = '';
    try {
      ivRes = await quantApi.iv({ ...contract(), price: Number(market) });
    } catch (e) {
      ivError = e.message;
      ivRes = null;
    }
  }

  // Greeks in trader units: vega and rho per point, theta per calendar day.
  const rows = (g) => [
    ['delta', g.delta, 4],
    ['gamma', g.gamma, 5],
    ['vega', g.vega / 100, 4],
    ['theta', g.theta / 365, 4],
    ['rho', g.rho / 100, 4]
  ];

  const build = $derived.by(() => {
    if (!data?.curve) return null;
    const names = [$t('quant.options.value'), $t('quant.options.intrinsic'), 'Delta'];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.options.curveTitle')),
      legend: legend(p, { data: names }),
      grid: { left: 48, right: 48, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => (+v).toFixed(3) }),
      xAxis: valueAxis(p, { scale: true }),
      yAxis: [valueAxis(p), valueAxis(p, { splitLine: { show: false }, min: kind === 'call' ? 0 : -1, max: kind === 'call' ? 1 : 0 })],
      series: [
        { name: names[0], type: 'line', symbol: 'none', data: data.curve.map((q) => [q.spot, q.price]), lineStyle: { color: p.series[0], width: 1.5 }, itemStyle: { color: p.series[0] } },
        { name: names[1], type: 'line', symbol: 'none', data: data.curve.map((q) => [q.spot, q.intrinsic]), lineStyle: { color: p.muted, width: 1, type: 'dashed' }, itemStyle: { color: p.muted } },
        { name: names[2], type: 'line', symbol: 'none', yAxisIndex: 1, data: data.curve.map((q) => [q.spot, q.delta]), lineStyle: { color: p.amber, width: 1 }, itemStyle: { color: p.amber } }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field">
      <span class="qp-label">{$t('quant.options.kind')}</span>
      <div class="qp-seg">
        {#each ['call', 'put'] as k (k)}
          <button class:active={kind === k} onclick={() => (kind = k)}>{$t(`quant.options.${k}`)}</button>
        {/each}
      </div>
    </div>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.options.style')}</span>
      <div class="qp-seg">
        {#each ['european', 'american'] as s (s)}
          <button class:active={style === s} onclick={() => (style = s)}>{$t(`quant.options.${s}`)}</button>
        {/each}
      </div>
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.spot')}</span><input type="number" min="0" step="any" bind:value={spot} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.strike')}</span><input type="number" min="0" step="any" bind:value={strike} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.days')}</span><input type="number" min="1" step="1" bind:value={days} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.vol')}</span><input type="number" min="0.1" step="0.5" bind:value={vol} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.rate')}</span><input type="number" step="0.1" bind:value={rate} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.dividend')}</span><input type="number" step="0.1" bind:value={dividend} /></label>
    <button class="primary" onclick={run} disabled={busy}>{busy ? $t('quant.page.computing') : $t('quant.page.compute')}</button>
  </div>

  <ErrorText {error} />

  {#if data}
    <div class="qp-grid2">
      <section class="qp-block">
        <h3>{$t('quant.options.price')}</h3>
        <div class="qp-cards">
          <div class="qp-card"><span class="k">{$t('quant.options.bsm')}</span><span class="v">{fmtNum(data.bsm.price, 4)}</span><span class="s">{$t('quant.options.european')}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.options.binomial', { n: data.steps })}</span><span class="v">{data.binomial == null ? '−' : fmtNum(data.binomial, 4)}</span><span class="s">{$t(`quant.options.${data.american ? 'american' : 'european'}`)}</span></div>
          {#if data.early_exercise_premium != null}
            <div class="qp-card"><span class="k">{$t('quant.options.premium')}</span><span class="v">{fmtNum(data.early_exercise_premium, 4)}</span><span class="s">{$t('quant.options.premiumHint')}</span></div>
          {/if}
        </div>
        <div class="qp-scroll">
          <table class="qp-table">
            <thead><tr><th>{$t('quant.options.greek')}</th><th class="num">{$t('quant.options.valueCol')}</th><th>{$t('quant.options.unit')}</th></tr></thead>
            <tbody>
              {#each rows(data.bsm) as [k, v, d] (k)}
                <tr><td>{$t(`quant.options.g.${k}`)}</td><td class="num">{fmtNum(v, d)}</td><td class="qp-muted">{$t(`quant.options.u.${k}`)}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
      <section class="qp-block">
        <QChart {build} height={300} />
      </section>
    </div>
  {/if}

  <section class="qp-block">
    <h3>{$t('quant.options.ivTitle')}</h3>
    <p class="qp-hint">{$t('quant.options.ivHint')}</p>
    <div class="iv-row">
      <label class="qp-field"><span class="qp-label">{$t('quant.options.marketPrice')}</span><input type="number" min="0" step="any" bind:value={market} /></label>
      <button class="btn" onclick={solve} disabled={!(Number(market) > 0)}>{$t('quant.options.solve')}</button>
    </div>
    <ErrorText error={ivError} />
    {#if ivRes}
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.options.iv')}</span><span class="v">{fmtPc(ivRes.iv, 2)}</span></div>
        {#each rows(ivRes.greeks) as [k, v, d] (k)}
          <div class="qp-card"><span class="k">{$t(`quant.options.g.${k}`)}</span><span class="v">{fmtNum(v, d)}</span><span class="s">{$t(`quant.options.u.${k}`)}</span></div>
        {/each}
      </div>
    {/if}
  </section>
</div>

<style>
  .iv-row {
    display: flex;
    align-items: flex-end;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

</style>
