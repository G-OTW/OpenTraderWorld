<script>
  // A dated calendar of one value per day (daily PnL, check-in history), at two scales:
  //
  //   year  — trailing weeks as Mon-first columns, month names above, weekday names beside.
  //   month — one month laid out as a real calendar, day numbers in the cells.
  //
  // Cells are keyed by ISO date; a missing day draws as the empty track rather than as
  // zero, because "no trading" and "flat day" are not the same reading.
  //
  // The grid is sized from the measured box: the year scale drops the oldest weeks until
  // the remaining ones fit, so the calendar never scrolls sideways inside a widget and
  // never inflates its cells to fill a tall card.
  //
  // Hovering a cell reads its day: the full date and the value, or "no data" when empty.
  import { dateKey, fmtNum } from '$lib/format';
  import { tip } from '$lib/ui/tip.svelte.js';
  import { t, locale } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';

  let {
    days = new Map(),      // Map<'YYYY-MM-DD', number>
    weeks = 53,            // how many week columns the year scale aims for
    signed = true,         // signed scale (red/green) vs single-hue intensity
    label = 'activity',
    valueFormat = (v) => fmtNum(v, 2),
    emptyText = '—',
    scale = 'auto'         // 'auto' | 'year' | 'month'
  } = $props();

  const GAP = 3;
  const GUTTER = 22;       // weekday-name column of the year scale
  const HEADROW = 13;      // month-name row of the year scale

  let w = $state(0);
  let h = $state(0);

  // ── scale ─────────────────────────────────────────────────────────────────
  // A year of weeks needs both the width for its columns and the height for seven rows;
  // below either, the month calendar is the readable one.
  const fits = $derived(w >= 260 && h >= 74);
  let picked = $state(null);
  const view = $derived(picked ?? (scale !== 'auto' ? scale : fits ? 'year' : 'month'));

  // ── shading ───────────────────────────────────────────────────────────────
  const max = $derived(Math.max(1, ...[...days.values()].map((v) => Math.abs(Number(v) || 0))));

  function shade(v) {
    if (v === undefined || v === null) return null;
    const mag = Math.min(1, Math.abs(Number(v)) / max);
    // Floor the mix so a small but real day is still visible against the track.
    const pct = Math.round((0.2 + 0.8 * mag) * 100);
    const hue = !signed ? 'var(--accent)' : Number(v) < 0 ? 'var(--red)' : 'var(--green)';
    return `color-mix(in srgb, ${hue} ${pct}%, transparent)`;
  }

  const dayName = (key) =>
    new Date(`${key}T00:00`).toLocaleDateString($locale, {
      weekday: 'short',
      day: 'numeric',
      month: 'short',
      year: 'numeric'
    });

  let hover = $state('');
  function enter(event, cell) {
    hover = cell.key;
    const v = cell.value;
    tip.show(event, {
      title: dayName(cell.key),
      rows: [
        {
          label,
          value: v === undefined || v === null ? emptyText : valueFormat(v),
          tone: v === undefined || v === null || !signed ? '' : Number(v) < 0 ? 'neg' : 'pos'
        }
      ]
    });
  }
  function leave() {
    hover = '';
    tip.hide();
  }

  // Mon-first weekday names, in the user's locale. Built off a known Monday so the
  // list never depends on today's weekday.
  const weekdays = $derived.by(() => {
    const monday = new Date(2024, 0, 1); // a Monday
    return Array.from({ length: 7 }, (_, i) => {
      const d = new Date(monday);
      d.setDate(monday.getDate() + i);
      return {
        short: d.toLocaleDateString($locale, { weekday: 'short' }),
        narrow: d.toLocaleDateString($locale, { weekday: 'narrow' })
      };
    });
  });

  // ── year scale ────────────────────────────────────────────────────────────
  // The grid takes the room it is given, in both directions: how many weeks fit is
  // settled first, at the floor size, then the cells grow into whatever width and height
  // are left. So the same card reads as a dense year when small and as a full-size
  // calendar when the widget is stretched, and never below MIN (an unreadable speck) or
  // above MAX (a wall of tiles).
  const MIN = 7;
  const MAX = 26;

  // Seven rows plus their gaps is the height budget; the month-name row sits above it.
  const capH = $derived(Math.max(MIN, Math.min(MAX, Math.floor((h - HEADROW - 6 * GAP - 2) / 7))));
  // How many weeks fit at that size. A narrow card drops the oldest weeks rather than
  // shrinking every cell to a speck.
  const cols = $derived(
    Math.max(4, Math.min(weeks, Math.floor((w - GUTTER + GAP) / (capH + GAP))))
  );
  // Once the year is complete, the leftover width goes back into the cells.
  const capW = $derived(Math.floor((w - GUTTER + GAP) / cols) - GAP);
  const cell = $derived(Math.max(MIN, Math.min(MAX, capH, capW)));

  const grid = $derived.by(() => {
    const end = new Date();
    end.setHours(0, 0, 0, 0);
    end.setDate(end.getDate() - ((end.getDay() + 6) % 7) + 6); // that week's Sunday
    const out = [];
    for (let c = cols - 1; c >= 0; c--) {
      const col = [];
      for (let d = 6; d >= 0; d--) {
        const day = new Date(end);
        day.setDate(end.getDate() - c * 7 - d);
        col.push({ key: dateKey(day), date: day, value: days.get(dateKey(day)) });
      }
      out.push(col);
    }
    return out;
  });

  // A month is named over the column that first carries it, and only when it has room
  // for the name (roughly three columns), so labels never overprint each other.
  const monthMarks = $derived.by(() => {
    const marks = [];
    let lastMonth = -1;
    grid.forEach((col, i) => {
      const m = col[0].date.getMonth();
      if (m === lastMonth) return;
      lastMonth = m;
      if (i > 0 && i > grid.length - 3) return;
      marks.push({
        i,
        text: col[0].date.toLocaleDateString($locale, { month: 'short' })
      });
    });
    return marks;
  });
  const markAt = $derived(new Map(monthMarks.map((m) => [m.i, m.text])));

  // ── month scale ───────────────────────────────────────────────────────────
  // The month in view, as an offset from the current one, so the arrows can walk back
  // through the history the card was given.
  let monthOffset = $state(0);
  const cursor = $derived.by(() => {
    const d = new Date();
    return new Date(d.getFullYear(), d.getMonth() + monthOffset, 1);
  });
  const monthTitle = $derived(
    cursor.toLocaleDateString($locale, { month: 'long', year: 'numeric' })
  );

  const monthCells = $derived.by(() => {
    const first = new Date(cursor);
    const lead = (first.getDay() + 6) % 7; // Mon-first offset of the 1st
    const start = new Date(first);
    start.setDate(1 - lead);
    const daysIn = new Date(cursor.getFullYear(), cursor.getMonth() + 1, 0).getDate();
    const rows = Math.ceil((lead + daysIn) / 7);
    return Array.from({ length: rows * 7 }, (_, i) => {
      const day = new Date(start);
      day.setDate(start.getDate() + i);
      const key = dateKey(day);
      return {
        key,
        num: day.getDate(),
        inMonth: day.getMonth() === cursor.getMonth(),
        value: days.get(key)
      };
    });
  });
  // Day numbers need a cell wide enough to hold them; below that the colour is the datum.
  const showNums = $derived(w / 7 >= 26 && h >= 100);

  // The month cells fill the box, but a day is a square-ish tile, not a band: past ~1.7
  // times its own height the grid stops widening and centres instead.
  const monthWidth = $derived.by(() => {
    const rows = Math.max(1, monthCells.length / 7);
    const cellH = (h - 14 - (rows - 1) * 3) / rows;
    if (!(cellH > 0)) return null;
    return Math.round(7 * cellH * 1.7 + 6 * 3);
  });
</script>

<div class="heat" role="group" aria-label={label} onpointerleave={leave}>
  <div class="bar">
    {#if view === 'month'}
      <div class="nav">
        <button type="button" class="navbtn" aria-label={$t('dashboard.widgets.heat.prev')}
          onclick={() => (monthOffset -= 1)}><Icon name="chevron-left" size={13} /></button>
        <span class="title">{monthTitle}</span>
        <button type="button" class="navbtn" disabled={monthOffset >= 0}
          aria-label={$t('dashboard.widgets.heat.next')}
          onclick={() => (monthOffset += 1)}><Icon name="chevron-right" size={13} /></button>
      </div>
    {:else}
      <span></span>
    {/if}
    <div class="w-chips" role="group" aria-label={$t('dashboard.widgets.heat.scale')}>
      <button type="button" class="w-chip" aria-pressed={view === 'month'}
        onclick={() => (picked = 'month')}>{$t('dashboard.widgets.heat.month')}</button>
      <button type="button" class="w-chip" aria-pressed={view === 'year'}
        onclick={() => (picked = 'year')}>{$t('dashboard.widgets.heat.year')}</button>
    </div>
  </div>

  <!-- The grid box is measured, not the card: the cell size has to come from the room
       left under the bar, or the last row falls out of the widget. -->
  <div class="body" bind:clientWidth={w} bind:clientHeight={h}>
  {#if view === 'year'}
    <div class="year" style:--cell="{cell}px" style:--gap="{GAP}px">
      <div class="wdays" style:padding-top="{HEADROW}px">
        {#each weekdays as d, i (i)}
          <span class="wday">{i % 2 === 0 ? d.narrow : ''}</span>
        {/each}
      </div>
      <div class="cols">
        <div class="months" style:height="{HEADROW}px">
          {#each grid as _, i (i)}
            <span class="mlabel">{markAt.get(i) ?? ''}</span>
          {/each}
        </div>
        <div class="weeks">
          {#each grid as col, ci (ci)}
            <div class="col">
              {#each col as c (c.key)}
                {@const bg = shade(c.value)}
                <span
                  class="cell"
                  role="img"
                  aria-label={`${dayName(c.key)}: ${c.value == null ? emptyText : valueFormat(c.value)}`}
                  class:off={bg === null}
                  class:on={hover === c.key}
                  style:background={bg}
                  onpointerenter={(e) => enter(e, c)}
                  onpointermove={(e) => tip.move(e)}
                ></span>
              {/each}
            </div>
          {/each}
        </div>
      </div>
    </div>
  {:else}
    <div class="month" style:max-width={monthWidth ? `${monthWidth}px` : null}>
      <div class="mhead">
        {#each weekdays as d, i (i)}
          <span class="wday">{showNums ? d.short : d.narrow}</span>
        {/each}
      </div>
      <div class="mgrid" style:--rows={monthCells.length / 7}>
        {#each monthCells as c (c.key)}
          {@const bg = c.inMonth ? shade(c.value) : null}
          <span
            class="mcell"
            role="img"
            aria-label={`${dayName(c.key)}: ${c.value == null ? emptyText : valueFormat(c.value)}`}
            class:off={c.inMonth && bg === null}
            class:out={!c.inMonth}
            class:on={hover === c.key}
            style:background={bg}
            onpointerenter={(e) => c.inMonth && enter(e, c)}
            onpointermove={(e) => tip.move(e)}
          >{#if showNums}<i class="num">{c.num}</i>{/if}</span>
        {/each}
      </div>
    </div>
  {/if}
  </div>
</div>

<style>
  .heat {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }
  .title {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--text);
    white-space: nowrap;
  }
  .navbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--dim);
    cursor: pointer;
  }
  .navbtn:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .navbtn:disabled {
    color: var(--faint);
    cursor: default;
    opacity: 0.4;
  }
  .body {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }
  .bar .w-chip {
    height: 20px;
    padding: 0 7px;
    font-size: 10px;
  }

  /* ── year ───────────────────────────────────────────────────────────────── */
  .year {
    display: flex;
    justify-content: center;
    gap: var(--gap);
    min-width: 0;
    /* The hover ring is drawn outside the cell box; keep it from being clipped. */
    padding: 1px;
  }
  .wdays {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    width: 18px;
    flex-shrink: 0;
  }
  .wday {
    color: var(--faint);
    font-size: 9px;
    line-height: var(--cell, 10px);
    height: var(--cell, 10px);
    text-align: right;
    white-space: nowrap;
  }
  .cols {
    min-width: 0;
  }
  .months,
  .weeks {
    display: flex;
    gap: var(--gap);
  }
  /* Each slot is one column wide; the name overflows it to the right rather than
     squeezing the grid to fit three letters. */
  .mlabel {
    position: relative;
    width: var(--cell);
    flex-shrink: 0;
    color: var(--faint);
    font-size: 9px;
    line-height: 1;
    white-space: nowrap;
    overflow: visible;
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  .cell {
    width: var(--cell);
    height: var(--cell);
    border-radius: 2px;
    flex-shrink: 0;
  }

  /* ── month ──────────────────────────────────────────────────────────────── */
  .month {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-height: 0;
    width: 100%;
    margin-inline: auto;
  }
  .mhead {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 3px;
    flex-shrink: 0;
  }
  .mhead .wday {
    height: auto;
    line-height: 1;
    text-align: center;
  }
  .mgrid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    grid-template-rows: repeat(var(--rows, 5), minmax(0, 1fr));
    gap: 3px;
    flex: 1;
    min-height: 0;
  }
  .mcell {
    position: relative;
    border-radius: var(--radius-sm);
    min-height: 8px;
  }
  .num {
    position: absolute;
    top: 2px;
    left: 4px;
    font-family: var(--mono);
    font-style: normal;
    font-size: 9px;
    line-height: 1;
    color: var(--muted);
  }
  .mcell.out .num {
    color: var(--faint);
    opacity: 0.5;
  }

  /* A day the series never covered: the track, not a zero. Scoped names, so the
     page-level `.empty` block cannot land on a calendar cell. */
  .off {
    background: var(--surface-3);
  }
  .mcell.out {
    background: transparent;
  }
  .on {
    box-shadow: 0 0 0 1.5px var(--text);
  }
</style>
