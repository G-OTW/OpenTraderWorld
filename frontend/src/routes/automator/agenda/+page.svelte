<script>
  // The agenda: what is going to run, and when. Occurrences are materialized server-side,
  // so the calendar shows the same instants the scheduler will act on rather than a second
  // implementation of the timezone maths.
  import { onMount, onDestroy } from 'svelte';
  import { Calendar } from '@fullcalendar/core';
  import dayGridPlugin from '@fullcalendar/daygrid';
  import timeGridPlugin from '@fullcalendar/timegrid';
  import Icon from '$lib/ui/Icon.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { automatorApi, NAV_LINKS } from '$lib/modules/automator/api.js';
  import { t, locale } from '$lib/i18n';

  let el = $state(null);
  let cal = null;
  let error = $state('');
  let truncated = $state(false);
  let workflows = $state([]);
  let filter = $state('');
  let range = $state(null);
  let title = $state('');
  let view = $state('dayGridMonth');
  let loading = $state(true);
  let count = $state(0);
  let timezone = $state('');
  let requestId = 0;

  // Workflow colour is stable across navigation and filtering; colour never carries text.
  function tone(id) {
    return [...id].reduce((hash, char) => (hash * 31 + char.charCodeAt(0)) >>> 0, 0) % 4;
  }

  function eventContent(info, h) {
    return h('div', { className: 'run-content' },
      h('span', { className: 'run-name' }, info.event.title),
      h('span', { className: 'run-time' }, info.timeText)
    );
  }

  onMount(() => {
    timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    cal = new Calendar(el, {
      plugins: [dayGridPlugin, timeGridPlugin],
      initialView: 'dayGridMonth',
      headerToolbar: false,
      height: '100%',
      firstDay: 1,
      nowIndicator: true,
      fixedWeekCount: false,
      allDaySlot: false,
      dayMaxEvents: 2,
      eventDisplay: 'block',
      eventTimeFormat: { hour: '2-digit', minute: '2-digit', hour12: false },
      displayEventEnd: false,
      eventContent,
      eventDidMount: ({ el, event }) => {
        el.title = `${event.title} · ${event.start?.toLocaleString($locale)} (${timezone})`;
      },
      datesSet: (info) => {
        title = info.view.title;
        view = info.view.type;
        range = { from: info.start.toISOString(), to: info.end.toISOString() };
        refresh();
      }
    });
    cal.render();
  });

  onDestroy(() => {
    requestId += 1;
    cal?.destroy();
    cal = null;
  });

  async function refresh() {
    if (!range) return;
    const id = ++requestId;
    loading = true;
    error = '';
    try {
      const r = await automatorApi.agenda(range.from, range.to, filter || null);
      if (id !== requestId || !cal) return;
      truncated = !!r.truncated;
      count = (r.events ?? []).filter((e) =>
        new Date(e.at) >= cal.view.currentStart && new Date(e.at) < cal.view.currentEnd
      ).length;
      cal.batchRendering(() => {
        cal.removeAllEvents();
        cal.getEventSources().forEach((source) => source.remove());
        cal.addEventSource(
          (r.events ?? []).map((e) => ({
            title: e.workflow,
            start: e.at,
            allDay: false,
            url: `/automator/${e.workflow_id}`,
            classNames: ['agenda-run', `run-tone-${tone(e.workflow_id)}`]
          }))
        );
      });
    } catch (e) {
      if (id !== requestId || !cal) return;
      error = e.message;
      cal.removeAllEvents();
      count = 0;
      truncated = false;
    } finally {
      if (id === requestId) loading = false;
    }
  }

  // The filter list comes from the schedules call, which the agenda does not return.
  onMount(async () => {
    try {
      const r = await automatorApi.schedules();
      workflows = r.workflows ?? [];
    } catch {
      workflows = [];
    }
  });

  const options = $derived([
    { value: '', label: $t('automator.agenda.allWorkflows') },
    ...workflows.map((w) => ({ value: w.id, label: w.name }))
  ]);

  function move(direction) {
    if (direction < 0) cal?.prev();
    else cal?.next();
  }

  function today() {
    cal?.today();
  }

  function setView(next) {
    cal?.changeView(next);
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="automator.nav.label" />
  <PageHeader title={$t('automator.agenda.title')} subtitle={$t('automator.agenda.subtitle')}>
    {#snippet actions()}
      <div class="filter">
        <Select options={options} bind:value={filter} onpick={refresh} placeholder={$t('automator.agenda.allWorkflows')} />
      </div>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} />
  {#if truncated}
    <p class="hint">{$t('automator.agenda.truncated')}</p>
  {/if}

  <section class="calendar-panel" aria-label={$t('automator.agenda.title')}>
    <div class="toolbar">
      <div class="period-title">
        <span class="calendar-mark"><Icon name="calendar-days" size={20} strokeWidth={1.5} /></span>
        <h2 aria-live="polite">{title}</h2>
      </div>
      <div class="period-nav">
        <button class="today-button" onclick={today}>{$t('calendar.toolbar.today')}</button>
        <div class="arrow-group">
          <button
            class="icon-button"
            onclick={() => move(-1)}
            title={$t('calendar.toolbar.previous')}
            aria-label={$t('calendar.toolbar.previous')}
          >
            <Icon name="chevron-left" size={15} />
          </button>
          <button
            class="icon-button"
            onclick={() => move(1)}
            title={$t('calendar.toolbar.next')}
            aria-label={$t('calendar.toolbar.next')}
          >
            <Icon name="chevron-right" size={15} />
          </button>
        </div>
      </div>

      <div class="view-switch" role="group" aria-label={$t('automator.agenda.view')}>
        <button
          class:active={view === 'dayGridMonth'}
          aria-pressed={view === 'dayGridMonth'}
          onclick={() => setView('dayGridMonth')}
        >
          <Icon name="calendar-days" size={14} />
          {$t('calendar.toolbar.month')}
        </button>
        <button
          class:active={view === 'timeGridWeek'}
          aria-pressed={view === 'timeGridWeek'}
          onclick={() => setView('timeGridWeek')}
        >
          <Icon name="timeline" size={14} />
          {$t('calendar.toolbar.week')}
        </button>
      </div>
    </div>
    <div class="calendar-scroll" aria-busy={loading}>
      <div class="cal" class:loading bind:this={el}></div>
    </div>
    <footer class="calendar-footer">
      <span class="calendar-status" role="status">
        <span class="status-dot" class:busy={loading} class:failed={!!error}></span>
        {#if loading}{$t('common.loading')}
        {:else if error}{$t('automator.agenda.unavailable')}
        {:else if count === 0}{$t('automator.agenda.empty')}
        {:else}{$t('automator.agenda.occurrences', { count: `${count}${truncated ? '+' : ''}` })}{/if}
      </span>
      <span class="timezone" title={$t('automator.agenda.localTime')}>
        <Icon name="globe" size={13} strokeWidth={1.5} />{timezone}
      </span>
    </footer>
  </section>
</div>

<style>
  .page {
    width: 100%;
    max-width: 100vw;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    min-height: 0;
    min-width: 0;
  }
  .page :global(.modnav) {
    flex: none;
    overflow-x: auto;
  }
  .page :global(.modnav a) { flex: none; }
  .filter {
    min-width: 200px;
  }
  .hint {
    margin: 0 0 var(--space-2);
    font-size: var(--text-xs);
    color: var(--amber-ink, var(--amber));
  }

  /* The agenda is one working surface, not a loose FullCalendar table. */
  .calendar-panel {
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    container-type: inline-size;
  }

  .toolbar {
    flex: none;
    min-height: 82px;
    display: flex;
    align-items: center;
    gap: var(--space-6);
    padding: var(--space-4) var(--space-6);
    background: var(--surface);
    border-bottom: var(--hairline) solid var(--border);
  }

  h2 {
    margin: 0;
    color: var(--text);
    font-size: 21px;
    font-weight: var(--fw-medium);
    letter-spacing: -0.015em;
    line-height: var(--lh-tight);
    white-space: nowrap;
  }

  .period-title {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .calendar-mark {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
  }

  .period-nav,
  .arrow-group,
  .view-switch {
    display: flex;
    align-items: center;
  }
  .period-nav {
    justify-self: start;
    gap: var(--space-2);
  }
  .arrow-group,
  .view-switch {
    gap: 2px;
  }
  .view-switch {
    margin-left: auto;
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: 4px;
    border: 1px solid var(--border);
  }

  .icon-button,
  .today-button,
  .view-switch button {
    border: 0;
    color: var(--muted);
    background: transparent;
    font: inherit;
    font-size: var(--text-sm);
    line-height: 1;
    cursor: pointer;
    transition:
      color var(--dur-fast) var(--ease),
      background var(--dur-fast) var(--ease);
  }
  .icon-button {
    width: 34px;
    height: 32px;
    display: grid;
    place-items: center;
    padding: 0;
    border-radius: var(--radius-sm);
  }
  .today-button {
    height: 34px;
    padding: 0 var(--space-3);
    border: 1px solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .view-switch button {
    min-height: 32px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-sm);
  }
  .icon-button:hover,
  .today-button:hover,
  .view-switch button:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .view-switch button.active {
    color: var(--text);
    background: var(--surface-raised);
    box-shadow: var(--shadow-1);
    font-weight: var(--fw-medium);
  }
  .icon-button:focus-visible,
  .today-button:focus-visible,
  .view-switch button:focus-visible {
    position: relative;
    z-index: 1;
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .calendar-scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .cal {
    height: 100%;
    min-width: 980px;
    background: var(--surface);
    transition: opacity var(--dur-fast) var(--ease);
  }
  .cal.loading {
    opacity: 0.45;
    pointer-events: none;
  }
  .calendar-footer {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
    min-height: 44px;
    padding: var(--space-3) var(--space-6);
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: 11px;
  }
  .calendar-status,
  .timezone {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .timezone { color: var(--muted); }
  .status-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
  }
  .status-dot.busy { background: var(--dim); }
  .status-dot.failed { background: var(--red); }

  /* FullCalendar mounts on `.cal` itself, so token overrides target the host node. */
  .cal:global(.fc) {
    --fc-border-color: color-mix(in srgb, var(--border-control) 38%, var(--surface));
    --fc-page-bg-color: var(--surface);
    --fc-neutral-bg-color: var(--surface-2);
    --fc-neutral-text-color: var(--dim);
    --fc-today-bg-color: color-mix(in srgb, var(--accent) 4%, var(--surface));
    --fc-event-bg-color: color-mix(in srgb, var(--accent) 16%, var(--surface));
    --fc-event-border-color: var(--accent);
    --fc-event-text-color: var(--text);
    --fc-now-indicator-color: var(--red);
    --fc-list-event-hover-bg-color: var(--surface-2);
    color: var(--text);
    font-family: var(--font);
  }

  .cal:global(.fc) :global(table),
  .cal:global(.fc) :global(thead),
  .cal:global(.fc) :global(tbody),
  .cal:global(.fc) :global(tr),
  .cal:global(.fc) :global(td),
  .cal:global(.fc) :global(th),
  .cal:global(.fc) :global(.fc-scrollgrid),
  .cal:global(.fc) :global(.fc-scrollgrid-section > *) {
    background-color: transparent;
  }
  .cal:global(.fc) :global(.fc-scrollgrid) {
    border: 0;
  }
  .cal:global(.fc) :global(a) {
    color: inherit;
    text-decoration: none;
  }

  /* Quiet weekday rail and calendar cells. */
  .cal:global(.fc) :global(.fc-col-header-cell) {
    height: 44px;
    vertical-align: middle;
    border-left-color: transparent;
    border-right-color: transparent;
    background: var(--surface);
    text-align: left;
  }
  .cal:global(.fc) :global(.fc-col-header-cell-cushion) {
    padding: 0 var(--space-4);
    color: var(--muted);
    font-size: 11px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.075em;
    text-transform: uppercase;
  }
  .cal:global(.fc) :global(.fc-daygrid-day),
  .cal:global(.fc) :global(.fc-timegrid-col) {
    background: var(--surface);
    transition: background var(--dur-fast) var(--ease);
  }
  .cal:global(.fc) :global(.fc-day-sat),
  .cal:global(.fc) :global(.fc-day-sun) {
    background: color-mix(in srgb, var(--surface-2) 28%, var(--surface));
  }
  .cal:global(.fc) :global(.fc-day-other) {
    background: color-mix(in srgb, var(--surface-2) 48%, var(--surface));
  }
  .cal:global(.fc) :global(.fc-daygrid-day:hover),
  .cal:global(.fc) :global(.fc-timegrid-col:hover) {
    background: var(--surface-hover);
  }
  .cal:global(.fc) :global(.fc-day-today) {
    background: var(--fc-today-bg-color) !important;
  }
  .cal:global(.fc) :global(.fc-daygrid-day-top) {
    flex-direction: row;
  }
  .cal:global(.fc) :global(.fc-daygrid-day-number) {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    margin: 10px 10px 6px;
    padding: 0;
    border-radius: 50%;
    color: var(--text);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    line-height: 1;
    text-align: center;
  }
  .cal:global(.fc) :global(.fc-day-other .fc-daygrid-day-number) {
    color: var(--faint);
  }
  .cal:global(.fc) :global(.fc-day-today .fc-daygrid-day-number) {
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: var(--fw-medium);
  }

  /* Upcoming runs read as compact records instead of loose coloured text. */
  .cal:global(.fc) :global(.agenda-run) {
    --run-color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--run-color) 15%, var(--surface));
    border-left: 2px solid color-mix(in srgb, var(--run-color) 75%, var(--surface));
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--run-color) 8%, var(--surface));
    color: var(--text);
    transition:
      background var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }
  .cal:global(.fc) :global(.run-tone-1) { --run-color: var(--chart-1); }
  .cal:global(.fc) :global(.run-tone-2) { --run-color: var(--chart-6); }
  .cal:global(.fc) :global(.run-tone-3) { --run-color: var(--chart-7); }
  .cal:global(.fc) :global(.agenda-run:hover) {
    border-color: color-mix(in srgb, var(--run-color) 55%, var(--surface));
    background: color-mix(in srgb, var(--run-color) 14%, var(--surface));
  }
  .cal:global(.fc) :global(.fc-daygrid-event) {
    margin: 0 8px 5px;
    padding: 6px 8px;
  }
  .cal:global(.fc) :global(.run-content) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    overflow: hidden;
    line-height: 1.35;
  }
  .cal:global(.fc) :global(.run-time) {
    flex: none;
    color: var(--muted);
    font-family: var(--mono);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    font-weight: var(--fw-normal);
  }
  .cal:global(.fc) :global(.run-name) {
    min-width: 0;
    color: var(--text);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cal:global(.fc) :global(.fc-daygrid-more-link) {
    margin: 2px 10px;
    padding: 3px 6px;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: 11px;
    font-weight: var(--fw-medium);
  }

  /* Week view keeps the same hierarchy, with restrained time rails and run blocks. */
  .cal:global(.fc) :global(.fc-timegrid-slot) {
    height: 2.5em;
  }
  .cal:global(.fc) :global(.fc-timegrid-slot-minor) {
    border-top-style: dotted;
  }
  .cal:global(.fc) :global(.fc-timegrid-slot-label-cushion),
  .cal:global(.fc) :global(.fc-timegrid-axis-cushion) {
    color: var(--dim);
    font-family: var(--mono);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .cal:global(.fc) :global(.fc-v-event) {
    padding: 5px 7px;
  }
  .cal:global(.fc) :global(.fc-v-event .run-content) {
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
  }
  .cal:global(.fc) :global(.fc-v-event .run-name) { max-width: 100%; }
  .cal:global(.fc) :global(.fc-timegrid-now-indicator-line) {
    border-color: var(--red);
  }
  .cal:global(.fc) :global(.fc-timegrid-now-indicator-arrow) {
    border-color: var(--red);
    color: var(--red);
  }

  .cal:global(.fc) :global(.fc-event:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .cal:global(.fc) :global(.fc-popover) {
    overflow: hidden;
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
  }
  .cal:global(.fc) :global(.fc-popover-header) {
    background: var(--surface-2);
    color: var(--text);
  }
  /* FC's embedded icon font is blocked by the app's font-src policy. The only
     remaining native icon is the overflow-popover close button. Draw it in CSS. */
  .cal:global(.fc) :global(.fc-icon) {
    font-family: var(--font) !important;
  }
  .cal:global(.fc) :global(.fc-icon-x) { position: relative; }
  .cal:global(.fc) :global(.fc-icon-x::before),
  .cal:global(.fc) :global(.fc-icon-x::after) {
    content: '';
    position: absolute;
    width: 12px;
    height: 1px;
    background: currentColor;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%) rotate(45deg);
  }
  .cal:global(.fc) :global(.fc-icon-x::after) {
    transform: translate(-50%, -50%) rotate(-45deg);
  }

  @media (max-width: 760px) {
    .page {
      padding: var(--space-4);
    }
  }

  @container (max-width: 780px) {
    .toolbar {
      flex-wrap: wrap;
      gap: var(--space-3);
      padding: var(--space-4);
    }
    .period-title {
      width: 100%;
    }
    .calendar-footer { padding: var(--space-3) var(--space-4); }
  }
  @container (max-width: 380px) {
    .view-switch button { padding: 0 8px; }
    .calendar-mark { display: none; }
    .period-nav { gap: 2px; }
  }
</style>
