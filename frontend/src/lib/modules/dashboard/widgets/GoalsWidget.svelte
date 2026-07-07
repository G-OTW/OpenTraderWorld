<script>
  // Goals widget: a short scrollable list of goals with progress bars, plus a quick
  // add-goal form (name only; opens minimal). Config: { limit }.
  import { goalsApi, progress, daysLeft } from '$lib/modules/goals/api.js';
  import { t } from '$lib/i18n';

  let { item, editing } = $props();
  const limit = $derived(item.config?.limit ?? 8);

  let goals = $state(null);
  let err = $state('');
  let adding = $state(false);
  let newName = $state('');

  async function load() {
    err = '';
    try {
      goals = await goalsApi.list();
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const shown = $derived((goals ?? []).slice(0, limit));

  async function add() {
    const name = newName.trim();
    if (!name) return;
    adding = true;
    try {
      const g = await goalsApi.add({ name, kpis: [] });
      goals = [g, ...(goals ?? [])];
      newName = '';
    } catch (e) {
      err = e.message;
    } finally {
      adding = false;
    }
  }
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.goals.preview')}</p>
{:else if err}
  <p class="err">{err}</p>
{:else}
  <form class="add" onsubmit={(e) => { e.preventDefault(); add(); }}>
    <input bind:value={newName} placeholder={$t('dashboard.widgets.goals.addPlaceholder')} disabled={adding} />
    <button class="go" disabled={adding || !newName.trim()}>{$t('dashboard.widgets.goals.add')}</button>
  </form>
  {#if goals === null}
    <p class="hint">{$t('common.loading')}</p>
  {:else if shown.length === 0}
    <p class="hint">{$t('dashboard.widgets.goals.empty')}</p>
  {:else}
    <ul class="list">
      {#each shown as g (g.id)}
        {@const pct = Math.round(progress(g.kpis ?? []) * 100)}
        {@const dl = daysLeft(g.deadline)}
        <li class="row">
          <div class="top">
            <span class="name">{g.name}</span>
            <span class="pct">{pct}%</span>
          </div>
          <div class="bar"><span style:width={`${pct}%`}></span></div>
          {#if dl !== null}<span class="dl" class:over={dl < 0}>{dl < 0 ? $t('dashboard.widgets.goals.overdue', { days: -dl }) : $t('dashboard.widgets.goals.daysLeft', { days: dl })}</span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
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
  .add {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .add input {
    flex: 1;
    min-width: 0;
  }
  .go {
    flex-shrink: 0;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .top {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: 0.82rem;
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pct {
    color: var(--muted);
    flex-shrink: 0;
  }
  .bar {
    height: 5px;
    border-radius: 3px;
    background: var(--surface-2);
    margin-top: 3px;
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .dl {
    font-size: 0.7rem;
    color: var(--muted);
  }
  .dl.over {
    color: var(--red);
  }
</style>
