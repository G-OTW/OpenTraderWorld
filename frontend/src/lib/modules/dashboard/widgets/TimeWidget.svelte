<script>
  // Time tracker widget: start/stop project timers from the dashboard. Config: { project_id }
  // — when set, shows just that project; otherwise lists all projects. Running timers tick.
  import { timeApi, fmtDuration } from '$lib/modules/time/api.js';
  import { t } from '$lib/i18n';

  let { item, editing } = $props();
  const only = $derived(item.config?.project_id || null);

  let projects = $state(null);
  let err = $state('');
  let loadedAt = $state(0);
  let now = $state(Date.now());

  async function load() {
    err = '';
    try {
      projects = await timeApi.listProjects();
      loadedAt = Date.now();
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  // Tick once a second while something runs so the running timer advances.
  $effect(() => {
    if (editing || !(projects ?? []).some((p) => p.running)) return;
    const h = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(h);
  });

  const shown = $derived(only ? (projects ?? []).filter((p) => p.id === only) : (projects ?? []));

  function liveSeconds(p) {
    if (!p.running) return p.tracked_seconds;
    return p.tracked_seconds + Math.max(now - loadedAt, 0) / 1000;
  }

  async function toggle(p) {
    try {
      if (p.running) await timeApi.stop(p.id);
      else await timeApi.start(p.id);
      await load();
    } catch (e) {
      err = e.message;
    }
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.time.preview')}</p>
{:else if err}
  <p class="err">{err}</p>
{:else if projects === null}
  <p class="hint">{$t('common.loading')}</p>
{:else if shown.length === 0}
  <p class="hint">{$t('dashboard.widgets.time.empty')}</p>
{:else}
  <ul class="list">
    {#each shown as p (p.id)}
      <li class="row">
        <button class="toggle" class:on={p.running} onclick={() => toggle(p)} aria-label={p.running ? $t('dashboard.widgets.time.stop') : $t('dashboard.widgets.time.start')}>
          {p.running ? '■' : '▶'}
        </button>
        <span class="name">{p.name}</span>
        <span class="time" class:run={p.running}>{fmtDuration(Math.floor(liveSeconds(p)))}</span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .hint,
  .err {
    font-size: 0.82rem;
    color: var(--muted);
  }
  .err {
    color: var(--red);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.85rem;
  }
  .toggle {
    flex-shrink: 0;
    width: 26px;
    height: 26px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
    font-size: 0.7rem;
  }
  .toggle.on {
    color: #fff;
    background: var(--green);
    border-color: var(--green);
  }
  .name {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .time {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    flex-shrink: 0;
  }
  .time.run {
    color: var(--green);
  }
</style>
