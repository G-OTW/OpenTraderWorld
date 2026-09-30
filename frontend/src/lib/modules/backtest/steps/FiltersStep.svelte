<script>
  // Step 3 — when the strategy is allowed to open. Two kinds of rule: the clock (offset,
  // weekdays, intraday sessions) and the calendar (only / never these dates).
  //
  // Filters gate entries only: exits, stops and take-profits keep running on every bar, so a
  // rule here can never trap an open position. They also never trim bars, so switching one on
  // leaves every indicator reading exactly what it read before.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';
  import { filtersActive } from '../api.js';
  import { DAY_KEYS, tzLabel, filtersSummary } from '../summary.js';
  import './form.css';

  // `timeframe` is only used to warn: an intraday session means nothing on daily bars, where
  // every timestamp sits at midnight.
  let { settings = $bindable(), timeframe = '' } = $props();

  const f = $derived(settings.filters);
  const active = $derived(filtersActive(f));
  const preview = $derived(filtersSummary(settings, $t));
  const coarse = $derived(/^\d+(d|w|mo|M)$/.test(timeframe));
  const sessionOnDaily = $derived(coarse && f.sessions.length > 0);

  // Whole hours plus the half/quarter offsets that real exchanges sit on.
  const TZ_MINUTES = [
    -720, -660, -600, -570, -540, -480, -420, -360, -300, -240, -210, -180, -120, -60, 0, 60, 120,
    180, 210, 240, 270, 300, 330, 345, 360, 390, 420, 480, 525, 540, 570, 600, 630, 660, 720, 765,
    780, 840
  ];
  const tzOptions = TZ_MINUTES.map((m) => ({ value: m, label: tzLabel(m) }));

  const WEEK = [1, 2, 3, 4, 5, 6, 7];
  const selected = (d) => (f.weekdays ?? []).includes(d);
  function toggleDay(d) {
    const set = new Set(f.weekdays ?? []);
    if (set.has(d)) set.delete(d);
    else set.add(d);
    f.weekdays = [...set].sort((a, b) => a - b);
  }

  const addSession = () => (f.sessions = [...f.sessions, { from: '09:30', to: '16:00' }]);
  const dropSession = (i) => (f.sessions = f.sessions.filter((_, k) => k !== i));

  const addDate = (key) => (f[key] = [...f[key], { from: '', to: null }]);
  const dropDate = (key, i) => (f[key] = f[key].filter((_, k) => k !== i));
  // An empty end date means "that one day", which is a null in the payload, not "".
  const setEnd = (key, i, v) => (f[key][i].to = v || null);

  const clearAll = () => {
    f.weekdays = [];
    f.sessions = [];
    f.include_dates = [];
    f.exclude_dates = [];
  };
</script>

<div class="bt-step">
  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.filters.windowCap')}</span>

    <div class="preview" class:on={active}>
      <Icon name={active ? 'filter' : 'clock'} size={13} />
      <span class="txt">{preview}</span>
      {#if active}
        <button type="button" class="clear" onclick={clearAll}>{$t('backtest.filters.clearAll')}</button>
      {/if}
    </div>

    <div class="row">
      <span class="lbl">{$t('backtest.filters.timezone')}</span>
      <div class="stack">
        <div class="tz">
          <Dropdown
            bind:value={f.tz_offset_min}
            options={tzOptions}
            ariaLabel={$t('backtest.filters.timezone')}
            searchPlaceholder={$t('backtest.filters.timezone')}
          />
        </div>
        <p class="bt-note">{$t('backtest.filters.tzNote')}</p>
      </div>
    </div>

    <div class="row">
      <span class="lbl">{$t('backtest.filters.days')}</span>
      <div class="days">
        {#each WEEK as d (d)}
          <button type="button" class="day" class:on={selected(d)} aria-pressed={selected(d)} onclick={() => toggleDay(d)}>
            {$t(`common.weekday.${DAY_KEYS[d - 1]}`)}
          </button>
        {/each}
        <button type="button" class="preset" onclick={() => (f.weekdays = [1, 2, 3, 4, 5])}>
          {$t('backtest.filters.weekdaysOnly')}
        </button>
        {#if f.weekdays?.length}
          <button type="button" class="preset" onclick={() => (f.weekdays = [])}>{$t('backtest.filters.everyDay')}</button>
        {/if}
      </div>
    </div>

    <div class="row">
      <span class="lbl">{$t('backtest.filters.sessions')}</span>
      <div class="stack">
        {#each f.sessions as _s, i (i)}
          <div class="line">
            <input type="time" bind:value={f.sessions[i].from} aria-label={$t('backtest.filters.sessionFrom')} />
            <span class="sep">→</span>
            <input type="time" bind:value={f.sessions[i].to} aria-label={$t('backtest.filters.sessionTo')} />
            <button type="button" class="kill" onclick={() => dropSession(i)} aria-label={$t('common.remove')}>
              <Icon name="trash" size={12} />
            </button>
          </div>
        {/each}
        <button type="button" class="add" onclick={addSession}>
          <Icon name="plus" size={11} /> {$t('backtest.filters.addSession')}
        </button>
        {#if sessionOnDaily}
          <p class="bt-warn">
            <Icon name="alert-triangle" size={12} />
            {$t('backtest.filters.sessionDaily', { tf: timeframe })}
          </p>
        {/if}
        <p class="bt-note">
          {f.sessions.length ? $t('backtest.filters.sessionNote') : $t('backtest.filters.sessionEmpty')}
        </p>
      </div>
    </div>
  </div>

  <div class="bt-block">
    <span class="bt-cap">{$t('backtest.filters.datesCap')}</span>
    <div class="row">
      <span class="lbl">{$t('backtest.filters.only')}</span>
      <div class="stack">
        {#each f.include_dates as _d, i (i)}
          <div class="line">
            <input type="date" bind:value={f.include_dates[i].from} aria-label={$t('backtest.filters.dateFrom')} />
            <span class="sep">→</span>
            <input type="date" value={f.include_dates[i].to ?? ''} onchange={(e) => setEnd('include_dates', i, e.target.value)}
              aria-label={$t('backtest.filters.dateTo')} />
            <button type="button" class="kill" onclick={() => dropDate('include_dates', i)} aria-label={$t('common.remove')}>
              <Icon name="trash" size={12} />
            </button>
          </div>
        {/each}
        <button type="button" class="add" onclick={() => addDate('include_dates')}>
          <Icon name="plus" size={11} /> {$t('backtest.filters.addDate')}
        </button>
      </div>
    </div>

    <div class="row">
      <span class="lbl">{$t('backtest.filters.never')}</span>
      <div class="stack">
        {#each f.exclude_dates as _d, i (i)}
          <div class="line">
            <input type="date" bind:value={f.exclude_dates[i].from} aria-label={$t('backtest.filters.dateFrom')} />
            <span class="sep">→</span>
            <input type="date" value={f.exclude_dates[i].to ?? ''} onchange={(e) => setEnd('exclude_dates', i, e.target.value)}
              aria-label={$t('backtest.filters.dateTo')} />
            <button type="button" class="kill" onclick={() => dropDate('exclude_dates', i)} aria-label={$t('common.remove')}>
              <Icon name="trash" size={12} />
            </button>
          </div>
        {/each}
        <button type="button" class="add" onclick={() => addDate('exclude_dates')}>
          <Icon name="plus" size={11} /> {$t('backtest.filters.addDate')}
        </button>
      </div>
    </div>
    <p class="bt-note">{$t('backtest.filters.datesNote')}</p>
  </div>

  {#if active}
    <div class="bt-block">
      <span class="bt-cap">{$t('backtest.filters.outsideCap')}</span>
      <div class="bt-seg">
        <button class:on={f.on_window_end !== 'flat'} onclick={() => (f.on_window_end = 'hold')}>
          {$t('backtest.filters.hold')}
        </button>
        <button class:on={f.on_window_end === 'flat'} onclick={() => (f.on_window_end = 'flat')}>
          {$t('backtest.filters.flat')}
        </button>
      </div>
      <label class="bt-check">
        <input type="checkbox" bind:checked={f.block_adds} />
        {$t('backtest.filters.blockAdds')}
      </label>
      <p class="bt-note">{$t('backtest.filters.outsideNote')}</p>
    </div>
  {/if}
</div>

<style>
  /* The window as one sentence, above the controls that write it. */
  .preview {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--surface-2);
    border-left: 2px solid var(--border);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .preview.on {
    border-left-color: var(--accent);
    color: var(--text);
  }
  .preview .txt {
    flex: 1;
    min-width: 0;
  }
  .clear {
    background: transparent;
    border: none;
    padding: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    text-decoration: underline;
    cursor: pointer;
  }
  .clear:hover {
    color: var(--text);
  }

  /* Label left, controls right: the rows line up whatever the control is. */
  .row {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr);
    align-items: start;
    gap: var(--space-3);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--muted);
    padding-top: 6px;
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .line input {
    width: 140px;
  }
  .sep {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .tz {
    width: 150px;
  }

  .days {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    align-items: center;
  }
  .day {
    min-width: 44px;
    padding: var(--space-1) var(--space-2);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: color var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
  }
  .day:hover {
    color: var(--text);
  }
  .day.on {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border-color: var(--accent);
    color: var(--text);
  }
  .preset {
    margin-left: var(--space-2);
    background: transparent;
    border: none;
    padding: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    text-decoration: underline;
    cursor: pointer;
  }
  .preset:hover {
    color: var(--text);
  }

  .add {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    align-self: flex-start;
    background: transparent;
    border: var(--hairline) dashed var(--border-control);
    color: var(--muted);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
  }
  .add:hover {
    border-style: solid;
    color: var(--text);
  }
  .kill {
    display: inline-flex;
    background: transparent;
    border: none;
    padding: var(--space-1);
    color: var(--muted);
    cursor: pointer;
  }
  .kill:hover {
    color: var(--red);
  }

  @media (max-width: 720px) {
    .row {
      grid-template-columns: 1fr;
      gap: var(--space-1);
    }
    .lbl {
      padding-top: 0;
    }
  }
</style>
