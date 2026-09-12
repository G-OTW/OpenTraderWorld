<script>
  // One run, step by step.
  import { onMount, onDestroy } from 'svelte';
  import { page } from '$app/stores';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import RunTrace from '$lib/modules/automator/RunTrace.svelte';
  import {
    automatorApi,
    NAV_LINKS,
    STATUS_TONE,
    fmtDuration,
    fmtLocal
  } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  const id = $derived($page.params.id);

  let run = $state(null);
  let steps = $state([]);
  let workflow = $state(null);
  let loading = $state(true);
  let error = $state('');
  let timer = null;

  onMount(() => {
    reload();
    timer = setInterval(() => {
      if (run && (run.status === 'running' || run.status === 'queued')) reload();
    }, 3000);
  });
  onDestroy(() => clearInterval(timer));

  async function reload() {
    try {
      const r = await automatorApi.runDetail(id);
      run = r.run;
      steps = r.steps ?? [];
      workflow = r.workflow;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function cancel() {
    try {
      await automatorApi.cancel(id);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="automator.nav.label" />

  {#if loading}
    <Skeleton height="12rem" />
  {:else if run}
    <PageHeader
      title={workflow?.name ?? $t('automator.runs.title')}
      subtitle={fmtLocal(run.started_at)}
    >
      {#snippet actions()}
        <a class="back" href="/automator/runs">
          <Icon name="arrow-left" size={14} />
          {$t('automator.runs.back')}
        </a>
        {#if run.status === 'running' || run.status === 'queued'}
          <Button icon="stop" onclick={cancel}>{$t('automator.runs.stop')}</Button>
        {/if}
      {/snippet}
      <div class="meta">
        <Badge tone={STATUS_TONE[run.status] ?? 'neutral'}>
          {$t(`automator.status.${run.status}`)}
        </Badge>
        <span>{$t(`automator.trigger.${run.trigger}`)}</span>
        <span class="mono">{fmtDuration(run.duration_ms)}</span>
        {#if run.mode === 'test'}
          <Badge tone="warn">{$t('automator.runs.test')}</Badge>
        {/if}
      </div>
      {#if run.error}
        <p class="err">{run.error}</p>
      {/if}
    </PageHeader>

    <ErrorText error={error} />
    <RunTrace {steps} runId={run.id} />
  {:else}
    <ErrorText error={error} />
  {/if}
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-sm);
    color: var(--dim);
    text-decoration: none;
    margin-right: auto;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-2);
    font-size: var(--text-sm);
    color: var(--dim);
  }
  .mono {
    font-family: var(--mono);
  }
  .err {
    margin: var(--space-2) 0 0;
    color: var(--red);
    font-size: var(--text-sm);
  }
</style>
