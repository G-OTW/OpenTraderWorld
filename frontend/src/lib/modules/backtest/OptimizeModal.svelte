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
    period = { from: '', to: '' },
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

  const body = () => ({
    dataset_ids: datasetIds,
    settings,
    axes,
    metric,
    from: period.from || null,
    to: period.to || null
  });

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
      <div class="confirm-heading"><Icon name="timer" size={20} /><p class="lead">{$t('backtest.opt.confirmLead', { n: fmtNum(estimate.trials, 0) })}</p></div>
      <div class="estimate-grid">
        <div><span>{$t('backtest.opt.variants')}</span><strong>{fmtNum(estimate.trials, 0)}</strong></div>
        <div><span>{$t('backtest.opt.estimate')}</span><strong>{fmtDuration(estimate.est_ms, $t)}</strong></div>
        <div><span>{$t('backtest.opt.ranking')}</span><strong>{$t(`backtest.opt.metric.${metric}`)}</strong></div>
      </div>
      <div class="selected-axes">
        {#each axes as axis (axis.paths[0])}
          <span>{axis.label}<b>{$t('backtest.opt.values', { n: axis.values.length })}</b></span>
        {/each}
      </div>
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
      {#if estimate.running}<p class="notice">{$t('backtest.opt.alreadyRunning')}</p>{/if}
      {#if error}<p class="err">{error}</p>{/if}
    </div>
  {:else}
    <div class="pickers">
      <div class="intro"><Icon name="settings" size={18} /><p>{$t('backtest.opt.intro')}</p></div>

      {#if !specs.length}
        <p class="empty">{$t('backtest.opt.noParams')}</p>
      {/if}

      {#each groups as g (g.id)}
        <div class="block">
          <div class="section-head">
            <h4 class="cap">{$t(`backtest.opt.group.${g.id}`)}</h4>
            <span class="group-count">{g.items.filter((s) => pick(s).on).length} / {g.items.length}</span>
          </div>
          <div class="rows">
            {#each g.items as s (s.id)}
              {@const p = pick(s)}
              {@const n = countOf(s)}
              <div class="row" class:on={p.on}>
                <label class="pickbox">
                  <input type="checkbox" checked={p.on} onchange={() => toggle(s)} />
                  <span class="name">
                    {s.label}
                    {#if s.sub}<span class="sub" title={s.sub}>{s.sub}</span>{/if}
                    <span class="cur">{$t('backtest.opt.now')} <b>{s.value}{s.unit ?? ''}</b></span>
                  </span>
                </label>
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
      <div class="options">
        <div class="block option">
          <h4 class="cap">{$t('backtest.opt.group.filters')}</h4>
          <p class="hint">{$t('backtest.opt.weekdayHint')}</p>
          <div class="days">
            {#each WEEKDAYS as d, i (d)}
              <button type="button" aria-pressed={excludeDays.includes(d)} class:on={excludeDays.includes(d)} onclick={() => toggleDay(d)}>
                {$t(`common.weekday.${DAY_KEYS[i]}`)}
              </button>
            {/each}
            {#if dayCount}
              <span class="n">{$t('backtest.opt.values', { n: dayCount })}</span>
            {/if}
          </div>
        </div>

        <div class="block option">
          <h4 class="cap">{$t('backtest.opt.ranking')}</h4>
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
      </div>
      {#if error}<p class="err" role="alert">{error}</p>{/if}
    </div>
  {/if}

  {#snippet footer()}
    <div class="footer-content">
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
    </div>
  {/snippet}
</Modal>

<style>
  .pickers, .confirm {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .intro {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }
  .intro :global(svg) {
    flex-shrink: 0;
    margin-top: 2px;
  }
  .intro p, .hint, .note, .time, .lead {
    margin: 0;
  }
  .hint, .note {
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: var(--lh-base);
  }
  .empty {
    padding: var(--space-4);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .block {
    min-width: 0;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-2);
    border-bottom: var(--hairline) solid var(--border-control);
  }
  .cap {
    margin: 0;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .group-count {
    color: var(--muted);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(160px, 1fr) minmax(240px, 1fr) 76px;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
    border-left: 2px solid transparent;
    transition: background-color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.on {
    border-left-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 6%, var(--surface));
  }
  .row:last-child {
    border-bottom: none;
  }
  .pickbox {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    cursor: pointer;
  }
  .pickbox input {
    flex-shrink: 0;
  }
  .name {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    font-size: var(--text-sm);
    color: var(--text);
  }
  .sub {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cur {
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .cur b {
    font-weight: var(--fw-normal);
    color: var(--text);
  }
  .range {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-2);
  }
  .range.dim {
    opacity: 0.65;
  }
  .range label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-xs);
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
  .row input[type='number']::-webkit-outer-spin-button, .row input[type='number']::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .row input[type='number'] {
    -moz-appearance: textfield;
    appearance: textfield;
  }
  .n {
    font-size: var(--text-xs);
    color: var(--muted);
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .n.zero {
    color: var(--amber-ink);
  }
  .options {
    display: grid;
    grid-template-columns: 1.4fr 1fr;
    gap: var(--space-3);
  }
  .option {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3);
    overflow: visible;
  }
  .days {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
  }
  .days button {
    min-width: 38px;
    min-height: var(--control-h);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
  }
  .days button:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .days button.on {
    background: var(--warning-soft);
    border-color: var(--amber);
    color: var(--amber-ink);
  }
  .days button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .days .n {
    flex-basis: 100%;
    text-align: left;
    margin-top: var(--space-1);
  }
  .metric {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .metric :global(.dd) {
    width: 100%;
  }
  .confirm-heading {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .confirm-heading :global(svg) {
    flex-shrink: 0;
    color: var(--muted);
  }
  .lead {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .estimate-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
  }
  .estimate-grid > div {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4);
    min-width: 0;
  }
  .estimate-grid > div + div {
    border-left: var(--hairline) solid var(--border-control);
  }
  .estimate-grid span {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .estimate-grid strong {
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
  .selected-axes {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .selected-axes > span {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
  }
  .selected-axes b {
    color: var(--muted);
    font-weight: var(--fw-normal);
  }
  .time {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .notice, .err {
    padding: var(--space-3);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }
  .notice {
    background: var(--warning-soft);
    color: var(--amber-ink);
  }
  .err {
    background: var(--negative-soft);
    color: var(--red-ink);
  }
  .footer-content {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    flex-wrap: wrap;
  }
  .tally {
    flex: 1;
    margin-right: auto;
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .tally.bad {
    color: var(--amber-ink);
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
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
  @media (max-width: 640px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto;
      gap: var(--space-2);
    }
    .range {
      grid-column: 1 / -1;
      grid-row: 2;
    }
    .n {
      grid-column: 2;
      grid-row: 1;
    }
    .options {
      grid-template-columns: minmax(0, 1fr);
    }
    .tally {
      flex-basis: 100%;
      padding-bottom: var(--space-1);
    }
    .footer-content > button:first-of-type {
      margin-left: auto;
    }
    .estimate-grid > div {
      padding: var(--space-3) var(--space-2);
    }
    .estimate-grid strong {
      font-size: var(--text-md);
    }
  }
</style>
