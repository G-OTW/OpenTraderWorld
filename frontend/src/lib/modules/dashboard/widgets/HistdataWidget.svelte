<script>
  // Historical-data widgets — the cards of the Histdata mockup, each a `variant`. Two
  // reads: `datasets()` and `jobs()`, both already served by the module.
  //
  // Config: { variant, limit }.
  import { histdataApi } from '$lib/modules/histdata/api.js';
  import { fmtNum, fmtBytes, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'datasets');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 5)));

  const needsDatasets = $derived(['datasets', 'storage', 'gaps', 'stale'].includes(variant));
  const needsJobs = $derived(['queue', 'parked', 'failures'].includes(variant));

  let datasets = $state(null);
  let jobs = $state(null);
  let err = $state('');

  $effect(() => {
    if (editing || !needsDatasets) return;
    let alive = true;
    histdataApi.datasets()
      .then((d) => { if (alive) datasets = d; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });
  $effect(() => {
    if (editing || !needsJobs) return;
    let alive = true;
    histdataApi.jobs()
      .then((j) => { if (alive) jobs = j; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const totalBars = $derived((datasets ?? []).reduce((s, d) => s + (d.bar_count ?? 0), 0));
  const totalBytes = $derived((datasets ?? []).reduce((s, d) => s + (d.size_bytes ?? 0), 0));

  const gapCount = (d) => (Array.isArray(d.gaps) ? d.gaps.length : 0);
  const withGaps = $derived(
    (datasets ?? []).filter((d) => gapCount(d) > 0).sort((a, b) => gapCount(b) - gapCount(a))
  );
  const stalest = $derived(
    [...(datasets ?? [])].sort((a, b) => a.last_updated.localeCompare(b.last_updated)).slice(0, limit)
  );

  const queue = $derived.by(() => {
    const c = { queued: 0, running: 0, failed: 0, waiting: 0 };
    for (const j of jobs ?? []) if (j.status in c) c[j.status]++;
    return c;
  });
  const queueTotal = $derived((jobs ?? []).length);
  const parked = $derived((jobs ?? []).filter((j) => j.status === 'waiting' && j.resume_at));
  const failed = $derived(
    (jobs ?? []).filter((j) => j.status === 'failed')
      .sort((a, b) => b.created_at.localeCompare(a.created_at))
      .slice(0, limit)
  );

  // "Resumes in 12m" — a countdown reads better than a wall-clock stamp for a pause.
  function resumesIn(iso) {
    const ms = new Date(iso).getTime() - Date.now();
    if (ms <= 0) return $t('dashboard.widgets.histdata.resumingNow');
    const mins = Math.round(ms / 60000);
    if (mins < 60) return $t('dashboard.widgets.histdata.resumesInMin', { min: mins });
    return $t('dashboard.widgets.histdata.resumesInHm', { h: Math.floor(mins / 60), m: mins % 60 });
  }
</script>

<WidgetState
  {editing}
  error={err}
  loading={(needsDatasets && datasets === null) || (needsJobs && jobs === null)}
  preview={$t('dashboard.widgets.histdata.preview')}
  rows={4}
>
  {#if variant === 'datasets'}
    <Stat
      value={String((datasets ?? []).length)}
      note={$t('dashboard.widgets.histdata.barsStored', { bars: fmtNum(totalBars, 0) })}
    />
  {:else if variant === 'storage'}
    <Stat value={fmtBytes(totalBytes)} note={$t('dashboard.widgets.histdata.onDisk')} />
  {:else if variant === 'queue'}
    <div class="w-body">
      <ul class="w-list">
        <li class="w-row">
          <span class="w-dot"></span>
          <span class="w-name grow">{$t('dashboard.widgets.histdata.queued')}</span>
          <span class="w-num">{queue.queued}</span>
        </li>
        <li class="w-row">
          <span class="w-dot run"></span>
          <span class="w-name grow">{$t('dashboard.widgets.histdata.running')}</span>
          <span class="w-num">{queue.running}</span>
        </li>
        <li class="w-row">
          <span class="w-dot bad"></span>
          <span class="w-name grow">{$t('dashboard.widgets.histdata.failed')}</span>
          <span class="w-num">{queue.failed}</span>
        </li>
      </ul>
      <!-- One track split by status, so the mix is legible without three bars. -->
      <div class="mix" role="img" aria-label={$t('dashboard.widgets.histdata.jobsTotal', { count: queueTotal })}>
        <span class="seg run" style:flex={queue.running || 0}></span>
        <span class="seg queued" style:flex={queue.queued || 0}></span>
        <span class="seg bad" style:flex={queue.failed || 0}></span>
      </div>
      <span class="w-sub">{$t('dashboard.widgets.histdata.jobsTotal', { count: queueTotal })}</span>
    </div>
  {:else if variant === 'parked'}
    {#if parked.length === 0}
      <p class="w-state">{$t('dashboard.widgets.histdata.noneParked')}</p>
    {:else}
      <div class="w-body">
        <ul class="w-list">
          {#each parked.slice(0, limit) as j (j.id)}
            <li class="w-row">
              <span class="w-dot warn"></span>
              <span class="w-name grow">{j.ticker} {j.timeframe}</span>
              <span class="w-sub when">{resumesIn(j.resume_at)}</span>
            </li>
          {/each}
        </ul>
        <span class="w-sub">{$t('dashboard.widgets.histdata.jobsPaused', { count: parked.length })}</span>
      </div>
    {/if}
  {:else if variant === 'gaps'}
    {#if withGaps.length === 0}
      <p class="w-state">{$t('dashboard.widgets.histdata.noGaps')}</p>
    {:else}
      <div class="w-body">
        <ul class="w-list">
          {#each withGaps.slice(0, limit) as d (d.id)}
            <li class="w-row">
              <span class="w-name tick">{d.ticker}</span>
              <span class="w-sub tf grow">{d.timeframe}</span>
              <span class="w-num w-warn">{$t('dashboard.widgets.histdata.gaps', { count: gapCount(d) })}</span>
            </li>
          {/each}
        </ul>
        <span class="w-sub">{$t('dashboard.widgets.histdata.needFill', { count: withGaps.length })}</span>
      </div>
    {/if}
  {:else if variant === 'stale'}
    <ul class="w-list">
      {#each stalest as d (d.id)}
        <li class="w-row">
          <span class="w-name tick">{d.ticker}</span>
          <span class="w-sub tf grow">{d.timeframe}</span>
          <span class="w-sub when">{$ago(d.last_updated)}</span>
        </li>
      {/each}
    </ul>
  {:else if variant === 'failures'}
    {#if failed.length === 0}
      <p class="w-state">{$t('dashboard.widgets.histdata.noFailures')}</p>
    {:else}
      <ul class="w-list">
        {#each failed as j (j.id)}
          <li class="w-row stack">
            <span class="line">
              <span class="badmark"><Icon name="alert-triangle" size={13} /></span>
              <span class="w-name grow">{j.provider} {j.ticker} {j.timeframe}</span>
              <span class="w-sub when">{$ago(j.created_at)}</span>
            </span>
            {#if j.error}<span class="w-name w-sub reason">{j.error}</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
    min-width: 0;
  }
  .when {
    flex-shrink: 0;
  }
  .tick {
    font-family: var(--mono);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }
  .tf {
    flex-shrink: 0;
  }
  .w-dot.run {
    background: var(--accent);
  }
  .w-dot.bad {
    background: var(--red);
  }
  .w-dot.warn {
    background: var(--amber);
  }
  .mix {
    display: flex;
    gap: 2px;
    height: 6px;
    border-radius: 999px;
    overflow: hidden;
    background: var(--surface-3);
  }
  .seg {
    min-width: 0;
  }
  .seg.run {
    background: var(--accent);
  }
  .seg.queued {
    background: var(--chart-4);
  }
  .seg.bad {
    background: var(--red);
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .badmark {
    display: inline-flex;
    color: var(--red);
    flex-shrink: 0;
  }
  .reason {
    color: var(--dim);
    font-size: var(--text-xs);
  }
</style>
