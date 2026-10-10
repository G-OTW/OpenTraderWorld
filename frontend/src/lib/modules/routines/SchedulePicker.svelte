<script>
  // The recurrence editor: presets first, then the fine controls for whichever rule kind
  // is selected, then an optional active window — and a live preview of the next dates so
  // the user never has to guess what a rule means.
  //
  // Presets carry most of the traffic ("weekdays", "every Friday", "1st of the month"), so
  // they sit on top as one-click answers; the detailed controls below stay open for the
  // cases a preset cannot express. Editing a control simply stops matching a preset — no
  // mode switch, no lost state.
  import {
    WEEKDAYS,
    SCHEDULE_KINDS,
    MASK_EVERY_DAY,
    MASK_WEEKDAYS,
    MASK_WEEKEND,
    schedulePresets,
    windowPresets,
    nextOccurrences,
    describeSchedule,
    todayStr
  } from './api.js';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t, locale } from '$lib/i18n';

  // `schedule` is the rule object; `startDate`/`endDate` are the active window, all bound
  // back to the parent form so saving reads one source of truth.
  let {
    schedule = $bindable({ kind: 'weekly', weekdays: MASK_WEEKDAYS, interval: 1 }),
    startDate = $bindable(''),
    endDate = $bindable('')
  } = $props();

  const presets = $derived(schedulePresets($t));
  const windows = $derived(windowPresets($t));

  // A preset is "active" when the current rule is exactly what it would produce — so the
  // chips keep reflecting the state after a reload, not just right after a click.
  function sameRule(a, b) {
    return JSON.stringify(normalize(a)) === JSON.stringify(normalize(b));
  }
  function normalize(s) {
    if (!s) return null;
    if (s.kind === 'weekly') return { kind: 'weekly', weekdays: s.weekdays, interval: s.interval ?? 1 };
    if (s.kind === 'daily') return { kind: 'daily', interval: s.interval ?? 1 };
    if (s.kind === 'monthly') return { kind: 'monthly', days: [...(s.days ?? [])].sort((x, y) => x - y) };
    return s;
  }

  function applyPreset(p) {
    schedule = p.make();
  }

  // Switching kind builds a sensible rule of that kind rather than clearing the row, so
  // the preview never goes blank mid-edit.
  function setKind(kind) {
    if (schedule.kind === kind) return;
    if (kind === 'weekly') schedule = { kind, weekdays: MASK_WEEKDAYS, interval: 1 };
    else if (kind === 'daily') schedule = { kind, interval: 1 };
    else if (kind === 'monthly') schedule = { kind, days: [1] };
    else if (kind === 'nth_weekday') schedule = { kind, weekday: 0, nth: 1 };
    else if (kind === 'once') schedule = { kind, date: startDate || todayStr() };
  }

  function toggleDay(bit) {
    const next = (schedule.weekdays ?? 0) ^ bit;
    if (next === 0) return; // a rule with no day would never fire
    schedule = { ...schedule, weekdays: next };
  }

  function setMask(mask) {
    schedule = { ...schedule, weekdays: mask };
  }

  function toggleMonthDay(day) {
    const days = schedule.days ?? [];
    const next = days.includes(day) ? days.filter((d) => d !== day) : [...days, day];
    if (next.length === 0) return;
    schedule = { ...schedule, days: next.sort((a, b) => a - b) };
  }

  function applyWindow(w) {
    const { start_date, end_date } = w.make();
    startDate = start_date ?? '';
    endDate = end_date ?? '';
  }

  // The preview is computed over the same evaluator the board uses, so what it lists is
  // what will actually come due.
  const preview = $derived(
    nextOccurrences(
      { schedule, start_date: startDate || null, end_date: endDate || null },
      new Date(),
      6
    )
  );

  const summary = $derived(describeSchedule({ schedule }, $t));

  const monthDays = Array.from({ length: 31 }, (_, i) => i + 1);
  const ordinals = [1, 2, 3, 4, 5, -1];

  function fmtPreview(iso) {
    const d = new Date(`${iso}T00:00:00`);
    return d.toLocaleDateString($locale, { weekday: 'short', day: 'numeric', month: 'short' });
  }
</script>

<div class="picker">
  <!-- ── Presets ── -->
  <div class="block">
    <span class="lbl">{$t('routines.schedule.quickPick')}</span>
    <div class="chips">
      {#each presets as p (p.id)}
        <button
          type="button"
          class="chip"
          class:active={sameRule(schedule, p.make())}
          onclick={() => applyPreset(p)}
        >
          {p.label}
        </button>
      {/each}
    </div>
  </div>

  <!-- ── Rule kind ── -->
  <div class="block">
    <span class="lbl">{$t('routines.schedule.repeats')}</span>
    <div class="chips">
      {#each SCHEDULE_KINDS as k (k)}
        <button type="button" class="chip" class:active={schedule.kind === k} onclick={() => setKind(k)}>
          {$t(`routines.schedule.kind.${k}`)}
        </button>
      {/each}
    </div>
  </div>

  <!-- ── Per-kind controls ── -->
  <div class="block detail">
    {#if schedule.kind === 'weekly'}
      <div class="daybtns" role="group" aria-label={$t('routines.schedule.daysOfWeek')}>
        {#each WEEKDAYS as d (d.bit)}
          <button
            type="button"
            class:on={((schedule.weekdays ?? 0) & d.bit) !== 0}
            aria-pressed={((schedule.weekdays ?? 0) & d.bit) !== 0}
            onclick={() => toggleDay(d.bit)}
          >
            {$t(`routines.weekdayShort.${d.key}`)}
          </button>
        {/each}
      </div>
      <div class="chips small">
        <button type="button" class="chip" onclick={() => setMask(MASK_WEEKDAYS)}>{$t('routines.schedule.weekdays')}</button>
        <button type="button" class="chip" onclick={() => setMask(MASK_WEEKEND)}>{$t('routines.schedule.weekend')}</button>
        <button type="button" class="chip" onclick={() => setMask(MASK_EVERY_DAY)}>{$t('routines.schedule.everyDay')}</button>
      </div>
      <label class="inline">
        {$t('routines.schedule.everyNWeeks')}
        <input
          type="number"
          min="1"
          max="52"
          value={schedule.interval ?? 1}
          oninput={(e) =>
            (schedule = { ...schedule, interval: Math.max(1, Number(e.currentTarget.value) || 1) })}
        />
      </label>
    {:else if schedule.kind === 'daily'}
      <label class="inline">
        {$t('routines.schedule.everyNDaysLabel')}
        <input
          type="number"
          min="1"
          max="365"
          value={schedule.interval ?? 1}
          oninput={(e) =>
            (schedule = { ...schedule, interval: Math.max(1, Number(e.currentTarget.value) || 1) })}
        />
      </label>
    {:else if schedule.kind === 'monthly'}
      <span class="hint">{$t('routines.schedule.pickMonthDays')}</span>
      <div class="grid" role="group" aria-label={$t('routines.schedule.pickMonthDays')}>
        {#each monthDays as d (d)}
          <button
            type="button"
            class:on={(schedule.days ?? []).includes(d)}
            aria-pressed={(schedule.days ?? []).includes(d)}
            onclick={() => toggleMonthDay(d)}
          >
            {d}
          </button>
        {/each}
        <button
          type="button"
          class="last"
          class:on={(schedule.days ?? []).includes(-1)}
          aria-pressed={(schedule.days ?? []).includes(-1)}
          onclick={() => toggleMonthDay(-1)}
        >
          {$t('routines.schedule.lastDay')}
        </button>
      </div>
    {:else if schedule.kind === 'nth_weekday'}
      <div class="row">
        <div class="inline">
          <span>{$t('routines.schedule.which')}</span>
          <div class="pick">
            <Dropdown
              value={String(schedule.nth)}
              ariaLabel={$t('routines.schedule.which')}
              options={ordinals.map((n) => ({
                value: String(n),
                label: n === -1 ? $t('routines.schedule.ordLast') : $t(`routines.schedule.ord${n}`)
              }))}
              onpick={(v) => (schedule = { ...schedule, nth: Number(v) })}
            />
          </div>
        </div>
        <div class="inline">
          <span>{$t('routines.schedule.dayOfWeek')}</span>
          <div class="pick">
            <Dropdown
              value={String(schedule.weekday)}
              ariaLabel={$t('routines.schedule.dayOfWeek')}
              options={WEEKDAYS.map((d) => ({
                value: String(d.index),
                label: $t(`routines.weekday.${d.key}`)
              }))}
              onpick={(v) => (schedule = { ...schedule, weekday: Number(v) })}
            />
          </div>
        </div>
      </div>
    {:else if schedule.kind === 'once'}
      <label class="inline">
        {$t('routines.schedule.onDate')}
        <input
          type="date"
          value={schedule.date ?? todayStr()}
          oninput={(e) => (schedule = { ...schedule, date: e.currentTarget.value })}
        />
      </label>
    {/if}
  </div>

  <!-- ── Active window ── -->
  <div class="block">
    <span class="lbl">{$t('routines.schedule.activePeriod')}</span>
    <div class="chips">
      {#each windows as w (w.id)}
        <button type="button" class="chip" onclick={() => applyWindow(w)}>{w.label}</button>
      {/each}
    </div>
    <div class="row dates">
      <label class="inline">
        {$t('routines.schedule.from')}
        <input type="date" bind:value={startDate} />
      </label>
      <label class="inline">
        {$t('routines.schedule.to')}
        <input type="date" bind:value={endDate} />
      </label>
      {#if startDate || endDate}
        <button type="button" class="clear" onclick={() => { startDate = ''; endDate = ''; }}>
          <Icon name="x" size={12} /> {$t('routines.schedule.clearWindow')}
        </button>
      {/if}
    </div>
    {#if startDate && endDate && endDate < startDate}
      <p class="warn">{$t('routines.schedule.endBeforeStart')}</p>
    {/if}
  </div>

  <!-- ── Live preview ── -->
  <div class="preview">
    <div class="ptop">
      <Icon name="calendar-days" size={13} />
      <strong>{summary}</strong>
    </div>
    {#if preview.length > 0}
      <div class="pdates">
        {#each preview as d (d)}<span class="pdate">{fmtPreview(d)}</span>{/each}
      </div>
    {:else}
      <p class="warn">{$t('routines.schedule.neverDue')}</p>
    {/if}
  </div>
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
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

  /* .chip and .chip.active come from the global component layer (theme/components.css) —
     the same chip the rest of the app uses. Only the row spacing is local. */
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  /* The per-kind controls are inset so it reads as "the detail of the choice above". */
  .detail {
    border-left: 2px solid var(--border);
    padding-left: var(--space-4);
    gap: var(--space-3);
  }

  .daybtns {
    display: flex;
    gap: var(--space-1);
  }
  .daybtns button {
    width: 40px;
    height: 32px;
    border-radius: var(--radius);
    border: 0.5px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
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
</style>
