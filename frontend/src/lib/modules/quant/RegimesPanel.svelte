<script>
  // Regimes tab: a Gaussian hidden Markov model splits the return series into K states (calm
  // first), shades the price chart by the decoded state, and reports how each state behaves,
  // how long it tends to last, and which one the market is most likely in now.
  import './panel.css';
  import QChart from './QChart.svelte';
  import { quantApi } from './api.js';
  import { valueAxis, timeAxis, tooltip, title, fmtX, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { datasetId, win } = $props();

  let states = $state(2);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  $effect(() => {
    void datasetId;
    void win;
    data = null;
  });

  async function run() {
    if (!datasetId) return;
    busy = true;
    error = '';
    try {
      data = await quantApi.regimes(datasetId, { ...win, states });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const r = $derived(data?.result);
  // Calm → wild: green, amber, red, then the accent for a fourth state.
  const stateColor = (p, i, k) => [p.green, p.amber, p.red, p.accent][k === 2 && i === 1 ? 2 : i];
  const stateName = (i, k) =>
    k === 2 ? [$t('quant.regimes.calm'), $t('quant.regimes.turbulent')][i] : $t('quant.regimes.stateN', { n: i + 1 });

  const priceBuild = $derived.by(() => {
    if (!r) return null;
    const k = r.k;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.regimes.priceTitle')),
      grid: { left: 56, right: 12, top: 28, bottom: 28 },
      tooltip: tooltip(p),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { scale: true }),
      series: [
        {
          type: 'line',
          symbol: 'none',
          data: data.price,
          lineStyle: { color: p.text, width: 1 },
          markArea: {
            silent: true,
            data: r.segments.map((s) => [
              { xAxis: s.from, itemStyle: { color: stateColor(p, s.state, k), opacity: 0.18 } },
              { xAxis: s.to }
            ])
          }
        }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field">
      <span class="qp-label">{$t('quant.regimes.states')}</span>
      <div class="qp-seg">
        {#each [2, 3, 4] as k (k)}
          <button class:active={states === k} onclick={() => (states = k)}>{k}</button>
        {/each}
      </div>
    </div>
    <button class="primary" onclick={run} disabled={!datasetId || busy}>
      {busy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if r}
    <section class="qp-block">
      <h3>{$t('quant.regimes.title')}</h3>
      <p class="qp-hint">{$t('quant.regimes.hint')}</p>
      <QChart build={priceBuild} height={300} />
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr>
              <th>{$t('quant.regimes.state')}</th>
              <th class="num">{$t('quant.regimes.meanAnnual')}</th>
              <th class="num">{$t('quant.regimes.volAnnual')}</th>
              <th class="num">{$t('quant.regimes.occupancy')}</th>
              <th class="num">{$t('quant.regimes.duration')}</th>
              <th class="num">{$t('quant.regimes.now')}</th>
            </tr>
          </thead>
          <tbody>
            {#each r.states as s, i (i)}
              <tr>
                <td>{stateName(i, r.k)}</td>
                <td class="num" class:qp-good={s.mean_annual > 0} class:qp-bad={s.mean_annual < 0}>{fmtPc(s.mean_annual, 1)}</td>
                <td class="num">{fmtPc(s.vol_annual, 1)}</td>
                <td class="num">{fmtPc(s.occupancy, 0)}</td>
                <td class="num">{fmtX(s.expected_duration, 1)}</td>
                <td class="num">{fmtPc(r.current[i], 0)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    <section class="qp-block">
      <h3>{$t('quant.regimes.transitions')}</h3>
      <p class="qp-hint">{$t('quant.regimes.transitionsHint')}</p>
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr><th>{$t('quant.regimes.fromTo')}</th>{#each r.states as _, j (j)}<th class="num">{stateName(j, r.k)}</th>{/each}</tr>
          </thead>
          <tbody>
            {#each r.transition as row, i (i)}
              <tr><td>{stateName(i, r.k)}</td>{#each row as v, j (j)}<td class="num">{fmtPc(v, 1)}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="qp-hint">
        {$t('quant.regimes.fitInfo', { it: r.iterations, ll: fmtX(r.loglik, 1), seg: r.segments.length })}
        {#if !r.converged}<span class="qp-warn"> · {$t('quant.regimes.notConverged')}</span>{/if}
      </p>
    </section>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.regimes.pickHint')}</p>
  {/if}
</div>
