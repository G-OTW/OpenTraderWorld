<script>
  // Automator widgets — the cards of the Automator mockup, each a `variant`. Two reads:
  // `runs()` (which already carries the workflow list beside the runs) and `agenda()` for
  // what is scheduled next. Nothing new server-side.
  //
  // Config: { variant, limit, days, window }.
  import { automatorApi } from '$lib/modules/automator/api.js';
  import { fmtDateTime, fmtPct, fmtDuration, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import Donut from './parts/Donut.svelte';
  import RankBars from './parts/RankBars.svelte';
  import { livePulse, LIVE } from '../live.svelte.js';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'running');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 6)));
  const days = $derived(Math.max(1, Math.min(30, item.config?.days ?? 7)));
  // How many runs the status/rate cards look back over. The endpoint caps at 200.
  const runWindow = $derived(Math.max(20, Math.min(200, item.config?.window ?? 200)));

  const needsRuns = $derived(['running', 'status', 'rates', 'failures', 'favorites'].includes(variant));

  let data = $state.raw(null);
  let agenda = $state(null);
  let err = $state('');
  let busy = $state('');

  const live = livePulse(LIVE.runs);
  $effect(() => {
    live.n;
    if (editing || !needsRuns) return;
    const n = runWindow;
    let alive = true;
    automatorApi.runs({ limit: n })
      .then((d) => { if (alive) data = d; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  $effect(() => {
    live.n;
    if (editing || variant !== 'agenda') return;
    const from = new Date().toISOString();
    const to = new Date(Date.now() + days * 86400000).toISOString();
    let alive = true;
    automatorApi.agenda(from, to)
      .then((d) => { if (alive) agenda = d.events ?? []; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const runs = $derived(data?.runs ?? []);
  const workflows = $derived(data?.workflows ?? []);
  const nameOf = (id) => workflows.find((w) => w.id === id)?.name ?? '—';

  const running = $derived(runs.filter((r) => r.status === 'running'));
  const counts = $derived.by(() => {
    const c = { ok: 0, failed: 0, timeout: 0, cancelled: 0 };
    for (const r of runs) if (r.status in c) c[r.status]++;
    return c;
  });
  const finished = $derived(counts.ok + counts.failed + counts.timeout + counts.cancelled);
  const successRate = $derived(finished ? (counts.ok / finished) * 100 : null);

  // Per-workflow success rate over the same window, worst last: the mockup's bar list.
  const rates = $derived.by(() => {
    const per = new Map();
    for (const r of runs) {
      if (r.status === 'running') continue;
      const e = per.get(r.workflow_id) ?? { ok: 0, total: 0 };
      e.total++;
      if (r.status === 'ok') e.ok++;
      per.set(r.workflow_id, e);
    }
    return [...per]
      .filter(([, e]) => e.total > 0)
      .map(([id, e]) => {
        const pct = (e.ok / e.total) * 100;
        return {
          label: nameOf(id),
          value: pct,
          display: fmtPct(pct, 0),
          tone: pct >= 90 ? 'pos' : pct >= 70 ? '' : 'neg',
          color: pct >= 90 ? 'var(--green)' : pct >= 70 ? 'var(--amber)' : 'var(--red)'
        };
      })
      .sort((a, b) => b.value - a.value)
      .slice(0, limit);
  });

  const failures = $derived(
    runs.filter((r) => r.status === 'failed' || r.status === 'timeout').slice(0, limit)
  );
  const favorites = $derived(workflows.filter((w) => w.favorite).slice(0, limit));

  async function runNow(w) {
    if (busy) return;
    busy = w.id;
    try {
      await automatorApi.run(w.id);
    } catch (e) {
      err = e.message;
    } finally {
      busy = '';
    }
  }

  // A run in flight has no duration yet; count from its start so the row ticks.
  let now = $state(Date.now());
  $effect(() => {
    if (editing || running.length === 0) return;
    const h = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(h);
  });
  const elapsed = (r) => Math.max(0, (now - new Date(r.started_at).getTime()) / 1000);
</script>

<WidgetState
  {editing}
  error={err}
  loading={(needsRuns && data === null) || (variant === 'agenda' && agenda === null)}
  preview={$t('dashboard.widgets.automator.preview')}
  rows={4}
>
  {#if variant === 'running'}
    <div class="w-body">
      <Stat value={String(running.length)} note={$t('dashboard.widgets.automator.currentlyRunning')} />
      <ul class="w-list">
        {#each running.slice(0, limit) as r (r.id)}
          <li class="w-row">
            <span class="w-dot live"></span>
            <span class="w-name grow">{nameOf(r.workflow_id)}</span>
            <span class="w-num">{fmtDuration(elapsed(r))}</span>
          </li>
        {/each}
      </ul>
    </div>
  {:else if variant === 'agenda'}
    {#if agenda.length === 0}
      <p class="w-state">{$t('dashboard.widgets.automator.noScheduled')}</p>
    {:else}
      <ul class="w-list">
        {#each agenda.slice(0, limit) as e (e.schedule_id + e.at)}
          <li class="w-row">
            <span class="w-dot"></span>
            <span class="w-name grow">{e.workflow}</span>
            <span class="w-sub when">{fmtDateTime(e.at)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if variant === 'status'}
    <div class="w-body">
      <Donut
        segments={[
          { label: $t('dashboard.widgets.automator.ok'), value: counts.ok, color: 'var(--green)' },
          { label: $t('dashboard.widgets.automator.failed'), value: counts.failed, color: 'var(--red)' },
          { label: $t('dashboard.widgets.automator.timeout'), value: counts.timeout, color: 'var(--amber)' },
          { label: $t('dashboard.widgets.automator.cancelled'), value: counts.cancelled, color: 'var(--chart-4)' }
        ]}
        center={String(finished)}
        centerLabel={$t('dashboard.widgets.automator.runs')}
        showPct={false}
      />
      <div class="rate">
        <span class="w-eyebrow">{$t('dashboard.widgets.automator.successRate')}</span>
        <span class="ratefig" class:w-pos={(successRate ?? 0) >= 90} class:w-neg={(successRate ?? 100) < 70}>
          {successRate == null ? '—' : fmtPct(successRate, 1)}
        </span>
      </div>
    </div>
  {:else if variant === 'rates'}
    <RankBars rows={rates} />
  {:else if variant === 'failures'}
    {#if failures.length === 0}
      <p class="w-state">{$t('dashboard.widgets.automator.noFailures')}</p>
    {:else}
      <ul class="w-list">
        {#each failures as r (r.id)}
          <li class="w-row stack">
            <span class="line">
              <span class="fail"><Icon name="alert-triangle" size={14} /></span>
              <span class="w-name grow">{nameOf(r.workflow_id)}</span>
              <span class="w-sub when">{$ago(r.started_at)}</span>
            </span>
            {#if r.error}<span class="w-name w-sub reason">{r.error}</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  {:else if variant === 'favorites'}
    {#if favorites.length === 0}
      <p class="w-state">{$t('dashboard.widgets.automator.noFavorites')}</p>
    {:else}
      <ul class="w-list">
        {#each favorites as w (w.id)}
          <li class="w-row">
            <span class="stackcol grow">
              <span class="w-name">{w.name}</span>
              {#if w.description}<span class="w-sub w-name">{w.description}</span>{/if}
            </span>
            <button class="play" disabled={!!busy || !w.enabled} onclick={() => runNow(w)}
              title={$t('dashboard.widgets.automator.runNow')} aria-label={$t('dashboard.widgets.automator.runNow')}>
              <Icon name="play" size={12} />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
    min-width: 0;
  }
  .when {
    flex-shrink: 0;
  }
  .live {
    background: var(--green);
  }
  .rate {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ratefig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 22px;
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .fail {
    display: inline-flex;
    color: var(--red);
    flex-shrink: 0;
  }
  .reason {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .stackcol {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .play {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-sm);
    border: var(--hairline) solid var(--border-control);
    background: var(--surface-2);
    color: var(--accent);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .play:hover:not(:disabled) {
    background: var(--accent-soft);
  }
  .play:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .play:focus-visible {
    outline: none;
    box-shadow: var(--ring);
  }
</style>
