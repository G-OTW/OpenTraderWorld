<script>
  // A themed date picker. Replaces the native <input type="date">, whose popup is drawn by the
  // OS and ignores the app's palette. Value is a plain `YYYY-MM-DD` string ('' = unset), the
  // same shape the native input binds, so it drops in where one stood.
  //
  // Month grid (Monday first) with a year view one click away on the title: a dataset can span
  // decades, and walking there month by month is not a picker. `min`/`max` dim what is out of
  // range and keep the navigation inside it.
  //
  // Keyboard: Enter/Space/↓ on the trigger opens; arrows move by day/week, PageUp/PageDown by
  // month (with Shift by year), Enter picks, Escape closes and refocuses the trigger.
  import Icon from '$lib/ui/Icon.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { locale } from '$lib/i18n';

  let {
    value = $bindable(''),
    min = '',
    max = '',
    placeholder = '',
    ariaLabel = '',
    disabled = false,
    clearable = true,
    onpick = null
  } = $props();

  let open = $state(false);
  let view = $state('days'); // 'days' | 'years'
  let cursor = $state(''); // the keyboard-highlighted day, YYYY-MM-DD
  let btn = $state(null);
  let grid = $state(null);

  const pad = (n) => String(n).padStart(2, '0');
  const iso = (y, m, d) => `${y}-${pad(m + 1)}-${pad(d)}`;
  const parse = (s) => {
    const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(s ?? '');
    return m ? { y: +m[1], m: +m[2] - 1, d: +m[3] } : null;
  };
  const shift = (s, days) => {
    const p = parse(s);
    const dt = new Date(Date.UTC(p.y, p.m, p.d + days));
    return iso(dt.getUTCFullYear(), dt.getUTCMonth(), dt.getUTCDate());
  };
  const shiftMonths = (s, n) => {
    const p = parse(s);
    const last = new Date(Date.UTC(p.y, p.m + n + 1, 0)).getUTCDate();
    const dt = new Date(Date.UTC(p.y, p.m + n, Math.min(p.d, last)));
    return iso(dt.getUTCFullYear(), dt.getUTCMonth(), dt.getUTCDate());
  };
  const clamp = (s) => (min && s < min ? min : max && s > max ? max : s);
  const outside = (s) => (min && s < min) || (max && s > max);
  const today = (() => {
    const d = new Date();
    return iso(d.getFullYear(), d.getMonth(), d.getDate());
  })();

  const cur = $derived(parse(cursor) ?? parse(clamp(today)));
  const fmtLong = $derived(new Intl.DateTimeFormat($locale, { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' }));
  const fmtMonth = $derived(new Intl.DateTimeFormat($locale, { month: 'long', year: 'numeric', timeZone: 'UTC' }));
  const weekdays = $derived.by(() => {
    const f = new Intl.DateTimeFormat($locale, { weekday: 'narrow', timeZone: 'UTC' });
    // 2024-01-01 is a Monday.
    return Array.from({ length: 7 }, (_, i) => f.format(new Date(Date.UTC(2024, 0, 1 + i))));
  });

  const label = $derived.by(() => {
    const p = parse(value);
    return p ? fmtLong.format(new Date(Date.UTC(p.y, p.m, p.d))) : '';
  });
  const title = $derived(cur ? fmtMonth.format(new Date(Date.UTC(cur.y, cur.m, 1))) : '');

  /** Six weeks from the Monday on or before the 1st, so the grid never changes height. */
  const days = $derived.by(() => {
    if (!cur) return [];
    const first = new Date(Date.UTC(cur.y, cur.m, 1));
    const lead = (first.getUTCDay() + 6) % 7;
    return Array.from({ length: 42 }, (_, i) => {
      const dt = new Date(Date.UTC(cur.y, cur.m, 1 - lead + i));
      const y = dt.getUTCFullYear();
      const m = dt.getUTCMonth();
      const d = dt.getUTCDate();
      return { key: iso(y, m, d), d, inMonth: m === cur.m };
    });
  });

  const minY = $derived(parse(min)?.y ?? null);
  const maxY = $derived(parse(max)?.y ?? null);
  const years = $derived.by(() => {
    if (!cur) return [];
    const lo = minY ?? cur.y - 30;
    const hi = maxY ?? Math.max(cur.y + 5, new Date().getFullYear());
    return Array.from({ length: hi - lo + 1 }, (_, i) => lo + i);
  });

  const canPrev = $derived(!min || !cur || iso(cur.y, cur.m, 1) > min);
  const canNext = $derived(!max || !cur || shiftMonths(iso(cur.y, cur.m, 1), 1) <= max);

  function openPop() {
    cursor = clamp(value || today);
    view = 'days';
    open = true;
  }
  function close({ refocus = false } = {}) {
    open = false;
    if (refocus) btn?.focus();
  }
  function pick(s) {
    if (outside(s)) return;
    value = s;
    onpick?.(s);
    close({ refocus: true });
  }
  function clear(e) {
    e.stopPropagation();
    value = '';
    onpick?.('');
  }
  function month(n) {
    cursor = clamp(shiftMonths(cursor || today, n));
  }
  function pickYear(y) {
    const p = cur;
    const last = new Date(Date.UTC(y, p.m + 1, 0)).getUTCDate();
    cursor = clamp(iso(y, p.m, Math.min(p.d, last)));
    view = 'days';
    grid?.focus();
  }

  function onTriggerKey(e) {
    if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      openPop();
    }
  }
  function onGridKey(e) {
    const moves = {
      ArrowLeft: () => shift(cursor, -1),
      ArrowRight: () => shift(cursor, 1),
      ArrowUp: () => shift(cursor, -7),
      ArrowDown: () => shift(cursor, 7),
      PageUp: () => shiftMonths(cursor, e.shiftKey ? -12 : -1),
      PageDown: () => shiftMonths(cursor, e.shiftKey ? 12 : 1)
    };
    if (moves[e.key]) {
      e.preventDefault();
      cursor = clamp(moves[e.key]());
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      pick(cursor);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close({ refocus: true });
    } else if (e.key === 'Tab') {
      close();
    }
  }

  $effect(() => {
    if (open && view === 'days') grid?.focus();
  });
  // The year list opens on the year in view rather than at the top of a thirty-row column.
  let yearList = $state(null);
  $effect(() => {
    if (open && view === 'years') yearList?.querySelector('.on')?.scrollIntoView({ block: 'center' });
  });
</script>

<div class="dp" use:clickOutside={() => close()}>
  <button
    bind:this={btn}
    type="button"
    class="trigger"
    class:placeholder={!value}
    {disabled}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-label={ariaLabel || undefined}
    onclick={() => (open ? close() : openPop())}
    onkeydown={onTriggerKey}
  >
    <Icon name="calendar" size={13} />
    <span class="lbl num">{label || placeholder}</span>
    {#if clearable && value && !disabled}
      <span class="clear" role="presentation" onclick={clear} title="×"><Icon name="x" size={11} /></span>
    {/if}
  </button>

  {#if open}
    <div class="pop" role="dialog" aria-label={ariaLabel || undefined}>
      <div class="head">
        <button type="button" class="nav" disabled={!canPrev || view === 'years'} onclick={() => month(-1)} aria-label="‹">
          <Icon name="chevron-left" size={14} />
        </button>
        <button type="button" class="title" class:on={view === 'years'} onclick={() => (view = view === 'years' ? 'days' : 'years')}>
          {title}
          <Icon name={view === 'years' ? 'chevron-up' : 'chevron-down'} size={12} />
        </button>
        <button type="button" class="nav" disabled={!canNext || view === 'years'} onclick={() => month(1)} aria-label="›">
          <Icon name="chevron-right" size={14} />
        </button>
      </div>

      {#if view === 'years'}
        <div class="years" bind:this={yearList}>
          {#each years as y (y)}
            <button type="button" class="yr num" class:on={cur && y === cur.y} onclick={() => pickYear(y)}>{y}</button>
          {/each}
        </div>
      {:else}
        <div class="wk" aria-hidden="true">
          {#each weekdays as w, i (i)}<span>{w}</span>{/each}
        </div>
        <!-- One focusable grid: arrows move the highlight, as in the listbox pattern. -->
        <div
          bind:this={grid}
          class="grid"
          role="grid"
          tabindex="-1"
          aria-activedescendant="dp-{cursor}"
          onkeydown={onGridKey}
        >
          {#each days as c (c.key)}
            <button
              type="button"
              id="dp-{c.key}"
              tabindex="-1"
              class="day num"
              class:dim={!c.inMonth}
              class:sel={c.key === value}
              class:cur={c.key === cursor}
              class:today={c.key === today}
              disabled={outside(c.key)}
              onclick={() => pick(c.key)}
              onmouseenter={() => (cursor = c.key)}
            >{c.d}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .dp {
    position: relative;
    min-width: 0;
  }

  .trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    background: var(--surface);
    border: var(--hairline, 0.5px) solid var(--border-control);
    border-radius: var(--radius-sm);
    height: var(--control-h);
    padding: 0 var(--space-2) 0 var(--space-3);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-body);
    text-align: left;
    cursor: pointer;
  }
  .trigger:hover {
    background: var(--surface-2);
  }
  .trigger[aria-expanded='true'] {
    border-color: var(--accent);
  }
  .trigger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .lbl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .trigger.placeholder .lbl {
    color: var(--faint, var(--muted));
  }
  .clear {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--radius-sm);
    color: var(--muted);
  }
  .clear:hover {
    color: var(--text);
    background: var(--surface);
  }

  .pop {
    position: absolute;
    z-index: var(--z-dropdown, 50);
    top: calc(100% + 2px);
    left: 0;
    width: 248px;
    padding: var(--space-2);
    background: var(--surface);
    border: var(--hairline, 0.5px) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-bottom: var(--space-2);
  }
  .nav,
  .title {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    height: 26px;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  .nav {
    width: 26px;
  }
  .title {
    flex: 1;
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    text-transform: capitalize;
  }
  .nav:hover:not(:disabled),
  .title:hover,
  .title.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .nav:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .wk,
  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 2px;
  }
  .wk span {
    text-align: center;
    font-size: var(--text-xs);
    color: var(--faint, var(--muted));
    padding-bottom: var(--space-1);
    text-transform: uppercase;
  }
  .grid {
    outline: none;
  }
  .day {
    height: 30px;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    position: relative;
  }
  .day.dim {
    color: var(--faint, var(--muted));
  }
  .day.cur {
    background: var(--surface-2);
  }
  .day.today::after {
    content: '';
    position: absolute;
    left: 50%;
    bottom: 4px;
    width: 3px;
    height: 3px;
    margin-left: -1.5px;
    border-radius: 50%;
    background: var(--accent);
  }
  .day.sel {
    background: color-mix(in srgb, var(--accent) 30%, transparent);
    box-shadow: inset 0 0 0 1px var(--accent);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .day:disabled {
    opacity: 0.25;
    cursor: not-allowed;
    background: transparent;
  }

  .years {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 2px;
    max-height: 228px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .yr {
    height: 30px;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .yr:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .yr.on {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
</style>
