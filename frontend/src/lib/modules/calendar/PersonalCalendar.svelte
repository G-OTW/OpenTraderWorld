<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Personal calendar built on FullCalendar with year (multi-month) / month / week /
  // day views and zoom-in/out controls. Events are fetched per visible range and
  // CRUD'd through the parent via callbacks.
  import { onMount, onDestroy } from 'svelte';
  import { Calendar } from '@fullcalendar/core';
  import dayGridPlugin from '@fullcalendar/daygrid';
  import timeGridPlugin from '@fullcalendar/timegrid';
  import multiMonthPlugin from '@fullcalendar/multimonth';
  import interactionPlugin from '@fullcalendar/interaction';
  import { t } from '$lib/i18n';

  let {
    // FullCalendar event objects (already mapped via toFcEvent).
    events = [],
    // (start ISO, end ISO) -> void  — called when the visible range changes.
    onrange = () => {},
    // (event) -> void  — clicked an existing event.
    oneventclick = () => {},
    // ({ startStr, endStr, allDay }) -> void  — selected an empty slot/day.
    onselect = () => {},
    // (event) -> void  — dragged/resized an existing event.
    onchange = () => {}
  } = $props();

  // Zoom ladder, coarse -> fine.
  const VIEWS = [
    { view: 'multiMonthYear', labelKey: 'calendar.toolbar.year' },
    { view: 'dayGridMonth', labelKey: 'calendar.toolbar.month' },
    { view: 'timeGridWeek', labelKey: 'calendar.toolbar.week' },
    { view: 'timeGridDay', labelKey: 'calendar.toolbar.day' }
  ];
  // Persist the chosen view across reloads (Year/Month/Week/Day).
  const VIEW_KEY = 'otw.calendar.viewIndex';
  function loadViewIndex() {
    if (typeof localStorage === 'undefined') return 1; // default: Month
    const n = parseInt(localStorage.getItem(VIEW_KEY) ?? '', 10);
    return Number.isInteger(n) && n >= 0 && n < VIEWS.length ? n : 1;
  }
  let viewIndex = $state(loadViewIndex());

  let el;
  let cal;

  onMount(() => {
    cal = new Calendar(el, {
      plugins: [dayGridPlugin, timeGridPlugin, multiMonthPlugin, interactionPlugin],
      initialView: VIEWS[viewIndex].view,
      headerToolbar: false, // we render our own controls
      height: '100%',
      firstDay: 1,
      nowIndicator: true,
      selectable: true,
      editable: true,
      dayMaxEvents: true,
      multiMonthMaxColumns: 3,
      events,
      datesSet: (info) => onrange(info.start.toISOString(), info.end.toISOString()),
      eventClick: (info) => oneventclick(info.event),
      select: (info) => onselect({ startStr: info.startStr, endStr: info.endStr, allDay: info.allDay }),
      eventDrop: (info) => onchange(info.event),
      eventResize: (info) => onchange(info.event)
    });
    cal.render();
    syncTitle();
  });

  onDestroy(() => cal?.destroy());

  // Re-feed events whenever the parent updates them.
  $effect(() => {
    if (!cal) return;
    cal.removeAllEvents();
    cal.addEventSource(events);
  });

  let title = $state('');
  function syncTitle() {
    title = cal?.view?.title ?? '';
  }

  function setView(i) {
    viewIndex = i;
    if (typeof localStorage !== 'undefined') localStorage.setItem(VIEW_KEY, String(i));
    cal.changeView(VIEWS[i].view);
    syncTitle();
  }
  function zoomIn() {
    if (viewIndex < VIEWS.length - 1) setView(viewIndex + 1);
  }
  function zoomOut() {
    if (viewIndex > 0) setView(viewIndex - 1);
  }
  function prev() {
    cal.prev();
    syncTitle();
  }
  function next() {
    cal.next();
    syncTitle();
  }
  function today() {
    cal.today();
    syncTitle();
  }
</script>

<div class="cal-wrap">
  <div class="toolbar">
    <div class="nav">
      <div class="seg">
        <button class="btn arrow" onclick={prev} title={$t('calendar.toolbar.previous')} aria-label={$t('calendar.toolbar.previous')}><Icon name="chevron-left" size={15} /></button>
        <button class="btn today" onclick={today}>{$t('calendar.toolbar.today')}</button>
        <button class="btn arrow" onclick={next} title={$t('calendar.toolbar.next')} aria-label={$t('calendar.toolbar.next')}><Icon name="chevron-right" size={15} /></button>
      </div>
      <span class="title">{title}</span>
    </div>
    <div class="zoom">
      <button class="btn round" onclick={zoomOut} disabled={viewIndex === 0} title={$t('calendar.toolbar.zoomOut')} aria-label={$t('calendar.toolbar.zoomOut')}><Icon name="minus" size={13} /></button>
      <div class="views">
        {#each VIEWS as v, i}
          <button class="chip" class:active={viewIndex === i} onclick={() => setView(i)}>
            {$t(v.labelKey)}
          </button>
        {/each}
      </div>
      <button
        class="btn round"
        onclick={zoomIn}
        disabled={viewIndex === VIEWS.length - 1}
        title={$t('calendar.toolbar.zoomIn')}
        aria-label={$t('calendar.toolbar.zoomIn')}><Icon name="plus" size={13} /></button
      >
    </div>
  </div>
  <div class="fc-host" bind:this={el}></div>
</div>

<style>
  .cal-wrap {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    gap: var(--space-4);
  }

  /* ── Toolbar ───────────────────────────────────────────────────────────── */
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .nav,
  .zoom {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .title {
    font-weight: 600;
    font-size: 1.05rem;
    letter-spacing: -0.01em;
    color: var(--text);
  }
  /* Segmented prev / today / next group. */
  .seg {
    display: inline-flex;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px;
    gap: 2px;
  }
  .seg .btn {
    border: none;
    background: transparent;
    border-radius: 999px;
  }
  .btn {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: 999px;
    padding: 7px 12px;
    cursor: pointer;
    font-size: 0.85rem;
    line-height: 1;
    transition: background 0.12s, border-color 0.12s, color 0.12s, transform 0.06s;
  }
  .btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--text);
  }
  .btn:active:not(:disabled) {
    transform: scale(0.96);
  }
  .btn:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .btn.arrow {
    font-size: 1.1rem;
    padding: 5px 12px;
  }
  .btn.today {
    font-weight: 600;
  }
  .btn.round {
    width: 32px;
    height: 32px;
    padding: 0;
    display: grid;
    place-items: center;
    font-size: 1.1rem;
  }
  /* Zoom view picker: a pill-shaped segmented control with a sliding accent. */
  .views {
    display: inline-flex;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 3px;
    gap: 2px;
  }
  .chip.active {
    background: var(--accent);
    color: #fff;
    box-shadow: 0 1px 6px color-mix(in srgb, var(--accent) 45%, transparent);
  }

  /* ── Calendar surface ──────────────────────────────────────────────────── */
  .fc-host {
    flex: 1;
    min-height: 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    overflow: auto;
  }

  /* ── Monday-style theming ──────────────────────────────────────────────────
     Calm dark surfaces, soft hairline borders, subtle hovers and refined type.
     We tune --fc-* tokens AND a handful of structural rules (cell padding, header
     pills, event geometry). The earlier "tokens only" caution was about wholesale
     scrollgrid rewrites — these targeted tweaks keep FC's layout math intact. */
  /* NOTE: FullCalendar mounts ON the `.fc-host` element itself, so the host node
     ends up with BOTH classes: `fc-host fc`. Use `.fc-host:global(.fc)` (no
     descendant combinator) so these token overrides actually match — and so they
     out-specify FC's own `:root{--fc-*}` block, which is only (0,1,0). */
  .fc-host:global(.fc) {
    /* Softer hairline borders instead of the harsh default grey. */
    --fc-border-color: color-mix(in srgb, var(--border) 70%, transparent);
    /* Dark surface for everything FC paints with the "page" colour (incl. the
       multimonth daygrid that was rendering white). */
    --fc-page-bg-color: var(--surface);
    --fc-neutral-bg-color: var(--surface-2);
    --fc-today-bg-color: color-mix(in srgb, var(--accent) 14%, transparent);
    --fc-event-bg-color: var(--accent);
    --fc-event-border-color: transparent;
    --fc-event-text-color: #fff;
    --fc-now-indicator-color: var(--red);
    --fc-list-event-hover-bg-color: var(--surface-2);
    color: var(--text);
    font-family: var(--font);
  }

  /* ── Force every FC surface dark ────────────────────────────────────────────
     FC's light theme paints tables/cells white via several rules. Rather than
     chase each one, blanket every grid container + cell transparent so the dark
     .fc-host shows through, then re-apply specific surfaces (daygrid, today)
     below. `!important` here is deliberate: FC injects its stylesheet at runtime
     into <head>, so it can win same-specificity battles otherwise. */
  .fc-host :global(.fc),
  .fc-host:global(.fc) :global(table),
  .fc-host:global(.fc) :global(thead),
  .fc-host:global(.fc) :global(tbody),
  .fc-host:global(.fc) :global(tr),
  .fc-host:global(.fc) :global(td),
  .fc-host:global(.fc) :global(th),
  .fc-host:global(.fc) :global(.fc-scrollgrid),
  .fc-host:global(.fc) :global(.fc-scrollgrid-section > *),
  .fc-host:global(.fc) :global(.fc-scrollgrid-sync-table),
  .fc-host:global(.fc) :global(.fc-daygrid-body),
  .fc-host:global(.fc) :global(.fc-multimonth-header),
  .fc-host:global(.fc) :global(.fc-multimonth-header-table),
  .fc-host:global(.fc) :global(.fc-multimonth-daygrid-table) {
    background-color: transparent !important;
  }

  /* The month card itself: a calm dark panel. */
  .fc-host:global(.fc) :global(.fc-multimonth-daygrid) {
    background-color: var(--surface-2) !important;
  }

  .fc-host:global(.fc) :global(a) {
    color: inherit;
    text-decoration: none;
  }

  /* Column headers (Mon/Tue/…): quiet uppercase labels, no heavy underline. */
  .fc-host:global(.fc) :global(.fc-col-header-cell) {
    background: transparent;
    border-bottom-color: transparent;
    padding: 6px 0;
  }
  .fc-host:global(.fc) :global(.fc-col-header-cell-cushion) {
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 0.68rem;
    font-weight: 700;
    padding: 4px 8px;
  }

  /* Day numbers: light weight, generous breathing room. */
  .fc-host:global(.fc) :global(.fc-daygrid-day-top) {
    flex-direction: row;
  }
  .fc-host:global(.fc) :global(.fc-daygrid-day-number) {
    color: var(--text);
    font-weight: 500;
    font-size: 0.8rem;
    padding: 6px 8px;
    opacity: 0.85;
  }
  .fc-host:global(.fc) :global(.fc-day-other .fc-daygrid-day-number) {
    color: var(--muted);
    opacity: 0.5;
  }
  /* Today: a soft accent pill around the number rather than a flooded cell. */
  .fc-host:global(.fc) :global(.fc-day-today) {
    background: var(--fc-today-bg-color) !important;
  }
  .fc-host:global(.fc) :global(.fc-day-today .fc-daygrid-day-number) {
    color: #fff;
    opacity: 1;
    font-weight: 700;
  }

  /* Hover affordance on month/week cells. */
  .fc-host:global(.fc) :global(.fc-daygrid-day:hover),
  .fc-host:global(.fc) :global(.fc-timegrid-col:hover) {
    background: color-mix(in srgb, var(--text) 3%, transparent);
  }

  /* Events: rounded, soft, with a left accent edge — Monday-ish chips. */
  .fc-host:global(.fc) :global(.fc-event) {
    cursor: pointer;
    border: none;
    border-radius: 6px;
    padding: 1px 0;
    font-size: 0.76rem;
    font-weight: 500;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: filter 0.12s, transform 0.06s;
  }
  .fc-host:global(.fc) :global(.fc-event:hover) {
    filter: brightness(1.08);
  }
  .fc-host:global(.fc) :global(.fc-event:active) {
    transform: scale(0.99);
  }
  .fc-host:global(.fc) :global(.fc-daygrid-event) {
    margin: 1px 4px;
    padding: 2px 7px;
  }
  .fc-host:global(.fc) :global(.fc-h-event .fc-event-title),
  .fc-host:global(.fc) :global(.fc-timegrid-event .fc-event-title) {
    font-weight: 500;
  }
  /* Dot events (all-day in list rows) get an accent dot. */
  .fc-host:global(.fc) :global(.fc-daygrid-dot-event:hover) {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }

  .fc-host:global(.fc) :global(.fc-daygrid-more-link) {
    color: var(--accent);
    font-weight: 600;
    font-size: 0.72rem;
    margin: 0 4px;
  }

  /* Completed ToDo overlays: dimmed + struck through. */
  .fc-host:global(.fc) :global(.otw-overlay-done) {
    opacity: 0.55;
  }
  .fc-host:global(.fc) :global(.otw-overlay-done .fc-event-title) {
    text-decoration: line-through;
  }

  /* ── Year (multi-month) view — was rendering white. ──────────────────────── */
  .fc-host:global(.fc) :global(.fc-multimonth) {
    background: transparent;
    border-color: transparent;
  }
  .fc-host:global(.fc) :global(.fc-multimonth-month) {
    padding: 0 var(--space-3) var(--space-6);
  }
  .fc-host:global(.fc) :global(.fc-multimonth-title) {
    color: var(--text);
    font-weight: 700;
    font-size: 0.95rem;
    padding: 0 0 var(--space-2);
    background: transparent;
  }
  /* Card border/clip lives here; background is forced dark in the blanket rule above. */
  .fc-host:global(.fc) :global(.fc-multimonth-daygrid) {
    border: 1px solid var(--fc-border-color);
    border-radius: var(--radius);
    overflow: hidden;
  }
  /* Compact day numbers inside the year grid. */
  .fc-host:global(.fc) :global(.fc-multimonth .fc-daygrid-day-number) {
    font-size: 0.72rem;
    padding: 3px 5px;
  }

  /* ── Time grid (week / day) ──────────────────────────────────────────────── */
  .fc-host:global(.fc) :global(.fc-timegrid-slot) {
    height: 2.4em;
  }
  .fc-host:global(.fc) :global(.fc-timegrid-slot-label-cushion),
  .fc-host:global(.fc) :global(.fc-timegrid-axis-cushion) {
    color: var(--muted);
    font-size: 0.7rem;
  }
  /* Minor (half-hour) lines barely visible; major lines a touch stronger. */
  .fc-host:global(.fc) :global(.fc-timegrid-slot-minor) {
    border-top-style: none;
  }
  .fc-host:global(.fc) :global(.fc-timegrid-now-indicator-line) {
    border-color: var(--red);
  }
  .fc-host:global(.fc) :global(.fc-timegrid-now-indicator-arrow) {
    border-color: var(--red);
    color: var(--red);
  }

  /* Selection highlight when dragging across cells. */
  .fc-host:global(.fc) :global(.fc-highlight) {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }

  /* Popover (the "+N more" panel) themed to the app surface. */
  .fc-host:global(.fc) :global(.fc-popover) {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.5);
  }
  .fc-host:global(.fc) :global(.fc-popover-header) {
    background: var(--surface);
    color: var(--text);
    border-radius: var(--radius) var(--radius) 0 0;
  }

  /* Thin, dark scrollbars to match the app chrome. */
  .fc-host::-webkit-scrollbar,
  .fc-host :global(.fc-scroller::-webkit-scrollbar) {
    width: 9px;
    height: 9px;
  }
  .fc-host::-webkit-scrollbar-thumb,
  .fc-host :global(.fc-scroller::-webkit-scrollbar-thumb) {
    background: var(--border);
    border-radius: 999px;
  }
  .fc-host::-webkit-scrollbar-track,
  .fc-host :global(.fc-scroller::-webkit-scrollbar-track) {
    background: transparent;
  }
</style>
