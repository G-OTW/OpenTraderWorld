<script>
  // Winning and losing streaks as paths in a box turned by hand. Every streak starts at
  // zero and climbs (or sinks) one trade at a time, so their shapes compare directly:
  //
  //   x  rank of the trade inside its streak (0 = the start)
  //   y  the streak's place in time, oldest at the front; the average path sits at 0
  //   z  what the streak has made so far, in money or in R
  //
  // Front view lays every streak over the others; top view spreads them along time.
  //
  // Same rule as the server's streak count (`journal_analytics.rs`): a breakeven trade
  // ends the run without joining one.
  import { onMount, untrack } from 'svelte';
  import { fmtMoney, fmtSignedMoney, fmtNum } from './api.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';

  // Loaded when the tab opens, like the 3D scatter. `$state.raw`: never proxy a module.
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

  let {
    points = [],
    currency = 'USD',
    // The backtest has no planned risk per trade, so it offers money only.
    units = ['money', 'r'],
    storageKey = 'otw.journal.streaks3d.v1'
  } = $props();

  // ── Reader's choices, kept across visits ──
  // Fixed for the component's life: a caller does not swap its storage key or units.
  const KEY = untrack(() => storageKey);
  const saved = (() => {
    try {
      return JSON.parse(localStorage.getItem(KEY)) ?? {};
    } catch {
      return {};
    }
  })();
  const UNITS = untrack(() => units);
  const SHOWS = ['both', 'wins', 'losses'];
  const MIN_LENS = [1, 2, 3, 4, 5];

  let unit = $state(UNITS.includes(saved.unit) ? saved.unit : UNITS[0]);
  let show = $state(SHOWS.includes(saved.show) ? saved.show : 'both');
  let minLen = $state(MIN_LENS.includes(saved.minLen) ? saved.minLen : 2);
  let fold = $state(saved.fold ?? false);

  $effect(() => {
    const state = { unit, show, minLen, fold };
    try {
      localStorage.setItem(KEY, JSON.stringify(state));
    } catch {
      /* private mode: the chart still works, it just forgets */
    }
  });

  // ── Data ──

  /** Every run of same-sign trades, oldest first. Points arrive oldest first. */
  const runs = $derived.by(() => {
    const out = [];
    let cur = null;
    for (const p of points) {
      const dir = p.net > 0 ? 1 : p.net < 0 ? -1 : 0;
      if (dir === 0) {
        if (cur) out.push(cur);
        cur = null;
        continue;
      }
      if (!cur || cur.dir !== dir) {
        if (cur) out.push(cur);
        cur = { dir, trades: [] };
      }
      cur.trades.push(p);
    }
    if (cur) out.push(cur);
    return out;
  });

  // The streaks on screen, each with its running total. In R, a streak with one trade
  // lacking a stop cannot be summed: it is left out and counted, never read as zero.
  const plotted = $derived.by(() => {
    const paths = [];
    let skipped = 0;
    for (const run of runs) {
      if (run.trades.length < minLen) continue;
      if (show === 'wins' && run.dir < 0) continue;
      if (show === 'losses' && run.dir > 0) continue;
      const vals = run.trades.map((p) => (unit === 'r' ? p.r : p.net));
      if (vals.some((v) => v == null || !Number.isFinite(v))) {
        skipped += 1;
        continue;
      }
      let sum = 0;
      const cum = vals.map((v) => (sum += v));
      paths.push({ ord: paths.length + 1, dir: run.dir, trades: run.trades, vals, cum });
    }
    return { paths, skipped };
  });

  const z = (v) => (fold ? Math.abs(v) : v);

  /**
   * The average path of one side: at rank k, the mean over the streaks that reached k.
   * Stops where fewer than two streaks are left, since one streak is not an average.
   */
  function meanPath(dir) {
    const ps = plotted.paths.filter((p) => p.dir === dir);
    const out = [[0, 0, 0]];
    for (let k = 0; ; k++) {
      const at = ps.filter((p) => p.cum.length > k);
      if (at.length < 2) break;
      out.push([k + 1, 0, z(at.reduce((a, p) => a + p.cum[k], 0) / at.length)]);
    }
    return out.length > 1 ? out : null;
  }

  const summary = $derived.by(() => {
    const side = (dir) => {
      const ps = plotted.paths.filter((p) => p.dir === dir);
      const lens = ps.map((p) => p.cum.length);
      return {
        count: ps.length,
        avgLen: lens.length ? lens.reduce((a, b) => a + b, 0) / lens.length : 0,
        longest: lens.length ? Math.max(...lens) : 0
      };
    };
    return { wins: side(1), losses: side(-1) };
  });

  // ── Formatting ──
  const val = (v) => (unit === 'r' ? `${fmtNum(v)}R` : fmtSignedMoney(v, currency));
  function tick(v) {
    if (unit === 'r') return `${+v.toFixed(1)}R`;
    const a = Math.abs(v);
    if (a >= 1e6) return `${fmtMoney(v / 1e6, currency, 1)}M`;
    if (a >= 1e4) return `${fmtMoney(v / 1e3, currency, 0)}k`;
    return fmtMoney(v, currency, 0);
  }
  const day = (ms) => new Date(ms).toLocaleDateString();

  const UNIT_OPTIONS = $derived(
    UNITS.map((u) => ({ value: u, label: $t(`journal.analytics.streaks.unit.${u}`) }))
  );
  const SHOW_OPTIONS = $derived(
    SHOWS.map((s) => ({ value: s, label: $t(`journal.analytics.streaks.show.${s}`) }))
  );
  const LEN_OPTIONS = $derived(
    MIN_LENS.map((n) => ({ value: n, label: $t('journal.analytics.streaks.minLenN', { n }) }))
  );

  // ── Chart ──
  let el = $state(null);
  // Write-only in the init effect, read by the paint effect (see ScatterPanel).
  let chart = $state.raw(null);
  const colors = $derived(chartColors());

  // Fixed views use an orthographic camera: perspective would make the far streaks look
  // smaller than the near ones, which is exactly the comparison the view is for.
  const VIEWS = {
    free: { projection: 'perspective', alpha: 22, beta: 35, distance: 230, center: [0, 0, 0] },
    front: { projection: 'orthographic', alpha: 0, beta: 0, orthographicSize: 115, center: [0, 0, 0] },
    top: { projection: 'orthographic', alpha: 90, beta: 0, orthographicSize: 140, center: [0, 0, 0] }
  };
  let view = $state('free');

  function build(pal, camera, mode) {
    const { dim, border, gridLine, surface, text, mono, green, red, accent } = pal;
    const { paths } = plotted;
    const labelStyle = { color: dim, fontFamily: mono, fontSize: 10 };
    const nameStyle = { color: dim, fontFamily: mono, fontSize: 11 };
    const winName = $t('journal.analytics.streaks.win');
    const lossName = $t('journal.analytics.streaks.loss');
    const maxLen = Math.max(1, ...paths.map((p) => p.cum.length));
    const faint = paths.length > 60 ? 0.25 : 0.45;

    // One polyline per streak, from the zero start. Series of a side share its name, so
    // one legend entry shows or hides the whole side.
    const lines = paths.map((p) => ({
      type: 'line3D',
      name: p.dir > 0 ? winName : lossName,
      silent: true,
      lineStyle: { color: p.dir > 0 ? green : red, width: 1.5, opacity: faint },
      data: [[0, p.ord, 0], ...p.cum.map((c, k) => [k + 1, p.ord, z(c)])]
    }));

    // The trades themselves, one scatter per side: what the tooltip reads.
    const dots = [1, -1].map((dir) => ({
      type: 'scatter3D',
      name: dir > 0 ? winName : lossName,
      symbolSize: 5,
      itemStyle: { color: dir > 0 ? green : red, opacity: 0.9 },
      emphasis: { itemStyle: { color: accent }, label: { show: false } },
      data: paths
        .filter((p) => p.dir === dir)
        .flatMap((p) => p.cum.map((c, k) => ({ value: [k + 1, p.ord, z(c)], path: p, k })))
    }));

    const means = [1, -1]
      .map((dir) => ({ dir, data: meanPath(dir) }))
      .filter((m) => m.data)
      .map((m) => ({
        type: 'line3D',
        name: m.dir > 0 ? winName : lossName,
        silent: true,
        lineStyle: { color: m.dir > 0 ? green : red, width: 4, opacity: 1 },
        data: m.data
      }));

    // Seen end on, an axis piles every label onto one spot: a fixed view hides the one
    // pointing at the reader, and the axis names, which land on the ticks once the box is
    // flat. Turning the box by hand brings them back (the drag handler in the init effect).
    const endOn = (axis) => (mode === 'front' && axis === 'y') || (mode === 'top' && axis === 'z');
    const flat = mode === 'front' || mode === 'top';
    const shown = (axis, name) => ({
      name: flat ? ' ' : name,
      ...(endOn(axis) ? { axisLabel: { show: false } } : {})
    });

    const ordLabel = (v) => {
      const i = Math.round(v);
      if (Math.abs(v - i) > 1e-6) return '';
      if (i === 0) return $t('journal.analytics.streaks.average');
      const p = paths[i - 1];
      return p ? day(p.trades[0].at) : '';
    };

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
        data: [winName, lossName].filter((n) =>
          [...lines, ...means].some((s) => s.name === n)
        )
      },
      tooltip: {
        trigger: 'item',
        backgroundColor: surface,
        borderColor: border,
        textStyle: { color: text, fontFamily: mono, fontSize: 11 },
        formatter: (e) => {
          const { path, k } = e.data ?? {};
          if (!path) return '';
          const tr = path.trades[k];
          const len = path.cum.length;
          const first = path.trades[0].at;
          const last = path.trades[len - 1].at;
          const span = day(first) === day(last) ? day(first) : `${day(first)} → ${day(last)}`;
          return [
            `<b>${$t(path.dir > 0 ? 'journal.analytics.streaks.tipWin' : 'journal.analytics.streaks.tipLoss', { n: len })}</b> · ${span}`,
            `${$t('journal.analytics.streaks.tipTrade', { i: k + 1, n: len })}: ${tr.ticker ? `${tr.ticker} ` : ''}${val(path.vals[k])}`,
            `${$t('journal.analytics.streaks.tipSoFar')}: ${val(path.cum[k])}`,
            `${$t('journal.analytics.streaks.tipTotal')}: ${val(path.cum[len - 1])}`
          ].join('<br>');
        }
      },
      grid3D: {
        boxWidth: 110,
        boxDepth: 110,
        boxHeight: 80,
        environment: 'none',
        axisLine: { lineStyle: { color: border } },
        axisTick: { show: false },
        splitLine: { lineStyle: { color: gridLine, width: 1 } },
        splitArea: { show: false },
        axisPointer: { show: false },
        viewControl: {
          autoRotate: false,
          rotateSensitivity: 1,
          zoomSensitivity: 1,
          panMouseButton: 'right',
          rotateMouseButton: 'left',
          minDistance: 80,
          maxDistance: 400,
          ...camera
        }
      },
      xAxis3D: {
        type: 'value',
        nameTextStyle: nameStyle,
        min: 0,
        max: maxLen,
        interval: maxLen > 12 ? Math.ceil(maxLen / 12) : 1,
        axisLabel: { textStyle: labelStyle, formatter: (v) => String(Math.round(v)) },
        ...shown('x', $t('journal.analytics.streaks.rank'))
      },
      yAxis3D: {
        type: 'value',
        nameTextStyle: nameStyle,
        min: 0,
        max: Math.max(1, paths.length),
        axisLabel: { textStyle: labelStyle, formatter: ordLabel },
        ...shown('y', $t('journal.analytics.streaks.order'))
      },
      zAxis3D: {
        type: 'value',
        nameTextStyle: nameStyle,
        nameGap: 36,
        axisLabel: { textStyle: labelStyle, formatter: tick },
        ...shown('z', $t(`journal.analytics.streaks.cum.${unit}`))
      },
      series: [...lines, ...means, ...dots]
    };
  }

  /** Where the reader left the camera, so a filter or a toggle does not reset it. */
  function currentCamera(c) {
    const vc = c.getOption()?.grid3D?.[0]?.viewControl;
    if (!vc) return VIEWS[view];
    const { projection, alpha, beta, distance, center, orthographicSize } = vc;
    return { projection, alpha, beta, distance, center, orthographicSize };
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
    // A drag away from a fixed view makes it a free one again, labels included. A plain
    // click (legend, a dot) is not a drag and keeps the view.
    let down = null;
    c.getZr().on('mousedown', (e) => (down = [e.offsetX, e.offsetY]));
    c.getZr().on('mouseup', (e) => {
      const moved = down && Math.hypot(e.offsetX - down[0], e.offsetY - down[1]) > 4;
      down = null;
      if (moved && untrack(() => view) !== 'free') view = 'free';
    });
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(node);
    return () => {
      ro.disconnect();
      c.dispose();
      chart = null;
    };
  });

  $effect(() => {
    // Read every input so a theme flip, a toggle or a filter repaints the box.
    const c = chart;
    if (!c) return;
    c.setOption(build(colors, currentCamera(c), view), true);
  });

  function setView(v) {
    view = v;
    chart?.setOption(build(colors, VIEWS[v], v), true);
  }
</script>

<section class="card">
  <div class="controls">
    <div class="pickers">
      {#if UNITS.length > 1}
        <Select label={$t('journal.analytics.streaks.unitLabel')} options={UNIT_OPTIONS} bind:value={unit} />
      {/if}
      <Select label={$t('journal.analytics.streaks.showLabel')} options={SHOW_OPTIONS} bind:value={show} />
      <Select label={$t('journal.analytics.streaks.minLen')} options={LEN_OPTIONS} bind:value={minLen} />
    </div>
    <div class="chips">
      <button type="button" class="chip" class:on={fold} onclick={() => (fold = !fold)}>
        {$t('journal.analytics.streaks.fold')}
      </button>
      {#each ['front', 'top', 'free'] as v (v)}
        <button type="button" class="chip" class:on={view === v} onclick={() => setView(v)}>
          {$t(`journal.analytics.streaks.view.${v}`)}
        </button>
      {/each}
    </div>
  </div>

  {#if glError}
    <EmptyState
      compact
      icon="alert-triangle"
      title={$t('journal.analytics.scatter3d.noGl')}
      description={$t('journal.analytics.scatter3d.noGlHint')}
    />
  {:else if plotted.paths.length === 0}
    <EmptyState
      compact
      icon="bar-chart"
      title={$t('journal.analytics.streaks.noData')}
      description={$t('journal.analytics.streaks.noDataHint')}
    />
  {:else if !echarts}
    <Skeleton height="540px" />
  {:else}
    <div class="chart" bind:this={el}></div>
    <p class="caption">
      {#if show !== 'losses'}
        <span class="pos">
          {$t('journal.analytics.streaks.sumWin', {
            count: summary.wins.count,
            avg: fmtNum(summary.wins.avgLen),
            max: summary.wins.longest
          })}
        </span>
      {/if}
      {#if show !== 'wins'}
        <span class="neg">
          {$t('journal.analytics.streaks.sumLoss', {
            count: summary.losses.count,
            avg: fmtNum(summary.losses.avgLen),
            max: summary.losses.longest
          })}
        </span>
      {/if}
      {#if plotted.skipped > 0}
        <span class="skipped">
          {$t('journal.analytics.streaks.skippedR', { count: plotted.skipped })}
        </span>
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
    grid-template-columns: repeat(3, minmax(140px, 1fr));
    align-items: end;
    gap: var(--space-2);
    flex: 1 1 440px;
  }
  .chips {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
  }
  /* Same frame as the scatter chips (ScatterPanel), one control row, one baseline. */
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
  .chip.on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .chart {
    width: 100%;
    height: 540px;
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
  @media (max-width: 720px) {
    .pickers {
      grid-template-columns: 1fr;
    }
  }
</style>
