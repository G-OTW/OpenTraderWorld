<script>
  // Events tab: pick a condition (gap, big move, SMA cross, RSI cross, breakout, streak, volume
  // spike) and see what the market did next, horizon by horizon, against the unconditional
  // baseline over the same bars, plus the average path around the event.
  import './panel.css';
  import QChart from './QChart.svelte';
  import { quantApi } from './api.js';
  import { valueAxis, categoryAxis, tooltip, title, legend, fmtP, fmtPc, fmtX, stars } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';

  let { datasetId, win } = $props();

  // Each kind lists the parameters it takes, with defaults. `pct` fields are typed in percent.
  const KINDS = {
    gap_down: { pct: 1 },
    gap_up: { pct: 1 },
    big_down: { pct: 2 },
    big_up: { pct: 2 },
    cross_above_sma: { period: 50 },
    cross_below_sma: { period: 50 },
    rsi_below: { period: 14, level: 30 },
    rsi_above: { period: 14, level: 70 },
    new_high: { period: 20 },
    new_low: { period: 20 },
    streak_up: { count: 3 },
    streak_down: { count: 3 },
    volume_spike: { period: 20, mult: 2 }
  };

  let kind = $state('gap_down');
  let params = $state({ ...KINDS.gap_down });
  let horizonsText = $state('1, 5, 10, 20');
  let minGap = $state(1);
  let data = $state(null);
  let busy = $state(false);
  let error = $state('');

  function pickKind(k) {
    kind = k;
    params = { ...KINDS[k] };
  }

  $effect(() => {
    void datasetId;
    void win;
    data = null;
  });

  async function run() {
    if (!datasetId) return;
    busy = true;
    error = '';
    const condition = { kind };
    for (const [k, v] of Object.entries(params)) condition[k] = k === 'pct' ? Number(v) / 100 : Number(v);
    const horizons = horizonsText
      .split(/[\s,;]+/)
      .map(Number)
      .filter((n) => Number.isInteger(n) && n > 0);
    try {
      data = await quantApi.events(datasetId, condition, { ...win, horizons: horizons.length ? horizons : null, min_gap: Number(minGap) || 1 });
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const r = $derived(data?.result);

  const pathBuild = $derived.by(() => {
    if (!r?.path?.length) return null;
    const x = r.path.map((q) => String(q.offset));
    const pc = (v) => +(v * 100).toFixed(3);
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.events.pathTitle')),
      legend: legend(p, { data: [$t('quant.events.meanPath')] }),
      grid: { left: 52, right: 12, top: 32, bottom: 36 },
      tooltip: tooltip(p, { valueFormatter: (v) => `${v}%` }),
      xAxis: categoryAxis(p, x, { name: $t('quant.events.barsFromEvent'), nameLocation: 'middle', nameGap: 22, nameTextStyle: { color: p.dim, fontSize: 10 } }),
      yAxis: valueAxis(p, { axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: '{value}%' } }),
      series: [
        // 95% band drawn as a stacked area: the lower edge, then the width on top of it.
        { type: 'line', data: r.path.map((q) => pc(q.low)), stack: 'band', symbol: 'none', lineStyle: { opacity: 0 }, silent: true },
        { type: 'line', data: r.path.map((q) => pc(q.high - q.low)), stack: 'band', symbol: 'none', lineStyle: { opacity: 0 }, areaStyle: { color: p.series[0], opacity: 0.15 }, silent: true },
        {
          name: $t('quant.events.meanPath'),
          type: 'line',
          data: r.path.map((q) => pc(q.mean)),
          symbol: 'none',
          lineStyle: { color: p.series[0], width: 1.8 },
          itemStyle: { color: p.series[0] },
          markLine: { symbol: 'none', silent: true, label: { show: false }, lineStyle: { color: p.muted, type: 'dashed' }, data: [{ xAxis: '0' }, { yAxis: 0 }] }
        }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <div class="qp-field kind">
      <span class="qp-label">{$t('quant.events.condition')}</span>
      <Dropdown
        value={kind}
        options={Object.keys(KINDS).map((k) => ({ value: k, label: $t(`quant.events.kind.${k}`) }))}
        onpick={(v) => pickKind(v)}
        ariaLabel={$t('quant.events.condition')}
      />
    </div>
    {#each Object.keys(params) as key (key)}
      <label class="qp-field">
        <span class="qp-label">{$t(`quant.events.param.${key}`)}</span>
        <input type="number" step="any" bind:value={params[key]} />
      </label>
    {/each}
    <label class="qp-field">
      <span class="qp-label">{$t('quant.events.horizons')}</span>
      <input type="text" bind:value={horizonsText} style="width: 140px" />
    </label>
    <label class="qp-field">
      <span class="qp-label">{$t('quant.events.minGap')}</span>
      <input type="number" min="1" bind:value={minGap} />
    </label>
    <button class="primary" onclick={run} disabled={!datasetId || busy}>
      {busy ? $t('quant.page.analyzing') : $t('quant.page.analyze')}
    </button>
  </div>

  <ErrorText {error} />

  {#if r}
    <section class="qp-block">
      <h3>{$t('quant.events.forward')}</h3>
      <p class="qp-hint">
        {$t('quant.events.counts', { n: r.events, raw: r.events_raw })}
      </p>
      {#if r.events === 0}
        <p class="qp-hint">{$t('quant.events.none')}</p>
      {:else}
        <div class="qp-scroll">
          <table class="qp-table">
            <thead>
              <tr>
                <th>{$t('quant.events.horizon')}</th>
                <th class="num">n</th>
                <th class="num">{$t('quant.events.mean')}</th>
                <th class="num">{$t('quant.events.median')}</th>
                <th class="num">{$t('quant.events.hitRate')}</th>
                <th class="num">p</th>
                <th class="num">95% CI</th>
                <th class="num">{$t('quant.events.baseline')}</th>
                <th class="num">{$t('quant.events.edge')}</th>
                <th class="num">{$t('quant.events.pVsBase')}</th>
              </tr>
            </thead>
            <tbody>
              {#each r.horizons as h (h.horizon)}
                <tr>
                  <td>{$t('quant.events.bars', { n: h.horizon })}</td>
                  <td class="num">{h.n}</td>
                  <td class="num" class:qp-good={h.mean > 0} class:qp-bad={h.mean < 0}>{fmtPc(h.mean)}</td>
                  <td class="num">{fmtPc(h.median)}</td>
                  <td class="num">{fmtPc(h.hit_rate, 0)}</td>
                  <td class="num">{fmtP(h.p_value)} {stars(h.p_value)}</td>
                  <td class="num">[{fmtPc(h.ci_low)}, {fmtPc(h.ci_high)}]</td>
                  <td class="num qp-muted">{fmtPc(h.base_mean)} · {fmtPc(h.base_hit_rate, 0)}</td>
                  <td class="num" class:qp-good={h.mean - h.base_mean > 0} class:qp-bad={h.mean - h.base_mean < 0}>{fmtPc(h.mean - h.base_mean)}</td>
                  <td class="num">{fmtP(h.p_vs_base)} {stars(h.p_vs_base)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <p class="qp-hint">{$t('quant.events.readHint', { t: fmtX(r.horizons[0]?.t_vs_base) })}</p>
        <QChart build={pathBuild} height={280} />
      {/if}
    </section>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.events.pickHint')}</p>
  {/if}
</div>

<style>
  .kind {
    min-width: 0;
    flex-basis: 240px;
  }

</style>
