<script>
  // Month grid of billing dates and amounts (at the user's share, in each subscription's
  // own currency, the display-currency month total in the bar). Subscriptions without a
  // start date have no billing day and are counted apart instead of guessed. With budgets
  // set, the days from which the month's billing runs past the total budget are tinted.
  import { money } from './fmt.svelte.js';
  import { monthGrid, monthEvents, dayKey, anchorOf, isEnded } from './calc.js';
  import Icon from '$lib/ui/Icon.svelte';
  import { t, locale } from '$lib/i18n';

  // `toDisplay(amount, currency)` converts to `cur`, or returns null with no rate.
  // `budget` is the sum of the category caps (display currency, 0 = none).
  let { subs = [], cur = 'USD', toDisplay = (a) => a, budget = 0, onexport = null } = $props();

  const now = new Date();
  let year = $state(now.getFullYear());
  let month0 = $state(now.getMonth());

  function shift(n) {
    const d = new Date(year, month0 + n, 1);
    year = d.getFullYear();
    month0 = d.getMonth();
  }
  function toToday() {
    year = now.getFullYear();
    month0 = now.getMonth();
  }
  const isThisMonth = $derived(year === now.getFullYear() && month0 === now.getMonth());

  const weeks = $derived(monthGrid(year, month0));
  const events = $derived(monthEvents(subs, year, month0));
  const todayKey = dayKey(now);
  const title = $derived(
    new Date(year, month0, 1).toLocaleDateString($locale || undefined, { month: 'long', year: 'numeric' })
  );
  const weekdays = $derived(
    Array.from({ length: 7 }, (_, i) =>
      new Date(2024, 0, 1 + i).toLocaleDateString($locale || undefined, { weekday: 'short' })
    )
  );

  // Month total and, day by day, whether the running total has passed the budget.
  const tally = $derived.by(() => {
    let sum = 0;
    let missing = 0;
    const over = new Set();
    const days = Object.keys(events).sort();
    for (const k of days) {
      for (const e of events[k]) {
        const v = toDisplay(e.amount, e.currency);
        if (v === null || v === undefined) missing++;
        else sum += v;
      }
      if (budget > 0 && sum > budget) over.add(k);
    }
    // Once over, every later day of the month stays over.
    const first = [...over].sort()[0];
    return { sum, missing, first };
  });
  const overFrom = (k) => !!tally.first && k >= tally.first;
  const undated = $derived(subs.filter((s) => s.active && !isEnded(s) && !anchorOf(s)).length);
</script>

<div class="cal">
  <div class="bar">
    <div class="nav">
      <button type="button" class="btn pill square" aria-label={$t('subscriptions.calendar.prev')} onclick={() => shift(-1)}><Icon name="chevron-left" size={14} /></button>
      <strong class="title">{title}</strong>
      <button type="button" class="btn pill square" aria-label={$t('subscriptions.calendar.next')} onclick={() => shift(1)}><Icon name="chevron-right" size={14} /></button>
      {#if !isThisMonth}
        <button type="button" class="btn pill" onclick={toToday}>{$t('subscriptions.calendar.today')}</button>
      {/if}
    </div>
    <span class="total">
      <span class="lbl">{$t('subscriptions.calendar.total')}</span>
      <span class="fig">{money(tally.sum, cur)}</span>
    </span>
    {#if onexport}
      <button type="button" class="btn sm" onclick={onexport} title={$t('subscriptions.page.exportIcsTitle')}>
        <Icon name="download" size={13} />
        {$t('subscriptions.page.exportIcs')}
      </button>
    {/if}
  </div>
  <div class="grid" role="grid" aria-label={title}>
    <div class="row" role="row">
      {#each weekdays as w}<div class="wd" role="columnheader">{w}</div>{/each}
    </div>
    {#each weeks as week}
      <div class="row" role="row">
        {#each week as d}
          {@const k = dayKey(d)}
          {@const inMonth = d.getMonth() === month0}
          <div
            class="day"
            class:out={!inMonth}
            class:over={inMonth && overFrom(k)}
            role="gridcell"
            aria-current={k === todayKey ? 'date' : undefined}
          >
            <span class="n" class:today={k === todayKey}>{d.getDate()}</span>
            {#if inMonth}
              {#each events[k] ?? [] as e}
                <span class="ev" title="{e.sub.name}: {money(e.amount, e.currency)}">
                  <span class="name">{e.sub.name}</span>
                  <span class="amt">{money(e.amount, e.currency)}</span>
                </span>
              {/each}
            {/if}
          </div>
        {/each}
      </div>
    {/each}
  </div>
  {#if tally.first}
    <p class="note warn"><span class="swatch" aria-hidden="true"></span>{$t('subscriptions.calendar.overNote', { cap: money(budget, cur) })}</p>
  {/if}
  {#if undated > 0}
    <p class="note">{$t('subscriptions.calendar.undated', { count: undated })}</p>
  {/if}
  {#if tally.missing > 0}
    <p class="note">{$t('subscriptions.page.unconvertedWarning', { count: tally.missing, cur })}</p>
  {/if}
</div>

<style>
  .cal {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .nav {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .title {
    min-width: 132px;
    font-size: 13px;
    font-weight: 600;
    text-align: center;
    text-transform: capitalize;
  }
  .total {
    margin-left: auto;
    display: inline-flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .lbl {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 15px;
    color: var(--text);
  }
  .grid {
    display: flex;
    flex-direction: column;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .row {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
  }
  .row + .row {
    border-top: var(--hairline) solid var(--border);
  }
  .wd {
    padding: var(--space-1) var(--space-2);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .day {
    min-height: 84px;
    padding: var(--space-1);
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    transition: background-color 160ms ease;
  }
  .day + .day,
  .wd + .wd {
    border-left: var(--hairline) solid var(--border);
  }
  .day.out {
    background: var(--surface-2);
  }
  .day.out .n {
    color: var(--faint);
  }
  .day.over {
    background: var(--warning-soft);
  }
  .n {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 3px;
    border-radius: 9px;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
    color: var(--muted);
  }
  /* Today: a ring, never a fill. */
  .n.today {
    border: 1px solid var(--accent);
    color: var(--text);
    font-weight: 600;
  }
  .ev {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-1);
    min-width: 0;
    padding: 1px var(--space-1);
    border-radius: 4px;
    background: var(--surface-3);
    font-size: 10px;
    line-height: 1.4;
  }
  .ev .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .ev .amt {
    flex: none;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .note {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 12px;
    color: var(--dim);
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    background: var(--warning-soft);
    border: 1px solid var(--amber);
  }
  @media (max-width: 767px) {
    .day {
      min-height: 56px;
      padding: 2px;
    }
    .ev .name {
      display: none;
    }
    .bar .btn {
      min-width: 44px;
      min-height: 44px;
    }
  }
</style>
