<script>
  // The consistency calendar: a month grid where each day carries the user's verdict.
  //
  // Clicking a day cycles green → amber → nothing:
  //   green  — acted and followed the routine
  //   amber  — acted, but without the routine
  //   empty  — nothing claimed
  //
  // The verdict is a judgement only the trader can make (following the routine and not
  // trading is a good day; trading without it is not), so it is set by hand here rather
  // than inferred from ticks. Ticked days still show a marker, as a reminder of where the
  // work actually happened.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import {
    nextMark,
    MARK_COLORS,
    fmtLocal,
    todayStr,
    weekdayIndex,
    WEEKDAYS,
    periodStats
  } from '$lib/ui/consistency.js';
  import { t } from '$lib/i18n';

  // `setMark(day, mark)` is injected so this component belongs to no module: routines and
  // mindset keep their own marks table and pass their own writer.
  let {
    open = $bindable(false),
    marks = [],
    setMark,
    title = '',
    onchanged = () => {}
  } = $props();

  // Month currently shown, as a Date on the 1st.
  let cursor = $state(new Date(new Date().getFullYear(), new Date().getMonth(), 1));
  let local = $state([]); // working copy so a click paints immediately
  let error = $state('');

  $effect(() => {
    if (open) {
      local = (marks ?? []).map((m) => ({ ...m }));
      cursor = new Date(new Date().getFullYear(), new Date().getMonth(), 1);
    }
  });

  const markOf = (key) => local.find((m) => m.day === key)?.mark ?? null;

  function shiftMonth(n) {
    cursor = new Date(cursor.getFullYear(), cursor.getMonth() + n, 1);
  }

  const monthLabel = $derived(
    cursor.toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
  );

  // The grid: leading blanks so the 1st lands under its weekday, then the month's days.
  const cells = $derived.by(() => {
    const y = cursor.getFullYear();
    const m = cursor.getMonth();
    const lead = weekdayIndex(new Date(y, m, 1));
    const days = new Date(y, m + 1, 0).getDate();
    const out = Array.from({ length: lead }, () => null);
    for (let d = 1; d <= days; d++) {
      const date = new Date(y, m, d);
      const key = fmtLocal(date);
      out.push({
        key,
        d,
        mark: markOf(key),
        today: key === todayStr(),
        future: key > todayStr(),
        weekend: [0, 6].includes(date.getDay())
      });
    }
    return out;
  });

  // Counters for the month on screen, so the header answers "how did this month go".
  const stats = $derived(periodStats(local, 'month', cursor));

  async function cycle(cell) {
    if (!cell || cell.future) return; // a day that hasn't happened can't be judged
    const next = nextMark(cell.mark);
    // Optimistic: the grid is the feedback, a round trip would make it feel laggy.
    local =
      next === null
        ? local.filter((m) => m.day !== cell.key)
        : local.some((m) => m.day === cell.key)
          ? local.map((m) => (m.day === cell.key ? { ...m, mark: next } : m))
          : [...local, { day: cell.key, mark: next }];
    try {
      await setMark(cell.key, next);
      onchanged(local);
    } catch (e) {
      error = e.message;
    }
  }
</script>

<Modal bind:open size="md" title={title || $t('routines.consistency.title')}>
  <div class="wrap">
    <p class="intro">{$t('routines.consistency.intro')}</p>

    <div class="legend">
      <span><i style:background={MARK_COLORS.full}></i>{$t('routines.consistency.legendFull')}</span>
      <span><i style:background={MARK_COLORS.action}></i>{$t('routines.consistency.legendAction')}</span>
      <span><i class="none"></i>{$t('routines.consistency.legendNone')}</span>
    </div>

    <div class="navrow">
      <button class="nav" onclick={() => shiftMonth(-1)} aria-label={$t('routines.consistency.prevMonth')}>
        <Icon name="chevron-left" size={14} />
      </button>
      <strong class="mlabel">{monthLabel}</strong>
      <button class="nav" onclick={() => shiftMonth(1)} aria-label={$t('routines.consistency.nextMonth')}>
        <Icon name="chevron-right" size={14} />
      </button>
      <div class="spacer"></div>
      <div class="tally">
        <span class="pair">
          <i style:background={MARK_COLORS.full}></i><b>{stats.full}</b>
        </span>
        <span class="pair">
          <i style:background={MARK_COLORS.action}></i><b>{stats.action}</b>
        </span>
        <span class="tlbl">
          {$t('routines.consistency.monthTally', { done: stats.done, total: stats.total })}
        </span>
      </div>
    </div>

    <div class="cal">
      <div class="dow">
        {#each WEEKDAYS as d (d.bit)}
          <span>{$t(`routines.weekdayShort.${d.key}`)}</span>
        {/each}
      </div>
      <div class="grid">
        {#each cells as c, i (c?.key ?? `pad-${i}`)}
          {#if c}
            <button
              class="day"
              class:today={c.today}
              class:weekend={c.weekend}
              class:future={c.future}
              disabled={c.future}
              style:background={c.mark ? MARK_COLORS[c.mark] : undefined}
              title={c.key}
              aria-label="{c.key} — {c.mark
                ? $t(`routines.consistency.legend${c.mark === 'full' ? 'Full' : 'Action'}`)
                : $t('routines.consistency.legendNone')}"
              onclick={() => cycle(c)}
            >
              {c.d}
            </button>
          {:else}
            <span class="pad"></span>
          {/if}
        {/each}
      </div>
    </div>

    <ErrorText error={error} />

    <div class="foot">
      <div class="spacer"></div>
      <button class="btn primary" onclick={() => (open = false)}>{$t('common.done')}</button>
    </div>
  </div>
</Modal>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .intro {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  /* Deliberately not `.empty`: that global class is the big dashed "nothing here" panel
     and its padding would blow the swatch up to a block. */
  .legend i {
    width: 10px;
    height: 10px;
    flex: none;
    border-radius: 2px;
    display: inline-block;
  }
  .legend i.none {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
  }

  .navrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .nav {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 4px;
    border-radius: var(--radius);
    display: inline-flex;
  }
  .nav:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .mlabel {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    min-width: 9rem;
    text-align: center;
  }
  .spacer {
    flex: 1;
  }
  /* Three separate readings (green count, amber count, days covered) — spaced apart and
     each tied to its own swatch, so they don't run together into one number salad. */
  .tally {
    display: inline-flex;
    align-items: center;
    gap: var(--space-4);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .pair {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
  }
  .pair i {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 2px;
    display: inline-block;
  }
  .pair b {
    font-weight: var(--fw-medium);
  }
  .tlbl {
    color: var(--muted);
    padding-left: var(--space-3);
    border-left: 0.5px solid var(--border);
  }

  .cal {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .dow,
  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: var(--space-1);
  }
  .dow span {
    text-align: center;
    font-size: var(--text-xs);
    color: var(--muted);
  }

  .day {
    aspect-ratio: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
    /* A marked day is painted by an inline background, so the text has to stay legible
       on both the green and the amber. */
    mix-blend-mode: normal;
  }
  .day:hover:not(:disabled) {
    border-color: var(--border-control);
  }
  .day.weekend:not([style*='background']) {
    opacity: 0.62;
  }
  .day.today {
    outline: 1px solid var(--accent);
    outline-offset: 1px;
  }
  .day.future {
    opacity: 0.28;
    cursor: default;
  }
  .pad {
    aspect-ratio: 1;
  }

  .foot {
    display: flex;
    gap: var(--space-2);
  }
  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
</style>
