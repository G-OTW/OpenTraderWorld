<script>
  // History — past check-ins and the trend of every 1–5 scale prompt.
  //
  // Was a tab on the day page; it is its own route now that the module has a top menu, so
  // a trend is linkable and the day view no longer carries two datasets at once.
  //
  // Entries are grouped by date and, within a date, by the template that framed them —
  // answers keyed to a deleted prompt simply stop rendering, which is what keeps old
  // entries readable after a template is reworked.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import TrendChart from '$lib/modules/mindset/TrendChart.svelte';
  import { mindsetApi, NAV_LINKS, PHASES, isAnswered } from '$lib/modules/mindset/nav.js';
  import { t } from '$lib/i18n';

  // Ranges named by span, not by a raw day count.
  const RANGES = [
    { days: 30, key: 'mindset.history.range1m' },
    { days: 60, key: 'mindset.history.range2m' },
    { days: 180, key: 'mindset.history.range6m' },
    { days: 365, key: 'mindset.history.range1y' }
  ];

  let hist = $state(null);
  let error = $state('');
  let limit = $state(60);

  onMount(load);

  function load() {
    error = '';
    mindsetApi
      .history(limit)
      .then((r) => (hist = r))
      .catch((e) => (error = e.message));
  }
  $effect(() => {
    limit;
    load();
  });

  const promptById = $derived(new Map((hist?.prompts ?? []).map((p) => [p.id, p])));
  const templateById = $derived(new Map((hist?.templates ?? []).map((tpl) => [tpl.id, tpl])));
  const catById = $derived(new Map((hist?.categories ?? []).map((c) => [c.id, c])));

  // Trend series per scale prompt: [{ prompt, series: [{date, value}] }], oldest→newest.
  const trends = $derived.by(() => {
    if (!hist) return [];
    const scales = hist.prompts.filter((p) => p.kind === 'scale' && p.active);
    const entries = [...hist.entries].sort((a, b) => a.entry_date.localeCompare(b.entry_date));
    return scales
      .map((p) => ({
        prompt: p,
        series: entries
          .filter((e) => typeof e.answers?.[p.id] === 'number')
          .map((e) => ({ date: e.entry_date, value: e.answers[p.id] }))
      }))
      .filter((tr) => tr.series.length > 0);
  });

  // Entries grouped by date (newest first), each carrying its template's entries.
  const histDays = $derived.by(() => {
    if (!hist) return [];
    const byDate = new Map();
    for (const e of hist.entries) {
      if (!byDate.has(e.entry_date)) byDate.set(e.entry_date, []);
      byDate.get(e.entry_date).push(e);
    }
    const phaseRank = (p) => PHASES.findIndex((x) => x.key === p);
    return [...byDate.entries()]
      .sort((a, b) => b[0].localeCompare(a[0]))
      .map(([date, entries]) => ({
        date,
        entries: entries.sort((a, b) => phaseRank(a.phase) - phaseRank(b.phase))
      }));
  });

  /// One entry's filled answers, as "label: value" lines.
  function summarize(entry) {
    const parts = [];
    for (const [pid, v] of Object.entries(entry.answers ?? {})) {
      const p = promptById.get(pid);
      if (!p || !isAnswered(v)) continue;
      const value = Array.isArray(v) ? v.join(', ') : v;
      parts.push(
        p.kind === 'scale'
          ? $t('mindset.page.summaryScale', { label: p.label, value })
          : $t('mindset.page.summaryPlain', { label: p.label, value })
      );
    }
    return parts;
  }

  const accentOf = (tpl) => catById.get(tpl?.category_id)?.color || 'var(--border)';

  function fmtDate(iso) {
    return new Date(`${iso}T00:00:00`).toLocaleDateString(undefined, {
      weekday: 'short',
      day: 'numeric',
      month: 'long',
      year: 'numeric'
    });
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="mindset.nav.label" />

  <PageHeader
    title={$t('mindset.history.title')}
    subtitle={hist ? $t('mindset.page.lastNCheckins', { count: hist.entries.length }) : ''}
  >
    {#snippet actions()}
      <!-- A bare "30 / 60 / 180" says nothing; each chip names its own span. -->
      <div class="limits" role="group" aria-label={$t('mindset.history.range')}>
        <span class="rlbl">{$t('mindset.history.range')}</span>
        {#each RANGES as r (r.days)}
          <button
            class="chip"
            class:active={limit === r.days}
            onclick={() => (limit = r.days)}
          >
            {$t(r.key)}
          </button>
        {/each}
      </div>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} copyable />

  {#if !hist}
    <Skeleton height="200px" />
  {:else if histDays.length === 0}
    <div class="fill">
    <EmptyState
      icon="bar-chart"
      title={$t('mindset.history.emptyTitle')}
      description={$t('mindset.page.noCheckinsYet')}
    >
      {#snippet action()}
        <a class="btn primary link" href="/mindset">{$t('mindset.history.goToCheckin')}</a>
      {/snippet}
    </EmptyState>
    </div>
  {:else}
    {#if trends.length > 0}
      <section class="panel">
        <h2>{$t('mindset.page.trends')}</h2>
        <div class="trendgrid">
          {#each trends as tr (tr.prompt.id)}
            <TrendChart
              label="{templateById.get(tr.prompt.template_id)?.name ?? ''} — {tr.prompt.label}"
              series={tr.series}
            />
          {/each}
        </div>
      </section>
    {/if}

    <section class="days">
      {#each histDays as d (d.date)}
        <article class="day">
          <button class="daylink" onclick={() => goto(`/mindset?date=${d.date}`)}>
            {fmtDate(d.date)}
          </button>
          <div class="entries">
            {#each d.entries as e (e.id)}
              {@const tpl = templateById.get(e.template_id)}
              {@const parts = summarize(e)}
              <div class="entry" style:--cat={accentOf(tpl)}>
                <span class="elbl">
                  {PHASES.find((p) => p.key === e.phase)?.icon}
                  {tpl?.name ?? $t('mindset.history.deletedTemplate')}
                </span>
                {#if parts.length > 0}
                  <ul>
                    {#each parts as part, i (i)}<li>{part}</li>{/each}
                  </ul>
                {:else}
                  <span class="muted small">{$t('mindset.page.notFilled')}</span>
                {/if}
              </div>
            {/each}
          </div>
        </article>
      {/each}
    </section>
  {/if}
</div>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }

  .limits {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .rlbl {
    font-size: var(--text-xs);
    color: var(--muted);
  }

  /* The empty state takes the page's free height, so it sits in the middle of the view
     instead of clinging under the header. */
  .fill {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  /* An <a> styled as a button: keep the global .btn layer's inline-flex centring, or the
     label sits at the top of the box with the padding pooled underneath. */
  .btn.link {
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .panel {
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .panel h2 {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .trendgrid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-4);
  }

  .days {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .day {
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .daylink {
    align-self: flex-start;
    background: transparent;
    border: none;
    padding: 0;
    color: var(--text);
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    cursor: pointer;
  }
  .daylink:hover {
    color: var(--accent);
  }

  .entries {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: var(--space-4);
  }
  .entry {
    border-left: 2px solid var(--cat);
    padding-left: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .elbl {
    font-size: var(--text-sm);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .entry ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .entry li {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-xs);
  }
</style>
