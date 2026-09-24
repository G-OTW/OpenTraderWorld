<script>
  // Every schedule of every workflow, in one table: what runs, on what rule, and when next.
  // Pausing lives here too, because "stop this thing" should never require opening an editor.
  import { onMount } from 'svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import {
    automatorApi,
    NAV_LINKS,
    describeSchedule,
    fmtLocal
  } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let schedules = $state([]);
  let workflows = $state([]);
  let loading = $state(true);
  let error = $state('');

  onMount(reload);

  async function reload() {
    loading = true;
    try {
      const r = await automatorApi.schedules();
      schedules = r.schedules ?? [];
      workflows = r.workflows ?? [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  const weekdayNames = $derived([
    $t('common.weekday.mon'),
    $t('common.weekday.tue'),
    $t('common.weekday.wed'),
    $t('common.weekday.thu'),
    $t('common.weekday.fri'),
    $t('common.weekday.sat'),
    $t('common.weekday.sun')
  ]);

  const nameOf = (id) => workflows.find((w) => w.id === id)?.name ?? id;

  async function toggle(s) {
    try {
      await automatorApi.updateSchedule(s.id, { active: !s.active });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove(s) {
    try {
      await automatorApi.deleteSchedule(s.id);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="automator.nav.label" />
  <PageHeader
    title={$t('automator.schedules.title')}
    subtitle={$t('automator.schedules.count', { count: schedules.length })}
  >
    <p class="hint">{$t('automator.schedules.hint')}</p>
  </PageHeader>

  <ErrorText error={error} />

  {#if loading}
    <Skeleton height="12rem" />
  {:else if !schedules.length}
    <EmptyState
      icon="clock"
      title={$t('automator.schedules.emptyTitle')}
      description={$t('automator.schedules.emptyBody')}
    />
  {:else}
    <div class="tablewrap">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('automator.schedules.colWorkflow')}</th>
            <th>{$t('automator.schedules.colRule')}</th>
            <th>{$t('automator.schedules.colTimezone')}</th>
            <th>{$t('automator.schedules.colNext')}</th>
            <th>{$t('automator.schedules.colLast')}</th>
            <th>{$t('automator.schedules.colState')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each schedules as s (s.id)}
            <tr class:off={!s.active}>
              <td class="strong">
                <a href={`/automator/${s.workflow_id}`}>{nameOf(s.workflow_id)}</a>
              </td>
              <td>{describeSchedule(s, $t, weekdayNames)}</td>
              <td class="mono">{s.timezone}</td>
              <td>{s.active ? fmtLocal(s.next_run_at) : '-'}</td>
              <td>{s.last_run_at ? fmtLocal(s.last_run_at) : $t('automator.never')}</td>
              <td>
                <Badge tone={s.active ? 'success' : 'neutral'}>
                  {s.active ? $t('automator.sched.active') : $t('automator.sched.paused')}
                </Badge>
                {#if s.catch_up}
                  <Badge tone="accent">{$t('automator.sched.catchUpShort')}</Badge>
                {/if}
              </td>
              <td class="actions">
                <button
                  class="icon"
                  title={s.active ? $t('automator.sched.pause') : $t('automator.sched.resume')}
                  onclick={() => toggle(s)}
                >
                  <Icon name={s.active ? 'pause' : 'play'} size={14} />
                </button>
                <button class="icon danger-hover" title={$t('common.delete')} onclick={() => remove(s)}>
                  <Icon name="trash" size={14} />
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
  }
  .hint {
    margin: var(--space-2) 0 0;
  }
  .tablewrap {
    overflow-x: auto;
    border: 0.5px solid var(--border);
  }
  tr.off td {
    opacity: 0.6;
  }
  .strong a {
    color: var(--text);
    font-weight: var(--fw-medium);
    text-decoration: none;
  }
  .strong a:hover {
    text-decoration: underline;
  }
  .mono {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
</style>
