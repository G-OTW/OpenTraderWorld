<script>
  // Run history: one row per run, newest first. Live runs can be stopped from here.
  import { onMount, onDestroy } from 'svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Select from '$lib/ui/Select.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import {
    automatorApi,
    NAV_LINKS,
    STATUS_TONE,
    fmtDuration,
    fmtLocal
  } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let runs = $state([]);
  let workflows = $state([]);
  let loading = $state(true);
  let error = $state('');
  let filter = $state('');
  let timer = null;

  onMount(() => {
    reload();
    // A run in flight finishes without a page reload; a slow poll is enough here.
    timer = setInterval(() => {
      if (runs.some((r) => r.status === 'running' || r.status === 'queued')) reload();
    }, 5000);
  });
  onDestroy(() => clearInterval(timer));

  async function reload() {
    try {
      const r = await automatorApi.runs({ workflow: filter || undefined });
      runs = r.runs ?? [];
      workflows = r.workflows ?? [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  const nameOf = (id) => workflows.find((w) => w.id === id)?.name ?? id;

  async function cancel(run) {
    try {
      await automatorApi.cancel(run.id);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  const options = $derived([
    { value: '', label: $t('automator.runs.allWorkflows') },
    ...workflows.map((w) => ({ value: w.id, label: w.name }))
  ]);
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="automator.nav.label" />
  <PageHeader title={$t('automator.runs.title')} subtitle={$t('automator.runs.subtitle')}>
    {#snippet actions()}
      <div class="filter">
        <Select options={options} bind:value={filter} onchange={reload} />
      </div>
    {/snippet}
  </PageHeader>

  <ErrorText error={error} />

  {#if loading}
    <Skeleton height="12rem" />
  {:else if !runs.length}
    <EmptyState
      icon="list"
      title={$t('automator.runs.emptyTitle')}
      description={$t('automator.runs.emptyBody')}
    />
  {:else}
    <div class="tablewrap">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('automator.runs.colWorkflow')}</th>
            <th>{$t('automator.runs.colStarted')}</th>
            <th>{$t('automator.runs.colTrigger')}</th>
            <th>{$t('automator.runs.colDuration')}</th>
            <th>{$t('automator.runs.colStatus')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each runs as run (run.id)}
            <tr>
              <td class="strong">
                <a href={`/automator/runs/${run.id}`}>{nameOf(run.workflow_id)}</a>
                {#if run.mode === 'test'}
                  <Badge tone="warn">{$t('automator.runs.test')}</Badge>
                {/if}
              </td>
              <td>{fmtLocal(run.started_at)}</td>
              <td>{$t(`automator.trigger.${run.trigger}`)}</td>
              <td class="mono">{fmtDuration(run.duration_ms)}</td>
              <td>
                <Badge tone={STATUS_TONE[run.status] ?? 'neutral'}>
                  {$t(`automator.status.${run.status}`)}
                </Badge>
                {#if run.error}
                  <span class="err" title={run.error}>{run.error}</span>
                {/if}
              </td>
              <td class="actions">
                {#if run.status === 'running' || run.status === 'queued'}
                  <button class="icon" title={$t('automator.runs.stop')} onclick={() => cancel(run)}>
                    <Icon name="stop" size={14} />
                  </button>
                {/if}
                <a class="icon" href={`/automator/runs/${run.id}`} title={$t('automator.runs.open')}>
                  <Icon name="chevron-right" size={14} />
                </a>
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
  .tablewrap {
    overflow-x: auto;
    border: 0.5px solid var(--border);
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
  }
  .err {
    display: block;
    font-size: var(--text-xs);
    color: var(--red);
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .filter {
    min-width: 200px;
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
</style>
