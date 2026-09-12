<script>
  // A recurrence rule. Every field the backend needs, and nothing it does not: the next
  // occurrence is computed server-side, so this form never guesses a timezone offset.
  import Select from '$lib/ui/Select.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { onMount } from 'svelte';
  import {
    automatorApi,
    SCHEDULE_KINDS,
    WEEKDAY_BITS,
    guessTimezone
  } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let { rule = $bindable({}) } = $props();

  let zones = $state([]);

  onMount(async () => {
    try {
      zones = await automatorApi.timezones();
    } catch {
      zones = [guessTimezone(), 'UTC'];
    }
  });

  const kindOptions = $derived(
    SCHEDULE_KINDS.map((k) => ({ value: k, label: $t(`automator.sched.kind.${k}`) }))
  );
  const zoneOptions = $derived(zones.map((z) => ({ value: z, label: z })));
  const weekdayNames = $derived([
    $t('common.weekday.mon'),
    $t('common.weekday.tue'),
    $t('common.weekday.wed'),
    $t('common.weekday.thu'),
    $t('common.weekday.fri'),
    $t('common.weekday.sat'),
    $t('common.weekday.sun')
  ]);

  function toggleDay(bit) {
    const mask = rule.weekdays ?? 0;
    rule.weekdays = mask & (1 << bit) ? mask & ~(1 << bit) : mask | (1 << bit);
  }

  // The time inputs bind to a single HH:MM value; the API keeps hour and minute apart.
  const pad = (n) => String(n).padStart(2, '0');
  let at = $state(`${pad(rule.at_hour ?? 8)}:${pad(rule.at_minute ?? 0)}`);
  $effect(() => {
    const [h, m] = at.split(':');
    rule.at_hour = Number(h) || 0;
    rule.at_minute = Number(m) || 0;
  });
</script>

<div class="stack">
  <div class="row">
    <Select label={$t('automator.sched.type')} options={kindOptions} bind:value={rule.kind} />
    <Select label={$t('automator.sched.timezone')} options={zoneOptions} bind:value={rule.timezone} />
  </div>

  {#if rule.kind === 'interval'}
    <Input
      label={$t('automator.sched.everyLabel')}
      type="number"
      min="1"
      max="10080"
      bind:value={rule.every_minutes}
    />
  {:else if rule.kind === 'once'}
    <label class="datetime">
      <span class="lbl">{$t('automator.sched.runAt')}</span>
      <input
        type="datetime-local"
        value={rule.run_at ? rule.run_at.slice(0, 16) : ''}
        onchange={(e) => (rule.run_at = new Date(e.currentTarget.value).toISOString())}
      />
    </label>
  {:else}
    <label class="datetime">
      <span class="lbl">{$t('automator.sched.at')}</span>
      <input type="time" bind:value={at} />
    </label>
  {/if}

  {#if rule.kind === 'weekly'}
    <div class="days">
      {#each WEEKDAY_BITS as bit (bit)}
        <button
          type="button"
          class="day"
          class:on={(rule.weekdays ?? 0) & (1 << bit)}
          onclick={() => toggleDay(bit)}
        >
          {weekdayNames[bit]}
        </button>
      {/each}
    </div>
  {/if}

  {#if rule.kind === 'monthly'}
    <Input
      label={$t('automator.sched.dayOfMonth')}
      type="number"
      min="1"
      max="31"
      bind:value={rule.day_of_month}
    />
    <p class="hint">{$t('automator.sched.dayOfMonthHint')}</p>
  {/if}

  <label class="check">
    <input type="checkbox" bind:checked={rule.catch_up} />
    <span>
      {$t('automator.sched.catchUp')}
      <em>{$t('automator.sched.catchUpHint')}</em>
    </span>
  </label>
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
  }
  .row :global(> *) {
    flex: 1;
  }
  .datetime {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .days {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
  }
  .day {
    padding: var(--space-1) var(--space-2);
    border: 0.5px solid var(--border);
    background: var(--surface);
    color: var(--dim);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .day.on {
    border-color: var(--accent);
    color: var(--text);
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
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
