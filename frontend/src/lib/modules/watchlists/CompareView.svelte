<script>
  // Compare: every covered symbol of the list rebased to 100 over 1M, 3M or 1Y, on one
  // axis (never two scales). 1M is the cached quote history; 3M and 1Y need stored daily
  // candles, and a symbol without them is listed as not covered with the fix instead of
  // drawn from a partial series. Colour follows the symbol: its slot is its position in
  // the list (`order`), so switching windows or losing coverage never repaints the others.
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ChartFrame from '$lib/ui/charts/ChartFrame.svelte';
  import TimeSeries from '$lib/ui/charts/TimeSeries.svelte';
  import { identityColor } from '$lib/ui/charts/palette.js';
  import { chartColors } from '$lib/theme/chart.svelte.js';
  import { fmtNum } from '$lib/format.js';
  import { watchlistsApi, fmtSignedPct } from './api.js';
  import { normalizeSeries } from './views.js';
  import { t } from '$lib/i18n';

  // order: item ids in the list's own order (the identity slot of each symbol).
  // onrefresh: re-quote the list (the fix the unavailable state offers for 1M).
  // stamp: the list's last refresh; a new one reloads the series.
  let { id, order = [], onrefresh = null, stamp = null } = $props();

  const WINDOWS = ['1m', '3m', '1y'];
  const SLOTS = 4;
  let win = $state('1m');
  let data = $state(null);
  let loading = $state(false);
  let error = $state('');
  let emphasized = $state(null);

  $effect(() => {
    const w = win;
    const listId = id;
    stamp;
    loading = true;
    error = '';
    watchlistsApi
      .series(listId, w)
      .then((r) => listId === id && (data = r))
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  const c = $derived(chartColors());
  const lines = $derived(
    (data?.series ?? [])
      .filter((s) => (s.points?.length ?? 0) > 0)
      .map((s) => {
        const slot = order.indexOf(s.item_id);
        const last = normalizeSeries(s.points).at(-1)?.[1];
        return {
          key: s.item_id,
          label: s.symbol,
          points: s.points,
          color: identityColor(slot, c, SLOTS),
          // A symbol past the named slots folds into the Other slate: thinner, so the
          // named ones lead.
          width: slot >= 0 && slot < SLOTS ? 2 : 1.5,
          change: last == null ? null : last - 100
        };
      })
  );
  // Always listed, even for one line: the title does not name the symbol, the legend does.
  const legend = $derived(
    lines.map((l) => ({ key: l.key, label: l.label, color: l.color, kind: 'line', value: fmtSignedPct(l.change) }))
  );
  const unavailable = $derived(
    !loading && data && lines.length === 0
      ? {
          reason: $t('watchlists.compare.none'),
          icon: 'git-compare',
          // 1M reads the cached quotes (a refresh fills them); 3M and 1Y need stored candles.
          action:
            win === '1m'
              ? onrefresh
                ? { label: $t('watchlists.detail.refresh'), icon: 'refresh-cw', onclick: onrefresh }
                : undefined
              : { label: $t('histdata.page.title'), href: '/histdata' }
        }
      : null
  );
</script>

<div class="cmp">
  <div class="bar">
    <div class="seg" role="group" aria-label={$t('watchlists.view.window')}>
      {#each WINDOWS as w (w)}
        <button class="chip" class:active={win === w} aria-pressed={win === w} onclick={() => (win = w)}>
          {$t(`watchlists.view.win.${w}`)}
        </button>
      {/each}
    </div>
    <span class="note">{$t('watchlists.compare.rebased')}</span>
  </div>
  <ErrorText {error} copyable />
  <div class:stale={loading && !!data}>
    <ChartFrame
      height={320}
      loading={loading && !data}
      {unavailable}
      legend={legend}
      bind:emphasized
      toggle
      sensitive={false}
    >
      <TimeSeries
        series={lines.map((l) => ({ key: l.key, label: l.label, points: l.points, color: l.color, width: l.width }))}
        indexed
        height={320}
        legend="none"
        format={(v) => fmtNum(v, 1)}
        axisFormat={(v) => fmtNum(v, 0)}
        bind:emphasized
        sensitive={false}
        ariaLabel={$t('watchlists.compare.rebased')}
      />
    </ChartFrame>
  </div>
  {#if data?.not_covered?.length}
    <div class="nc">
      <span class="lbl">{$t('watchlists.view.notCovered').replace(/:\s*$/, '')}</span>
      {#each data.not_covered as n (n.item_id)}
        <span class="ncrow"><b>{n.symbol}</b> {n.reason}</span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .cmp {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .seg {
    display: inline-flex;
    gap: var(--space-1);
  }
  .note {
    font-size: 12px;
    color: var(--dim);
  }
  .stale {
    opacity: 0.5;
    transition: opacity var(--dur-base) var(--ease);
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
</style>
