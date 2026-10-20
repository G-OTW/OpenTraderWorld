<script>
  // Single-asset extras under the headline risk cards: the tail beyond the VaR (normal vs
  // Cornish-Fisher vs a Generalized Pareto fit of the worst losses) and the worst drawdowns as
  // a table, since a single "max drawdown" hides how long the hole took to fill.
  import './panel.css';
  import { fmtPc, fmtX } from './charts.js';
  import { t } from '$lib/i18n';

  let { tail, episodes } = $props();

  const day = (s) => (s ? s.slice(0, 10) : '−');
</script>

{#if tail}
  <section class="qp-block">
    <h3>{$t('quant.tail.title')}</h3>
    <p class="qp-hint">{$t('quant.tail.hint')}</p>
    <div class="qp-cards">
      <div class="qp-card"><span class="k">{$t('quant.tail.varNormal', { c: fmtPc(tail.confidence, 0) })}</span><span class="v">−{fmtPc(tail.var_normal)}</span><span class="s">{$t('quant.tail.varNormalHint')}</span></div>
      <div class="qp-card"><span class="k">{$t('quant.tail.varCf', { c: fmtPc(tail.confidence, 0) })}</span><span class="v">−{fmtPc(tail.var_cornish_fisher)}</span><span class="s">{$t('quant.tail.varCfHint')}</span></div>
      <div class="qp-card"><span class="k">{$t('quant.tail.tailRatio')}</span><span class="v">{fmtX(tail.tail_ratio)}</span><span class="s">{$t('quant.tail.tailRatioHint')}</span></div>
      {#if tail.gpd}
        <div class="qp-card"><span class="k">{$t('quant.tail.shape')}</span><span class="v">{fmtX(tail.gpd.shape, 3)}</span><span class="s">{tail.gpd.shape > 0 ? $t('quant.tail.fatTail') : $t('quant.tail.thinTail')}</span></div>
      {/if}
    </div>
    {#if tail.gpd}
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr><th>{$t('quant.tail.evtLevel')}</th><th class="num">{$t('quant.tail.evtVar')}</th><th class="num">{$t('quant.tail.evtEs')}</th></tr>
          </thead>
          <tbody>
            {#each tail.gpd.levels as l (l.confidence)}
              <tr><td>{fmtPc(l.confidence, 1)}</td><td class="num">−{fmtPc(l.var)}</td><td class="num">−{fmtPc(l.es)}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="qp-hint">{$t('quant.tail.evtFoot', { u: fmtPc(tail.gpd.threshold), n: tail.gpd.exceedances })}</p>
    {/if}
  </section>
{/if}

{#if episodes?.length}
  <section class="qp-block">
    <h3>{$t('quant.episodes.title')}</h3>
    <p class="qp-hint">{$t('quant.episodes.hint')}</p>
    <div class="qp-scroll">
      <table class="qp-table">
        <thead>
          <tr>
            <th class="num">{$t('quant.episodes.depth')}</th>
            <th>{$t('quant.episodes.peak')}</th>
            <th>{$t('quant.episodes.trough')}</th>
            <th>{$t('quant.episodes.recovered')}</th>
            <th class="num">{$t('quant.episodes.toTrough')}</th>
            <th class="num">{$t('quant.episodes.toRecover')}</th>
            <th class="num">{$t('quant.episodes.underWater')}</th>
          </tr>
        </thead>
        <tbody>
          {#each episodes as e (e.peak_at)}
            <tr>
              <td class="num qp-bad">−{fmtPc(e.depth, 1)}</td>
              <td>{day(e.peak_at)}</td>
              <td>{day(e.trough_at)}</td>
              <td>{e.recovered_at ? day(e.recovered_at) : $t('quant.episodes.open')}</td>
              <td class="num">{e.to_trough}</td>
              <td class="num">{e.to_recover ?? '−'}</td>
              <td class="num">{e.under_water}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </section>
{/if}
