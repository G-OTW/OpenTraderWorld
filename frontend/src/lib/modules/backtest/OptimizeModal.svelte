<script>
  // Optimizer setup: pick the parameters to vary, see what that costs, then confirm.
  //
  // The parameters offered are the ones this strategy actually has, discovered from the settings
  // it is about to run, never a fixed list. Each row is a range around the value the user already
  // chose, so the default grid is small and widening it is a decision.
  //
  // Running is two steps on purpose. A grid is easy to write and expensive to run, so the second
  // step states the real numbers first: how many variants, and how long they take on *this*
  // machine (one variant is actually run and timed for it).
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { backtestApi, fmtNum } from './api.js';
  import {
    discoverParams,
    PARAM_GROUPS,
    OPT_METRICS,
    WEEKDAYS,
    rangeValues,
    weekdayValues,
    buildAxes,
    comboCount,
    fmtDuration,
    MAX_AXES,
    MAX_TRIALS
  } from './optimize.js';

  let {
    open = $bindable(false),
    /** Normalized + indicator-embedded settings: exactly what a run would post. */
    settings = null,
    datasetIds = [],
    /** True when the short side is the long one's mirror: the parameter then lives in two places
     *  of the posted settings, and one axis has to write both. */
    mirrored = false,
    /** Called with the new job id once the grid is started. */
    onstart = () => {}
  } = $props();

  const specs = $derived(settings ? discoverParams(settings, $t, { mirrored }) : []);
  const groups = $derived(
    PARAM_GROUPS.map((g) => ({ id: g, items: specs.filter((s) => s.group === g) })).filter((g) => g.items.length)
  );

  /** Per-parameter sweep state, keyed by spec id: `{ on, from, to, step }`. */
  let picks = $state({});
  let excludeDays = $state([]);
  let metric = $state('sharpe');
  let phase = $state('pick'); // 'pick' | 'confirm'
  let estimate = $state(null);
  let busy = $state(false);
  let error = $state('');

  // Seeding happens on open, from the specs of the strategy as it stands: a strategy edited
  // between two visits gets its new parameters, and the previous picks would point at nothing.
  let seededFor = null;
  $effect(() => {
    if (!open) {
      seededFor = null;
      return;
    }
    const key = specs.map((s) => s.id).join('|');
    if (seededFor === key) return;
    seededFor = key;
    const next = {};
    for (const s of specs) next[s.id] = { on: false, from: s.from, to: s.to, step: s.step };
    picks = next;
    excludeDays = [];
    phase = 'pick';
    estimate = null;
    error = '';
  });

  const pick = (s) => picks[s.id] ?? { on: false, from: s.from, to: s.to, step: s.step };
  const countOf = (s) => rangeValues({ ...s, ...pick(s) }).length;
  const toggle = (s) => (picks[s.id] = { ...pick(s), on: !pick(s).on });

  const chosen = $derived(specs.filter((s) => pick(s).on));
  const dayCount = $derived(excludeDays.length ? weekdayValues(excludeDays).length : 0);
  const axes = $derived(
    buildAxes(
      chosen.map((s) => ({ ...s, ...pick(s) })),
      excludeDays,
      $t
    )
  );
  const total = $derived(axes.length ? comboCount(axes) : 0);
  const tooMany = $derived(total > MAX_TRIALS);
  const tooWide = $derived(axes.length > MAX_AXES);
  const canRun = $derived(axes.length > 0 && !tooMany && !tooWide && datasetIds.length > 0 && !busy);

  const DAY_KEYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];
  function toggleDay(d) {
    excludeDays = excludeDays.includes(d) ? excludeDays.filter((x) => x !== d) : [...excludeDays, d];
  }

  const body = () => ({ dataset_ids: datasetIds, settings, axes, metric });

  /** Measure one variant, then show what the whole grid costs before anything runs. */
  async function measure() {
    if (!canRun) return;
    busy = true;
    error = '';
    try {
      estimate = await backtestApi.optimizeEstimate(body());
      phase = 'confirm';
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  async function start() {
    busy = true;
    error = '';
    try {
      const res = await backtestApi.optimizeStart(body());
      open = false;
      phase = 'pick';
      onstart(res.job_id);
    } catch (e) {
      error = e.message;
      phase = 'pick';
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open title={$t('backtest.opt.title')} size="lg" onclose={() => (phase = 'pick')}>
  {#if phase === 'confirm' && estimate}
    <div class="confirm">
      <p class="lead">{$t('backtest.opt.confirmLead', { n: fmtNum(estimate.trials, 0) })}</p>
      <p class="time">
        {$t('backtest.opt.confirmTime', {
          time: fmtDuration(estimate.est_ms, $t),
          per: fmtNum(estimate.per_trial_ms, 1),
          workers: estimate.workers
        })}
      </p>
      <p class="note">{$t('backtest.opt.confirmNote', { bars: fmtNum(estimate.bars, 0) })}</p>
      <!-- One grid at a time: two on the same cores make both slower and neither honest about
           its remaining time, so this is said here rather than as an error after the click. -->
      {#if estimate.running}<p class="time">{$t('backtest.opt.alreadyRunning')}</p>{/if}
      {#if error}<p class="err">{error}</p>{/if}
    </div>
  {:else}
    <div class="pickers">
      <p class="intro">{$t('backtest.opt.intro')}</p>

      {#if !specs.length}
        <p class="empty">{$t('backtest.opt.noParams')}</p>
      {/if}

      {#each groups as g (g.id)}
        <div class="block">
          <span class="cap">{$t(`backtest.opt.group.${g.id}`)}</span>
          <div class="rows">
            {#each g.items as s (s.id)}
              {@const p = pick(s)}
              {@const n = countOf(s)}
              <div class="row" class:on={p.on}>
                <label class="pickbox">
                  <input type="checkbox" checked={p.on} onchange={() => toggle(s)} />
                  <span class="name">
                    {s.label}
                    {#if s.sub}<span class="sub">{s.sub}</span>{/if}
                  </span>
                </label>
                <span class="cur">{$t('backtest.opt.now')} {s.value}</span>
                <div class="range" class:dim={!p.on}>
                  <label>{$t('backtest.opt.from')}
                    <input type="number" step="any" disabled={!p.on} value={p.from}
                      onchange={(e) => (picks[s.id] = { ...p, from: Number(e.currentTarget.value) })} /></label>
                  <label>{$t('backtest.opt.to')}
                    <input type="number" step="any" disabled={!p.on} value={p.to}
                      onchange={(e) => (picks[s.id] = { ...p, to: Number(e.currentTarget.value) })} /></label>
                  <label>{$t('backtest.opt.step')}
                    <input type="number" step="any" min="0" disabled={!p.on} value={p.step}
                      onchange={(e) => (picks[s.id] = { ...p, step: Number(e.currentTarget.value) })} /></label>
                </div>
                <span class="n" class:zero={p.on && !n}>{p.on ? $t('backtest.opt.values', { n }) : ''}</span>
              </div>
            {/each}
          </div>
        </div>
      {/each}

      <!-- Weekdays are a filter, not a number: the axis tries every subset of the days ticked
           here as an exclusion, plus the case where none is excluded. -->
      <div class="block">
        <span class="cap">{$t('backtest.opt.group.filters')}</span>
        <p class="hint">{$t('backtest.opt.weekdayHint')}</p>
        <div class="days">
          {#each WEEKDAYS as d, i (d)}
            <button type="button" class:on={excludeDays.includes(d)} onclick={() => toggleDay(d)}>
              {$t(`common.weekday.${DAY_KEYS[i]}`)}
            </button>
          {/each}
          {#if dayCount}
            <span class="n">{$t('backtest.opt.values', { n: dayCount })}</span>
          {/if}
        </div>
      </div>

      <div class="block">
        <span class="cap">{$t('backtest.opt.ranking')}</span>
        <div class="metric">
          <Dropdown
            bind:value={metric}
            options={OPT_METRICS.map((m) => ({ value: m.id, label: $t(`backtest.opt.metric.${m.id}`) }))}
            ariaLabel={$t('backtest.opt.ranking')}
            title={$t('backtest.opt.ranking')}
          />
          <span class="hint">{$t('backtest.opt.rankingHint')}</span>
        </div>
      </div>

      {#if error}<p class="err">{error}</p>{/if}
    </div>
  {/if}

  {#snippet footer()}
    {#if phase === 'confirm' && estimate}
      <span class="tally">{$t('backtest.opt.tally', { params: axes.length, n: fmtNum(estimate.trials, 0) })}</span>
      <button class="ghost" onclick={() => (phase = 'pick')} disabled={busy}>{$t('backtest.opt.back')}</button>
      <button class="primary" onclick={start} disabled={busy || !!estimate.running}>
        <Icon name="play" size={13} /> {$t('backtest.opt.runIt')}
      </button>
    {:else}
      <span class="tally" class:bad={tooMany || tooWide}>
        {#if tooWide}
          {$t('backtest.opt.tooWide', { max: MAX_AXES })}
        {:else if tooMany}
          {$t('backtest.opt.tooMany', { n: fmtNum(total, 0), max: fmtNum(MAX_TRIALS, 0) })}
        {:else if total}
          {$t('backtest.opt.tally', { params: axes.length, n: fmtNum(total, 0) })}
        {:else}
          {$t('backtest.opt.pickSomething')}
        {/if}
      </span>
      <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
      <button class="primary" onclick={measure} disabled={!canRun}>
        {#if busy}
          <span class="spin"><Icon name="refresh-cw" size={13} /></span> {$t('backtest.opt.measuring')}
        {:else}
          <Icon name="timer" size={13} /> {$t('backtest.opt.estimate')}
        {/if}
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .pickers,
  .confirm {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* Dense by design: a sweep setup is a table of ranges, not a form to linger in. */
  .pickers {
    --control-h: 26px;
  }
  .intro,
  .hint,
  .note {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .empty {
    font-size: var(--text-sm);
    color: var(--muted);
    font-style: italic;
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .cap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.68rem;
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .cap::after {
    content: '';
    flex: 1;
    height: 1px;
    background: var(--border);
  }
  .rows {
    display: flex;
    flex-direction: column;
  }

  /* One parameter = one row: name, current value, the range, the count it produces. A picked
     row carries the accent on its left edge, so the sweep reads at a glance down that column. */
  .row {
    display: grid;
    grid-template-columns: minmax(150px, 1.4fr) auto minmax(210px, 1fr) 64px;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    border-left: 2px solid transparent;
    transition: background-color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.on {
    border-left-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }
  .row:last-child {
    border-bottom: none;
  }
  .pickbox {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    cursor: pointer;
  }
  .name {
    display: flex;
    flex-direction: column;
    min-width: 0;
    font-size: var(--text-sm);
    color: var(--text);
  }
  .sub {
    font-size: 0.66rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cur {
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .range {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-2);
    transition: opacity var(--dur-fast) var(--ease);
  }
  .range.dim {
    opacity: 0.35;
  }
  .range label {
    display: flex;
    flex-direction: column;
    gap: 1px;
    font-size: 0.62rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    min-width: 0;
  }
  .range input {
    width: 100%;
    min-width: 0;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .row input[type='number']::-webkit-outer-spin-button,
  .row input[type='number']::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .row input[type='number'] {
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .n {
    font-size: var(--text-xs);
    color: var(--accent);
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .n.zero {
    color: var(--amber);
  }

  .days {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .days button {
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
  }
  .days button.on {
    background: color-mix(in srgb, var(--amber) 22%, var(--surface-2));
    border-color: var(--amber);
    color: var(--text);
  }
  .metric {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .metric :global(.dd) {
    min-width: 220px;
  }

  /* The confirmation: the number first, the arithmetic under it. Same left-edge idiom as the
     filters preview: one accent edge, no second frame inside the modal. */
  .confirm {
    gap: var(--space-2);
    padding: var(--space-3);
    background: var(--surface-2);
    border-left: 2px solid var(--accent);
  }
  .lead {
    font-size: 1.05rem;
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .time {
    font-size: var(--text-sm);
    color: var(--amber);
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
  }
  .tally {
    margin-right: auto;
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .tally.bad {
    color: var(--amber);
  }
  .spin {
    display: inline-flex;
    animation: opt-spin 1.1s linear infinite;
  }
  @keyframes opt-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
