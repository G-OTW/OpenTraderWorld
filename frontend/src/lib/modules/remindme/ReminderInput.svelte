<script>
  // The reminder fields, shared by every place a reminder is made: the RemindMe form, the
  // bell button on other pages (QuickReminderButton), the dashboard widget, and the
  // calendar event form. Kind selects what it links to (goal/todo/custom); for goal/todo a
  // linked item must be chosen from the existing items loaded by the parent. The schedule
  // is the shared RecurrencePicker; defaults, validation and the payload live in ./draft.js.
  //
  // `locked`: the reminder is derived from something else (a calendar event it follows),
  // so its fields are shown read-only, and the parts that caller never sets are left out.
  import { KINDS, FREQUENCIES, maskForDate } from './api.js';
  import { linkInvalid, normalizeUrl, startsInPast } from './draft.js';
  import { toModel, fromModel, nextFires } from './recurrence.js';
  import RecurrencePicker from '$lib/ui/RecurrencePicker.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';

  let {
    value = $bindable(),
    goals = [],
    todos = [],
    locked = false,
    autofocus = true
  } = $props();

  const linkOptions = $derived(value.kind === 'goal' ? goals : value.kind === 'todo' ? todos : []);

  // When switching to custom, drop any linked item.
  $effect(() => {
    if (value.kind === 'custom' && value.linked_id) value.linked_id = '';
  });

  // ── Weekly day picker ──
  // Until the user touches a day, the selection tracks the start date's weekday: picking
  // "Weekly" on a Tuesday means "every Tuesday" without a second click.
  let daysTouched = $state(!!value?.weekdays);

  $effect(() => {
    if (value.frequency !== 'weekly' || daysTouched) return;
    const mask = maskForDate(value.start_date);
    if (value.weekdays !== mask) value.weekdays = mask;
  });

  const model = $derived(toModel(value));

  function onRecurrence(next) {
    if (next.weekdays !== model.weekdays) daysTouched = true;
    const out = fromModel(next, value);
    for (const key of Object.keys(out)) if (out[key] !== value[key]) value[key] = out[key];
  }

  const kindLabel = (id) =>
    id === 'yearly' ? $t('recurrence.kind.yearly') : $t(`routines.schedule.kind.${id}`);
  const kinds = $derived(FREQUENCIES.map((f) => ({ value: f.id, label: kindLabel(f.id) })));

  // The next few fires of a weekly reminder, so the user sees what "Mon, Thu" means.
  const fires = $derived(nextFires(value));

  const pastStart = $derived(startsInPast(value));
  const badLink = $derived(linkInvalid(value));
</script>

<fieldset class="rem-input" disabled={locked}>
  <label class="field">
    <span>{$t('remindme.form.name')}</span>
    {#if autofocus && !locked}
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={value.name} autofocus placeholder={$t('remindme.form.namePlaceholder')} />
    {:else}
      <input bind:value={value.name} placeholder={$t('remindme.form.namePlaceholder')} />
    {/if}
  </label>

  {#if !locked}
    <div class="grid">
      <div class="field">
        <span>{$t('remindme.form.type')}</span>
        <Dropdown
          bind:value={value.kind}
          ariaLabel={$t('remindme.form.type')}
          options={KINDS.map((k) => ({ value: k.id, label: k.label }))}
        />
      </div>

      {#if value.kind !== 'custom'}
        <div class="field">
          <span>{$t('remindme.form.linkedItem')}</span>
          <Dropdown
            bind:value={value.linked_id}
            ariaLabel={$t('remindme.form.linkedItem')}
            placeholder={$t('remindme.form.pickOne')}
            options={[
              { value: '', label: $t('remindme.form.pickOne') },
              ...linkOptions.map((o) => ({ value: o.id, label: o.name }))
            ]}
          />
        </div>
      {/if}
    </div>
  {/if}

  <RecurrencePicker
    value={model}
    onchange={onRecurrence}
    {kinds}
    anchor
    time
    period="end"
    maxCount
    preview={fires.length ? fires : null}
    disabled={locked}
  />

  {#if pastStart}
    <p class="warn">{$t('remindme.form.pastStartWarning')}</p>
  {/if}

  <label class="field">
    <span>{$t('remindme.form.details')}</span>
    <textarea bind:value={value.details} rows="2" placeholder={$t('remindme.form.optional')}></textarea>
  </label>

  {#if !locked}
    <!-- One link the reminder is about, carried onto its notifications and channel messages. -->
    <div class="linkrow">
      <label class="field grow">
        <span>{$t('remindme.form.link')}</span>
        <input type="url" inputmode="url" bind:value={value.url} placeholder="https://" />
      </label>
      <label class="field grow">
        <span>{$t('remindme.form.linkLabel')}</span>
        <input
          bind:value={value.link_label}
          placeholder={$t('remindme.form.linkLabelPlaceholder')}
          disabled={!value.url.trim()}
        />
      </label>
    </div>
    {#if badLink}
      <p class="warn">{$t('remindme.form.linkInvalid')}</p>
    {:else if value.url.trim()}
      <a class="preview-link" href={normalizeUrl(value.url)} target="_blank" rel="noopener noreferrer nofollow">
        <Icon name="external-link" size={12} />
        {value.link_label.trim() || normalizeUrl(value.url)}
      </a>
    {/if}

    <label class="check">
      <input type="checkbox" bind:checked={value.active} />
      <span>{$t('remindme.form.active')}</span>
    </label>
  {/if}
</fieldset>

<style>
  .rem-input {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    border: none;
    min-width: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-3);
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
  @media (max-width: 767px) {
    .grid { grid-template-columns: repeat(auto-fill, minmax(min(100%, 130px), 1fr)); }
    .preview-link { white-space: normal; overflow-wrap: anywhere; }
  }
</style>
