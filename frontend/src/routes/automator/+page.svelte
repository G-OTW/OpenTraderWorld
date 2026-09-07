<script>
  // Automator, workflow list. Favorites first, then most recently edited: the list is a
  // launcher, not an archive.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import ModuleNav from '$lib/ui/ModuleNav.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Input from '$lib/ui/Input.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import {
    automatorApi,
    NAV_LINKS,
    STATUS_TONE,
    describeSchedule,
    fmtLocal
  } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let workflows = $state([]);
  let schedules = $state([]);
  let lastRuns = $state({});
  let loading = $state(true);
  let error = $state('');

  let createOpen = $state(false);
  let newName = $state('');
  let creating = $state(false);

  let confirmOpen = $state(false);
  let deleting = $state(null);

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      const r = await automatorApi.list();
      workflows = r.workflows ?? [];
      schedules = r.schedules ?? [];
      lastRuns = r.last_runs ?? {};
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

  const rulesOf = (id) => schedules.filter((s) => s.workflow_id === id);
  // One card, one switch: a workflow is scheduled as long as one of its rules is active.
  const scheduled = (id) => rulesOf(id).some((r) => r.active);

  async function create() {
    if (!newName.trim()) return;
    creating = true;
    try {
      const wf = await automatorApi.create({ name: newName.trim() });
      createOpen = false;
      newName = '';
      goto(`/automator/${wf.id}`);
    } catch (e) {
      error = e.message;
    } finally {
      creating = false;
    }
  }

  async function toggleFavorite(wf) {
    try {
      const updated = await automatorApi.patch(wf.id, { favorite: !wf.favorite });
      workflows = workflows.map((w) => (w.id === wf.id ? updated : w));
    } catch (e) {
      error = e.message;
    }
  }

  async function runNow(wf) {
    try {
      await automatorApi.run(wf.id);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  async function toggleSchedules(wf) {
    const rules = rulesOf(wf.id);
    if (!rules.length) return;
    const next = !scheduled(wf.id);
    try {
      const updated = await Promise.all(
        rules.map((r) => automatorApi.updateSchedule(r.id, { active: next }))
      );
      const byId = new Map(updated.map((s) => [s.id, s]));
      schedules = schedules.map((s) => byId.get(s.id) ?? s);
    } catch (e) {
      error = e.message;
    }
  }

  async function confirmDelete() {
    confirmOpen = false;
    if (!deleting) return;
    try {
      await automatorApi.remove(deleting.id);
      await reload();
    } catch (e) {
      error = e.message;
    } finally {
      deleting = null;
    }
  }
</script>

<div class="page">
  <ModuleNav links={NAV_LINKS} label="automator.nav.label" />

  <PageHeader
    title={$t('automator.title')}
    subtitle={$t('automator.count', { count: workflows.length })}
  >
    {#snippet actions()}
      <ChannelButton module="automator" />
      <Button variant="primary" icon="plus" onclick={() => (createOpen = true)}>
        {$t('automator.new')}
      </Button>
    {/snippet}
    <p class="hint">{$t('automator.hint')}</p>
  </PageHeader>

  <ErrorText error={error} />

  {#if loading}
    <div class="grid">
      {#each Array.from({ length: 3 }, (_, i) => i) as i (i)}
        <div class="card"><Skeleton height="3.5rem" /></div>
      {/each}
    </div>
  {:else if !workflows.length}
    <EmptyState
      icon="zap"
      title={$t('automator.emptyTitle')}
      description={$t('automator.emptyBody')}
    >
      {#snippet action()}
        <Button variant="primary" icon="plus" onclick={() => (createOpen = true)}>
          {$t('automator.new')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="grid">
      {#each workflows as wf (wf.id)}
        <article class="card" class:off={!wf.enabled}>
          <header>
            <button class="star" onclick={() => toggleFavorite(wf)} title={$t('automator.favorite')}>
              <Icon name="star" size={14} />
            </button>
            <a class="name" href={`/automator/${wf.id}`}>{wf.name}</a>
            {#if lastRuns[wf.id]}
              <Badge tone={STATUS_TONE[lastRuns[wf.id].status] ?? 'neutral'}>
                {$t(`automator.status.${lastRuns[wf.id].status}`)}
              </Badge>
            {/if}
          </header>

          {#if wf.description}
            <p class="desc">{wf.description}</p>
          {/if}

          <ul class="rules">
            {#each rulesOf(wf.id) as rule (rule.id)}
              <li class:paused={!rule.active}>
                <Icon name="clock" size={12} />
                {describeSchedule(rule, $t, weekdayNames)}
                {#if rule.next_run_at && rule.active}
                  <span class="next">{fmtLocal(rule.next_run_at)}</span>
                {/if}
              </li>
            {/each}
            {#if !rulesOf(wf.id).length}
              <li class="muted">{$t('automator.noSchedule')}</li>
            {/if}
          </ul>

          <footer>
            <span class="when">
              {#if lastRuns[wf.id]}
                {$t('automator.lastRun', { at: fmtLocal(lastRuns[wf.id].at) })}
              {:else}
                {$t('automator.neverRun')}
              {/if}
            </span>
            <div class="actions">
              <button class="icon" title={$t('automator.runNow')} onclick={() => runNow(wf)}>
                <Icon name="play" size={14} />
              </button>
              {#if rulesOf(wf.id).length}
                <button
                  class="icon sched"
                  class:paused={!scheduled(wf.id)}
                  title={scheduled(wf.id) ? $t('automator.sched.pause') : $t('automator.sched.resume')}
                  onclick={() => toggleSchedules(wf)}
                >
                  <Icon name={scheduled(wf.id) ? 'pause' : 'clock'} size={14} />
                </button>
              {/if}
              <a class="icon" href={`/automator/${wf.id}`} title={$t('automator.edit')}>
                <Icon name="pencil" size={14} />
              </a>
              <button
                class="icon danger-hover"
                title={$t('common.delete')}
                onclick={() => {
                  deleting = wf;
                  confirmOpen = true;
                }}
              >
                <Icon name="trash" size={14} />
              </button>
            </div>
          </footer>
        </article>
      {/each}
    </div>
  {/if}
</div>

<Modal bind:open={createOpen} title={$t('automator.new')} size="sm">
  <Input
    label={$t('automator.name')}
    bind:value={newName}
    placeholder={$t('automator.namePlaceholder')}
    maxlength="80"
  />
  {#snippet footer()}
    <Button onclick={() => (createOpen = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" loading={creating} onclick={create}>{$t('automator.create')}</Button>
  {/snippet}
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('automator.deleteTitle')}
  message={$t('automator.deleteBody', { name: deleting?.name ?? '' })}
  confirmLabel={$t('common.delete')}
  onconfirm={confirmDelete}
/>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
  }
  .hint {
    margin: var(--space-2) 0 0;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: var(--space-3);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: 0.5px solid var(--border);
    background: var(--surface);
    padding: var(--space-3);
  }
  .card.off {
    opacity: 0.6;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .star {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
  }
  .card:not(.off) .star:hover {
    color: var(--amber);
  }
  .name {
    font-weight: var(--fw-medium);
    color: var(--text);
    text-decoration: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name:hover {
    text-decoration: underline;
  }
  .desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--dim);
  }
  .rules {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .rules li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .rules li.paused {
    opacity: 0.5;
  }
  .rules li.muted {
    color: var(--muted);
  }
  .next {
    margin-left: auto;
    font-family: var(--mono);
    color: var(--muted);
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-2);
    margin-top: auto;
  }
  .when {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .actions {
    display: flex;
    gap: var(--space-1);
  }
  .icon.sched.paused {
    color: var(--amber);
  }
</style>
