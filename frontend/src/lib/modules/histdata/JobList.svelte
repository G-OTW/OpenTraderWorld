<script>
  // Recent download jobs with live progress. The page polls and passes jobs in.
  // A filter bar narrows by status / provider / ticker / timeframe.
  // Finished jobs link to the Visualization module to chart the dataset they filled.
  // A job that hit the provider's limits is `waiting`: it shows why and when it resumes,
  // and every unfinished job can be cancelled (a batch, in one click).
  import { goto } from '$app/navigation';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { copyLog } from '$lib/ui/copyLog.js';
  import { infiniteScroll } from '$lib/ui/infiniteScroll.js';
  import { t } from '$lib/i18n';
  import { fmtNum } from '$lib/format.js';
  import { histdataApi } from './api.js';

  let { jobs = [], onchanged } = $props();

  // Filters persist across refresh (per-browser).
  const FILTERS_KEY = 'otw.histdata.jobFilters.v1';
  function loadFilters() {
    try {
      return JSON.parse(localStorage.getItem(FILTERS_KEY) || '{}');
    } catch {
      return {};
    }
  }
  const saved = loadFilters();

  let fStatus = $state(saved.status ?? '');
  let fProvider = $state(saved.provider ?? '');
  let fTimeframe = $state(saved.timeframe ?? '');
  let fTicker = $state(saved.ticker ?? '');

  let loaded = $state(false);
  $effect(() => {
    loaded = true;
  });
  $effect(() => {
    const snap = { status: fStatus, provider: fProvider, timeframe: fTimeframe, ticker: fTicker };
    if (!loaded) return;
    try {
      localStorage.setItem(FILTERS_KEY, JSON.stringify(snap));
    } catch {
      /* non-fatal */
    }
  });

  // Distinct values for the dropdowns, derived from the current job list.
  const uniq = (key) => [...new Set(jobs.map((j) => j[key]).filter(Boolean))].sort();
  const providers = $derived(uniq('provider'));
  const timeframes = $derived(uniq('timeframe'));
  const statuses = $derived(uniq('status'));

  // Filter dropdowns: the "" entry is the no-filter row, so it is an option like any other.
  const opts = (all, values) => [{ value: '', label: all }, ...values.map((v) => ({ value: v, label: v }))];
  // Statuses read as words, not as the wire value the badge used to print.
  const statusOpts = $derived([
    { value: '', label: $t('histdata.jobs.allStatus') },
    ...statuses.map((v) => ({ value: v, label: $t(`histdata.jobs.status.${v}`) }))
  ]);
  const providerOpts = $derived(opts($t('histdata.jobs.allProviders'), providers));
  const timeframeOpts = $derived(opts($t('histdata.jobs.allTimeframes'), timeframes));

  const filtered = $derived(
    jobs.filter(
      (j) =>
        (!fStatus || j.status === fStatus) &&
        (!fProvider || j.provider === fProvider) &&
        (!fTimeframe || j.timeframe === fTimeframe) &&
        (!fTicker || j.ticker.toLowerCase().includes(fTicker.toLowerCase()))
    )
  );

  // Infinite scroll: render a page at a time, growing as the sentinel scrolls into view.
  // Reset back to one page whenever the filtered set changes (new filter or job list),
  // so filtering never leaves us scrolled deep into a now-shorter list.
  const PAGE = 20;
  let visibleCount = $state(PAGE);
  $effect(() => {
    // Touch the filter inputs so this re-runs when they change.
    void (fStatus + fProvider + fTimeframe + fTicker);
    visibleCount = PAGE;
  });
  const visible = $derived(filtered.slice(0, visibleCount));
  const hasMore = $derived(visibleCount < filtered.length);

  function clearFilters() {
    fStatus = fProvider = fTimeframe = fTicker = '';
  }
  const anyFilter = $derived(!!(fStatus || fProvider || fTimeframe || fTicker));

  // Show the chart shortcut once a job has produced data we can plot.
  const charted = new Set(['done', 'partial']);
  function viewChart(j) {
    goto(`/histviz?dataset=${j.dataset_id}`);
  }

  function pct(j) {
    if (!j.chunks_total) return j.status === 'done' ? 100 : 0;
    return Math.min(100, Math.round((j.chunks_done / j.chunks_total) * 100));
  }
  const statusColor = {
    queued: 'var(--muted)',
    running: 'var(--accent)',
    waiting: 'var(--amber)',
    cancelling: 'var(--muted)',
    cancelled: 'var(--muted)',
    done: 'var(--green)',
    partial: 'var(--amber)',
    error: 'var(--red)'
  };

  // ── Waiting jobs: the countdown to resume_at ──
  const LIVE = new Set(['queued', 'running', 'waiting', 'cancelling']);
  const isLive = (j) => LIVE.has(j.status);
  const anyWaiting = $derived(jobs.some((j) => j.status === 'waiting' && j.resume_at));

  // One clock for the whole list, ticking only while something is counting down.
  let now = $state(Date.now());
  $effect(() => {
    if (!anyWaiting) return;
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });

  /** "4 min 12 s" until the job resumes, or '' once the instant has passed. */
  function countdown(j) {
    if (!j.resume_at) return '';
    const left = Math.round((new Date(j.resume_at).getTime() - now) / 1000);
    if (left <= 0) return '';
    if (left < 60) return `${left} s`;
    if (left < 3600) return `${Math.floor(left / 60)} min ${left % 60} s`;
    return `${Math.floor(left / 3600)} h ${Math.floor((left % 3600) / 60)} min`;
  }
  function waitText(j) {
    const left = countdown(j);
    const why =
      j.wait_reason === 'quota'
        ? $t('histdata.jobs.waitQuota')
        : $t('histdata.jobs.waitRateLimit');
    return left ? $t('histdata.jobs.waitResumes', { why, left }) : $t('histdata.jobs.waitSoon', { why });
  }

  // ── Batches ──
  // Jobs arrive newest-first; the header rides on the first row of each batch and counts
  // over the whole batch, not just what is currently visible.
  const batchOf = (id) => jobs.filter((j) => j.batch_id === id);
  function batchStat(id) {
    const all = batchOf(id);
    return {
      total: all.length,
      done: all.filter((j) => j.status === 'done').length,
      live: all.filter(isLive).length,
      waiting: all.filter((j) => j.status === 'waiting').length
    };
  }
  // First visible row of each batch, so the header is rendered exactly once.
  const batchHead = $derived.by(() => {
    const seen = new Set();
    const heads = new Set();
    for (const j of visible) {
      if (!j.batch_id || seen.has(j.batch_id)) continue;
      seen.add(j.batch_id);
      heads.add(j.id);
    }
    return heads;
  });

  let busyId = $state('');
  async function cancel(j) {
    busyId = j.id;
    try {
      await histdataApi.cancelJob(j.id);
      onchanged?.();
    } catch {
      /* the row refreshes on the next poll */
    } finally {
      busyId = '';
    }
  }
  async function cancelBatch(id) {
    busyId = id;
    try {
      await histdataApi.cancelBatch(id);
      onchanged?.();
    } catch {
      /* idem */
    } finally {
      busyId = '';
    }
  }
</script>

<div class="filters">
  <Dropdown
    bind:value={fStatus}
    options={statusOpts}
    title={$t('histdata.jobs.filterStatus')}
    ariaLabel={$t('histdata.jobs.filterStatus')}
  />
  <Dropdown
    bind:value={fProvider}
    options={providerOpts}
    title={$t('histdata.jobs.filterProvider')}
    ariaLabel={$t('histdata.jobs.filterProvider')}
  />
  <Dropdown
    bind:value={fTimeframe}
    options={timeframeOpts}
    title={$t('histdata.jobs.filterTimeframe')}
    ariaLabel={$t('histdata.jobs.filterTimeframe')}
  />
  <input class="tk" placeholder={$t('histdata.jobs.tickerPlaceholder')} bind:value={fTicker} />
  {#if anyFilter}
    <Button onclick={clearFilters}>{$t('common.clear')}</Button>
    <span class="count">{filtered.length}/{jobs.length}</span>
  {/if}
</div>

<div class="jobs">
  {#if !jobs.length}
    <p class="empty">{$t('histdata.jobs.emptyNone')}</p>
  {:else if !filtered.length}
    <p class="empty">{$t('histdata.jobs.emptyFiltered')}</p>
  {/if}
  {#each visible as j (j.id)}
    {#if batchHead.has(j.id)}
      {@const st = batchStat(j.batch_id)}
      <div class="batch">
        <Icon name="layers" size={13} />
        <span class="btxt">
          {$t('histdata.jobs.batchSummary', { done: st.done, total: st.total })}
          {#if st.waiting}
            <span class="bwait">{$t('histdata.jobs.batchWaiting', { n: st.waiting })}</span>
          {/if}
        </span>
        {#if st.live}
          <Button
            variant="ghost"
            size="sm"
            disabled={busyId === j.batch_id}
            onclick={() => cancelBatch(j.batch_id)}
          >
            {$t('histdata.jobs.cancelBatch', { n: st.live })}
          </Button>
        {/if}
      </div>
    {/if}
    <div class="job" style="--st:{statusColor[j.status] ?? 'var(--text)'}">
      <div class="head">
        <span class="sym">{j.ticker}</span>
        <span class="badge">{$t(`histdata.jobs.status.${j.status}`)}</span>
        {#if charted.has(j.status) && j.dataset_id && j.bars_written > 0}
          <Button
            variant="ghost"
            size="sm"
            icon="bar-chart"
            onclick={() => viewChart(j)}
            title={$t('histdata.jobs.viewChart')}
          >
            {$t('histdata.jobs.viewChart')}
          </Button>
        {/if}
        {#if isLive(j)}
          <Button
            variant="ghost"
            size="sm"
            icon="x"
            disabled={busyId === j.id || j.status === 'cancelling'}
            onclick={() => cancel(j)}
            title={$t('histdata.jobs.cancel')}
          >
            {j.status === 'cancelling' ? $t('histdata.jobs.cancelling') : $t('histdata.jobs.cancel')}
          </Button>
        {/if}
      </div>
      <div class="tags">
        <span class="tag">{j.provider}</span>
        <span class="tag">{j.asset_type}</span>
        <span class="tag">{j.timeframe}</span>
        <span class="tag">{j.kind}</span>
      </div>
      <div class="bar">
        <div class="fill" style="width:{pct(j)}%"></div>
      </div>
      <div class="sub">
        <span class="stat">{$t('histdata.jobs.barsChunks', { bars: fmtNum(j.bars_written, 0), done: j.chunks_done, total: j.chunks_total || '?' })}</span>
        <span class="pctval">{pct(j)}%</span>
      </div>
      {#if j.status === 'waiting'}
        <div class="wait">
          <Icon name="clock" size={12} />
          <span>{waitText(j)}</span>
        </div>
      {/if}
      {#if j.error}
        <div class="err" title="{j.error} — {$t('histdata.jobs.clickToCopy')}" use:copyLog={j.error}>
          <span class="err-ic">⚠</span>{j.error}
        </div>
      {/if}
    </div>
  {/each}
  {#if hasMore}
    <div
      class="sentinel"
      use:infiniteScroll={{ onLoadMore: () => (visibleCount += PAGE), disabled: !hasMore }}
    >
      {$t('common.loadingMore')}
    </div>
  {/if}
</div>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .filters input.tk {
    text-transform: none;
    width: 120px;
  }
  /* Fixed width so the row doesn't jump between "All providers" and "yahoo". */
  .filters > :global(.dd) {
    width: 150px;
  }
  .count {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .jobs {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .empty {
    color: var(--muted);
    font-size: var(--text-base);
  }
  .sentinel {
    text-align: center;
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-2);
  }
  .job {
    position: relative;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
    padding-left: calc(var(--space-3) + 3px);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    transition: border-color var(--dur-fast) var(--ease);
  }
  /* Status accent rail down the left edge. */
  .job::before {
    content: '';
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 3px;
    background: var(--st);
    border-radius: var(--radius-lg) 0 0 var(--radius-lg);
  }
  .job:hover {
    border-color: color-mix(in srgb, var(--st) 45%, var(--border));
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .sym {
    font-weight: var(--fw-medium);
    font-size: var(--text-md);
    letter-spacing: 0.01em;
  }
  /* Status pill, tinted by the job status color. */
  .badge {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 2px var(--space-2);
    border-radius: 0;
    color: var(--st);
    background: color-mix(in srgb, var(--st) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--st) 35%, transparent);
    line-height: 1.4;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .tag {
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1px var(--space-2);
  }
  .bar {
    height: 5px;
    background: var(--surface-2);
    border-radius: 0;
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--st);
    border-radius: 0;
    transition: width 0.3s ease;
  }
  .sub {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .sub .stat {
    font-variant-numeric: tabular-nums;
  }
  .pctval {
    font-weight: var(--fw-medium);
    color: var(--st);
    font-variant-numeric: tabular-nums;
  }
  /* Batch header: sits above the first job of a submission, counts the whole batch. */
  .batch {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--border);
  }
  .btxt {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .bwait {
    color: var(--amber);
  }
  .batch > :global(.btn) {
    margin-left: auto;
  }
  /* Why a job is paused, and when it comes back on its own. */
  .wait {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--amber);
    font-size: var(--text-xs);
    background: color-mix(in srgb, var(--amber) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--amber) 25%, transparent);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
  }
  .err {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--red);
    font-size: var(--text-xs);
    background: color-mix(in srgb, var(--red) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--red) 25%, transparent);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .err-ic {
    flex: none;
  }
  /* Chart shortcut: a ghost button pushed to the end of the head row. It stays quiet
     until hovered — it's a shortcut off the card, not the card's subject. */
  .head > :global(.btn) {
    margin-left: auto;
  }
</style>
