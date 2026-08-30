<script>
  // Optimizer results: a page of its own, because a grid is not a run: it is a table of runs, and
  // it stays useful while it is still filling.
  //
  // Everything on screen comes from the job id in the URL, so a reload (or coming back later)
  // rejoins the same sweep instead of losing it. While it runs, the header polls; the ranking is
  // read from whatever has finished, which is also exactly what "Stop" leaves behind.
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { onDestroy } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { t } from '$lib/i18n';
  import RequireModule from '$lib/modules/RequireModule.svelte';
  import { backtestApi, fmtNum } from '$lib/modules/backtest/api.js';
  import { fmtDuration, metricHigherBetter } from '$lib/modules/backtest/optimize.js';
  import OptRanking from '$lib/modules/backtest/OptRanking.svelte';
  import OptSensitivity from '$lib/modules/backtest/OptSensitivity.svelte';
  import OptVariant from '$lib/modules/backtest/OptVariant.svelte';

  const id = $derived($page.params.id);

  const PAGE_SIZE = 50;
  /** Poll pace while the grid runs. Fast enough to feel live, slow enough to cost nothing. */
  const POLL_MS = 1200;

  let job = $state(null);
  let progress = $state(null);
  let rows = $state([]);
  let rowCount = $state(0);
  let sensitivity = $state([]);
  let distribution = $state(null);
  let deflated = $state(null);
  let error = $state('');
  let loading = $state(true);

  let sort = $state('');
  let dir = $state('desc');
  let offset = $state(0);
  let tab = $state('ranking'); // 'ranking' | 'analysis' | 'variant'
  let picked = $state(null);
  let stopping = $state(false);

  const running = $derived(progress?.status === 'running');
  const axes = $derived(job?.axes ?? []);
  const pct = $derived(
    progress?.total ? Math.min(1, (progress.done ?? 0) / progress.total) : 0
  );

  /** One read of the job: progress, the visible ranking page, and the analysis when it is shown. */
  async function load({ analysis = tab === 'analysis' } = {}) {
    const res = await backtestApi.optimizeStatus(id, {
      sort: sort || undefined,
      dir,
      offset,
      limit: PAGE_SIZE,
      analysis
    });
    job = res.job;
    progress = res.progress;
    rows = res.rows ?? [];
    rowCount = res.row_count ?? 0;
    if (!sort) {
      sort = res.sort;
      dir = res.dir;
    }
    if (analysis) {
      sensitivity = res.sensitivity ?? [];
      distribution = res.distribution ?? null;
      deflated = res.deflated_sharpe ?? null;
    }
  }

  // The poll is a single self-rescheduling timer keyed on the job's own status: it stops itself
  // when the grid does, and one last read lands the finished numbers.
  let timer = 0;
  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(tick, POLL_MS);
  }
  async function tick() {
    try {
      await load();
      if (running) schedule();
    } catch (e) {
      error = e.message;
    }
  }
  onDestroy(() => clearTimeout(timer));

  let started = false;
  $effect(() => {
    if (!id || started) return;
    started = true;
    load()
      .then(() => {
        if (running) schedule();
      })
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  async function refresh(opts) {
    try {
      await load(opts);
    } catch (e) {
      error = e.message;
    }
  }

  function onsort(metric) {
    if (sort === metric) dir = dir === 'desc' ? 'asc' : 'desc';
    else {
      sort = metric;
      dir = metricHigherBetter(metric) ? 'desc' : 'asc';
    }
    offset = 0;
    refresh();
  }

  function onpage(next) {
    offset = Math.max(0, next);
    refresh();
  }

  function onpick(row) {
    picked = row;
    tab = 'variant';
  }

  function openTab(next) {
    tab = next;
    // The analysis scans every finished row, so it is fetched when it is looked at, not on a timer.
    if (next === 'analysis') refresh({ analysis: true });
  }

  async function stop() {
    stopping = true;
    try {
      const res = await backtestApi.optimizeCancel(id);
      progress = res.progress;
      clearTimeout(timer);
      await refresh();
    } catch (e) {
      error = e.message;
    } finally {
      stopping = false;
    }
  }

  /** Drop the job from the server's memory. A finished grid is a few megabytes of rows that only
   *  this page reads; letting it go is the user's call, not a timer's. */
  async function forget() {
    if (!confirm($t('backtest.opt.forgetConfirm'))) return;
    try {
      await backtestApi.optimizeForget(id);
      goto('/backtest');
    } catch (e) {
      error = e.message;
    }
  }

  const statusLabel = $derived(
    progress ? $t(`backtest.opt.status.${progress.status}`) : ''
  );
</script>

<RequireModule module="backtest">
<div class="page">
  <header class="top">
    <button class="back" onclick={() => goto('/backtest')}>
      <Icon name="chevron-left" size={13} /> {$t('backtest.opt.backToBuilder')}
    </button>
    <h1>{$t('backtest.opt.title')}</h1>
    {#if job}
      <span class="title">{job.ticker}</span>
      <span class="chip">{job.timeframe}</span>
      <span class="sub">{$t('backtest.page.barsCount', { count: fmtNum(job.bars, 0) })}</span>
      <span class="sub">{$t('backtest.opt.paramsCount', { n: axes.length })}</span>
      <span class="sub">{$t('backtest.opt.workersCount', { n: job.workers })}</span>
    {/if}
    <div class="acts">
      {#if running}
        <button class="stop" disabled={stopping} onclick={stop}>
          <Icon name="square" size={12} /> {$t('backtest.opt.stop')}
        </button>
      {:else if job}
        <button class="back" onclick={forget}>
          <Icon name="trash" size={12} /> {$t('backtest.opt.forget')}
        </button>
      {/if}
    </div>
  </header>

  {#if error}<p class="err" title={$t('backtest.page.clickToCopy')} use:copyLog={error}>{error}</p>{/if}

  {#if progress}
    <!-- The bar is the whole status line: the fill, the count, the clock. A stopped grid keeps
         its bar where it got to, since that is what the ranking below is made of. -->
    <div class="bar" class:done={!running}>
      <div class="fill" style:transform="scaleX({pct})" aria-hidden="true"></div>
      <div class="bar-txt">
        <span class="state" class:run={running}>{statusLabel}</span>
        <span>{fmtNum(progress.done, 0)} / {fmtNum(progress.total, 0)}</span>
        <span class="sep">·</span>
        <span>{$t('backtest.opt.elapsed')} {fmtDuration(progress.elapsed_ms, $t)}</span>
        {#if running && progress.eta_ms != null}
          <span class="sep">·</span>
          <span>{$t('backtest.opt.remaining')} {fmtDuration(progress.eta_ms, $t)}</span>
        {/if}
        {#if progress.per_trial_ms}
          <span class="sep">·</span>
          <span>{$t('backtest.opt.perVariant', { ms: fmtNum(progress.per_trial_ms, 1) })}</span>
        {/if}
        {#if progress.failed}
          <span class="sep">·</span>
          <span class="warn" title={progress.error ?? ''}>{$t('backtest.opt.failed', { n: fmtNum(progress.failed, 0) })}</span>
        {/if}
      </div>
    </div>
  {/if}

  <div class="tabs" role="tablist" aria-label={$t('backtest.opt.title')}>
    <button role="tab" aria-selected={tab === 'ranking'} onclick={() => openTab('ranking')}>
      {$t('backtest.opt.tabRanking')}{#if rowCount} <span class="count">{fmtNum(rowCount, 0)}</span>{/if}
    </button>
    <button role="tab" aria-selected={tab === 'analysis'} onclick={() => openTab('analysis')}>
      {$t('backtest.opt.tabAnalysis')}
    </button>
    <button role="tab" aria-selected={tab === 'variant'} disabled={!picked} onclick={() => openTab('variant')}>
      {$t('backtest.opt.tabVariant')}{#if picked} <span class="count">#{picked.rank}</span>{/if}
    </button>
  </div>

  <section class="body">
    {#if loading}
      <Skeleton height="320px" />
    {:else if tab === 'ranking'}
      <OptRanking
        {axes}
        {rows}
        {sort}
        {dir}
        total={rowCount}
        {offset}
        limit={PAGE_SIZE}
        picked={picked?.i ?? null}
        {onsort}
        {onpage}
        {onpick}
      />
    {:else if tab === 'analysis'}
      <OptSensitivity {sensitivity} {distribution} {deflated} metric={sort} {axes} />
    {:else if picked}
      <OptVariant
        row={picked}
        {axes}
        baseSettings={job?.settings ?? null}
        datasetIds={job?.dataset_ids ?? []}
        from={job?.from ?? null}
        to={job?.to ?? null}
      />
    {/if}
  </section>
</div>
</RequireModule>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-4);
    gap: var(--space-3);
    overflow: hidden;
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    flex-shrink: 0;
  }
  h1 {
    font-size: 1.25rem;
    font-weight: var(--fw-medium);
    letter-spacing: -0.01em;
  }
  .back,
  .stop {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
  }
  .back:hover,
  .stop:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-2));
  }
  .stop {
    border-color: var(--amber);
    color: var(--amber);
  }
  .acts {
    margin-left: auto;
    display: flex;
    gap: var(--space-2);
  }
  .title {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .chip {
    font-size: var(--text-xs);
    color: var(--muted);
    border: var(--hairline) solid var(--border);
    padding: 1px var(--space-2);
  }
  .sub {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  /* Progress: one band, the fill behind its own reading. */
  .bar {
    position: relative;
    overflow: hidden;
    border: var(--hairline) solid var(--border);
    background: var(--surface);
    flex-shrink: 0;
  }
  .bar .fill {
    position: absolute;
    inset: 0;
    transform-origin: left center;
    background: color-mix(in srgb, var(--accent) 26%, transparent);
    transition: transform 400ms linear;
  }
  .bar.done .fill {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .bar-txt {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    flex-wrap: wrap;
  }
  .state {
    font-weight: var(--fw-medium);
    color: var(--text);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 0.62rem;
  }
  .state.run {
    color: var(--accent);
  }
  .sep {
    opacity: 0.5;
  }
  .warn {
    color: var(--amber);
  }

  .tabs {
    display: flex;
    border-bottom: var(--hairline) solid var(--border);
    flex-shrink: 0;
  }
  .tabs button {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    padding: var(--space-2) var(--space-4);
    color: var(--muted);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    cursor: pointer;
  }
  .tabs button:hover:not(:disabled) {
    color: var(--text);
  }
  .tabs button[aria-selected='true'] {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .tabs button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .count {
    font-size: 0.62rem;
    color: var(--muted);
    border: var(--hairline) solid var(--border);
    padding: 0 4px;
    font-variant-numeric: tabular-nums;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
</style>
