<script>
  // Goals statistics: four donuts over the same goal list — status, deadline coverage,
  // urgency of what's still running, and metric completion across every goal.
  //
  // Every donut is also a filter: clicking a slice narrows the table underneath, so a
  // number on the chart is one click away from the goals behind it. The page reads
  // from the same list endpoint as /goals and derives everything client-side; there is
  // no stats endpoint to keep in sync.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import StatDonut from '$lib/modules/goals/StatDonut.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { goalsApi, progress, status, urgency, fmtDate, daysLeft } from '$lib/modules/goals/api.js';
  import { t } from '$lib/i18n';

  let goals = $state([]);
  let loading = $state(true);

  onMount(async () => {
    goals = await goalsApi.list();
    loading = false;
  });

  let catFilter = $state('all');

  const categories = $derived(
    [...new Set(goals.map((g) => (g.category || '').trim()).filter(Boolean))].sort()
  );

  // The category filter is the page-wide scope: every donut, stat and table row below
  // is computed over this list.
  const scoped = $derived(
    goals.filter((g) => catFilter === 'all' || (g.category || '').trim() === catFilter)
  );

  const count = (fn) => scoped.filter(fn).length;

  // ── Donut 1: status ──
  const statusSlices = $derived([
    { key: 'open', label: $t('goals.filter.open'), value: count((g) => status(g) === 'open'), color: 'var(--chart-4)' },
    { key: 'reached', label: $t('goals.filter.reached'), value: count((g) => status(g) === 'reached'), color: 'var(--green)' },
    { key: 'overdue', label: $t('goals.filter.overdue'), value: count((g) => status(g) === 'overdue'), color: 'var(--red)' }
  ]);

  // ── Donut 2: deadline coverage ──
  const deadlineSlices = $derived([
    { key: 'with', label: $t('goals.stats.withDeadline'), value: count((g) => !!g.deadline), color: 'var(--chart-1)' },
    { key: 'without', label: $t('goals.stats.withoutDeadline'), value: count((g) => !g.deadline), color: 'var(--chart-6)' }
  ]);

  // ── Donut 3: urgency, running goals with a deadline only ──
  // Ordered nearest-first and coloured on a heat ramp, so the eye lands on what is due.
  const urgencySlices = $derived([
    { key: 'd1', label: $t('goals.stats.due1d'), value: count((g) => urgency(g) === 'd1'), color: 'var(--red)' },
    { key: 'd3', label: $t('goals.stats.due3d'), value: count((g) => urgency(g) === 'd3'), color: 'var(--chart-3)' },
    { key: 'w1', label: $t('goals.stats.dueWeek'), value: count((g) => urgency(g) === 'w1'), color: 'var(--amber)' },
    { key: 'm1', label: $t('goals.stats.dueMonth'), value: count((g) => urgency(g) === 'm1'), color: 'var(--chart-1)' },
    { key: 'later', label: $t('goals.stats.dueLater'), value: count((g) => urgency(g) === 'later'), color: 'var(--chart-2)' }
  ]);

  // ── Donut 4: individual metrics across every goal in scope ──
  const metrics = $derived(scoped.flatMap((g) => g.kpis ?? []));
  const metricSlices = $derived([
    { key: 'done', label: $t('goals.stats.metricsReached'), value: metrics.filter((k) => k.reached).length, color: 'var(--green)' },
    { key: 'todo', label: $t('goals.stats.metricsPending'), value: metrics.filter((k) => !k.reached).length, color: 'var(--chart-4)' }
  ]);

  // ── Headline figures ──
  // Average completion is the mean of each goal's own progress, not total points across
  // goals: otherwise a goal with 20 metrics would outvote five goals with one each.
  const avgPct = $derived(
    scoped.length === 0
      ? 0
      : Math.round((scoped.reduce((s, g) => s + progress(g.kpis), 0) / scoped.length) * 100)
  );
  const dueSoon = $derived(count((g) => ['d1', 'd3', 'w1'].includes(urgency(g))));

  // ── Drill-down: one active slice at a time, across all four donuts ──
  let pick = $state(null); // { dim, key } | null

  function choose(dim, key) {
    pick = pick && pick.dim === dim && pick.key === key ? null : { dim, key };
  }
  const activeIn = (dim) => (pick?.dim === dim ? pick.key : null);

  const MATCH = {
    status: (g, key) => status(g) === key,
    deadline: (g, key) => (key === 'with' ? !!g.deadline : !g.deadline),
    urgency: (g, key) => urgency(g) === key,
    metrics: (g, key) =>
      (g.kpis ?? []).some((k) => (key === 'done' ? k.reached : !k.reached))
  };

  const drill = $derived(pick ? scoped.filter((g) => MATCH[pick.dim](g, pick.key)) : []);

  const pickLabel = $derived.by(() => {
    if (!pick) return '';
    const src = { status: statusSlices, deadline: deadlineSlices, urgency: urgencySlices, metrics: metricSlices }[pick.dim];
    return src.find((s) => s.key === pick.key)?.label ?? '';
  });

  const pct = (g) => Math.round(progress(g.kpis) * 100);

  function deadlineText(g) {
    if (!g.deadline) return $t('goals.noDeadline');
    const d = daysLeft(g.deadline);
    if (d < 0) return $t('goals.deadline.overdue', { days: -d });
    if (d === 0) return $t('goals.deadline.today');
    return fmtDate(g.deadline);
  }
</script>

<div class="stats">
  <PageHeader title={$t('goals.stats.title')} subtitle={$t('goals.count', { count: scoped.length, s: scoped.length === 1 ? '' : 's' })}>
    {#snippet actions()}
      <a class="btn ghost" href="/goals"><Icon name="arrow-left" size={14} /> {$t('goals.stats.backToGoals')}</a>
    {/snippet}
  </PageHeader>

  {#if loading}
    <div class="grid">
      {#each Array.from({ length: 4 }, (_, i) => i) as i (i)}
        <div class="sk"><Skeleton height="148px" /></div>
      {/each}
    </div>
  {:else if goals.length === 0}
    <EmptyState icon="target" title={$t('goals.emptyTitle')} description={$t('goals.emptyBody')}>
      {#snippet action()}
        <a class="btn primary" href="/goals">{$t('goals.stats.backToGoals')}</a>
      {/snippet}
    </EmptyState>
  {:else}
    {#if categories.length > 0}
      <div class="filters">
        <button class="chip" class:active={catFilter === 'all'} onclick={() => (catFilter = 'all')}>
          {$t('goals.allCategories')}
        </button>
        {#each categories as c}
          <button class="chip" class:active={catFilter === c} onclick={() => (catFilter = c)}>{c}</button>
        {/each}
      </div>
    {/if}

    <div class="cards">
      <StatCard label={$t('goals.stats.total')} value={scoped.length} />
      <StatCard label={$t('goals.stats.avgProgress')} value="{avgPct}%" />
      <StatCard label={$t('goals.stats.dueSoon')} value={dueSoon} hint={$t('goals.stats.dueSoonHint')} />
      <StatCard label={$t('goals.form.metrics')} value="{metricSlices[0].value}/{metrics.length}" hint={$t('goals.stats.metricsReached')} />
    </div>

    <div class="grid">
      <StatDonut
        title={$t('goals.stats.byStatus')}
        slices={statusSlices}
        active={activeIn('status')}
        onpick={(k) => choose('status', k)}
      />
      <StatDonut
        title={$t('goals.stats.byDeadline')}
        slices={deadlineSlices}
        active={activeIn('deadline')}
        onpick={(k) => choose('deadline', k)}
      />
      <StatDonut
        title={$t('goals.stats.byUrgency')}
        slices={urgencySlices}
        active={activeIn('urgency')}
        onpick={(k) => choose('urgency', k)}
      />
      <StatDonut
        title={$t('goals.stats.byMetric')}
        slices={metricSlices}
        active={activeIn('metrics')}
        onpick={(k) => choose('metrics', k)}
      />
    </div>

    {#if pick}
      <section class="drill">
        <header>
          <h2>{pickLabel} <span class="n">{drill.length}</span></h2>
          <button class="chip" onclick={() => (pick = null)}>{$t('common.clear')}</button>
        </header>
        {#if drill.length === 0}
          <p class="muted">{$t('goals.stats.noneInBucket')}</p>
        {:else}
          <table class="tbl">
            <thead>
              <tr>
                <th>{$t('goals.form.name')}</th>
                <th>{$t('goals.form.category')}</th>
                <th>{$t('goals.form.deadline')}</th>
                <th class="num">{$t('goals.stats.progress')}</th>
              </tr>
            </thead>
            <tbody>
              {#each drill as g (g.id)}
                <tr class="row" onclick={() => goto('/goals')}>
                  <td>
                    <span class="swatch" style:background={g.color || 'var(--border-control)'}></span>
                    {g.name}
                  </td>
                  <td class="dim">{g.category || '—'}</td>
                  <td class="dim">{deadlineText(g)}</td>
                  <td class="num">
                    <div class="minibar"><div class="fill" style="width:{pct(g)}%; background:{g.color || 'var(--green)'}"></div></div>
                    <span class="p num">{pct(g)}%</span>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </section>
    {/if}
  {/if}
</div>

<style>
  .stats {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--space-3);
    margin-bottom: var(--space-4);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
    gap: var(--space-4);
  }
  .sk {
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    background: var(--surface);
  }
  .drill {
    margin-top: var(--space-6);
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
  }
  .drill header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  .drill h2 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .drill .n {
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-sm);
  }
  .muted {
    color: var(--dim);
    font-size: var(--text-sm);
  }
  .row {
    cursor: pointer;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .dim {
    color: var(--muted);
  }
  .swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: var(--space-2);
    vertical-align: middle;
  }
  .num {
    text-align: right;
  }
  td.num {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .minibar {
    width: 72px;
    height: 6px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .minibar .fill {
    height: 100%;
  }
  .p {
    min-width: 4ch;
    font-variant-numeric: tabular-nums;
    font-size: var(--text-sm);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    text-decoration: none;
  }
</style>
