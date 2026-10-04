<script>
  // Goals widgets. One component, seven presentations selected by the saved `variant`:
  // the quick list (with inline add), the status donut, average progress, the due-soon
  // urgency buckets, the overdue list, the category donut and a single goal's progress.
  //
  // All of it comes from the module's own `goals.list()` — the Goals module is untouched.
  import { goalsApi, progress, daysLeft, status } from '$lib/modules/goals/api.js';
  import { t } from '$lib/i18n';
  import { fmtDate, fmtNum } from '$lib/format';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import WidgetState from './WidgetState.svelte';
  import Donut from './parts/Donut.svelte';
  import Stat from './parts/Stat.svelte';
  import Gauge from './parts/Gauge.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  let { item, editing } = $props();
  const limit = $derived(item.config?.limit ?? 8);
  const variant = $derived(item.config?.variant ?? item.config?.scope ?? 'open');
  const scope = $derived(item.config?.scope ?? variant);

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

  const all = $derived(goals ?? []);
  const openGoals = $derived(all.filter((g) => status(g) === 'open'));
  const reached = $derived(all.filter((g) => status(g) === 'reached'));
  const overdue = $derived(all.filter((g) => status(g) === 'overdue'));

  const avgProgress = $derived(
    openGoals.length
      ? Math.round((openGoals.reduce((s, g) => s + progress(g.kpis ?? []), 0) / openGoals.length) * 100)
      : 0
  );

  // Urgency of the still-running goals, in the mockup's four horizons.
  const dueBuckets = $derived.by(() => {
    const b = [0, 0, 0, 0];
    for (const g of openGoals) {
      const d = daysLeft(g.deadline);
      if (d === null) continue;
      if (d <= 7) b[0]++;
      else if (d <= 30) b[1]++;
      else if (d <= 90) b[2]++;
      else b[3]++;
    }
    return [
      { icon: 'zap', label: $t('dashboard.widgets.goals.next7'), value: b[0] },
      { icon: 'clock', label: $t('dashboard.widgets.goals.in8to30'), value: b[1] },
      { icon: 'calendar', label: $t('dashboard.widgets.goals.in1to3m'), value: b[2] },
      { icon: 'timer', label: $t('dashboard.widgets.goals.later'), value: b[3] }
    ];
  });

  const categories = $derived.by(() => {
    const counts = new Map();
    for (const g of all) {
      const key = g.category?.trim() || $t('dashboard.widgets.goals.uncategorized');
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return [...counts].map(([label, value]) => ({ label, value })).sort((a, b) => b.value - a.value);
  });

  // The single-goal card: the configured goal, else the nearest deadline still running.
  const focus = $derived(
    all.find((g) => g.id === item.config?.goalId) ??
      [...openGoals].sort((a, b) => (a.deadline ?? '9999').localeCompare(b.deadline ?? '9999'))[0] ??
      null
  );
  // KPI targets are numeric when the goal is quantified; a checklist goal has none, and
  // then the ring stands on its own rather than inventing a total.
  const focusTotals = $derived.by(() => {
    const kpis = focus?.kpis ?? [];
    let current = 0;
    let target = 0;
    let numeric = false;
    for (const k of kpis) {
      const tgt = Number(k.target);
      if (!Number.isFinite(tgt) || tgt === 0) continue;
      numeric = true;
      target += tgt;
      current += Number(k.current) || 0;
    }
    return numeric ? { current, target } : null;
  });
  const focusPct = $derived(focus ? Math.round(progress(focus.kpis ?? []) * 100) : 0);
  // "In 2y 4m" — a deadline far out reads better in years and months than in days.
  const focusLeft = $derived.by(() => {
    const d = daysLeft(focus?.deadline);
    if (d === null) return '';
    if (d < 0) return $t('dashboard.widgets.goals.overdue', { days: -d });
    if (d < 60) return $t('dashboard.widgets.goals.daysLeft', { days: d });
    const months = Math.round(d / 30.44);
    if (months < 12) return $t('dashboard.widgets.goals.inMonths', { months });
    return $t('dashboard.widgets.goals.inYears', { years: Math.floor(months / 12), months: months % 12 });
  });

  const shown = $derived.by(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const inWeek = new Date(today);
    inWeek.setDate(inWeek.getDate() + 7);
    return all
      .filter((goal) => {
        const deadline = goal.deadline ? new Date(`${goal.deadline}T00:00:00`) : null;
        if (scope === 'overdue') return status(goal) === 'overdue';
        if (scope === 'due') return status(goal) === 'open' && deadline && deadline >= today && deadline <= inWeek;
        return status(goal) === 'open';
      })
      .sort((a, b) => (a.deadline ?? '9999').localeCompare(b.deadline ?? '9999'))
      .slice(0, limit);
  });

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

  const listVariant = $derived(scope === 'open');
</script>

{#if listVariant}
  <!-- The add form is a live control, not content, so it sits outside WidgetState: it must
       stay usable while the list below is still loading or empty. -->
  {#if editing}
    <p class="w-state">{$t('dashboard.widgets.goals.preview')}</p>
  {:else if err}
    <ErrorText error={err} compact />
  {:else}
    <div class="w-body">
      <form class="add" onsubmit={(e) => { e.preventDefault(); add(); }}>
        <input bind:value={newName} placeholder={$t('dashboard.widgets.goals.addPlaceholder')} disabled={adding} />
        <button class="go" disabled={adding || !newName.trim()}>{$t('dashboard.widgets.goals.add')}</button>
      </form>

      {#if goals === null}
        <div class="w-skeleton" aria-busy="true"><Skeleton rows={3} height="1rem" gap="10px" /></div>
      {:else if shown.length === 0}
        <p class="w-state">{$t('dashboard.widgets.goals.empty')}</p>
      {:else}
        <ul class="w-list">
          {#each shown as g (g.id)}
            {@const pct = Math.round(progress(g.kpis ?? []) * 100)}
            {@const dl = daysLeft(g.deadline)}
            <li class="w-row stack">
              <div class="line">
                <span class="w-name">{g.name}</span>
                <span class="w-num pct">{pct}%</span>
              </div>
              <div class="w-bar" class:pos={pct >= 100} role="progressbar"
                aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100" aria-label={g.name}>
                <span style:width={`${pct}%`}></span>
              </div>
              {#if dl !== null}
                <span class="w-sub" class:late={dl < 0}>
                  {dl < 0
                    ? $t('dashboard.widgets.goals.overdue', { days: -dl })
                    : $t('dashboard.widgets.goals.daysLeft', { days: dl })}
                </span>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
{:else}
  <WidgetState
    {editing}
    error={err}
    loading={goals === null}
    empty={all.length === 0 || (scope === 'overdue' && shown.length === 0) || (scope === 'goal' && !focus)}
    preview={$t('dashboard.widgets.goals.preview')}
    emptyText={scope === 'overdue' ? $t('dashboard.widgets.goals.emptyOverdue') : $t('dashboard.widgets.goals.empty')}
    rows={4}
  >
    {#if scope === 'status'}
      <Donut
        segments={[
          { label: $t('dashboard.widgets.goals.reached'), value: reached.length, color: 'var(--green)' },
          { label: $t('dashboard.widgets.goals.open'), value: openGoals.length, color: 'var(--chart-1)' },
          { label: $t('dashboard.widgets.goals.overdueLabel'), value: overdue.length, color: 'var(--red)' }
        ]}
        centerLabel={$t('dashboard.widgets.goals.goals')}
      />
    {:else if scope === 'avg'}
      <Stat
        value={`${avgProgress}%`}
        note={$t('dashboard.widgets.goals.acrossOpen', { count: openGoals.length })}
        percent={avgProgress}
        barTone="pos"
      />
    {:else if scope === 'due'}
      <ul class="w-list">
        {#each dueBuckets as b (b.label)}
          <li class="w-row">
            <Icon name={b.icon} size={15} />
            <span class="w-name grow">{b.label}</span>
            <span class="w-num">{b.value}</span>
          </li>
        {/each}
      </ul>
    {:else if scope === 'category'}
      <Donut segments={categories} centerLabel={$t('dashboard.widgets.goals.goals')} />
    {:else if scope === 'goal'}
      <div class="focus">
        <div class="fhead">
          <Icon name="target" size={16} />
          <span class="w-name fname">{focus?.name}</span>
        </div>
        <div class="fbody">
          <div class="fnums">
            {#if focusTotals}
              <p class="ftotals">
                <span class="cur">{fmtNum(focusTotals.current, 0)}</span>
                <span class="sep">/</span>
                <span class="tgt">{fmtNum(focusTotals.target, 0)}</span>
              </p>
            {/if}
            {#if focus?.deadline}
              <div class="fdate">
                <span class="w-eyebrow">{$t('dashboard.widgets.goals.targetDate')}</span>
                <span class="dval">{fmtDate(focus.deadline)}</span>
                <span class="w-sub">{focusLeft}</span>
              </div>
            {/if}
          </div>
          <Gauge
            percent={focusPct}
            tone="pos"
            label={focus?.name}
            rows={focusTotals
              ? [{ label: '', value: `${fmtNum(focusTotals.current, 0)} / ${fmtNum(focusTotals.target, 0)}` }]
              : []}
          />
        </div>
      </div>
    {:else}
      <!-- overdue -->
      <ul class="w-list">
        {#each shown as g (g.id)}
          {@const dl = daysLeft(g.deadline)}
          <li class="w-row late-row">
            <Icon name="alert-triangle" size={15} />
            <span class="grow stackcol">
              <span class="w-name">{g.name}</span>
              {#if g.category}<span class="w-sub">{g.category}</span>{/if}
            </span>
            <span class="w-num w-neg">{$t('dashboard.widgets.goals.overdue', { days: -dl })}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </WidgetState>
{/if}

<style>
  .add {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  .add input {
    flex: 1;
    min-width: 0;
  }
  .go {
    flex-shrink: 0;
  }
  .line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .pct {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .late {
    color: var(--red-ink);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .stackcol {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .w-row :global(svg) {
    color: var(--faint);
    flex-shrink: 0;
  }
  .late-row :global(svg) {
    color: var(--red);
  }
  .focus {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .fhead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .fhead :global(svg) {
    color: var(--dim);
  }
  .fname {
    font-size: var(--text-base);
  }
  .fbody {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-width: 0;
  }
  .fnums {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .ftotals {
    margin: 0;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-base);
    padding-bottom: var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
  }
  .cur {
    color: var(--text);
  }
  .sep,
  .tgt {
    color: var(--dim);
  }
  .fdate {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .dval {
    font-size: var(--text-sm);
    color: var(--text);
  }
</style>
