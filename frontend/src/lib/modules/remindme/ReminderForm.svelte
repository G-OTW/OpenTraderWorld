<script>
  // Add/edit a reminder. Kind selects what it links to (goal/todo/custom); for goal/todo a
  // linked item must be chosen from the existing items loaded by the parent.
  import { KINDS, FREQUENCIES } from './api.js';
  import { t } from '$lib/i18n';

  let {
    initial = null,
    goals = [],
    todos = [],
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  let r = $state(blank());

  function blank() {
    const today = new Date().toISOString().slice(0, 10);
    const base = {
      name: '',
      kind: 'custom',
      linked_id: '',
      details: '',
      frequency: 'once',
      start_date: today,
      start_time: '09:00',
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
        start_date: initial.start_date ?? today,
        start_time: initial.start_time ?? '09:00',
        end_date: initial.end_date ?? '',
        max_count: initial.max_count ?? ''
      };
    }
    return base;
  }

  const linkOptions = $derived(r.kind === 'goal' ? goals : r.kind === 'todo' ? todos : []);

  // When switching to custom, drop any linked item.
  $effect(() => {
    if (r.kind === 'custom') r.linked_id = '';
  });

  function submit() {
    onsubmit({
      name: r.name,
      kind: r.kind,
      linked_id: r.kind === 'custom' ? null : r.linked_id || null,
      details: r.details,
      frequency: r.frequency,
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
    <label class="field">
      <span>{$t('remindme.form.type')}</span>
      <select bind:value={r.kind}>
        {#each KINDS as k}<option value={k.id}>{k.label}</option>{/each}
      </select>
    </label>

    {#if r.kind !== 'custom'}
      <label class="field">
        <span>{$t('remindme.form.linkedItem')}</span>
        <select bind:value={r.linked_id}>
          <option value="">{$t('remindme.form.pickOne')}</option>
          {#each linkOptions as o}<option value={o.id}>{o.name}</option>{/each}
        </select>
      </label>
    {/if}

    <label class="field">
      <span>{$t('remindme.form.frequency')}</span>
      <select bind:value={r.frequency}>
        {#each FREQUENCIES as f}<option value={f.id}>{f.label}</option>{/each}
      </select>
    </label>

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

  <label class="field wide">
    <span>{$t('remindme.form.details')}</span>
    <textarea bind:value={r.details} rows="2" placeholder={$t('remindme.form.optional')}></textarea>
  </label>

  <label class="check">
    <input type="checkbox" bind:checked={r.active} />
    <span>{$t('remindme.form.active')}</span>
  </label>

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $t('common.save') : $t('remindme.addReminder')}</button>
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
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 0.85rem;
  }
  .check input {
    width: 16px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
