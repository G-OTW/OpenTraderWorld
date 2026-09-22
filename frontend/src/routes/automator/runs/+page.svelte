<script>
  // Run history: one row per run, newest first. Live runs can be stopped from here.
  import { onMount, onDestroy } from 'svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Input from '$lib/ui/Input.svelte';
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
  let search = $state('');
  let sortKey = $state('started'); // 'started' | 'workflow' | 'status'
  let sortDir = $state(-1); // newest first
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

  // Clicking a header sorts on it; clicking it again flips the direction. Dates open
  // newest first, text opens A-Z.
  function toggleSort(key) {
    if (sortKey === key) sortDir = -sortDir;
    else (sortKey = key), (sortDir = key === 'started' ? -1 : 1);
  }

  // The search runs on what the row shows, labels included, so typing "failed" or a
  // translated trigger name matches what the eye reads.
  const query = $derived(search.trim().toLowerCase());

  const shown = $derived.by(() => {
    const q = query;
    const label = (r) => ({
      workflow: nameOf(r.workflow_id),
      status: $t(`automator.status.${r.status}`),
      trigger: $t(`automator.trigger.${r.trigger}`)
    });
    let out = runs;
    if (q) {
      out = out.filter((r) => {
        const l = label(r);
        return `${l.workflow} ${l.status} ${l.trigger} ${r.error ?? ''}`.toLowerCase().includes(q);
      });
    }
    const at = (r) => Date.parse(r.started_at) || 0;
    return [...out].sort((a, b) => {
      if (sortKey === 'workflow') {
        const d = label(a).workflow.localeCompare(label(b).workflow) * sortDir;
        if (d) return d;
      } else if (sortKey === 'status') {
        const d = label(a).status.localeCompare(label(b).status) * sortDir;
        if (d) return d;
      } else {
        return (at(a) - at(b)) * sortDir;
      }
      return at(b) - at(a); // equal keys keep the newest on top
    });
  });
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="automator.nav.label" />
  <PageHeader title={$t('automator.runs.title')} subtitle={$t('automator.runs.subtitle')}>
    {#snippet actions()}
      <div class="search">
        <Input
          bind:value={search}
          placeholder={$t('automator.runs.searchPlaceholder')}
          aria-label={$t('automator.runs.searchPlaceholder')}
        />
      </div>
      <div class="filter">
        <Select options={options} bind:value={filter} onpick={reload} />
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
            <th>
              <button class="sort" onclick={() => toggleSort('workflow')}>
                {$t('automator.runs.colWorkflow')}
                {#if sortKey === 'workflow'}
                  <Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />
                {/if}
              </button>
            </th>
            <th>
              <button class="sort" onclick={() => toggleSort('started')}>
                {$t('automator.runs.colStarted')}
                {#if sortKey === 'started'}
                  <Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />
                {/if}
              </button>
            </th>
            <th>{$t('automator.runs.colTrigger')}</th>
            <th>{$t('automator.runs.colDuration')}</th>
            <th>
              <button class="sort" onclick={() => toggleSort('status')}>
                {$t('automator.runs.colStatus')}
                {#if sortKey === 'status'}
                  <Icon name={sortDir === 1 ? 'chevron-up' : 'chevron-down'} size={11} />
                {/if}
              </button>
            </th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#if !shown.length}
            <tr><td colspan="6" class="muted small">{$t('automator.runs.noMatch')}</td></tr>
          {:else}
            {#each shown as run (run.id)}
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
                    <button
                      class="icon"
                      title={$t('automator.runs.stop')}
                      onclick={() => cancel(run)}
                    >
                      <Icon name="stop" size={14} />
                    </button>
                  {/if}
                  <a
                    class="icon"
                    href={`/automator/runs/${run.id}`}
                    title={$t('automator.runs.open')}
                  >
                    <Icon name="chevron-right" size={14} />
                  </a>
                </td>
              </tr>
            {/each}
          {/if}
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
  .search {
    min-width: 220px;
  }
  .filter {
    min-width: 200px;
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    font: inherit;
    text-transform: inherit;
    letter-spacing: inherit;
    cursor: pointer;
  }
  .sort:hover {
    color: var(--text);
  }
</style>
