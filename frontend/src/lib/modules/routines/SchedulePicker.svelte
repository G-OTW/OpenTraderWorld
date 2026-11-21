<script>
  // The routine recurrence editor: the shared RecurrencePicker with everything a routine's
  // schedule can hold (presets, every N days/weeks, several month days, nth weekday, a once
  // date, an active window) and a live preview of the next dates, so the user never has to
  // guess what a rule means. The adapter in ./recurrence.js maps the stored schedule to the
  // shared model and back.
  import RecurrencePicker from '$lib/ui/RecurrencePicker.svelte';
  import {
    MASK_WEEKDAYS,
    SCHEDULE_KINDS,
    schedulePresets,
    windowPresets,
    nextOccurrences,
    describeSchedule
  } from './api.js';
  import { toModel, fromModel, ruleOf, kindDefaults } from './recurrence.js';
  import { t } from '$lib/i18n';

  // `schedule` is the rule object; `startDate`/`endDate` are the active window, all bound
  // back to the parent form so saving reads one source of truth.
  let {
    schedule = $bindable({ kind: 'weekly', weekdays: MASK_WEEKDAYS, interval: 1 }),
    startDate = $bindable(''),
    endDate = $bindable('')
  } = $props();

  const model = $derived(toModel({ schedule, start_date: startDate, end_date: endDate }));

  function onchange(next) {
    const out = fromModel(next, { schedule, start_date: startDate, end_date: endDate });
    if (out.schedule !== schedule) schedule = out.schedule;
    if (out.start_date !== startDate) startDate = out.start_date ?? '';
    if (out.end_date !== endDate) endDate = out.end_date ?? '';
  }

  const kinds = $derived(SCHEDULE_KINDS.map((k) => ({ value: k, label: $t(`routines.schedule.kind.${k}`) })));
  const presets = $derived(
    schedulePresets($t).map((p) => ({ id: p.id, label: p.label, rule: ruleOf(p.make()) }))
  );
  const windows = $derived(
    windowPresets($t).map((w) => ({
      id: w.id,
      label: w.label,
      make: () => {
        const { start_date, end_date } = w.make();
        return { start: start_date ?? '', end: end_date ?? '' };
      }
    }))
  );

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
</script>

<RecurrencePicker
  value={model}
  {onchange}
  {kinds}
  {presets}
  {kindDefaults}
  interval
  monthDays="multi"
  onceDate
  period="range"
  windowPresets={windows}
  {preview}
  {summary}
/>
