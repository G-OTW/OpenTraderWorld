<script>
  // Check-in — the day view of the Mindset module.
  //
  // Pick a date (default today); the page shows the active templates for that day grouped
  // by category, each rendering its prompts as controls. A progress bar counts answered
  // prompts across every check-in, and the consistency block works exactly as it does on
  // the routines board: a verdict the trader sets, not one inferred from filled fields.
  //
  // Authoring lives on the Templates page; this page only answers.
  import Icon from '$lib/ui/Icon.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConsistencyModal from '$lib/ui/ConsistencyModal.svelte';
  import CheckinCard from '$lib/modules/mindset/CheckinCard.svelte';
  import { mindsetApi, NAV_LINKS, PHASES, isAnswered } from '$lib/modules/mindset/nav.js';
  import {
    fmtLocal,
    todayStr,
    parseLocal,
    periodStats,
    PERIODS
  } from '$lib/ui/consistency.js';
  import { page } from '$app/stores';
  import { t } from '$lib/i18n';

  // History links here with ?date=, so a day opens on the entry that was clicked.
  let date = $state($page.url.searchParams.get('date') ?? todayStr());
  let day = $state(null);
  let error = $state('');
  let loading = $state(false);

  let period = $state('week');
  let showConsistency = $state(false);

  // Category ids the user has folded away, and templates whose card is collapsed.
  const COLLAPSE_KEY = 'mindset.day.collapsed';
  let collapsed = $state(new Set(readCollapsed()));

  function readCollapsed() {
    try {
      return JSON.parse(localStorage.getItem(COLLAPSE_KEY) ?? '[]');
    } catch {
      return [];
    }
  }
  function toggleCollapsed(key) {
    const next = new Set(collapsed);
    next.has(key) ? next.delete(key) : next.add(key);
    collapsed = next;
    try {
      localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...next]));
    } catch {
      /* private mode / quota — folding just stops persisting */
    }
  }

  function shiftDate(days) {
    const d = parseLocal(date);
    d.setDate(d.getDate() + days);
    date = fmtLocal(d);
  }

  function load() {
    loading = true;
    error = '';
    mindsetApi
      .day(date)
      .then((r) => (day = r))
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  }
  $effect(() => {
    date; // re-load whenever the date changes
    load();
  });

  const promptsOf = (tplId) => (day?.prompts ?? []).filter((p) => p.template_id === tplId);
  const entryOf = (tplId) => (day?.entries ?? []).find((e) => e.template_id === tplId) ?? null;

  // Progress across every prompt of every active template shown today.
  const progress = $derived.by(() => {
    let done = 0;
    let total = 0;
    for (const tpl of day?.templates ?? []) {
      const answers = entryOf(tpl.id)?.answers ?? {};
      for (const p of promptsOf(tpl.id)) {
        total++;
        if (isAnswered(answers[p.id])) done++;
      }
    }
    return { done, total, pct: total ? (done / total) * 100 : 0 };
  });

  // Grouped by category (phase order inside), uncategorised last — the same structure as
  // the Templates page, so a check-in sits in the same place on both.
  const groups = $derived.by(() => {
    const cats = day?.categories ?? [];
    const byCat = new Map(cats.map((c) => [c.id, []]));
    const loose = [];
    const phaseRank = (p) => PHASES.findIndex((x) => x.key === p);
    for (const tpl of day?.templates ?? []) {
      const bucket = byCat.get(tpl.category_id);
      if (bucket) bucket.push(tpl);
      else loose.push(tpl);
    }
    const sort = (list) =>
      [...list].sort((a, b) => phaseRank(a.phase) - phaseRank(b.phase) || a.position - b.position);
    const out = cats
      .map((c) => ({ category: c, templates: sort(byCat.get(c.id) ?? []) }))
      .filter((g) => g.templates.length > 0);
    if (loose.length > 0) out.push({ category: null, templates: sort(loose) });
    return out;
  });

  const stats = $derived(periodStats(day?.marks ?? [], period, parseLocal(date)));

  const dayLabel = $derived(
    parseLocal(date).toLocaleDateString(undefined, {
      weekday: 'long',
      day: 'numeric',
      month: 'long'
    })
  );

  // The bar's one-click verdict for the selected day: claim it, or take the claim back.
  async function markDayDone() {
    const next = day.mark === 'full' ? null : 'full';
    const before = day.mark;
    day = { ...day, mark: next, marks: applyMark(day.marks, date, next) };
    try {
      await mindsetApi.setMark(date, next);
    } catch (e) {
      error = e.message;
      day = { ...day, mark: before, marks: applyMark(day.marks, date, before) };
    }
  }

  function applyMark(marks, d, mark) {
    const rest = (marks ?? []).filter((m) => m.day !== d);
    return mark ? [...rest, { day: d, mark }] : rest;
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="mindset.nav.label" />

  <PageHeader title={$t('mindset.page.title')} subtitle={dayLabel}>
    {#snippet actions()}
      <div class="datenav">
        <button class="btn" onclick={() => shiftDate(-1)} aria-label={$t('mindset.page.previousDay')}>
          <Icon name="chevron-left" size={14} />
        </button>
        <input type="date" bind:value={date} />
        <button class="btn" onclick={() => shiftDate(1)} aria-label={$t('mindset.page.nextDay')}>
          <Icon name="chevron-right" size={14} />
        </button>
        {#if date !== todayStr()}
          <button class="btn" onclick={() => (date = todayStr())}>{$t('mindset.page.today')}</button>
        {/if}
      </div>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} copyable />

  {#if day}
    <div class="status">
      <div class="progress">
        <div class="ptext">
          {@html $t('mindset.page.promptsAnswered', {
            done: progress.done,
            total: progress.total
          })}
          {#if progress.total > 0 && progress.done === progress.total}
            <span class="allset">{$t('mindset.page.allSet')}</span>
          {/if}
        </div>
        <div class="pbar"><div class="pfill" style="width:{progress.pct}%"></div></div>
      </div>

      <div class="consist">
        <div class="periods" role="group" aria-label={$t('routines.period.label')}>
          {#each PERIODS as p (p)}
            <button class="chip" class:active={period === p} onclick={() => (period = p)}>
              {$t(`routines.period.${p}`)}
            </button>
          {/each}
        </div>
        <button class="tally" onclick={() => (showConsistency = true)}>
          <b>{stats.done}</b><span class="of">/{stats.total}</span>
          <span class="tlbl">{$t('routines.period.daysDone')}</span>
          <Icon name="calendar-days" size={13} />
        </button>
        <button
          class="markbtn"
          class:on={day.mark === 'full'}
          onclick={markDayDone}
          title={$t('mindset.page.markDayDoneHint')}
        >
          <Icon name={day.mark === 'full' ? 'check-circle' : 'check'} size={13} />
          {day.mark === 'full' ? $t('mindset.page.dayDone') : $t('mindset.page.markDayDone')}
        </button>
      </div>
    </div>

    {#if (day.templates ?? []).length === 0}
      <EmptyState
        icon="check-square"
        title={$t('mindset.page.noTemplates')}
        description={$t('mindset.page.noTemplatesHint')}
      >
        {#snippet action()}
          <a class="btn primary link" href="/mindset/templates">
            {$t('mindset.page.goToTemplates')}
          </a>
        {/snippet}
      </EmptyState>
    {/if}

    <div class="groups">
      {#each groups as g (g.category?.id ?? 'none')}
        {@const key = g.category?.id ?? 'none'}
        {@const open = !collapsed.has(key)}
        <section class="group">
          <h2 class="ghead">
            <button
              class="gtoggle"
              onclick={() => toggleCollapsed(key)}
              aria-expanded={open}
              aria-controls="group-{key}"
            >
              <Icon name={open ? 'chevron-down' : 'chevron-right'} size={13} />
              <span
                class="dot"
                style:background={g.category?.color || 'var(--muted)'}
                aria-hidden="true"
              ></span>
              {g.category?.name ?? $t('mindset.templates.uncategorised')}
              <span class="gcount">{g.templates.length}</span>
            </button>
          </h2>

          {#if open}
            <div class="glist" id="group-{key}">
              {#each g.templates as tpl (tpl.id)}
                <CheckinCard
                  template={tpl}
                  {date}
                  prompts={promptsOf(tpl.id)}
                  entry={entryOf(tpl.id)}
                  accent={g.category?.color || 'var(--border)'}
                  open={!collapsed.has(tpl.id)}
                  ontoggle={() => toggleCollapsed(tpl.id)}
                  onsaved={load}
                />
              {/each}
            </div>
          {/if}
        </section>
      {/each}
    </div>
  {:else if loading}
    <Skeleton rows={4} height="2.4rem" gap="var(--space-3)" />
  {/if}
</div>

<ConsistencyModal
  bind:open={showConsistency}
  marks={day?.marks ?? []}
  setMark={mindsetApi.setMark}
  title={$t('mindset.consistency.title')}
  onchanged={(next) =>
    (day = { ...day, marks: next, mark: next.find((m) => m.day === date)?.mark ?? null })}
/>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }

  .datenav {
    display: flex;
    align-items: center;
    gap: var(--space-1);
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

  /* Two fixed columns, so the period picker never slides when the counter's digits grow. */
  .status {
    display: grid;
    grid-template-columns: 58% minmax(0, 1fr);
    align-items: center;
    gap: var(--space-6);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
  }
  @media (max-width: 860px) {
    .status {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-3);
    }
  }
  .progress {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .ptext {
    font-size: var(--text-base);
    color: var(--muted);
  }
  /* The <strong> arrives via {@html} from the translation, so it needs :global. */
  .ptext :global(strong) {
    color: var(--text);
  }
  .allset {
    color: var(--green);
  }
  .pbar {
    height: 6px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .pfill {
    height: 100%;
    background: var(--green);
    transition: width 0.25s ease;
  }

  .consist {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
  }
  .periods {
    display: flex;
    gap: var(--space-1);
    flex: none;
  }
  .tally {
    display: inline-flex;
    align-items: baseline;
    justify-content: flex-end;
    gap: 3px;
    flex: none;
    min-width: 11rem;
    background: transparent;
    border: none;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-base);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .tally:hover {
    background: var(--surface-2);
  }
  .tally b {
    font-weight: var(--fw-medium);
  }
  .of,
  .tlbl {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .tlbl {
    margin-left: var(--space-1);
  }
  .markbtn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    flex: none;
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .markbtn:hover {
    color: var(--text);
  }
  .markbtn.on {
    color: var(--green);
    border-color: var(--green);
  }

  .groups {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .ghead {
    display: flex;
    align-items: center;
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .gtoggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    padding: var(--space-1) var(--space-2);
    margin-left: calc(var(--space-2) * -1);
    border-radius: var(--radius);
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .gtoggle:hover {
    background: var(--surface-2);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  .gcount {
    font-size: var(--text-xs);
    font-weight: 400;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .glist {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
</style>
