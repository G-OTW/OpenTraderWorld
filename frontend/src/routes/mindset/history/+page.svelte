<script>
  // History: the raw check-ins of the window, one folded row per day. The analysis of the
  // same data is the Dashboard page.
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { mindsetApi, NAV_LINKS, PHASES, isAnswered } from '$lib/modules/mindset/nav.js';
  import { buildDashboard, addDays, roundOf } from '$lib/modules/mindset/scoring.js';
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
  let span = $state(60);

  const today = new Date().toISOString().slice(0, 10);
  const to = today;
  const from = $derived(addDays(today, -(span - 1)));

  // The effect below runs once on mount, so no separate onMount load.
  function load() {
    error = '';
    mindsetApi
      .history(from, to)
      .then((r) => (hist = r))
      .catch((e) => (error = e.message));
  }
  $effect(() => {
    from;
    load();
  });

  const promptById = $derived(new Map((hist?.prompts ?? []).map((p) => [p.id, p])));
  const templateById = $derived(new Map((hist?.templates ?? []).map((tpl) => [tpl.id, tpl])));
  const catById = $derived(new Map((hist?.categories ?? []).map((c) => [c.id, c])));
  const templateName = (p) => templateById.get(p?.template_id)?.name ?? '';

  // Only the per-day score is needed here, for the row header.
  const dash = $derived(hist ? buildDashboard(hist, { from, to }) : null);
  const scoreByDate = $derived(new Map((dash?.days ?? []).map((d) => [d.date, d.score])));

  // Log: entries grouped by date (newest first), each carrying its template's entries.
  const histDays = $derived.by(() => {
    if (!hist) return [];
    const byDate = new Map();
    for (const e of hist.entries) {
      if (e.entry_date < from) continue;
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
  // Log rows start folded; a Set so several days can be open at once.
  let openDays = $state(new Set());
  function toggleDay(date) {
    const next = new Set(openDays);
    if (next.has(date)) next.delete(date);
    else next.add(date);
    openDays = next;
  }

  function fmtDate(iso, long = true) {
    return new Date(`${iso}T00:00:00`).toLocaleDateString(undefined, long
      ? { weekday: 'short', day: 'numeric', month: 'long', year: 'numeric' }
      : { day: 'numeric', month: 'short' });
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="mindset.nav.label" />

  <PageHeader
    title={$t('mindset.history.title')}
    subtitle={hist ? $t('mindset.history.logSubtitle', { days: histDays.length, from: fmtDate(from, false), to: fmtDate(to, false) }) : ''}
  >
    {#snippet actions()}
      <div class="limits" role="group" aria-label={$t('mindset.history.range')}>
        <span class="rlbl">{$t('mindset.history.range')}</span>
        {#each RANGES as r (r.days)}
          <button class="chip" class:active={span === r.days} onclick={() => (span = r.days)}>
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
      <EmptyState icon="bar-chart" title={$t('mindset.history.emptyTitle')} description={$t('mindset.page.noCheckinsYet')}>
        {#snippet action()}
          <a class="btn primary link" href="/mindset">{$t('mindset.history.goToCheckin')}</a>
        {/snippet}
      </EmptyState>
    </div>
  {:else}
      <section class="days">
        {#each histDays as d (d.date)}
          {@const expanded = openDays.has(d.date)}
          <article class="day" class:expanded>
            <!-- Collapsed by default: one line per day, the answers unfold on click. -->
            <button class="dayhead" aria-expanded={expanded} onclick={() => toggleDay(d.date)}>
              <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={14} />
              <span class="daylbl">{fmtDate(d.date)}</span>
              <span class="daymeta">
                {d.entries.map((e) => templateById.get(e.template_id)?.name ?? $t('mindset.history.deletedTemplate')).join(' · ')}
              </span>
              {#if scoreByDate.has(d.date)}
                <span class="score num">{$t('mindset.history.score')} {roundOf(scoreByDate.get(d.date))}</span>
              {/if}
            </button>
            {#if expanded}
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
            <a class="dayopen" href="/mindset?date={d.date}">{$t('mindset.history.openDay')}</a>
            {/if}
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
  .btn.link {
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    align-self: flex-start;
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
  .dayhead {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    background: transparent;
    border: none;
    padding: 0;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }
  .dayhead:hover .daylbl {
    color: var(--accent);
  }
  .daylbl {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }
  .daymeta {
    flex: 1;
    min-width: 0;
    font-size: var(--text-xs);
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .day.expanded .entries {
    padding-top: var(--space-2);
    border-top: var(--hairline) solid var(--border);
  }
  .dayopen {
    align-self: flex-start;
    font-size: var(--text-xs);
    color: var(--accent);
    text-decoration: none;
  }
  .dayopen:hover {
    text-decoration: underline;
  }
  .score {
    font-size: var(--text-xs);
    color: var(--muted);
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
  @media (max-width: 767px) {
    .page {
      padding: var(--space-4);
      padding-bottom: calc(80px + env(safe-area-inset-bottom));
    }
    .limits {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      width: 100%;
    }
    .rlbl {
      grid-column: 1 / -1;
    }
    .limits .chip {
      min-width: 0;
      min-height: 44px;
      white-space: normal;
    }
    .entries {
      grid-template-columns: minmax(0, 1fr);
    }
    .dayhead {
      min-height: 44px;
    }
    .daymeta {
      display: none;
    }
    .elbl,
    .entry li {
      overflow-wrap: anywhere;
    }
  }
</style>
