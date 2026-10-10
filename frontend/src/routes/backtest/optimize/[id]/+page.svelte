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
  import OptCloud from '$lib/modules/backtest/OptCloud.svelte';

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
  let tab = $state('ranking'); // 'ranking' | 'analysis' | 'cloud' | 'variant'
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
    <button class="ghost back" onclick={() => goto('/backtest')}>
      <Icon name="chevron-left" size={14} /> {$t('backtest.opt.backToBuilder')}
    </button>
    <div class="heading">
      <div class="identity">
        <h1>{$t('backtest.opt.title')}</h1>
        {#if job}
          <span class="instrument">{job.ticker}</span>
          <span class="timeframe">{job.timeframe}</span>
        {/if}
      </div>
      <div class="acts">
        {#if running}
          <button class="btn stop" disabled={stopping} onclick={stop}>
            <Icon name="stop" size={13} /> {$t('backtest.opt.stop')}
          </button>
        {:else if job}
          <button class="ghost" onclick={forget}>
            <Icon name="trash" size={13} /> {$t('backtest.opt.forget')}
          </button>
        {/if}
      </div>
    </div>
    {#if job}
      <div class="metadata">
        <span>{$t('backtest.page.barsCount', { count: fmtNum(job.bars, 0) })}</span>
        <span>{$t('backtest.opt.paramsCount', { n: axes.length })}</span>
        <span>{$t('backtest.opt.workersCount', { n: job.workers })}</span>
      </div>
    {/if}
  </header>

  {#if error}<p class="err" role="alert" title={$t('backtest.page.clickToCopy')} use:copyLog={error}>{error}</p>{/if}

  {#if progress}
    <div class="progress-panel">
      <div class="progress-main">
        <div class="progress-heading">
          <span class="state" class:run={running} class:failed={progress.status === 'failed'}>
            <span class="status-dot" aria-hidden="true"></span>{statusLabel}
          </span>
          <span class="percentage">{fmtNum(pct * 100, 0)}%</span>
        </div>
        <div class="completion">
          <strong>{fmtNum(progress.done, 0)}</strong>
          <span>/ {fmtNum(progress.total, 0)} {$t('backtest.opt.variants')}</span>
        </div>
        <div class="progress-track" role="progressbar" aria-label={statusLabel}
          aria-valuemin={0} aria-valuemax={progress.total || 1} aria-valuenow={progress.done}>
          <div class="fill" style:transform="scaleX({pct})"></div>
        </div>
      </div>
      <div class="timings">
        <div class="timing"><span>{$t('backtest.opt.elapsed')}</span><strong>{fmtDuration(progress.elapsed_ms, $t)}</strong></div>
        {#if running && progress.eta_ms != null}
          <div class="timing"><span>{$t('backtest.opt.remaining')}</span><strong>{fmtDuration(progress.eta_ms, $t)}</strong></div>
        {/if}
        {#if progress.per_trial_ms}
          <span class="per-variant">{$t('backtest.opt.perVariant', { ms: fmtNum(progress.per_trial_ms, 1) })}</span>
        {/if}
      </div>
      {#if progress.failed}
        <p class="warn" title={progress.error ?? ''}><Icon name="alert-triangle" size={13} />{$t('backtest.opt.failed', { n: fmtNum(progress.failed, 0) })}</p>
      {/if}
    </div>
  {/if}

  <div class="tabs" role="tablist" aria-label={$t('backtest.opt.title')}>
    <button role="tab" aria-selected={tab === 'ranking'} onclick={() => openTab('ranking')}>
      {$t('backtest.opt.tabRanking')}{#if rowCount} <span class="count">{fmtNum(rowCount, 0)}</span>{/if}
    </button>
    <button role="tab" aria-selected={tab === 'analysis'} onclick={() => openTab('analysis')}>
      {$t('backtest.opt.tabAnalysis')}
    </button>
    <button role="tab" aria-selected={tab === 'cloud'} onclick={() => openTab('cloud')}>
      {$t('backtest.opt.tabCloud')}
    </button>
    <button role="tab" aria-selected={tab === 'variant'} disabled={!picked} onclick={() => openTab('variant')}>
      {$t('backtest.opt.tabVariant')}{#if picked?.rank} <span class="count">#{picked.rank}</span>{/if}
    </button>
  </div>

  <section class="body" class:ranking={tab === 'ranking'}>
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
    {:else if tab === 'cloud'}
      <OptCloud jobId={id} {axes} {sort} {dir} done={progress?.done ?? 0} {onpick} />
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
    max-width: 100vw;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow: hidden;
  }
  .top {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  .back {
    align-self: flex-start;
    height: auto;
    padding: 0;
    margin-bottom: var(--space-1);
  }
  .heading, .identity {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .identity {
    flex-wrap: wrap;
  }
  h1 {
    margin: 0;
    font-size: var(--fs-page-title);
    font-weight: var(--fw-medium);
    letter-spacing: -0.02em;
  }
  .instrument {
    padding-left: var(--space-3);
    border-left: var(--hairline) solid var(--border-control);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    overflow-wrap: anywhere;
  }
  .timeframe {
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
  }
  .acts {
    margin-left: auto;
    flex-shrink: 0;
  }
  .stop {
    color: var(--amber-ink);
    border-color: color-mix(in srgb, var(--amber) 45%, var(--border-control));
  }
  .metadata {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .metadata > span + span {
    padding-left: var(--space-3);
    border-left: var(--hairline) solid var(--border-control);
  }
  .err {
    margin: 0;
    padding: var(--space-3);
    border-radius: var(--radius-sm);
    background: var(--negative-soft);
    color: var(--red-ink);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .progress-panel {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-4) var(--space-6);
    padding: var(--space-4);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    flex-shrink: 0;
  }
  .progress-main {
    flex: 1 1 240px;
    min-width: 0;
  }
  .progress-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    font-weight: var(--fw-medium);
  }
  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }
  .state.run {
    color: var(--green-ink);
  }
  .state.failed {
    color: var(--red-ink);
  }
  .percentage {
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .completion {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    margin: var(--space-2) 0 var(--space-3);
    font-variant-numeric: tabular-nums;
  }
  .completion strong {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    font-family: var(--mono);
  }
  .completion > span {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .progress-track {
    height: 4px;
    overflow: hidden;
    background: var(--surface-3);
    border-radius: var(--radius-sm);
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transform-origin: left center;
    transition: transform 400ms linear;
  }
  .timings {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    border-left: var(--hairline) solid var(--border-control);
    padding-left: var(--space-6);
    flex: 0 1 300px;
  }
  .timing {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: 1;
  }
  .timing > span, .per-variant {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .timing strong {
    font-family: var(--mono);
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }
  .per-variant {
    flex-basis: 100%;
    font-variant-numeric: tabular-nums;
  }
  .warn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-basis: 100%;
    margin: 0;
    color: var(--amber-ink);
    font-size: var(--text-xs);
  }
  .tabs {
    display: flex;
    gap: var(--space-4);
    border-bottom: var(--hairline) solid var(--border-control);
    flex-shrink: 0;
    overflow-x: auto;
  }
  .tabs button {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    padding: var(--space-3) var(--space-1);
    color: var(--muted);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    cursor: pointer;
    white-space: nowrap;
  }
  .tabs button:hover:not(:disabled) {
    color: var(--text);
  }
  .tabs button[aria-selected='true'] {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .tabs button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .tabs button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -3px;
  }
  .count {
    flex-shrink: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border-radius: var(--radius-sm);
    padding: 1px var(--space-2);
    font-variant-numeric: tabular-nums;
  }
  .body {
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow: auto;
  }
  .body.ranking {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  @media (prefers-reduced-motion: reduce) {
    .fill {
      transition: none;
    }
  }
  @media (max-width: 760px) {
    .page {
      padding: var(--space-4);
      gap: var(--space-3);
      overflow: auto;
    }
    .progress-panel {
      gap: var(--space-3);
    }
    .timings {
      flex-basis: 100%;
      border-left: 0;
      border-top: var(--hairline) solid var(--border);
      padding: var(--space-3) 0 0;
      align-items: center;
    }
    .per-variant {
      flex-basis: auto;
    }
    .body {
      flex: 1 0 320px;
    }
    .tabs {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 0 var(--space-2);
      overflow: visible;
    }
    .tabs button {
      min-width: 0;
      min-height: 44px;
      white-space: normal;
      overflow-wrap: anywhere;
    }
  }
</style>
