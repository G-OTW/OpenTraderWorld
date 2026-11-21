<script>
  // A recurrence rule. Every field the backend needs, and nothing it does not: the next
  // occurrence is computed server-side, so this form never guesses a timezone offset.
  // The rule is edited in the shared RecurrencePicker; ./recurrence.js maps it to the shared
  // model and writes back only the fields the user moved (robot sessions bind their whole
  // row here, so nothing else on it may change).
  import RecurrencePicker from '$lib/ui/RecurrencePicker.svelte';
  import { onMount } from 'svelte';
  import { automatorApi, SCHEDULE_KINDS, guessTimezone } from '$lib/modules/automator/api.js';
  import { toModel, fromModel } from './recurrence.js';
  import { t } from '$lib/i18n';

  // `catchUp` false hides the catch-up box, for a rule whose missed occurrence means nothing.
  let { rule = $bindable({}), catchUp = true } = $props();

  let zones = $state([]);

  onMount(async () => {
    try {
      zones = await automatorApi.timezones();
    } catch {
      zones = [guessTimezone(), 'UTC'];
    }
  });

  // The API keeps hour and minute apart and a rule always carries both: a rule opened
  // without them gets the 08:00 the time field shows.
  $effect(() => {
    if (rule.at_hour == null) rule.at_hour = 8;
    if (rule.at_minute == null) rule.at_minute = 0;
  });

  const model = $derived(toModel(rule));

  function onchange(next) {
    const out = fromModel(next, rule);
    for (const key of Object.keys(out)) if (out[key] !== rule[key]) rule[key] = out[key];
  }

  const kinds = $derived(
    SCHEDULE_KINDS.map((k) => ({ value: k, label: $t(`automator.sched.kind.${k}`) }))
  );
</script>

<div class="stack">
  <RecurrencePicker
    value={model}
    {onchange}
    {kinds}
    monthDays="single"
    runAt
    time
    timezones={zones}
  />

  {#if catchUp}
  <label class="check">
    <input type="checkbox" bind:checked={rule.catch_up} />
    <span>
      {$t('automator.sched.catchUp')}
      <em>{$t('automator.sched.catchUpHint')}</em>
    </span>
  </label>
  {/if}
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .check {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    font-size: var(--text-sm);
  }
  .check em {
    display: block;
    font-style: normal;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
