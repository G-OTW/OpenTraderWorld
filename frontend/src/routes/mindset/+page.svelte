<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Mindset — pre-mortem / post-mortem check-ins. "Today" shows the two check-in cards for
  // the selected date (prompts are customizable). "History" lists past entries and charts the
  // trend of each 1–5 scale prompt over recent sessions.
  import { mindsetApi, PHASES } from '$lib/modules/mindset/api.js';
  import CheckinCard from '$lib/modules/mindset/CheckinCard.svelte';
  import PromptsModal from '$lib/modules/mindset/PromptsModal.svelte';
  import TrendChart from '$lib/modules/mindset/TrendChart.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let view = $state('today'); // 'today' | 'history'
  let date = $state(todayStr());
  let day = $state(null);
  let hist = $state(null);
  let error = $state('');
  let showPrompts = $state(false);

  // Local-date formatting — toISOString() is UTC and lands on the wrong day for any
  // timezone ahead of/behind UTC (e.g. "+1 day" appearing to do nothing in UTC+2).
  function fmtLocal(d) {
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }
  function todayStr() {
    return fmtLocal(new Date());
  }
  function shiftDate(days) {
    const d = new Date(date + 'T00:00:00');
    d.setDate(d.getDate() + days);
    date = fmtLocal(d);
  }

  function loadDay() {
    error = '';
    mindsetApi
      .day(date)
      .then((r) => (day = r))
      .catch((e) => (error = e.message));
  }
  function loadHistory() {
    error = '';
    mindsetApi
      .history(60)
      .then((r) => (hist = r))
      .catch((e) => (error = e.message));
  }
  $effect(() => {
    date;
    loadDay();
  });
  $effect(() => {
    if (view === 'history') loadHistory();
  });

  function entryFor(phaseKey) {
    return (day?.entries ?? []).find((e) => e.phase === phaseKey) ?? null;
  }

  // History helpers ----------------------------------------------------------

  let promptById = $derived(new Map((hist?.prompts ?? []).map((p) => [p.id, p])));

  // Trend series per scale prompt: [{ prompt, series: [{date, value}] }], oldest→newest.
  let trends = $derived.by(() => {
    if (!hist) return [];
    const scales = hist.prompts.filter((p) => p.kind === 'scale' && p.active);
    const entries = [...hist.entries].sort((a, b) => a.entry_date.localeCompare(b.entry_date));
    return scales
      .map((p) => ({
        prompt: p,
        series: entries
          .filter((e) => e.phase === p.phase && typeof e.answers?.[p.id] === 'number')
          .map((e) => ({ date: e.entry_date, value: e.answers[p.id] }))
      }))
      .filter((t) => t.series.length > 0);
  });

  // Entries grouped by date (newest first) for the history list.
  let histDays = $derived.by(() => {
    if (!hist) return [];
    const byDate = new Map();
    for (const e of hist.entries) {
      if (!byDate.has(e.entry_date)) byDate.set(e.entry_date, {});
      byDate.get(e.entry_date)[e.phase] = e;
    }
    return [...byDate.entries()]
      .sort((a, b) => b[0].localeCompare(a[0]))
      .map(([d, phases]) => ({ date: d, ...phases }));
  });

  function summarize(entry) {
    if (!entry) return null;
    const parts = [];
    for (const [pid, v] of Object.entries(entry.answers ?? {})) {
      const p = promptById.get(pid);
      if (!p || v == null || v === '' || (Array.isArray(v) && v.length === 0)) continue;
      if (p.kind === 'scale') parts.push($t('mindset.page.summaryScale', { label: p.label, value: v }));
      else if (p.kind === 'choice') parts.push($t('mindset.page.summaryPlain', { label: p.label, value: v }));
      else if (p.kind === 'tags') parts.push($t('mindset.page.summaryPlain', { label: p.label, value: v.join(', ') }));
      else parts.push($t('mindset.page.summaryPlain', { label: p.label, value: v }));
    }
    return parts;
  }

  function openDay(d) {
    date = d;
    view = 'today';
  }
</script>

<div class="page">
  <header>
    <h1>{$t('mindset.page.title')}</h1>
    <div class="tabs">
      <button class:active={view === 'today'} onclick={() => (view = 'today')}>{$t('mindset.page.checkin')}</button>
      <button class:active={view === 'history'} onclick={() => (view = 'history')}>{$t('mindset.page.history')}</button>
    </div>
    <div class="spacer"></div>
    {#if view === 'today'}
      <div class="datenav">
        <button class="btn" onclick={() => shiftDate(-1)} aria-label={$t('mindset.page.previousDay')}><Icon name="chevron-left" size={14} /></button>
        <input type="date" bind:value={date} />
        <button class="btn" onclick={() => shiftDate(1)} aria-label={$t('mindset.page.nextDay')}><Icon name="chevron-right" size={14} /></button>
        {#if date !== todayStr()}
          <button class="btn" onclick={() => (date = todayStr())}>{$t('mindset.page.today')}</button>
        {/if}
      </div>
    {/if}
    <button class="btn" onclick={() => (showPrompts = true)}>⚙ {$t('mindset.page.customize')}</button>
  </header>

  <ErrorText error={error} copyable />

  {#if view === 'today'}
    {#if day}
      <div class="cards">
        {#each PHASES as phase (phase.key)}
          <CheckinCard
            {phase}
            {date}
            prompts={day.prompts}
            entry={entryFor(phase.key)}
            onsaved={loadDay}
          />
        {/each}
      </div>
    {:else}
      <div class="cards" aria-busy="true">
        {#each Array.from({ length: 2 }, (_, i) => i) as i (i)}
          <div class="sk-card">
            <Skeleton height="1.2rem" width="45%" />
            <Skeleton rows={3} height="2.4rem" gap="var(--space-2)" />
          </div>
        {/each}
      </div>
    {/if}
  {:else if hist}
    {#if trends.length > 0}
      <section class="panel">
        <h3>{$t('mindset.page.trends')} <span class="muted small">{$t('mindset.page.lastNCheckins', { count: hist.entries.length })}</span></h3>
        <div class="trendgrid">
          {#each trends as t (t.prompt.id)}
            <TrendChart label="{PHASES.find((p) => p.key === t.prompt.phase)?.icon} {t.prompt.label}" series={t.series} />
          {/each}
        </div>
      </section>
    {/if}

    <section class="days">
      {#each histDays as d (d.date)}
        <article class="day">
          <button class="daylink" onclick={() => openDay(d.date)}>{d.date}</button>
          <div class="phases">
            {#each PHASES as phase (phase.key)}
              {@const parts = summarize(d[phase.key])}
              <div class="phase">
                <span class="phlbl">{phase.icon} {phase.label}</span>
                {#if parts}
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
      {#if histDays.length === 0}
        <p class="muted">{$t('mindset.page.noCheckinsYet')}</p>
      {/if}
    </section>
  {:else}
    <div class="sk-panel" aria-busy="true"><Skeleton height="200px" /></div>
  {/if}
</div>

<PromptsModal bind:open={showPrompts} onchanged={() => (loadDay(), view === 'history' && loadHistory())} />

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  h1 {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
  }
  .tabs {
    display: inline-flex;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    overflow: hidden;
  }
  .tabs button {
    background: transparent;
    border: none;
    border-left: 0.5px solid var(--border-control);
    color: var(--muted);
    font-size: var(--text-base);
    font-weight: var(--fw-normal);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .tabs button:first-child {
    border-left: none;
  }
  .tabs button.active {
    color: var(--text);
    background: var(--surface-2);
    font-weight: var(--fw-medium);
  }
  .spacer {
    flex: 1;
  }
  .datenav {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .btn {
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .btn:hover {
    background: var(--surface-2);
  }
  .sk-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 0.5px solid var(--border);
    border-radius: 0;
    background: var(--surface);
  }
  .sk-panel {
    padding: var(--space-4);
    border: 0.5px solid var(--border);
    border-radius: 0;
    background: var(--surface);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
    gap: var(--space-4);
    align-items: start;
  }
  .panel {
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-4);
  }
  .panel h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    letter-spacing: 0.01em;
    margin-bottom: var(--space-3);
  }
  .trendgrid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
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
    border-radius: 0;
    padding: var(--space-3) var(--space-4);
  }
  .daylink {
    background: transparent;
    border: none;
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    cursor: pointer;
    padding: 0;
    margin-bottom: var(--space-2);
  }
  .daylink:hover {
    color: var(--muted);
  }
  .phases {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: var(--space-3);
  }
  .phase {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .phlbl {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .phase ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-sm);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
</style>
