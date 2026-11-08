<script>
  // Correlation matrix of daily returns over the window, on the diverging scale: the
  // negative pole, a neutral midpoint at 0, the positive pole (the --diverge-* tokens).
  // A pair with fewer than the minimum overlapping days stays on the neutral surface with
  // the reason on hover, not a coefficient from a handful of points. Hovering a cell
  // lights its row and column; values print inside cells wide enough to hold them.
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Tooltip from '$lib/ui/charts/Tooltip.svelte';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { watchlistsApi } from './api.js';
  import { divergeColor, inkOn } from './views.js';
  import { t } from '$lib/i18n';

  // onrefresh: re-quote the list (the fix offered when fewer than two symbols are covered).
  // stamp: the list's last refresh; a new one reloads the matrix.
  let { id, onrefresh = null, stamp = null } = $props();

  const WINDOWS = ['1m', '3m', '1y'];
  const HEAD = 64; // row header column, px
  const GAP = 2;
  let win = $state('3m');
  let data = $state(null);
  let loading = $state(false);
  let error = $state('');
  let hover = $state(null); // { i, j, x, y }
  let width = $state(0);

  $effect(() => {
    const w = win;
    const listId = id;
    stamp;
    loading = true;
    error = '';
    watchlistsApi
      .correlation(listId, w)
      .then((r) => listId === id && (data = r))
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  const c = $derived(chartColors());
  const syms = $derived(data?.symbols ?? []);
  const n = $derived(syms.length);
  const cell = $derived(
    n ? Math.max(26, Math.min(56, Math.floor((width - HEAD - GAP * n) / n))) : 0
  );
  const showValues = $derived(cell >= 36);

  const fmtR = (r) => `${r < 0 ? '−' : ''}${Math.abs(r).toFixed(2)}`;
  function fillOf(i, j) {
    if (i === j) return c.surface2;
    const v = data?.matrix?.[i]?.[j];
    return v ? divergeColor(v.r, c.diverge) : c.surface2;
  }

  const SCALE = [-1, -0.5, 0, 0.5, 1];

  const tip = $derived.by(() => {
    if (!hover || !data) return null;
    const { i, j } = hover;
    const v = data.matrix?.[i]?.[j];
    const title = `${syms[i]?.symbol} / ${syms[j]?.symbol}`;
    if (!v) return { x: hover.x, y: hover.y, title, rows: [], note: $t('watchlists.corr.tooFew', { n: data.min_overlap }) };
    return {
      x: hover.x,
      y: hover.y,
      title,
      rows: [
        { value: fmtR(v.r), label: $t('watchlists.corr.coef') },
        { value: $t('watchlists.corr.days', { n: v.n }), label: '' }
      ]
    };
  });

  function enter(e, i, j) {
    const el = e.currentTarget;
    hover = { i, j, x: el.offsetLeft + el.offsetWidth / 2, y: el.offsetTop };
  }
</script>

<div class="corr">
  <div class="bar">
    <div class="seg" role="group" aria-label={$t('watchlists.view.window')}>
      {#each WINDOWS as w (w)}
        <button class="chip" class:active={win === w} aria-pressed={win === w} onclick={() => (win = w)}>
          {$t(`watchlists.view.win.${w}`)}
        </button>
      {/each}
    </div>
    {#if n >= 2}
      <div class="scale" aria-hidden="true">
        {#each SCALE as s (s)}
          <span class="step">
            <span class="sw" style:background={s === 0 ? c.diverge[1] : divergeColor(s, c.diverge)}></span>
            <span class="sl">{s > 0 ? '+' : ''}{s === 0 ? '0' : fmtR(s)}</span>
          </span>
        {/each}
      </div>
    {/if}
  </div>
  <ErrorText {error} copyable />
  <div class="body" bind:clientWidth={width}>
    {#if loading && !data}
      <Skeleton height="240px" />
    {:else if data && n < 2}
      <EmptyState icon="grid" description={$t('watchlists.corr.needTwo')} compact>
        {#snippet action()}
          <!-- 1M reads the cached quotes (a refresh fills them); 3M and 1Y need stored candles. -->
          {#if win !== '1m'}
            <a class="btn sm fixlink" href="/histdata">{$t('histdata.page.title')}</a>
          {:else if onrefresh}
            <Button size="sm" icon="refresh-cw" onclick={onrefresh}>{$t('watchlists.detail.refresh')}</Button>
          {/if}
        {/snippet}
      </EmptyState>
    {:else if n >= 2}
      <div class="wrap" class:stale={loading}>
        <div
          class="mx"
          role="group"
          aria-label={$t('watchlists.view.correlation')}
          style:grid-template-columns="{HEAD}px repeat({n}, {cell}px)"
          style:gap="{GAP}px"
          onmouseleave={() => (hover = null)}
        >
          <span class="corner"></span>
          {#each syms as s, j (s.item_id)}
            <span class="h col" class:lit={hover?.j === j} title={s.symbol}>{s.symbol}</span>
          {/each}
          {#each syms as s, i (s.item_id)}
            <span class="h row" class:lit={hover?.i === i} title={s.symbol}>{s.symbol}</span>
            {#each syms as _, j (j)}
              {@const v = data.matrix[i]?.[j]}
              {@const bg = fillOf(i, j)}
              <span
                class="cell"
                class:diag={i === j}
                class:off={hover && hover.i !== i && hover.j !== j}
                class:on={hover?.i === i && hover?.j === j}
                role="img"
                style:height="{cell}px"
                style:background={bg}
                style:color={i === j || !v ? 'var(--dim)' : inkOn(bg, c.text, c.surface)}
                onmouseenter={(e) => enter(e, i, j)}
                aria-label="{s.symbol} / {syms[j].symbol}: {v ? fmtR(v.r) : $t('watchlists.col.na')}"
              >
                {#if showValues}{i === j ? '1' : v ? fmtR(v.r) : ''}{/if}
              </span>
            {/each}
          {/each}
        </div>
        {#if tip}
          <Tooltip x={tip.x} y={tip.y} {width} title={tip.title} rows={tip.rows} note={tip.note} />
        {/if}
      </div>
    {/if}
  </div>
  {#if data?.not_covered?.length}
    <div class="nc">
      <span class="lbl">{$t('watchlists.view.notCovered').replace(/:\s*$/, '')}</span>
      {#each data.not_covered as nc (nc.item_id)}
        <span class="ncrow"><b>{nc.symbol}</b> {nc.reason}</span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .corr {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .seg {
    display: inline-flex;
    gap: var(--space-1);
  }
  .scale {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    margin-left: auto;
  }
  .step {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .sw {
    width: 12px;
    height: 12px;
    border-radius: var(--radius-sm);
  }
  .sl {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
    color: var(--dim);
  }
  .body {
    min-width: 0;
  }
  .wrap {
    position: relative;
    overflow-x: auto;
    transition: opacity var(--dur-base) var(--ease);
  }
  .wrap.stale {
    opacity: 0.5;
  }
  .mx {
    display: grid;
    width: max-content;
  }
  .h {
    font-family: var(--mono);
    font-size: 10px;
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color var(--dur-fast) var(--ease);
  }
  .h.col {
    text-align: center;
    padding-bottom: var(--space-1);
    align-self: end;
  }
  .h.row {
    display: flex;
    align-items: center;
    padding-right: var(--space-2);
  }
  .h.lit {
    color: var(--text);
    font-weight: 600;
  }
  .cell {
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
    cursor: default;
    outline: 1px solid transparent;
    outline-offset: -1px;
    transition:
      opacity var(--dur-fast) var(--ease),
      outline-color var(--dur-fast) var(--ease);
  }
  .cell.off {
    opacity: 0.45;
  }
  .cell.on {
    outline-color: var(--text);
  }
  .fixlink {
    text-decoration: none;
  }
  .nc {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
    font-size: 12px;
    color: var(--muted);
  }
  .lbl {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .ncrow b {
    color: var(--text);
    font-weight: 600;
    margin-right: var(--space-1);
  }
  @media (max-width: 720px) {
    .scale {
      margin-left: 0;
    }
  }
</style>
