<script>
  // Every background job in one list: scheduled broker syncs (journal, portfolio), mailbox
  // polls, and the built-in loops (FX rates, managers' portfolios). Each job is set up in
  // its own module; here it is paused, re-timed, run now, or its schedule deleted, and its
  // run log is one click away.
  import { onDestroy, onMount } from 'svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { NAV_LINKS, STATUS_TONE, fmtLocal } from '$lib/modules/automator/api.js';
  import { jobsApi, fmtInterval, intervalOptions } from '$lib/jobs/api.js';
  import { t } from '$lib/i18n';

  let jobs = $state([]);
  let loading = $state(true);
  let error = $state('');
  let busy = $state('');
  // Job whose run log is unfolded, and that log.
  let openId = $state(null);
  let runs = $state([]);
  let confirmOpen = $state(false);
  let deleting = $state(null);
  let timer = null;

  const TONE = { ...STATUS_TONE, warning: 'warn' };

  onMount(() => {
    reload();
    // A run started from here finishes in the background; follow it while it lasts.
    timer = setInterval(() => {
      if (jobs.some((j) => j.running)) reload();
    }, 4000);
  });
  onDestroy(() => clearInterval(timer));

  async function reload() {
    try {
      jobs = await jobsApi.list();
      if (openId) runs = await jobsApi.runs(openId, 20);
      error = '';
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function act(job, fn) {
    busy = job.id;
    error = '';
    try {
      await fn();
      await reload();
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  const runNow = (job) => act(job, () => jobsApi.run(job.id));
  const togglePause = (job) => act(job, () => jobsApi.update(job.id, { paused: !job.paused }));
  const setInterval_ = (job, minutes) =>
    act(job, () => jobsApi.update(job.id, { interval_minutes: Number(minutes) }));

  function askDelete(job) {
    deleting = job;
    confirmOpen = true;
  }

  async function confirmDelete() {
    const job = deleting;
    deleting = null;
    if (job) await act(job, () => jobsApi.remove(job.id));
  }

  async function toggleRuns(job) {
    if (openId === job.id) {
      openId = null;
      runs = [];
      return;
    }
    openId = job.id;
    runs = [];
    try {
      runs = await jobsApi.runs(job.id, 20);
    } catch (e) {
      error = e.message;
    }
  }

  const title = (j) => $t(`jobs.kind.${j.kind}`);
  const target = (j) => [j.target, j.account].filter(Boolean).join(' · ');
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="automator.nav.label" />
  <PageHeader title={$t('jobs.title')} subtitle={$t('jobs.subtitle')} />

  <ErrorText {error} />

  {#if loading}
    <Skeleton height="12rem" />
  {:else if !jobs.length}
    <EmptyState icon="repeat" title={$t('jobs.emptyTitle')} description={$t('jobs.emptyBody')} />
  {:else}
    <div class="tablewrap">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('jobs.colJob')}</th>
            <th>{$t('jobs.colInterval')}</th>
            <th>{$t('jobs.colLast')}</th>
            <th>{$t('jobs.colNext')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each jobs as job (job.id)}
            <tr class:paused={job.paused}>
              <td>
                <div class="name">
                  <a href={job.link}>{title(job)}</a>
                  {#if job.paused}<Badge tone="warn">{$t('jobs.paused')}</Badge>{/if}
                  {#if job.conflicts}
                    <Badge tone="warn">{$t('jobs.conflicts', { n: job.conflicts })}</Badge>
                  {/if}
                </div>
                {#if target(job)}<span class="muted small">{target(job)}</span>{/if}
              </td>
              <td class="interval">
                {#if job.interval_editable}
                  <Dropdown
                    value={job.interval_minutes}
                    options={intervalOptions($t, job.interval_minutes)}
                    ariaLabel={$t('jobs.colInterval')}
                    float
                    disabled={busy === job.id}
                    onpick={(v) => setInterval_(job, v)}
                  />
                {:else}
                  <span class="muted">{fmtInterval(job.interval_minutes, $t)}</span>
                {/if}
              </td>
              <td>
                {#if job.running}
                  <Badge tone="accent">{$t('automator.status.running')}</Badge>
                {:else if job.last_run_at}
                  <div class="last">
                    <span>{fmtLocal(job.last_run_at)}</span>
                    {#if job.last_status}
                      <Badge tone={TONE[job.last_status] ?? 'neutral'}>
                        {$t(`jobs.status.${job.last_status}`)}
                      </Badge>
                    {/if}
                  </div>
                  {#if job.last_error}
                    <span class="err" title={job.last_error}>{job.last_error}</span>
                  {:else if job.last_summary}
                    <span class="muted small sum" title={job.last_summary}>{job.last_summary}</span>
                  {/if}
                {:else}
                  <span class="muted">{$t('jobs.never')}</span>
                {/if}
              </td>
              <td>
                {#if job.paused}
                  <span class="muted">·</span>
                {:else}
                  {fmtLocal(job.next_run_at)}
                {/if}
              </td>
              <td class="actions">
                <button
                  class="icon"
                  title={$t('jobs.runNow')}
                  disabled={busy === job.id || job.running}
                  onclick={() => runNow(job)}
                >
                  <Icon name="zap" size={14} />
                </button>
                <button
                  class="icon"
                  title={job.paused ? $t('jobs.resume') : $t('jobs.pause')}
                  disabled={busy === job.id}
                  onclick={() => togglePause(job)}
                >
                  <Icon name={job.paused ? 'play' : 'pause'} size={14} />
                </button>
                {#if job.kind !== 'mailbox'}
                  <button
                    class="icon"
                    class:on={openId === job.id}
                    title={$t('jobs.history')}
                    onclick={() => toggleRuns(job)}
                  >
                    <Icon name="list" size={14} />
                  </button>
                {/if}
                {#if job.deletable}
                  <button
                    class="icon"
                    title={$t('jobs.delete')}
                    disabled={busy === job.id}
                    onclick={() => askDelete(job)}
                  >
                    <Icon name="trash" size={14} />
                  </button>
                {/if}
              </td>
            </tr>
            {#if openId === job.id}
              <tr class="log">
                <td colspan="5">
                  {#if !runs.length}
                    <span class="muted small">{$t('jobs.noRuns')}</span>
                  {:else}
                    <ul class="runs">
                      {#each runs as r (r.id)}
                        <li>
                          <span class="when">{fmtLocal(r.started_at)}</span>
                          <Badge tone={TONE[r.status] ?? 'neutral'}>{$t(`jobs.status.${r.status}`)}</Badge>
                          <span class="muted small">{$t(`jobs.trigger.${r.trigger}`)}</span>
                          <span class:err={!!r.error} class="small detail">{r.error ?? r.summary ?? ''}</span>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('jobs.deleteTitle')}
  message={$t('jobs.deleteBody', { job: deleting ? title(deleting) : '' })}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
/>

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
  .name {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .name a {
    color: var(--text);
    font-weight: var(--fw-medium);
    text-decoration: none;
  }
  .name a:hover {
    text-decoration: underline;
  }
  .small {
    font-size: var(--text-xs);
  }
  .interval {
    min-width: 150px;
  }
  .last {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    white-space: nowrap;
  }
  .sum,
  .err {
    display: block;
    max-width: 360px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .err {
    font-size: var(--text-xs);
    color: var(--red);
  }
  /* Not the interval cell: opacity makes a stacking context, which would trap the open
     dropdown under the rows below it and show them through it. */
  tr.paused td:not(.interval) {
    opacity: 0.75;
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
  .icon.on {
    color: var(--accent);
  }
  .log td {
    background: var(--surface-2);
  }
  .runs {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .runs li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .when {
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .detail {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
