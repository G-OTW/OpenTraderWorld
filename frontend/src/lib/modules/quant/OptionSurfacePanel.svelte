<script>
  // Implied-volatility surface from an underlying's option chain, read from the provider: a
  // few expiries spread over the window, out-of-the-money strikes on each side, each price
  // inverted to a Black-Scholes-Merton volatility. Smiles, the ATM term structure and the
  // 25-delta skew per expiry.
  import './panel.css';
  import QChart from './QChart.svelte';
  import MarketSource from './MarketSource.svelte';
  import TaskProgress from './TaskProgress.svelte';
  import { quantApi, fmtNum } from './api.js';
  import { valueAxis, tooltip, title, legend, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let source = $state(null);
  let provider = $state(null);
  let underlying = $state('SPY');
  let assetType = $state('etf');
  let expiries = $state(4);
  let strikes = $state(5);
  let minDays = $state(7);
  let maxDays = $state(120);
  let rate = $state(4);
  let dividend = $state(1.2);
  let data = $state(null);
  let busy = $state(false);
  let progress = $state(null);
  let error = $state('');

  // What a run costs in provider requests, so a slow key is not a surprise.
  const requests = $derived(Number(expiries) * Number(strikes) * 2 + (provider === 'ibkr' ? Number(expiries) + 3 : 2));

  async function run() {
    if (!source) return;
    busy = true;
    error = '';
    progress = null;
    try {
      data = await quantApi.optionSurface(
        {
          connector_id: source,
          underlying: underlying.trim(),
          asset_type: assetType,
          expiries: Number(expiries),
          strikes: Number(strikes),
          min_days: Math.round(Number(minDays)),
          max_days: Math.round(Number(maxDays)),
          rate: Number(rate) / 100,
          dividend: Number(dividend) / 100
        },
        (p) => (progress = p)
      );
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      busy = false;
    }
  }

  const sf = $derived(data?.result);
  // IB prices at an hour (shown in the viewer's time), Massive at a session (a date).
  const stampLabel = (d) =>
    d.provider === 'ibkr'
      ? $t('quant.surf.hourOf', { t: new Date(d.stamp).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' }) })
      : d.stamp.slice(0, 10);
  const pctAxis = (p, d = 0) => ({ color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => fmtPc(v, d) });

  const smileBuild = $derived.by(() => {
    if (!sf?.expiries?.length) return null;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.surf.smileTitle')),
      legend: legend(p, { type: 'scroll', left: 120 }),
      grid: { left: 48, right: 16, top: 32, bottom: 36 },
      tooltip: tooltip(p, {
        trigger: 'item',
        formatter: (q) => {
          const pt = sf.expiries[q.seriesIndex]?.points[q.dataIndex];
          return pt ? `${pt.ticker}<br>${fmtNum(pt.price, 2)} · IV ${fmtPc(pt.iv, 2)} · Δ ${fmtNum(pt.delta, 3)}` : '';
        }
      }),
      xAxis: valueAxis(p, { name: 'ln(K/F)', nameLocation: 'middle', nameGap: 22, scale: true, axisLabel: pctAxis(p) }),
      yAxis: valueAxis(p, { scale: true, axisLabel: pctAxis(p) }),
      series: sf.expiries.map((e, i) => ({
        name: `${e.expiry} (${e.dte}d)`,
        type: 'line',
        symbolSize: 5,
        data: e.points.map((q) => [q.moneyness, q.iv]),
        lineStyle: { color: p.series[i % p.series.length], width: 1.3 },
        itemStyle: { color: p.series[i % p.series.length] }
      }))
    });
  });

  const termBuild = $derived.by(() => {
    const rows = sf?.expiries?.filter((e) => e.atm_iv != null);
    if (!rows?.length) return null;
    const names = [$t('quant.surf.atm'), $t('quant.surf.rr25')];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.surf.termTitle')),
      legend: legend(p, { data: names }),
      grid: { left: 48, right: 48, top: 32, bottom: 36 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtPc(+v, 2) }),
      xAxis: valueAxis(p, { name: $t('quant.fut.daysToExpiry'), nameLocation: 'middle', nameGap: 22 }),
      yAxis: [valueAxis(p, { scale: true, axisLabel: pctAxis(p, 1) }), valueAxis(p, { splitLine: { show: false }, axisLabel: pctAxis(p, 1) })],
      series: [
        { name: names[0], type: 'line', symbolSize: 6, data: rows.map((e) => [e.dte, e.atm_iv]), lineStyle: { color: p.series[0], width: 1.5 }, itemStyle: { color: p.series[0] } },
        { name: names[1], type: 'line', yAxisIndex: 1, symbolSize: 5, data: rows.filter((e) => e.rr25 != null).map((e) => [e.dte, e.rr25]), lineStyle: { color: p.red, width: 1, type: 'dashed' }, itemStyle: { color: p.red } }
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <MarketSource bind:value={source} bind:provider />
    <label class="qp-field"><span class="qp-label">{$t('quant.surf.underlying')}</span><input type="text" bind:value={underlying} placeholder="SPY" /></label>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.fut.spotType')}</span>
      <div class="qp-seg">
        {#each ['etf', 'equity', 'index'] as k (k)}
          <button class:active={assetType === k} onclick={() => (assetType = k)}>{$t(`quant.market.type.${k}`)}</button>
        {/each}
      </div>
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.surf.expiries')}</span><input type="number" min="1" max="8" step="1" bind:value={expiries} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.surf.strikes')}</span><input type="number" min="2" max="12" step="1" bind:value={strikes} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.surf.minDays')}</span><input type="number" min="0" max="730" step="1" bind:value={minDays} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.surf.maxDays')}</span><input type="number" min="1" max="730" step="1" bind:value={maxDays} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.rate')}</span><input type="number" step="0.1" bind:value={rate} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.dividend')}</span><input type="number" step="0.1" bind:value={dividend} /></label>
    <button class="primary" onclick={run} disabled={!source || busy || !underlying.trim()}>{busy ? $t('quant.page.computing') : $t('quant.page.compute')}</button>
  </div>
  <p class="qp-hint">
    {$t('quant.surf.cost', { n: requests })}
    {#if provider === 'massive'}{$t('quant.surf.costMassive', { m: Math.ceil(requests / 5) })}{/if}
  </p>

  {#if busy}
    <TaskProgress {progress} />
  {/if}
  <ErrorText {error} />

  {#if sf}
    <p class="qp-hint">
      {$t('quant.surf.summary', { u: data.underlying, spot: fmtNum(sf.spot, 2), at: stampLabel(data), n: sf.solved, q: sf.quotes })}
      {#if data.missing || data.stale}{$t('quant.surf.skipped', { m: data.missing, s: data.stale })}{/if}
    </p>
    <section class="qp-block">
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.surf.frontAtm')}</span><span class="v">{fmtPc(sf.front_atm, 2)}</span><span class="s">{sf.expiries.find((e) => e.atm_iv != null)?.expiry ?? '−'}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.surf.backAtm')}</span><span class="v">{fmtPc(sf.back_atm, 2)}</span><span class="s">{[...sf.expiries].reverse().find((e) => e.atm_iv != null)?.expiry ?? '−'}</span></div>
        {#if sf.front_atm != null && sf.back_atm != null}
          <div class="qp-card"><span class="k">{$t('quant.surf.slope')}</span><span class="v" class:qp-bad={sf.back_atm < sf.front_atm}>{fmtPc(sf.back_atm - sf.front_atm, 2)}</span><span class="s">{sf.back_atm < sf.front_atm ? $t('quant.surf.inverted') : $t('quant.surf.upward')}</span></div>
        {/if}
        <div class="qp-card"><span class="k">{$t('quant.surf.calendar')}</span><span class="v" class:qp-bad={sf.calendar_violations.length}>{sf.calendar_violations.length}</span><span class="s">{$t('quant.surf.calendarHint')}</span></div>
      </div>
      <p class="qp-hint">{$t('quant.surf.hint')}</p>
    </section>
    <div class="qp-grid2">
      <section class="qp-block"><QChart build={smileBuild} height={320} /><p class="qp-hint">{$t('quant.surf.smileHint')}</p></section>
      <section class="qp-block"><QChart build={termBuild} height={320} /><p class="qp-hint">{$t('quant.surf.termHint')}</p></section>
    </div>
    <section class="qp-block">
      <div class="qp-scroll">
        <table class="qp-table">
          <thead>
            <tr>
              <th>{$t('quant.surf.expiry')}</th><th class="num">{$t('quant.surf.days')}</th><th class="num">{$t('quant.surf.forward')}</th><th class="num">ATM</th>
              <th class="num">25Δ put</th><th class="num">25Δ call</th><th class="num">RR25</th><th class="num">BF25</th><th class="num">{$t('quant.surf.points')}</th>
            </tr>
          </thead>
          <tbody>
            {#each sf.expiries as e (e.expiry)}
              <tr>
                <td>{e.expiry}</td><td class="num">{e.dte}</td><td class="num">{fmtNum(e.forward, 2)}</td><td class="num">{fmtPc(e.atm_iv, 2)}</td>
                <td class="num">{fmtPc(e.put25, 2)}</td><td class="num">{fmtPc(e.call25, 2)}</td>
                <td class="num" class:qp-bad={e.rr25 < 0}>{fmtPc(e.rr25, 2)}</td><td class="num">{fmtPc(e.bf25, 2)}</td>
                <td class="num">{e.points.length}{#if e.dropped}<span class="qp-warn"> −{e.dropped}</span>{/if}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <details>
        <summary class="qp-hint">{$t('quant.surf.quotes')}</summary>
        <div class="qp-scroll">
          <table class="qp-table">
            <thead><tr><th>{$t('quant.surf.contract')}</th><th class="num">{$t('quant.options.strike')}</th><th class="num">{$t('quant.surf.price')}</th><th class="num">IV</th><th class="num">Δ</th></tr></thead>
            <tbody>
              {#each sf.expiries.flatMap((e) => e.points) as q (q.ticker)}
                <tr><td>{q.ticker}</td><td class="num">{fmtNum(q.strike, 2)}</td><td class="num">{fmtNum(q.price, 2)}</td><td class="num">{fmtPc(q.iv, 2)}</td><td class="num">{fmtNum(q.delta, 3)}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      </details>
    </section>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.surf.pickHint')}</p>
  {/if}
</div>
