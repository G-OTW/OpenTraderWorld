<script>
  // The one recurrence editor: reminders, routines and automator schedules (and the robot
  // sessions that reuse them) all edit their rule here. It works on the shared model of
  // $lib/ui/recurrence.js; each module maps its own payload to it with an adapter, and the
  // props below switch on only what that payload can carry, so a control is never offered
  // for a field the module cannot save.
  //
  // Layout: presets first (one-click answers for the common rules), then the rule kind, then
  // the fine controls of that kind (inset, so they read as the detail of the choice above),
  // then the dates and time, then a live preview of the next dates when the module has one.
  // Editing a control simply stops matching a preset: no mode switch, no lost state.
  //
  // Controlled: it never keeps its own copy. Every edit calls `onchange(nextModel)` and the
  // owner writes it back through its adapter, so a parent that swaps the rule under it
  // (another schedule opened in the same modal) is shown as it is.
  import { WEEKDAYS, MASK_EVERY_DAY, MASK_WEEKDAYS, MASK_WEEKEND, sameRule } from './recurrence.js';
  import Icon from './Icon.svelte';
  import Dropdown from './Dropdown.svelte';
  import EveryInput from './EveryInput.svelte';
  import { t, locale } from '$lib/i18n';

  let {
    value, // the model
    onchange = () => {},
    kinds = [], // [{ value, label }], in display order
    presets = [], // [{ id, label, rule }]: `rule` is the partial model a click applies
    kindDefaults = null, // (kind, model) => partial model applied when the kind changes
    intervalHours = false, // the `interval` period can also be typed in hours
    dayRepeat = false, // several fires in a day: from `time`, every `repeatEvery` min, `repeatCount` in all
    interval = false, // every N days / weeks
    monthDays = false, // 'multi' (several days and the last day) | 'single' (one day)
    onceDate = false, // a once rule has its own date (`date`)
    runAt = false, // a once rule is an instant (`runAt`, date and time)
    time = false, // a time of day
    anchor = false, // a start date the rule repeats from, always shown (`start`)
    period = false, // 'range': optional active period | 'end': end date of a repeating rule
    windowPresets = [], // [{ id, label, make: () => ({ start, end }) }]
    maxCount = false, // a cap on how many times it fires
    timezones = null, // zone list: shows the timezone picker
    preview = null, // dates (Date or YYYY-MM-DD) to list; [] warns it never comes due; null hides
    summary = '', // one-line description shown above the preview
    disabled = false
  } = $props();

  const set = (patch) => onchange({ ...value, ...patch });

  function setKind(kind) {
    if (value.kind === kind) return;
    set({ kind, ...(kindDefaults?.(kind, value) ?? {}) });
  }

  function toggleDay(bit) {
    const next = (value.weekdays ?? 0) ^ bit;
    if (next === 0) return; // a rule with no day would never fire
    set({ weekdays: next });
  }

  function pickMonthDay(day) {
    if (monthDays === 'single') return set({ monthDays: [day] });
    const days = value.monthDays ?? [];
    const next = days.includes(day) ? days.filter((d) => d !== day) : [...days, day];
    if (next.length === 0) return;
    set({ monthDays: next.sort((a, b) => a - b) });
  }

  const positive = (v) => Math.max(1, Number(v) || 1);

  // ── Several fires in a day ──
  const DAY = 1440;
  const DAY_DEFAULT = { repeatEvery: 60, repeatCount: 3 };
  const showDayRepeat = $derived(dayRepeat && k !== 'interval');
  const dayOn = $derived(showDayRepeat && !!value.repeatEvery);
  const dayCount = $derived(Math.max(2, Number(value.repeatCount) || 2));
  const dayTooLong = $derived(dayOn && (dayCount - 1) * value.repeatEvery >= DAY);

  /** The clock times of one day's fires: all of them when few, else the first ones and the last. */
  const dayTimes = $derived.by(() => {
    if (!dayOn) return [];
    const [h, m] = (value.time || '00:00').split(':').map(Number);
    const at = (i) => {
      const mins = (h * 60 + m + i * value.repeatEvery) % DAY;
      return `${String(Math.floor(mins / 60)).padStart(2, '0')}:${String(mins % 60).padStart(2, '0')}`;
    };
    const n = Math.min(dayCount, DAY);
    if (n <= 8) return Array.from({ length: n }, (_, i) => at(i));
    return [...Array.from({ length: 6 }, (_, i) => at(i)), '…', at(n - 1)];
  });

  const monthGrid = Array.from({ length: 31 }, (_, i) => i + 1);
  const ordinals = [1, 2, 3, 4, 5, -1];

  const k = $derived(value.kind);
  const hasDetail = $derived(
    k === 'weekly' ||
      k === 'interval' ||
      (k === 'daily' && interval) ||
      (k === 'monthly' && !!monthDays) ||
      k === 'nth_weekday' ||
      (k === 'once' && (onceDate || runAt))
  );
  const showTime = $derived(time && k !== 'interval' && !(k === 'once' && runAt));
  const showWhen = $derived(anchor || showTime || !!timezones);
  const showEnd = $derived(period === 'end' && k !== 'once');

  function fmtPreview(d) {
    const date = typeof d === 'string' ? new Date(`${d}T00:00:00`) : d;
    return date.toLocaleDateString($locale, { weekday: 'short', day: 'numeric', month: 'short' });
  }
</script>

<fieldset class="picker" {disabled}>
  {#if presets.length}
    <div class="block">
      <span class="lbl">{$t('routines.schedule.quickPick')}</span>
      <div class="chips">
        {#each presets as p (p.id)}
          <button
            type="button"
            class="chip"
            class:active={sameRule(value, { ...value, ...p.rule })}
            onclick={() => set(p.rule)}
          >
            {p.label}
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <div class="block">
    <span class="lbl">{$t('routines.schedule.repeats')}</span>
    <div class="chips" role="group" aria-label={$t('routines.schedule.repeats')}>
      {#each kinds as kd (kd.value)}
        <button
          type="button"
          class="chip"
          class:active={k === kd.value}
          aria-pressed={k === kd.value}
          onclick={() => setKind(kd.value)}
        >
          {kd.label}
        </button>
      {/each}
    </div>
  </div>

  {#if hasDetail}
    <div class="block detail">
      {#if k === 'interval' && intervalHours}
        <EveryInput
          minutes={value.everyMinutes ?? 60}
          label={$t('recurrence.every')}
          onchange={(v) => set({ everyMinutes: v })}
        />
      {:else if k === 'interval'}
        <label class="inline">
          {$t('automator.sched.everyLabel')}
          <input
            type="number"
            min="1"
            max="10080"
            value={value.everyMinutes ?? 60}
            oninput={(e) => set({ everyMinutes: positive(e.currentTarget.value) })}
          />
        </label>
      {:else if k === 'weekly'}
        <div class="daybtns" role="group" aria-label={$t('routines.schedule.daysOfWeek')}>
          {#each WEEKDAYS as d (d.bit)}
            <button
              type="button"
              class:on={((value.weekdays ?? 0) & d.bit) !== 0}
              aria-pressed={((value.weekdays ?? 0) & d.bit) !== 0}
              onclick={() => toggleDay(d.bit)}
            >
              {$t(`routines.weekdayShort.${d.key}`)}
            </button>
          {/each}
        </div>
        <div class="chips">
          <button type="button" class="chip" class:active={value.weekdays === MASK_WEEKDAYS} onclick={() => set({ weekdays: MASK_WEEKDAYS })}>{$t('routines.schedule.weekdays')}</button>
          <button type="button" class="chip" class:active={value.weekdays === MASK_WEEKEND} onclick={() => set({ weekdays: MASK_WEEKEND })}>{$t('routines.schedule.weekend')}</button>
          <button type="button" class="chip" class:active={value.weekdays === MASK_EVERY_DAY} onclick={() => set({ weekdays: MASK_EVERY_DAY })}>{$t('routines.schedule.everyDay')}</button>
        </div>
        {#if interval}
          <label class="inline">
            {$t('routines.schedule.everyNWeeks')}
            <input
              type="number"
              min="1"
              max="52"
              value={value.every ?? 1}
              oninput={(e) => set({ every: positive(e.currentTarget.value) })}
            />
          </label>
        {/if}
      {:else if k === 'daily'}
        <label class="inline">
          {$t('routines.schedule.everyNDaysLabel')}
          <input
            type="number"
            min="1"
            max="365"
            value={value.every ?? 1}
            oninput={(e) => set({ every: positive(e.currentTarget.value) })}
          />
        </label>
      {:else if k === 'monthly'}
        <span class="hint">
          {monthDays === 'single' ? $t('automator.sched.dayOfMonth') : $t('routines.schedule.pickMonthDays')}
        </span>
        <div class="grid" role="group" aria-label={$t('routines.schedule.pickMonthDays')}>
          {#each monthGrid as d (d)}
            <button
              type="button"
              class:on={(value.monthDays ?? []).includes(d)}
              aria-pressed={(value.monthDays ?? []).includes(d)}
              onclick={() => pickMonthDay(d)}
            >
              {d}
            </button>
          {/each}
          {#if monthDays === 'multi'}
            <button
              type="button"
              class="last"
              class:on={(value.monthDays ?? []).includes(-1)}
              aria-pressed={(value.monthDays ?? []).includes(-1)}
              onclick={() => pickMonthDay(-1)}
            >
              {$t('routines.schedule.lastDay')}
            </button>
          {/if}
        </div>
        {#if monthDays === 'single'}
          <span class="hint">{$t('automator.sched.dayOfMonthHint')}</span>
        {/if}
      {:else if k === 'nth_weekday'}
        <div class="row">
          <div class="inline">
            <span>{$t('routines.schedule.which')}</span>
            <div class="pick">
              <Dropdown
                value={String(value.nth)}
                ariaLabel={$t('routines.schedule.which')}
                options={ordinals.map((n) => ({
                  value: String(n),
                  label: n === -1 ? $t('routines.schedule.ordLast') : $t(`routines.schedule.ord${n}`)
                }))}
                onpick={(v) => set({ nth: Number(v) })}
              />
            </div>
          </div>
          <div class="inline">
            <span>{$t('routines.schedule.dayOfWeek')}</span>
            <div class="pick">
              <Dropdown
                value={String(value.weekday)}
                ariaLabel={$t('routines.schedule.dayOfWeek')}
                options={WEEKDAYS.map((d) => ({
                  value: String(d.index),
                  label: $t(`routines.weekday.${d.key}`)
                }))}
                onpick={(v) => set({ weekday: Number(v) })}
              />
            </div>
          </div>
        </div>
      {:else if k === 'once' && runAt}
        <label class="inline">
          {$t('automator.sched.runAt')}
          <input
            type="datetime-local"
            value={value.runAt ?? ''}
            onchange={(e) => set({ runAt: e.currentTarget.value })}
          />
        </label>
      {:else if k === 'once' && onceDate}
        <label class="inline">
          {$t('routines.schedule.onDate')}
          <input
            type="date"
            value={value.date ?? ''}
            oninput={(e) => set({ date: e.currentTarget.value })}
          />
        </label>
      {/if}
    </div>
  {/if}

  {#if showWhen || showEnd}
    <div class="fields">
      {#if anchor}
        <label class="field">
          <span>{$t('remindme.form.startDate')}</span>
          <input type="date" value={value.start ?? ''} oninput={(e) => set({ start: e.currentTarget.value })} />
        </label>
      {/if}
      {#if showTime}
        <label class="field">
          <span>{$t('remindme.form.time')}</span>
          <input type="time" value={value.time ?? ''} oninput={(e) => set({ time: e.currentTarget.value })} />
        </label>
      {/if}
      {#if timezones}
        <div class="field">
          <span>{$t('automator.sched.timezone')}</span>
          <Dropdown
            value={value.timezone ?? ''}
            ariaLabel={$t('automator.sched.timezone')}
            options={timezones.map((z) => ({ value: z, label: z }))}
            onpick={(v) => set({ timezone: v })}
          />
        </div>
      {/if}
      {#if showEnd}
        <label class="field">
          <span>{$t('remindme.form.endDate')}</span>
          <input type="date" value={value.end ?? ''} oninput={(e) => set({ end: e.currentTarget.value })} />
        </label>
        {#if maxCount}
          <label class="field">
            <span>{dayOn ? $t('remindme.form.maxCountTotal') : $t('remindme.form.maxCount')}</span>
            <input
              type="number"
              min="1"
              step="1"
              placeholder="∞"
              value={value.maxCount ?? ''}
              oninput={(e) => {
                const v = e.currentTarget.value;
                set({ maxCount: v === '' ? '' : Number(v) });
              }}
            />
          </label>
        {/if}
      {/if}
    </div>
    {#if showEnd && value.start && value.end && value.end < value.start}
      <p class="warn">{$t('routines.schedule.endBeforeStart')}</p>
    {/if}
  {/if}

  {#if showDayRepeat}
    <div class="block">
      <span class="lbl">{$t('recurrence.day.label')}</span>
      <div class="chips" role="group" aria-label={$t('recurrence.day.label')}>
        <button
          type="button"
          class="chip"
          class:active={!dayOn}
          aria-pressed={!dayOn}
          onclick={() => set({ repeatEvery: null, repeatCount: '' })}
        >
          {$t('recurrence.day.single', { time: value.time || '00:00' })}
        </button>
        <button
          type="button"
          class="chip"
          class:active={dayOn}
          aria-pressed={dayOn}
          onclick={() => !dayOn && set(DAY_DEFAULT)}
        >
          {$t('recurrence.day.multi')}
        </button>
      </div>
      {#if dayOn}
        <div class="block detail">
          <div class="row">
            <EveryInput
              minutes={value.repeatEvery}
              label={$t('recurrence.day.then')}
              maxMinutes={DAY - 1}
              onchange={(v) => set({ repeatEvery: v })}
            />
            <label class="inline">
              <input
                type="number"
                min="2"
                max={DAY}
                value={dayCount}
                oninput={(e) => {
                  const v = e.currentTarget.value;
                  if (v !== '') set({ repeatCount: Math.max(2, Math.floor(Number(v)) || 2) });
                }}
                onblur={(e) => (e.currentTarget.value = String(dayCount))}
              />
              {$t('recurrence.day.count')}
            </label>
          </div>
          <div class="times" aria-label={$t('recurrence.day.times')}>
            {#each dayTimes as tm, i (i)}
              {#if tm === '…'}<span class="more">…</span>{:else}<span class="pdate">{tm}</span>{/if}
            {/each}
          </div>
          {#if dayTooLong}
            <p class="warn">{$t('recurrence.day.tooLong')}</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if period === 'range'}
    <div class="block">
      <span class="lbl">{$t('routines.schedule.activePeriod')}</span>
      {#if windowPresets.length}
        <div class="chips">
          {#each windowPresets as w (w.id)}
            <button type="button" class="chip" onclick={() => set(w.make())}>{w.label}</button>
          {/each}
        </div>
      {/if}
      <div class="row dates">
        <label class="inline">
          {$t('routines.schedule.from')}
          <input type="date" value={value.start ?? ''} oninput={(e) => set({ start: e.currentTarget.value })} />
        </label>
        <label class="inline">
          {$t('routines.schedule.to')}
          <input type="date" value={value.end ?? ''} oninput={(e) => set({ end: e.currentTarget.value })} />
        </label>
        {#if value.start || value.end}
          <button type="button" class="clear" onclick={() => set({ start: '', end: '' })}>
            <Icon name="x" size={12} /> {$t('routines.schedule.clearWindow')}
          </button>
        {/if}
      </div>
      {#if value.start && value.end && value.end < value.start}
        <p class="warn">{$t('routines.schedule.endBeforeStart')}</p>
      {/if}
    </div>
  {/if}

  {#if preview !== null}
    <div class="preview">
      <div class="ptop">
        <Icon name="calendar-days" size={13} />
        <strong>{summary || $t('remindme.form.nextFires')}</strong>
      </div>
      {#if preview.length > 0}
        <div class="pdates">
          {#each preview as d, i (i)}<span class="pdate">{fmtPreview(d)}</span>{/each}
        </div>
      {:else}
        <p class="warn">{$t('routines.schedule.neverDue')}</p>
      {/if}
    </div>
  {/if}
</fieldset>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    margin: 0;
    padding: 0;
    border: none;
    min-width: 0;
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .lbl {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--muted);
  }

  /* .chip and .chip.active come from the global component layer (theme/components.css),
     the same chip the rest of the app uses. Only the row spacing is local. */
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  /* The per-kind controls are inset so they read as "the detail of the choice above". */
  .detail {
    border-left: 2px solid var(--border);
    padding-left: var(--space-4);
    gap: var(--space-3);
  }

  /* Seven equal cells that shrink with the container. */
  .daybtns {
    display: flex;
    gap: var(--space-1);
    max-width: 320px;
  }
  .daybtns button {
    flex: 1 1 0;
    min-width: 0;
    height: 32px;
    border-radius: var(--radius);
    border: 0.5px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .daybtns button:hover {
    color: var(--text);
  }
  .daybtns button.on {
    color: var(--text);
    background: var(--surface-2);
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(34px, 1fr));
    gap: var(--space-1);
    max-width: 340px;
  }
  .grid button {
    height: 28px;
    border-radius: var(--radius);
    border: 0.5px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .grid button.on {
    color: var(--text);
    background: var(--surface-2);
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  .grid .last {
    grid-column: span 3;
  }

  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-3);
  }

  .row {
    display: flex;
    gap: var(--space-4);
    flex-wrap: wrap;
    align-items: flex-end;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .inline input[type='number'] {
    width: 68px;
  }
  /* The Dropdown trigger fills its parent, so an inline one needs a width to size to. */
  .pick {
    width: 150px;
  }
  .times {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
  }
  .times .more {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .dates {
    gap: var(--space-3);
  }
  .clear {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0;
  }
  .clear:hover {
    color: var(--red);
  }

  .warn {
    margin: 0;
    color: var(--amber);
    font-size: var(--text-xs);
  }

  .preview {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .ptop {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .pdates {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .pdate {
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    white-space: nowrap;
  }

  /* Phone: day and date controls get a 44 px tap height. */
  @media (max-width: 767px) {
    .detail {
      padding-left: var(--space-3);
    }
    .daybtns {
      max-width: none;
    }
    .daybtns button,
    .grid button,
    .chip {
      min-height: 44px;
    }
    .fields {
      grid-template-columns: repeat(auto-fill, minmax(min(100%, 130px), 1fr));
    }
    .fields input {
      width: 100%;
      min-width: 0;
      min-height: 44px;
    }
    .clear {
      min-height: 44px;
    }
  }
</style>
