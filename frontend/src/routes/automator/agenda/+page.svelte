<script>
  // The agenda: what is going to run, and when. Occurrences are materialized server-side,
  // so the calendar shows the same instants the scheduler will act on rather than a second
  // implementation of the timezone maths.
  import { onMount, onDestroy } from 'svelte';
  import { Calendar } from '@fullcalendar/core';
  import dayGridPlugin from '@fullcalendar/daygrid';
  import timeGridPlugin from '@fullcalendar/timegrid';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { automatorApi, NAV_LINKS } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let el = $state(null);
  let cal = null;
  let error = $state('');
  let truncated = $state(false);
  let workflows = $state([]);
  let filter = $state('');
  let range = $state(null);

  onMount(() => {
    cal = new Calendar(el, {
      plugins: [dayGridPlugin, timeGridPlugin],
      initialView: 'dayGridMonth',
      headerToolbar: { left: 'prev,next today', center: 'title', right: 'dayGridMonth,timeGridWeek' },
      height: '100%',
      firstDay: 1,
      nowIndicator: true,
      datesSet: (info) => {
        range = { from: info.start.toISOString(), to: info.end.toISOString() };
        refresh();
      }
    });
    cal.render();
  });

  onDestroy(() => cal?.destroy());

  async function refresh() {
    if (!range) return;
    try {
      const r = await automatorApi.agenda(range.from, range.to, filter || null);
      workflows = r.workflows ?? workflows;
      truncated = !!r.truncated;
      cal.removeAllEvents();
      cal.addEventSource(
        (r.events ?? []).map((e) => ({
          title: e.workflow,
          start: e.at,
          allDay: false,
          url: `/automator/${e.workflow_id}`
        }))
      );
    } catch (e) {
      error = e.message;
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
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="automator.nav.label" />
  <PageHeader title={$t('automator.agenda.title')} subtitle={$t('automator.agenda.subtitle')}>
    {#snippet actions()}
      <div class="filter">
        <Select options={options} bind:value={filter} onchange={refresh} />
      </div>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} />
  {#if truncated}
    <p class="hint">{$t('automator.agenda.truncated')}</p>
  {/if}

  <div class="cal" bind:this={el}></div>
</div>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    min-height: 0;
  }
  .cal {
    flex: 1;
    min-height: 0;
  }
  .filter {
    min-width: 200px;
  }
  .hint {
    margin: 0 0 var(--space-2);
    font-size: var(--text-xs);
    color: var(--amber-ink, var(--amber));
  }
</style>
