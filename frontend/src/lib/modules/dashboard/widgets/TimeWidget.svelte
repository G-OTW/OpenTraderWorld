<script>
  // Time-tracker widgets — the cards of the Time Tracker mockup, each a `variant`.
  // Projects come from `listProjects()` (which already carries tracked seconds and the
  // running flag), totals from `breakdown()`, and the entry feed from `listEntries()`.
  //
  // Config: { variant, project_id, limit, bucket, since }.
  import { timeApi, fmtDuration, fmtHours, budgetLevel, entrySeconds } from '$lib/modules/time/api.js';
  import { fmtMoney, fmtTime, fmtSignedPct, dateKey } from '$lib/format';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import RankBars from './parts/RankBars.svelte';
  import { livePulse, LIVE } from '../live.svelte.js';

  let { item, editing } = $props();
  const only = $derived(item.config?.project_id || null);
  const variant = $derived(item.config?.variant ?? item.config?.scope ?? 'all');
  const scope = $derived(item.config?.scope ?? variant);
  const limit = $derived(Math.max(1, Math.min(50, item.config?.limit ?? 8)));
  // How far back the totals reach: today, this week, this month.
  const period = $derived(item.config?.period ?? 'week');

  const needsBreakdown = $derived(['hours', 'value', 'top'].includes(variant));
  const needsEntries = $derived(variant === 'entries');

  const live = livePulse(LIVE.runs);

  let projects = $state(null);
  let bd = $state.raw(null);
  let entries = $state(null);
  let err = $state('');
  let loadedAt = $state(0);
  let now = $state(Date.now());

  // Only the newest request in flight writes: at a few seconds apart, responses can
  // come back out of order.
  let seq = 0;
  async function load() {
    const mine = ++seq;
    err = '';
    try {
      const rows = await timeApi.listProjects();
      if (mine !== seq) return;
      projects = rows;
      loadedAt = Date.now();
    } catch (e) {
      if (mine === seq) err = e.message;
    }
  }
  $effect(() => {
    live.n;
    if (!editing) load();
  });

  // Day buckets over the chosen period: one call, and the bars come out of its points.
  $effect(() => {
    live.n;
    if (editing || !needsBreakdown) return;
    const days = period === 'today' ? 1 : period === 'month' ? 30 : 7;
    const from = new Date();
    from.setDate(from.getDate() - (days - 1));
    let alive = true;
    timeApi
      .breakdown({ bucket: 'day', since: dateKey(from), project_id: only ?? '' })
      .then((d) => { if (alive) bd = d; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // The feed has no cross-project endpoint, so it merges each project's own runs. Capped
  // at eight projects: a feed is the newest handful, not the whole ledger.
  $effect(() => {
    live.n;
    if (editing || !needsEntries || !projects) return;
    let alive = true;
    const pick = [...projects].slice(0, 8);
    Promise.all(pick.map((p) => timeApi.listEntries(p.id, limit).then((rows) =>
      rows.map((e) => ({ ...e, project: p })))))
      .then((lists) => {
        if (!alive) return;
        entries = lists.flat().sort((a, b) => b.started_at.localeCompare(a.started_at)).slice(0, limit);
      })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // Tick once a second while something runs so the running timer advances.
  $effect(() => {
    if (editing || !(projects ?? []).some((p) => p.running)) return;
    const h = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(h);
  });

  const shown = $derived(
    (only ? (projects ?? []).filter((p) => p.id === only) : (projects ?? []).filter((p) => scope !== 'running' || p.running))
      .slice(0, limit)
  );

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

  const cur = $derived(bd?.display_currency ?? 'USD');
  const dayHours = $derived((bd?.points ?? []).map((p) => p.hours));
  // The bucket key is the point's period ("2026-02-09", "2026-W07", "2026-02"); read it
  // back as a date when it parses, else show the key itself.
  const bucketLabels = $derived(
    (bd?.points ?? []).map((p) => {
      const d = new Date(`${p.bucket}T00:00:00`);
      return Number.isNaN(d.getTime())
        ? p.bucket
        : d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
    })
  );
  // Latest bucket against the one before it: the mockup's "vs yesterday".
  const dayDelta = $derived.by(() => {
    if (dayHours.length < 2) return null;
    const prev = dayHours.at(-2);
    return prev ? ((dayHours.at(-1) - prev) / prev) * 100 : null;
  });

  const budgets = $derived(
    (projects ?? [])
      .filter((p) => p.time_budget_hours)
      .map((p) => {
        const spent = p.tracked_seconds / 3600;
        return { p, spent, pct: (spent / p.time_budget_hours) * 100, level: budgetLevel(spent, p.time_budget_hours) };
      })
      .sort((a, b) => b.pct - a.pct)
      .slice(0, limit)
  );

  const deadlines = $derived.by(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    return (projects ?? [])
      .filter((p) => p.planned_end && !p.archived)
      .map((p) => ({ p, days: Math.round((new Date(`${p.planned_end}T00:00:00`) - today) / 86400000) }))
      .sort((a, b) => a.days - b.days)
      .slice(0, limit);
  });

  const topProjects = $derived(
    (bd?.projects ?? [])
      .slice(0, limit)
      .map((p) => ({ label: p.name, value: p.hours, display: fmtHours(p.hours), color: p.color || undefined }))
  );

  const running = $derived((projects ?? []).filter((p) => p.running));
</script>

<WidgetState
  {editing}
  error={err}
  loading={projects === null || (needsBreakdown && bd === null) || (needsEntries && entries === null)}
  empty={(projects ?? []).length === 0}
  preview={$t('dashboard.widgets.time.preview')}
  emptyText={$t('dashboard.widgets.time.empty')}
  rows={3}
>
  {#if variant === 'hours' || variant === 'value'}
    {@const isValue = variant === 'value'}
    <div class="split">
      <Stat
        value={isValue ? fmtMoney(bd.total_value, cur, 0) : fmtHours(bd.total_hours)}
        delta={dayDelta == null ? '' : fmtSignedPct(dayDelta, 0)}
        deltaTone={dayDelta == null ? '' : dayDelta < 0 ? 'neg' : 'pos'}
        note={dayDelta == null ? '' : $t('dashboard.widgets.time.vsPrevious')}
      />
      {#if dayHours.length > 1}
        <div class="barbox">
          <MiniBars values={isValue ? bd.points.map((p) => p.value) : dayHours} labels={bucketLabels}
            tone="pos" height={46}
            valueFormat={(v) => (isValue ? fmtMoney(v, cur, 0) : fmtHours(v))}
            label={$t('dashboard.widgets.time.tracked')} />
        </div>
      {/if}
    </div>
  {:else if variant === 'budget'}
    {#if budgets.length === 0}
      <p class="w-state">{$t('dashboard.widgets.time.noBudget')}</p>
    {:else}
      <table class="w-tbl">
        <thead>
          <tr>
            <th>{$t('dashboard.widgets.time.project')}</th>
            <th class="num">{$t('dashboard.widgets.time.spent')}</th>
            <th class="num">{$t('dashboard.widgets.time.budget')}</th>
            <th class="num">{$t('dashboard.widgets.time.usage')}</th>
          </tr>
        </thead>
        <tbody>
          {#each budgets as b (b.p.id)}
            <tr>
              <td class="w-name">{b.p.name}</td>
              <td class="num">{fmtHours(b.spent)}</td>
              <td class="num">{fmtHours(b.p.time_budget_hours)}</td>
              <td class="num" class:w-neg={b.level === 'over' || b.level === 'high'}
                class:w-warn={b.level === 'warn'} class:w-pos={!b.level}>{Math.round(b.pct)}%</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else if variant === 'deadline'}
    {#if deadlines.length === 0}
      <p class="w-state">{$t('dashboard.widgets.time.noDeadline')}</p>
    {:else}
      <ul class="w-list">
        {#each deadlines as d (d.p.id)}
          <li class="w-row">
            <span class="w-dot" style:background={d.p.color || undefined}></span>
            <span class="w-name grow">{d.p.name}</span>
            <span class="w-pill" class:neg={d.days <= 2} class:warn={d.days > 2 && d.days <= 7}>
              {d.days < 0
                ? $t('dashboard.widgets.time.daysLate', { days: -d.days })
                : $t('dashboard.widgets.time.daysLeft', { days: d.days })}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if variant === 'top'}
    <RankBars rows={topProjects} />
  {:else if variant === 'entries'}
    <table class="w-tbl">
      <thead>
        <tr>
          <th>{$t('dashboard.widgets.time.time')}</th>
          <th>{$t('dashboard.widgets.time.project')}</th>
          <th class="num">{$t('dashboard.widgets.time.duration')}</th>
        </tr>
      </thead>
      <tbody>
        {#each entries ?? [] as e (e.id)}
          <tr>
            <td class="w-sub">{fmtTime(e.started_at)}</td>
            <td class="w-name">{e.project.name}</td>
            <td class="num">{fmtDuration(Math.floor(entrySeconds(e)))}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else if variant === 'running' || scope === 'running'}
    {#if running.length === 0}
      <p class="w-state">{$t('dashboard.widgets.time.noneRunning')}</p>
    {:else}
      <div class="w-body">
        {#each running.slice(0, limit) as p (p.id)}
          <div class="runcard">
            <div class="runid">
              <span class="w-dot live"></span>
              <span class="w-name">{p.name}</span>
            </div>
            <div class="runline">
              <span class="clock">{fmtDuration(Math.floor(liveSeconds(p)))}</span>
              <button class="toggle on big" onclick={() => toggle(p)}
                aria-label={$t('dashboard.widgets.time.stop')}>
                <Icon name="square" size={13} />
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <ul class="w-list">
      {#each shown as p (p.id)}
        <li class="w-row">
          <button
            class="toggle"
            class:on={p.running}
            onclick={() => toggle(p)}
            aria-label={p.running ? $t('dashboard.widgets.time.stop') : $t('dashboard.widgets.time.start')}
          >
            <Icon name={p.running ? 'square' : 'play'} size={11} />
          </button>
          <span class="w-name grow">{p.name}</span>
          <!-- A running clock is the one thing here that earns colour, and it also
               carries the ▪ running dot, so the state is never colour alone. -->
          {#if p.running}<span class="w-dot live"></span>{/if}
          <span class="w-num time" class:run={p.running}>{fmtDuration(Math.floor(liveSeconds(p)))}</span>
        </li>
      {/each}
    </ul>
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
    border: var(--hairline) solid var(--border-control);
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
  }
  .toggle:hover {
    color: var(--text);
    background: var(--surface-3);
  }
  .toggle.on {
    color: var(--green-contrast);
    background: var(--green);
    border-color: var(--green);
  }
  .toggle.big {
    width: 36px;
    height: 36px;
    border-radius: 50%;
  }
  .toggle:focus-visible {
    outline: none;
    box-shadow: var(--ring);
  }
  .live {
    background: var(--green);
  }
  .time {
    font-size: var(--text-sm);
    color: var(--dim);
  }
  .time.run {
    color: var(--green-ink);
  }
  .runcard {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .runcard + .runcard {
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .runid {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .runline {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .clock {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 26px;
    font-weight: var(--fw-medium);
    letter-spacing: -0.01em;
    color: var(--text);
  }
  .split {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-width: 0;
  }
  .barbox {
    flex: 1;
    max-width: 52%;
    min-width: 0;
  }
</style>
