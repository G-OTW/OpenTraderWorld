<script>
  // Compounding: where a capital and a return per period lead, with contributions; the return
  // needed to reach a target; and the gain it takes to climb back from a drawdown.
  import './panel.css';
  import QChart from './QChart.svelte';
  import { quantApi, fmtNum } from './api.js';
  import { valueAxis, tooltip, title, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let principal = $state(10000);
  let rate = $state(7);
  let periods = $state(20);
  let contribution = $state(0);
  let target = $state(50000);
  let data = $state(null);
  let error = $state('');

  async function run() {
    error = '';
    try {
      data = await quantApi.compound({
        principal: Number(principal),
        rate: Number(rate) / 100,
        periods: Math.round(Number(periods)),
        contribution: Number(contribution) || 0,
        target: Number(target) > 0 ? Number(target) : null
      });
    } catch (e) {
      error = e.message;
      data = null;
    }
  }

  const build = $derived.by(() => {
    if (!data?.path) return null;
    const pv = Number(principal);
    const c = Number(contribution) || 0;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.compound.pathTitle')),
      grid: { left: 64, right: 12, top: 32, bottom: 36 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtNum(+v, 0) }),
      xAxis: valueAxis(p, { name: $t('quant.compound.period'), nameLocation: 'middle', nameGap: 22, max: data.path.length - 1 }),
      yAxis: valueAxis(p),
      series: [
        { type: 'line', symbol: 'none', data: data.path.map((v, i) => [i, v]), lineStyle: { color: p.series[0], width: 1.5 }, areaStyle: { color: p.series[0], opacity: 0.08 } },
        { type: 'line', symbol: 'none', data: data.path.map((_, i) => [i, pv + c * i]), lineStyle: { color: p.muted, width: 1, type: 'dashed' } }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <label class="qp-field"><span class="qp-label">{$t('quant.compound.principal')}</span><input type="number" min="0" step="any" bind:value={principal} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.compound.rate')}</span><input type="number" step="0.1" bind:value={rate} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.compound.periods')}</span><input type="number" min="0" max="10000" step="1" bind:value={periods} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.compound.contribution')}</span><input type="number" step="any" bind:value={contribution} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.compound.target')}</span><input type="number" min="0" step="any" bind:value={target} /></label>
    <button class="primary" onclick={run}>{$t('quant.page.compute')}</button>
  </div>

  <ErrorText {error} />

  {#if data}
    <div class="qp-grid2">
      <section class="qp-block">
        <div class="qp-cards">
          <div class="qp-card"><span class="k">{$t('quant.compound.fv')}</span><span class="v">{fmtNum(data.future_value, 2)}</span><span class="s">{$t('quant.compound.fvHint', { n: periods })}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.compound.contributed')}</span><span class="v">{fmtNum(data.contributed, 2)}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.compound.growth')}</span><span class="v qp-good">{fmtNum(data.growth, 2)}</span></div>
          {#if data.required_rate != null}
            <div class="qp-card"><span class="k">{$t('quant.compound.required')}</span><span class="v">{fmtPc(data.required_rate, 3)}</span><span class="s">{$t('quant.compound.requiredHint', { n: periods })}</span></div>
          {/if}
          {#if data.periods_to_target != null}
            <div class="qp-card"><span class="k">{$t('quant.compound.toTarget')}</span><span class="v">{fmtNum(data.periods_to_target, 1)}</span><span class="s">{$t('quant.compound.toTargetHint')}</span></div>
          {/if}
        </div>
        <QChart {build} height={260} />
      </section>
      <section class="qp-block">
        <h3>{$t('quant.compound.recovery')}</h3>
        <p class="qp-hint">{$t('quant.compound.recoveryHint')}</p>
        <div class="qp-scroll">
          <table class="qp-table">
            <thead><tr><th>{$t('quant.compound.drawdown')}</th><th class="num">{$t('quant.compound.gainNeeded')}</th><th class="num">{$t('quant.compound.periodsAt', { r: fmtPc(Number(rate) / 100, 1) })}</th></tr></thead>
            <tbody>
              {#each data.recovery as [d, g, n] (d)}
                <tr><td class="qp-bad">−{fmtPc(d, 0)}</td><td class="num">+{fmtPc(g, 1)}</td><td class="num">{n == null ? '−' : fmtNum(n, 1)}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  {/if}
</div>
