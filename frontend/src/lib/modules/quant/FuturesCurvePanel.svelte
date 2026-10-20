<script>
  // Futures curve: the dated contracts of one product, read from the provider. The term
  // structure today, the roll yield between the front and the next contract over time, the
  // continuous series of holding the front and rolling, and the basis to a spot when one is
  // given.
  import './panel.css';
  import QChart from './QChart.svelte';
  import MarketSource from './MarketSource.svelte';
  import TaskProgress from './TaskProgress.svelte';
  import { quantApi, fmtNum } from './api.js';
  import { valueAxis, timeAxis, tooltip, title, legend, fmtPc } from './charts.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let source = $state(null);
  let provider = $state(null);
  let root = $state('ES@CME');
  let years = $state(2);
  let ahead = $state(15);
  let spotTicker = $state('SPX');
  let spotType = $state('index');
  let rate = $state(4);
  let yieldRate = $state(1.3);
  let rollDays = $state(5);
  let data = $state(null);
  let busy = $state(false);
  let progress = $state(null);
  let error = $state('');

  async function run() {
    if (!source) return;
    busy = true;
    error = '';
    progress = null;
    try {
      data = await quantApi.futuresCurve(
        {
          connector_id: source,
          root: root.trim(),
          years: Number(years),
          months_ahead: Math.round(Number(ahead)),
          spot_ticker: spotTicker.trim() || null,
          spot_asset_type: spotType,
          rate: rate === '' || rate == null ? null : Number(rate) / 100,
          yield_rate: Number(yieldRate) / 100,
          roll_days: Math.round(Number(rollDays))
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

  const c = $derived(data?.result);
  const s = $derived(c?.summary);
  const hasSpot = $derived(!!c?.rows?.some((r) => r.spot != null));

  const curveBuild = $derived.by(() => {
    if (!c?.curve?.length) return null;
    const spot = c.rows.at(-1)?.spot;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.fut.curveTitle', { d: c.as_of })),
      grid: { left: 56, right: 16, top: 32, bottom: 36 },
      tooltip: tooltip(p, {
        trigger: 'item',
        formatter: (q) => {
          const pt = c.curve[q.dataIndex];
          return pt ? `${pt.ticker} · ${pt.last_trade}<br>${fmtNum(pt.price, 2)} · ${$t('quant.fut.dte', { n: pt.dte })}` : '';
        }
      }),
      xAxis: valueAxis(p, { name: $t('quant.fut.daysToExpiry'), nameLocation: 'middle', nameGap: 22 }),
      yAxis: valueAxis(p, { scale: true }),
      series: [
        {
          type: 'line',
          data: c.curve.map((q) => [q.dte, q.price]),
          symbolSize: 7,
          lineStyle: { color: p.series[0], width: 1.5 },
          itemStyle: { color: p.series[0] },
          label: { show: true, position: 'top', color: p.muted, fontSize: 10, formatter: (q) => c.curve[q.dataIndex]?.ticker },
          markLine: spot
            ? { symbol: 'none', silent: true, label: { formatter: `${$t('quant.fut.spot')} ${fmtNum(spot, 2)}`, color: p.muted, position: 'insideStartTop' }, lineStyle: { color: p.muted, type: 'dashed' }, data: [{ yAxis: spot }] }
            : undefined
        }
      ]
    });
  });

  const priceBuild = $derived.by(() => {
    if (!c?.rows?.length) return null;
    const names = [$t('quant.fut.continuous'), $t('quant.fut.spliced'), $t('quant.fut.spot')];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.fut.priceTitle')),
      legend: legend(p, { data: hasSpot ? names : names.slice(0, 2) }),
      grid: { left: 56, right: 16, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtNum(+v, 2) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { scale: true }),
      series: [
        { name: names[0], type: 'line', symbol: 'none', data: c.rows.map((r) => [r.date, r.continuous]), lineStyle: { color: p.series[0], width: 1.5 }, itemStyle: { color: p.series[0] } },
        {
          name: names[1],
          type: 'line',
          symbol: 'none',
          data: c.rows.map((r) => [r.date, r.front_price]),
          lineStyle: { color: p.muted, width: 1 },
          itemStyle: { color: p.muted },
          markLine: { symbol: 'none', silent: true, label: { show: false }, lineStyle: { color: p.amber, type: 'dotted', width: 1 }, data: c.rolls.map((r) => ({ xAxis: r.date })) }
        },
        ...(hasSpot
          ? [{ name: names[2], type: 'line', symbol: 'none', data: c.rows.filter((r) => r.spot != null).map((r) => [r.date, r.spot]), lineStyle: { color: p.series[2], width: 1 }, itemStyle: { color: p.series[2] } }]
          : [])
      ]
    });
  });

  const rollBuild = $derived.by(() => {
    if (!c?.rows?.some((r) => r.roll_yield != null)) return null;
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.fut.rollTitle')),
      grid: { left: 56, right: 16, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtPc(+v, 2) }),
      xAxis: timeAxis(p),
      yAxis: valueAxis(p, { axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => fmtPc(v, 1) } }),
      series: [
        {
          type: 'line',
          symbol: 'none',
          data: c.rows.filter((r) => r.roll_yield != null).map((r) => [r.date, r.roll_yield]),
          lineStyle: { color: p.series[1], width: 1.2 },
          areaStyle: { color: p.series[1], opacity: 0.08 },
          markLine: { symbol: 'none', silent: true, label: { show: false }, lineStyle: { color: p.border }, data: [{ yAxis: 0 }] }
        }
      ]
    });
  });

  const basisBuild = $derived.by(() => {
    if (!hasSpot) return null;
    const rows = c.rows.filter((r) => r.carry != null);
    const withFair = rows.some((r) => r.mispricing != null);
    const names = [$t('quant.fut.carry'), $t('quant.fut.mispricingPct')];
    return (p) => ({
      animation: false,
      title: title(p, $t('quant.fut.basisTitle')),
      legend: legend(p, { data: withFair ? names : names.slice(0, 1) }),
      grid: { left: 56, right: 56, top: 32, bottom: 28 },
      tooltip: tooltip(p, { valueFormatter: (v) => fmtPc(+v, 2) }),
      xAxis: timeAxis(p),
      yAxis: [
        valueAxis(p, { axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => fmtPc(v, 1) } }),
        valueAxis(p, { splitLine: { show: false }, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => fmtPc(v, 2) } })
      ],
      series: [
        { name: names[0], type: 'line', symbol: 'none', data: rows.map((r) => [r.date, r.carry]), lineStyle: { color: p.series[0], width: 1.2 }, itemStyle: { color: p.series[0] } },
        ...(withFair
          ? [{ name: names[1], type: 'line', symbol: 'none', yAxisIndex: 1, data: rows.filter((r) => r.mispricing != null).map((r) => [r.date, r.mispricing / r.spot]), lineStyle: { color: p.amber, width: 1 }, itemStyle: { color: p.amber } }]
          : [])
      ]
    });
  });
</script>

<div class="qp">
  <div class="qp-params">
    <MarketSource bind:value={source} bind:provider />
    <label class="qp-field"><span class="qp-label">{$t('quant.fut.root')}</span><input type="text" bind:value={root} placeholder={provider === 'massive' ? 'ES' : 'ES@CME'} /></label>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.fut.years')}</span>
      <div class="qp-seg">
        {#each [1, 2, 3, 5] as y (y)}
          <button class:active={years === y} onclick={() => (years = y)}>{y}</button>
        {/each}
      </div>
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.fut.ahead')}</span><input type="number" min="0" max="36" step="1" bind:value={ahead} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.fut.spotTicker')}</span><input type="text" bind:value={spotTicker} placeholder="SPX" /></label>
    <div class="qp-field">
      <span class="qp-label">{$t('quant.fut.spotType')}</span>
      <div class="qp-seg">
        {#each ['index', 'etf', 'equity', 'fx'] as k (k)}
          <button class:active={spotType === k} onclick={() => (spotType = k)}>{$t(`quant.market.type.${k}`)}</button>
        {/each}
      </div>
    </div>
    <label class="qp-field"><span class="qp-label">{$t('quant.options.rate')}</span><input type="number" step="0.1" bind:value={rate} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.basis.yield')}</span><input type="number" step="0.1" bind:value={yieldRate} /></label>
    <label class="qp-field"><span class="qp-label">{$t('quant.fut.rollDays')}</span><input type="number" min="0" max="60" step="1" bind:value={rollDays} /></label>
    <button class="primary" onclick={run} disabled={!source || busy || !root.trim()}>{busy ? $t('quant.page.computing') : $t('quant.page.compute')}</button>
  </div>

  {#if busy}
    <TaskProgress {progress} />
  {/if}
  <ErrorText {error} />

  {#if c}
    <p class="qp-hint">
      {$t('quant.fut.summary', { root: data.root, n: c.contracts.length, from: s.first, to: s.last, d: c.as_of })}
      {#if data.notes?.length}<span class="qp-warn">{$t('quant.fut.notes', { n: data.notes.length })}</span>{/if}
    </p>
    <section class="qp-block">
      <div class="qp-cards">
        <div class="qp-card"><span class="k">{$t('quant.fut.shape')}</span><span class="v">{$t(`quant.fut.shapes.${c.shape}`)}</span><span class="s">{$t('quant.fut.shapeHint', { n: c.curve.length })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.fut.rollNow')}</span><span class="v">{fmtPc(s.current_roll_yield, 2)}</span><span class="s">{$t('quant.fut.rollNowHint')}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.fut.rollMean')}</span><span class="v">{fmtPc(s.mean_roll_yield, 2)}</span><span class="s">{$t('quant.fut.contangoShare', { v: fmtPc(s.contango_share, 0) })}</span></div>
        <div class="qp-card"><span class="k">{$t('quant.fut.futReturn')}</span><span class="v" class:qp-good={s.futures_return > 0} class:qp-bad={s.futures_return < 0}>{fmtPc(s.futures_return, 1)}</span><span class="s">{$t('quant.fut.perYear', { v: fmtPc(s.futures_ann, 1) })} · {$t('quant.fut.rolls', { n: s.rolls })}</span></div>
        {#if s.spot_return != null}
          <div class="qp-card"><span class="k">{$t('quant.fut.spotReturn')}</span><span class="v">{fmtPc(s.spot_return, 1)}</span><span class="s">{$t('quant.fut.futOnSpot', { v: fmtPc(s.futures_return_on_spot_dates, 1) })}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.fut.rollReturn')}</span><span class="v" class:qp-good={s.roll_return_ann > 0} class:qp-bad={s.roll_return_ann < 0}>{fmtPc(s.roll_return_ann, 2)}</span><span class="s">{$t('quant.fut.rollReturnHint')}</span></div>
        {/if}
        {#if s.current_carry != null}
          <div class="qp-card"><span class="k">{$t('quant.fut.carryNow')}</span><span class="v">{fmtPc(s.current_carry, 2)}</span><span class="s">{$t('quant.fut.repo', { v: fmtPc(c.rows.at(-1).implied_repo, 2) })}</span></div>
          <div class="qp-card"><span class="k">{$t('quant.basis.basis')}</span><span class="v">{fmtNum(c.rows.at(-1).basis, 2)}</span><span class="s">{fmtPc(c.rows.at(-1).basis_pct, 3)}</span></div>
        {/if}
        {#if s.mean_mispricing_pct != null}
          <div class="qp-card"><span class="k">{$t('quant.fut.mispricing')}</span><span class="v">{fmtPc(s.mean_mispricing_pct, 3)}</span><span class="s">{$t('quant.fut.mispricingHint')}</span></div>
        {/if}
      </div>
      <p class="qp-hint">{$t('quant.fut.hint')}</p>
    </section>
    <div class="qp-grid2">
      <section class="qp-block"><QChart build={curveBuild} height={280} /></section>
      <section class="qp-block"><QChart build={rollBuild} height={280} /><p class="qp-hint">{$t('quant.fut.rollChartHint')}</p></section>
    </div>
    <section class="qp-block">
      <QChart build={priceBuild} height={300} />
      <p class="qp-hint">{$t('quant.fut.priceHint')}</p>
    </section>
    {#if basisBuild}
      <section class="qp-block"><QChart build={basisBuild} height={260} /><p class="qp-hint">{$t('quant.fut.basisHint')}</p></section>
    {/if}
    <div class="qp-grid2">
      <section class="qp-block">
        <h3>{$t('quant.fut.rollsTitle')}</h3>
        <div class="qp-scroll">
          <table class="qp-table">
            <thead><tr><th>{$t('quant.fut.date')}</th><th>{$t('quant.fut.from')}</th><th>{$t('quant.fut.to')}</th><th class="num">{$t('quant.fut.gap')}</th></tr></thead>
            <tbody>
              {#each c.rolls as r (r.date)}
                <tr><td>{r.date}</td><td>{r.from} <span class="qp-muted">{fmtNum(r.from_price, 2)}</span></td><td>{r.to} <span class="qp-muted">{fmtNum(r.to_price, 2)}</span></td><td class="num">{fmtPc(r.gap, 2)}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
      <section class="qp-block">
        <h3>{$t('quant.fut.contractsTitle')}</h3>
        <div class="qp-scroll">
          <table class="qp-table">
            <thead><tr><th>{$t('quant.fut.contract')}</th><th>{$t('quant.fut.lastTrade')}</th><th class="num">{$t('quant.fut.bars')}</th><th>{$t('quant.fut.range')}</th></tr></thead>
            <tbody>
              {#each c.contracts as k (k.ticker)}
                <tr><td>{k.ticker}</td><td>{k.last_trade}</td><td class="num">{k.bars}</td><td class="qp-muted">{k.first ?? '−'} → {k.last ?? '−'}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#each data.notes ?? [] as n (n)}
          <p class="qp-hint qp-warn">{n}</p>
        {/each}
      </section>
    </div>
  {:else if !busy}
    <p class="qp-hint">{$t('quant.fut.pickHint')}</p>
  {/if}
</div>
