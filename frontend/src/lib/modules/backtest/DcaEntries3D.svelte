<script>
  // The buys of a savings plan as a line in a box turned by hand, one line per asset:
  //
  //   x  when the tranche was bought
  //   y  the asset
  //   z  what that tranche is worth now against what it cost (percent, money), or its price
  //
  // A dot is green when the tranche is in profit at the last bar, red when it is in loss,
  // fees included: a buy at the last price reads as a small loss, which is what it is.
  import { onMount } from 'svelte';
  import { fmtNum } from './api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  // Loaded when the tab opens, like the journal's 3D views. `$state.raw`: never proxy a module.
  let echarts = $state.raw(null);
  let glError = $state(false);
  onMount(async () => {
    try {
      const lib = await import('echarts');
      await import('echarts-gl');
      echarts = lib;
    } catch {
      glError = true;
    }
  });

  let { dca = null } = $props();

  // ── Reader's choice, kept across visits ──
  const KEY = 'otw.backtest.dca3d.v1';
  const ZS = ['pnlPct', 'pnl', 'price'];
  let zId = $state(
    (() => {
      try {
        const z = JSON.parse(localStorage.getItem(KEY))?.z;
        return ZS.includes(z) ? z : 'pnlPct';
      } catch {
        return 'pnlPct';
      }
    })()
  );
  $effect(() => {
    const state = { z: zId };
    try {
      localStorage.setItem(KEY, JSON.stringify(state));
    } catch {
      /* private mode: the chart still works, it just forgets */
    }
  });

  // ── Data ──
  const lastPrice = $derived(new Map((dca?.assets ?? []).map((a) => [a.ticker, a.last_price])));

  /** Every buy, oldest first, with what it is worth at the last bar. A buy whose asset has no
   *  last price cannot be marked, so it is left out rather than read as flat. */
  const entries = $derived.by(() => {
    const out = [];
    for (const e of dca?.events ?? []) {
      if (e.action !== 'buy' || !(e.qty > 0)) continue;
      const last = lastPrice.get(e.ticker);
      const ts = Date.parse(e.ts);
      if (!(last > 0) || !Number.isFinite(ts)) continue;
      const cost = -e.amount;
      const pnl = last * e.qty - cost;
      out.push({ e, ts, last, cost, pnl, pnlPct: cost > 0 ? (pnl / cost) * 100 : 0 });
    }
    return out.sort((a, b) => a.ts - b.ts);
  });

  const tickers = $derived([...new Set(entries.map((r) => r.e.ticker))]);
  const zOf = (r) => (zId === 'price' ? r.e.price : r[zId]);

  const counts = $derived({
    win: entries.filter((r) => r.pnl > 0).length,
    loss: entries.filter((r) => r.pnl <= 0).length
  });
  // The engine sends the tail of a long plan only; say so rather than draw a partial history
  // as the whole one.
  const capped = $derived(
    dca?.events_total != null && dca.events_total > (dca.events?.length ?? 0) ? dca.events_total : null
  );

  // ── Formatting ──
  const day = (ms) => new Date(ms).toLocaleDateString();
  const signed = (v, d = 2) => `${v > 0 ? '+' : ''}${fmtNum(v, d)}`;
  function fmtZ(v) {
    if (zId === 'pnlPct') return `${signed(v)}%`;
    if (zId === 'pnl') return signed(v);
    return fmtNum(v);
  }
  function tickZ(v) {
    if (zId === 'pnlPct') return `${+v.toFixed(1)}%`;
    const a = Math.abs(v);
    if (a >= 1e6) return `${+(v / 1e6).toFixed(1)}M`;
    if (a >= 1e4) return `${+(v / 1e3).toFixed(0)}k`;
    return String(+v.toFixed(a < 10 ? 2 : 0));
  }

  const Z_OPTIONS = $derived(ZS.map((z) => ({ value: z, label: $t(`backtest.dca3d.z.${z}`) })));

  // ── Chart ──
  let el = $state(null);
  // Write-only in the init effect, read by the paint effect (see the journal's ScatterPanel).
  let chart = $state.raw(null);
  const colors = $derived(chartColors());

  const DEFAULT_VIEW = { alpha: 18, beta: 30, distance: 220, center: [0, 0, 0] };

  function build(pal, view) {
    const { muted, dim, border, gridLine, surface, text, mono, green, red, accent } = pal;
    const labelStyle = { color: dim, fontFamily: mono, fontSize: 10 };
    const nameStyle = { color: dim, fontFamily: mono, fontSize: 11 };
    const winName = $t('backtest.dca3d.inProfit');
    const lossName = $t('backtest.dca3d.inLoss');
    const refName = $t(zId === 'price' ? 'backtest.dca3d.lastPrice' : 'backtest.dca3d.breakEven');
    const first = entries[0]?.ts ?? 0;
    const lastTs = entries[entries.length - 1]?.ts ?? first;
    const dense = entries.length > 600;

    // The path of each asset's buys, in time: the timeline the dots sit on.
    const paths = tickers.map((tk, y) => ({
      type: 'line3D',
      name: tk,
      silent: true,
      lineStyle: { color: muted, width: 1.5, opacity: 0.6 },
      data: entries.filter((r) => r.e.ticker === tk).map((r) => [r.ts, y, zOf(r)])
    }));

    // The line a dot must clear to be in profit: the last price, or zero on a result axis.
    const refs = tickers.map((tk, y) => {
      const z = zId === 'price' ? lastPrice.get(tk) : 0;
      return {
        type: 'line3D',
        name: refName,
        silent: true,
        lineStyle: { color: accent, width: 2, opacity: 0.8 },
        data: [
          [first, y, z],
          [lastTs, y, z]
        ]
      };
    });

    const dots = [
      { name: winName, color: green, keep: (r) => r.pnl > 0 },
      { name: lossName, color: red, keep: (r) => r.pnl <= 0 }
    ].map((s) => ({
      type: 'scatter3D',
      name: s.name,
      symbolSize: dense ? 5 : 8,
      itemStyle: { color: s.color, opacity: 0.9 },
      emphasis: { itemStyle: { color: accent }, label: { show: false } },
      data: entries
        .filter(s.keep)
        .map((r) => ({ value: [r.ts, tickers.indexOf(r.e.ticker), zOf(r)], row: r }))
    }));

    return {
      animation: false,
      legend: {
        top: 0,
        right: 0,
        icon: 'circle',
        itemWidth: 8,
        itemHeight: 8,
        textStyle: { color: dim, fontFamily: mono, fontSize: 10 },
        inactiveColor: border,
        data: [winName, lossName, refName]
      },
      tooltip: {
        trigger: 'item',
        backgroundColor: surface,
        borderColor: border,
        textStyle: { color: text, fontFamily: mono, fontSize: 11 },
        formatter: (p) => {
          const r = p.data?.row;
          if (!r) return '';
          return [
            `<b>${r.e.ticker} · ${day(r.ts)}</b> · ${r.e.source}`,
            `${$t('backtest.dca3d.paid')}: ${fmtNum(r.e.price)} × ${fmtNum(r.e.qty, 4)}`,
            `${$t('backtest.dca3d.lastPrice')}: ${fmtNum(r.last)}`,
            `${$t('backtest.dca3d.result')}: ${signed(r.pnl)} (${signed(r.pnlPct)}%)`
          ].join('<br>');
        }
      },
      grid3D: {
        boxWidth: 120,
        boxDepth: Math.min(100, 25 * tickers.length),
        boxHeight: 70,
        environment: 'none',
        axisLine: { lineStyle: { color: border } },
        axisTick: { show: false },
        splitLine: { lineStyle: { color: gridLine, width: 1 } },
        splitArea: { show: false },
        axisPointer: { show: false },
        viewControl: {
          projection: 'perspective',
          autoRotate: false,
          rotateSensitivity: 1,
          zoomSensitivity: 1,
          panMouseButton: 'right',
          rotateMouseButton: 'left',
          minDistance: 80,
          maxDistance: 400,
          ...view
        }
      },
      xAxis3D: {
        type: 'time',
        name: $t('backtest.dca.date'),
        nameTextStyle: nameStyle,
        axisLabel: { textStyle: labelStyle, formatter: (v) => day(v) }
      },
      yAxis3D: {
        type: 'category',
        name: $t('backtest.asset.ticker'),
        nameTextStyle: nameStyle,
        data: tickers,
        axisLabel: { textStyle: labelStyle }
      },
      zAxis3D: {
        type: 'value',
        name: $t(`backtest.dca3d.z.${zId}`),
        nameTextStyle: nameStyle,
        nameGap: 36,
        scale: zId === 'price',
        axisLabel: { textStyle: labelStyle, formatter: tickZ }
      },
      series: [...paths, ...refs, ...dots]
    };
  }

  function currentView(c) {
    const vc = c.getOption()?.grid3D?.[0]?.viewControl;
    if (!vc) return DEFAULT_VIEW;
    const { alpha, beta, distance, center } = vc;
    return { alpha, beta, distance, center };
  }

  $effect(() => {
    const node = el;
    const lib = echarts;
    if (!node || !lib) return;
    let c;
    try {
      c = lib.init(node, null, { renderer: 'canvas' });
    } catch {
      glError = true;
      return;
    }
    chart = c;
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(node);
    return () => {
      ro.disconnect();
      c.dispose();
      chart = null;
    };
  });

  $effect(() => {
    // Read every input so a theme flip, an axis change or a new run repaints the box.
    const c = chart;
    if (!c) return;
    c.setOption(build(colors, currentView(c)), true);
  });

  function resetView() {
    chart?.setOption(build(colors, DEFAULT_VIEW), true);
  }
</script>

<section class="card">
  <div class="controls">
    <div class="pickers">
      <Select label={$t('backtest.dca3d.zLabel')} options={Z_OPTIONS} bind:value={zId} />
    </div>
    <div class="chips">
      <button type="button" class="chip" onclick={resetView}>
        {$t('journal.analytics.scatter3d.reset')}
      </button>
    </div>
  </div>

  {#if glError}
    <EmptyState
      compact
      icon="alert-triangle"
      title={$t('journal.analytics.scatter3d.noGl')}
      description={$t('journal.analytics.scatter3d.noGlHint')}
    />
  {:else if entries.length === 0}
    <EmptyState
      compact
      icon="bar-chart"
      title={$t('backtest.dca3d.noData')}
      description={$t('backtest.dca3d.noDataHint')}
    />
  {:else if !echarts}
    <Skeleton height="520px" />
  {:else}
    <div class="chart" bind:this={el}></div>
    <p class="caption">
      <span class="pos">{$t('backtest.dca3d.sumWin', { count: counts.win })}</span>
      <span class="neg">{$t('backtest.dca3d.sumLoss', { count: counts.loss })}</span>
      {#if capped}
        <span class="skipped">{$t('backtest.dca3d.capped', { n: dca.events.length, total: capped })}</span>
      {/if}
      <span class="hint">{$t('journal.analytics.scatter3d.hint')}</span>
    </p>
  {/if}
</section>

<style>
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    padding: var(--space-4);
  }
  .controls {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
    margin-bottom: var(--space-4);
  }
  .pickers {
    display: grid;
    grid-template-columns: minmax(180px, 240px);
    align-items: end;
    gap: var(--space-2);
  }
  .chips {
    display: flex;
    gap: var(--space-1);
  }
  /* Same frame as the journal's 3D chips, one control row, one baseline. */
  .chip {
    height: var(--control-h);
    padding: 0 var(--space-3);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    color: var(--text);
    font-size: 11.5px;
    letter-spacing: 0.04em;
    line-height: 1;
    cursor: pointer;
    white-space: nowrap;
  }
  .chip:hover {
    border-color: var(--accent);
  }
  .chart {
    width: 100%;
    height: 520px;
  }
  .caption {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
    font-size: 11px;
    color: var(--dim);
    font-family: var(--mono);
    padding-top: var(--space-2);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .skipped {
    color: var(--faint);
  }
  .hint {
    margin-left: auto;
    color: var(--faint);
  }
</style>
