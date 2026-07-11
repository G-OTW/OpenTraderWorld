<script>
  // Add/edit a personal calendar event. Toggles between all-day (date inputs) and
  // timed (datetime-local inputs); emits RFC3339 start/end on submit.
  import {
    localToRfc3339,
    rfc3339ToLocal,
    rfc3339ToDate,
    dateToRfc3339
  } from '$lib/modules/calendar/api.js';
  import { t } from '$lib/i18n';

  let {
    initial = null,
    // True when creating (add button or empty-slot select); false when editing an
    // existing event. `initial` is also set for empty-slot creates (date prefill),
    // so it can't distinguish create vs edit on its own.
    isNew = true,
    // True when a reminder is already linked to this event (edit mode) — the box
    // is shown checked+disabled with an "already exists" note, so we never dupe.
    hasReminder = false,
    categories = [],
    onsubmit = () => {},
    oncancel = () => {},
    ondelete = null
  } = $props();

  // "Also create a reminder at the start time." Pre-checked (and locked) when one
  // already exists for this event.
  let withReminder = $state(hasReminder);

  const PALETTE = ['', '#4f8ef7', '#22c55e', '#f59e0b', '#ef4444', '#a855f7', '#14b8a6'];

  let e = $state(blank());

  function blank() {
    if (initial) {
      const allDay = !!initial.all_day;
      return {
        title: initial.title ?? '',
        all_day: allDay,
        start: allDay ? rfc3339ToDate(initial.start_at) : rfc3339ToLocal(initial.start_at),
        end: initial.end_at
          ? allDay
            ? rfc3339ToDate(initial.end_at)
            : rfc3339ToLocal(initial.end_at)
          : '',
        category: initial.category ?? '',
        color: initial.color ?? '',
        location: initial.location ?? '',
        notes: initial.notes ?? ''
      };
    }
    // Default new event: today, all-day.
    const today = new Date();
    const pad = (n) => String(n).padStart(2, '0');
    const d = `${today.getFullYear()}-${pad(today.getMonth() + 1)}-${pad(today.getDate())}`;
    return {
      title: '',
      all_day: true,
      start: d,
      end: '',
      category: '',
      color: '',
      location: '',
      notes: ''
    };
  }

  // When switching all-day on/off, convert the in-progress values so they stay valid.
  function toggleAllDay() {
    const next = !e.all_day;
    if (next) {
      e.start = e.start ? e.start.slice(0, 10) : '';
      e.end = e.end ? e.end.slice(0, 10) : '';
    } else {
      e.start = e.start ? `${e.start}T09:00` : '';
      e.end = e.end ? `${e.end}T10:00` : '';
    }
    e.all_day = next;
  }

  function submit() {
    const conv = e.all_day ? dateToRfc3339 : localToRfc3339;
    onsubmit(
      {
        title: e.title,
        start_at: conv(e.start),
        end_at: e.end ? conv(e.end) : null,
        all_day: e.all_day,
        category: (e.category || '').trim(),
        color: e.color,
        location: e.location,
        notes: e.notes
      },
      // Second arg: also create a linked reminder at the start time (add or edit).
      withReminder
    );
  }
</script>

<form
  class="ev-form"
  onsubmit={(ev) => {
    ev.preventDefault();
    submit();
  }}
>
  <label class="field">
    <span>{$t('calendar.form.title')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={e.title} autofocus placeholder={$t('calendar.form.titlePlaceholder')} />
  </label>

  <label class="checkrow">
    <input type="checkbox" checked={e.all_day} onchange={toggleAllDay} />
    <span>{$t('calendar.form.allDay')}</span>
  </label>

  <div class="row">
    <label class="field">
      <span>{$t('calendar.form.start')}</span>
      {#if e.all_day}
        <input type="date" bind:value={e.start} />
      {:else}
        <input type="datetime-local" bind:value={e.start} />
      {/if}
    </label>
    <label class="field">
      <span>{$t('calendar.form.end')}</span>
      {#if e.all_day}
        <input type="date" bind:value={e.end} />
      {:else}
        <input type="datetime-local" bind:value={e.end} />
      {/if}
    </label>
  </div>

  <div class="row">
    <label class="field">
      <span>{$t('calendar.form.category')}</span>
      <input bind:value={e.category} list="cal-categories" placeholder={$t('calendar.form.categoryPlaceholder')} />
      <datalist id="cal-categories">
        {#each categories as c}<option value={c}></option>{/each}
      </datalist>
    </label>
    <div class="field">
      <span>{$t('calendar.form.colour')}</span>
      <div class="swatches">
        {#each PALETTE as c}
          <button
            type="button"
            class="swatch"
            class:active={e.color === c}
            class:none={c === ''}
            style={c ? `background:${c}` : ''}
            title={c || $t('calendar.form.default')}
            onclick={() => (e.color = c)}
            aria-label={c || $t('calendar.form.defaultColour')}
          ></button>
        {/each}
      </div>
    </div>
  </div>

  <label class="field">
    <span>{$t('calendar.form.location')}</span>
    <input bind:value={e.location} placeholder={$t('calendar.form.optional')} />
  </label>

  <label class="field">
    <span>{$t('calendar.form.notes')}</span>
    <textarea bind:value={e.notes} rows="3" placeholder={$t('calendar.form.optional')}></textarea>
  </label>

  <label class="checkrow remind" class:exists={hasReminder}>
    <input type="checkbox" bind:checked={withReminder} disabled={hasReminder} />
    <span>
      🔔 {hasReminder
        ? $t('calendar.form.reminderExists')
        : $t('calendar.form.reminderCreate')}
    </span>
  </label>

  <div class="actions">
    {#if ondelete}
      <button type="button" class="danger" onclick={ondelete}>{$t('calendar.form.delete')}</button>
    {/if}
    <span class="spacer"></span>
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{isNew ? $t('calendar.form.addEvent') : $t('common.save')}</button>
  </div>
</form>

<style>
  .ev-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
  }
  .row .field {
    flex: 1;
  }
  .checkrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
  }
  .checkrow input {
    width: auto;
  }
  .checkrow.remind {
    padding: 8px 10px;
    background: color-mix(in srgb, var(--amber) 10%, var(--surface-2));
    border: 1px solid color-mix(in srgb, var(--amber) 30%, var(--border));
    border-radius: var(--radius);
  }
  /* Already-linked state: greener, calmer, non-actionable. */
  .checkrow.remind.exists {
    background: color-mix(in srgb, var(--green) 10%, var(--surface-2));
    border-color: color-mix(in srgb, var(--green) 30%, var(--border));
    color: var(--muted);
    cursor: default;
  }
  .swatches {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .swatch {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 2px solid var(--border);
    cursor: pointer;
    padding: 0;
  }
  .swatch.none {
    background: var(--surface-2);
    position: relative;
  }
  .swatch.none::after {
    content: '⊘';
    color: var(--muted);
    font-size: var(--text-xs);
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  .swatch.active {
    border-color: var(--text);
    box-shadow: 0 0 0 2px var(--surface), 0 0 0 4px var(--accent);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .spacer {
    flex: 1;
  }
</style>
