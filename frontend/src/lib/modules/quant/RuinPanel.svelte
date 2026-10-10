<script>
  // Risk of ruin: the chance a win rate, a payoff and a risk per trade reach a given drawdown.
  // Closed forms (Vince for a fixed amount, the Cramér-Lundberg bound for either sizing) next
  // to a simulation over a finite number of trades.
  import './panel.css';
  import QChart from './QChart.svelte';
  import { quantApi } from './api.js';
  import { valueAxis, tooltip, title, fmtX, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let winRate = $state(45);
  let payoff = $state(1.8);
  let risk = $state(2);
  let ruinPct = $state(50);
  let sizing = $state('fixed');
  let horizon = $state(1000);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  async function run() {
    busy = true;
    error = '';
    try {
      data = await quantApi.ruin({
        win_rate: Number(winRate) / 100,
        payoff: Number(payoff),
        risk: Number(risk) / 100,
        ruin: Number(ruinPct) / 100,
        sizing,
        horizon: Math.round(Number(horizon)) || 1000
      });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const fmtProb = (v) => (v == null ? '−' : v < 1e-4 && v > 0 ? v.toExponential(1) : fmtPc(v, 2));

  const build = $derived.by(() => {
    if (!data?.by_trade?.length) return null;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.ruin.byTrade')),
      grid: { left: 48, right: 12, top: 32, bottom: 36 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtPc(+v, 2) }),
      xAxis: valueAxis(p, { name: $t('quant.ruin.trades'), nameLocation: 'middle', nameGap: 22 }),
      yAxis: valueAxis(p, { axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => `${(v * 100).toFixed(v > 0 && v < 0.01 ? 2 : 0)}%` } }),
      series: [
        {
          type: 'line',
          symbol: 'none',
          data: data.by_trade,
          lineStyle: { color: p.red, width: 1.5 },
          areaStyle: { color: p.red, opacity: 0.08 },
          markLine: data.lundberg != null && data.lundberg < 1
            ? { symbol: 'none', silent: true, label: { formatter: $t('quant.ruin.bound'), color: p.muted, position: 'insideEndTop' }, lineStyle: { color: p.muted, type: 'dashed' }, data: [{ yAxis: data.lundberg }] }
            : undefined
        }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <label class="qp-field"><span class="qp-label">{$t('quant.ruin.winRate')}</span><input type="number" min="1" max="99" step="1" bind:value={winRate} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.ruin.payoff')}</span><input type="number" min="0.1" step="0.1" bind:value={payoff} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.ruin.risk')}</span><input type="number" min="0.1" max="50" step="0.1" bind:value={risk} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.ruin.ruin')}</span><input type="number" min="1" max="100" step="1" bind:value={ruinPct} /></label>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.ruin.sizing')}</span>
      <div class="qp-seg">
        {#each ['fixed', 'fractional'] as s (s)}
          <button class:active={sizing === s} onclick={() => (sizing = s)}>{$t(`quant.ruin.${s}`)}</button>
        {/each}
      </div>
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.ruin.horizon')}</span><input type="number" min="10" max="100000" step="100" bind:value={horizon} /></label>
    <button class="primary" onclick={run} disabled={busy}>{busy ? $t('quant.page.computing') : $t('quant.page.compute')}</button>
  </div>

  <ErrorText {error} />

  {#if data}
    <section class="qp-block">
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.ruin.expectancy')}</span><span class="v" class:qp-good={data.expectancy_r > 0} class:qp-bad={data.expectancy_r <= 0}>{fmtX(data.expectancy_r)}R</span><span class="s">{$t('quant.ruin.expectancyHint')}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.ruin.simulated', { n: data.horizon })}</span><span class="v">{fmtProb(data.simulated)}</span><span class="s">± {fmtPc(data.simulated_se, 2)} · {data.paths.toLocaleString()} {$t('quant.ruin.paths')}</span></div>
        {#if data.vince != null}
          <div class="qp-card"><span class="k">{$t('quant.ruin.vince')}</span><span class="v">{fmtProb(data.vince)}</span><span class="s">{$t('quant.ruin.vinceHint')}</span></div>
        {/if}
        <div class="qp-card"><span class="k">{$t('quant.ruin.lundberg')}</span><span class="v">{fmtProb(data.lundberg)}</span><span class="s">{$t('quant.ruin.lundbergHint')}</span></div>
      </div>
      {#if data.expectancy_r <= 0}
        <p class="qp-hint qp-bad">{$t('quant.ruin.negative')}</p>
      {/if}
      <QChart {build} height={260} />
      <p class="qp-hint">{$t('quant.ruin.hint')}</p>
    </section>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.ruin.pickHint')}</p>
  {/if}
</div>
