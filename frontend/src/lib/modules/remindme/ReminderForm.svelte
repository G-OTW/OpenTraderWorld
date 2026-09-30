<script>
  // Add/edit a reminder. Kind selects what it links to (goal/todo/custom); for goal/todo a
  // linked item must be chosen from the existing items loaded by the parent.
  import {
    KINDS,
    FREQUENCIES,
    WEEKDAYS,
    MASK_EVERY_DAY,
    MASK_WEEKDAYS,
    MASK_WEEKEND,
    maskForDate
  } from './api.js';
  import { dateKey } from '$lib/format.js';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';

  let {
    initial = null,
    goals = [],
    todos = [],
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  const DEFAULT_TIME = '09:00';

  let r = $state(blank());

  /** Local day the default time still lies ahead: today before 09:00, else tomorrow. */
  function defaultStartDate() {
    const now = new Date();
    const [h, m] = DEFAULT_TIME.split(':').map(Number);
    const at = new Date(now);
    at.setHours(h, m, 0, 0);
    if (at > now) return dateKey(now);
    const tomorrow = new Date(now);
    tomorrow.setDate(tomorrow.getDate() + 1);
    return dateKey(tomorrow);
  }

  function blank() {
    // Local day, not UTC — a reminder created at 23:30 must default to today, not tomorrow.
    const today = dateKey();
    const base = {
      name: '',
      kind: 'custom',
      linked_id: '',
      details: '',
      url: '',
      link_label: '',
      frequency: 'once',
      weekdays: null,
      start_date: defaultStartDate(),
      start_time: DEFAULT_TIME,
      end_date: '',
      max_count: '',
      active: true
    };
    if (initial) {
      return {
        ...base,
        ...initial,
        linked_id: initial.linked_id ?? '',
        details: initial.details ?? '',
        url: initial.url ?? '',
        link_label: initial.link_label ?? '',
        weekdays: initial.weekdays ?? null,
        start_date: initial.start_date ?? today,
        start_time: initial.start_time ?? DEFAULT_TIME,
        end_date: initial.end_date ?? '',
        max_count: initial.max_count ?? ''
      };
    }
    return base;
  }

  // A `once` reminder whose start already passed fires immediately on the next tick —
  // warn rather than let that look like a scheduling bug.
  const startsInPast = $derived.by(() => {
    if (r.frequency !== 'once' || !r.active || !r.start_date) return false;
    const at = new Date(`${r.start_date}T${r.start_time || '00:00'}`);
    return !Number.isNaN(at.getTime()) && at < new Date();
  });

  const linkOptions = $derived(r.kind === 'goal' ? goals : r.kind === 'todo' ? todos : []);

  // When switching to custom, drop any linked item.
  $effect(() => {
    if (r.kind === 'custom') r.linked_id = '';
  });

  // ── Weekly day picker ──
  // Until the user touches a day, the selection tracks the start date's weekday — picking
  // "Weekly" on a Tuesday means "every Tuesday" without a second click.
  let daysTouched = $state(!!initial?.weekdays);

  $effect(() => {
    if (r.frequency === 'weekly' && !daysTouched) r.weekdays = maskForDate(r.start_date);
  });

  function toggleDay(bit) {
    const next = (r.weekdays ?? 0) ^ bit;
    if (next === 0) return; // a weekly with no day would never fire
    daysTouched = true;
    r.weekdays = next;
  }

  function setMask(mask) {
    daysTouched = true;
    r.weekdays = mask;
  }

  // The next few fires, computed the same way the server walks the mask, so the user sees
  // what "Mon, Thu" actually means before saving.
  const nextDates = $derived.by(() => {
    const mask = r.frequency === 'weekly' ? (r.weekdays ?? 0) : 0;
    if (!mask || !r.start_date) return [];
    const [h, min] = (r.start_time || '00:00').split(':').map(Number);
    const cursor = new Date(`${r.start_date}T00:00:00`);
    if (Number.isNaN(cursor.getTime())) return [];
    cursor.setHours(h || 0, min || 0, 0, 0);
    const end = r.end_date ? new Date(`${r.end_date}T23:59:59`) : null;
    const now = new Date();
    const out = [];
    for (let i = 0; i < 400 && out.length < 4; i++) {
      const bit = 1 << ((cursor.getDay() + 6) % 7);
      if (mask & bit && cursor >= now) {
        if (end && cursor > end) break;
        out.push(new Date(cursor));
      }
      cursor.setDate(cursor.getDate() + 1);
    }
    return out;
  });

  const fmtNext = (d) =>
    d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });

  // The browser accepts a bare "example.com" in a text input; https:// is what the server
  // requires and what the user meant, so add it rather than rejecting the input.
  function normalizeUrl(u) {
    const s = (u ?? '').trim();
    if (!s || /^https?:\/\//i.test(s)) return s;
    return `https://${s}`;
  }

  const linkInvalid = $derived(!!r.url.trim() && !/^https?:\/\/\S+$/i.test(normalizeUrl(r.url)));

  function submit() {
    onsubmit({
      name: r.name,
      kind: r.kind,
      linked_id: r.kind === 'custom' ? null : r.linked_id || null,
      details: r.details,
      url: normalizeUrl(r.url),
      link_label: (r.link_label ?? '').trim(),
      frequency: r.frequency,
      weekdays: r.frequency === 'weekly' ? (r.weekdays || null) : null,
      start_date: r.start_date || null,
      start_time: r.start_time || '00:00',
      // Browser UTC offset in minutes (getTimezoneOffset is inverted, so negate it),
      // so the chosen local time fires at the right instant server-side.
      tz_offset_minutes: -new Date().getTimezoneOffset(),
      end_date: r.end_date || null,
      max_count: r.max_count === '' || r.max_count === null ? null : Number(r.max_count),
      active: !!r.active
    });
  }
</script>

<form
  class="rem-form"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <label class="field wide">
    <span>{$t('remindme.form.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={r.name} autofocus placeholder={$t('remindme.form.namePlaceholder')} />
  </label>

  <div class="grid">
    <div class="field">
      <span>{$t('remindme.form.type')}</span>
      <Dropdown
        bind:value={r.kind}
        ariaLabel={$t('remindme.form.type')}
        options={KINDS.map((k) => ({ value: k.id, label: k.label }))}
      />
    </div>

    {#if r.kind !== 'custom'}
      <div class="field">
        <span>{$t('remindme.form.linkedItem')}</span>
        <Dropdown
          bind:value={r.linked_id}
          ariaLabel={$t('remindme.form.linkedItem')}
          placeholder={$t('remindme.form.pickOne')}
          options={[
            { value: '', label: $t('remindme.form.pickOne') },
            ...linkOptions.map((o) => ({ value: o.id, label: o.name }))
          ]}
        />
      </div>
    {/if}

    <div class="field">
      <span>{$t('remindme.form.frequency')}</span>
      <Dropdown
        bind:value={r.frequency}
        ariaLabel={$t('remindme.form.frequency')}
        options={FREQUENCIES.map((f) => ({ value: f.id, label: f.label }))}
      />
    </div>

    <label class="field">
      <span>{$t('remindme.form.startDate')}</span>
      <input type="date" bind:value={r.start_date} />
    </label>

    <label class="field">
      <span>{$t('remindme.form.time')}</span>
      <input type="time" bind:value={r.start_time} />
    </label>

    {#if r.frequency !== 'once'}
      <label class="field">
        <span>{$t('remindme.form.endDate')}</span>
        <input type="date" bind:value={r.end_date} />
      </label>
      <label class="field">
        <span>{$t('remindme.form.maxCount')}</span>
        <input type="number" min="1" step="1" bind:value={r.max_count} placeholder="∞" />
      </label>
    {/if}
  </div>

  {#if r.frequency === 'weekly'}
    <div class="days">
      <span class="lbl">{$t('remindme.form.repeatOn')}</span>
      <div class="daybtns" role="group" aria-label={$t('remindme.form.repeatOn')}>
        {#each WEEKDAYS as d (d.bit)}
          <button
            type="button"
            class:on={((r.weekdays ?? 0) & d.bit) !== 0}
            aria-pressed={((r.weekdays ?? 0) & d.bit) !== 0}
            onclick={() => toggleDay(d.bit)}
          >
            {$t(`routines.weekdayShort.${d.key}`)}
          </button>
        {/each}
      </div>
      <div class="chips">
        <button type="button" class="chip" class:active={r.weekdays === MASK_WEEKDAYS} onclick={() => setMask(MASK_WEEKDAYS)}>{$t('remindme.form.weekdays')}</button>
        <button type="button" class="chip" class:active={r.weekdays === MASK_WEEKEND} onclick={() => setMask(MASK_WEEKEND)}>{$t('remindme.form.weekend')}</button>
        <button type="button" class="chip" class:active={r.weekdays === MASK_EVERY_DAY} onclick={() => setMask(MASK_EVERY_DAY)}>{$t('remindme.form.everyDay')}</button>
      </div>
      {#if nextDates.length > 0}
        <div class="next">
          <span>{$t('remindme.form.nextFires')}</span>
          {#each nextDates as d (d.getTime())}<span class="pdate">{fmtNext(d)}</span>{/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if startsInPast}
    <p class="warn">{$t('remindme.form.pastStartWarning')}</p>
  {/if}

  <label class="field wide">
    <span>{$t('remindme.form.details')}</span>
    <textarea bind:value={r.details} rows="2" placeholder={$t('remindme.form.optional')}></textarea>
  </label>

  <!-- One link the reminder is about — carried onto its notifications and channel messages. -->
  <div class="linkrow">
    <label class="field grow">
      <span>{$t('remindme.form.link')}</span>
      <input type="url" inputmode="url" bind:value={r.url} placeholder="https://" />
    </label>
    <label class="field grow">
      <span>{$t('remindme.form.linkLabel')}</span>
      <input
        bind:value={r.link_label}
        placeholder={$t('remindme.form.linkLabelPlaceholder')}
        disabled={!r.url.trim()}
      />
    </label>
  </div>
  {#if linkInvalid}
    <p class="warn">{$t('remindme.form.linkInvalid')}</p>
  {:else if r.url.trim()}
    <a class="preview-link" href={normalizeUrl(r.url)} target="_blank" rel="noopener noreferrer nofollow">
      <Icon name="external-link" size={12} />
      {r.link_label.trim() || normalizeUrl(r.url)}
    </a>
  {/if}

  <label class="check">
    <input type="checkbox" bind:checked={r.active} />
    <span>{$t('remindme.form.active')}</span>
  </label>

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary" disabled={linkInvalid}>{initial ? $t('common.save') : $t('remindme.addReminder')}</button>
  </div>
</form>

<style>
  .rem-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-3);
  }
  .field.wide {
    width: 100%;
  }
  .linkrow {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .linkrow .grow {
    flex: 1 1 200px;
    min-width: 0;
  }
  .preview-link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    align-self: flex-start;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-xs);
    color: var(--accent);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-base);
  }

  /* Weekly day picker: 7 equal cells that shrink with the modal, presets under them. */
  .days {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-left: 2px solid var(--border);
    padding-left: var(--space-4);
  }
  .lbl {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .daybtns {
    display: flex;
    gap: var(--space-1);
  }
  .daybtns button {
    flex: 1;
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
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .next {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .next span:first-child {
    margin-right: var(--space-1);
  }
  .pdate {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    white-space: nowrap;
  }
  .check input {
    width: 16px;
  }
  .warn {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--amber);
    border-radius: var(--radius);
    color: var(--amber);
    font-size: var(--text-sm);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
