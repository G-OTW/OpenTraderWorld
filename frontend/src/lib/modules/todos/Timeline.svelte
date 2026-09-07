<script>
  // Due-date timeline rail. Three zoom levels — years → months → days — each drawn as a
  // stack of bars whose length is the task count relative to the busiest bucket in view.
  // Clicking a bucket zooms in (and filters); the breadcrumb walks back out.
  import Icon from '$lib/ui/Icon.svelte';
  import { t, locale } from '$lib/i18n';

  // range: null (no filter) or { level: 'year'|'month'|'day', y, m?, d? }
  let { todos = [], range = $bindable(null) } = $props();

  // Level the rail is *showing*. Derived from the selection: picking a year shows its
  // months, picking a month shows its days. A null range shows the years.
  const view = $derived(
    range === null
      ? { level: 'years' }
      : range.level === 'year'
        ? { level: 'months', y: range.y }
        : { level: 'days', y: range.y, m: range.m }
  );

  // Tasks that carry a due date, parsed once into {y, m, d}.
  const dated = $derived(
    todos
      .filter((td) => td.due_date)
      .map((td) => {
        const [y, m, d] = td.due_date.split('-').map(Number);
        return { y, m, d, done: td.done };
      })
  );

  const today = new Date();
  const nowY = today.getFullYear();
  const nowM = today.getMonth() + 1;
  const nowD = today.getDate();

  const monthNames = $derived.by(() => {
    void $locale;
    const f = new Intl.DateTimeFormat(undefined, { month: 'short' });
    return Array.from({ length: 12 }, (_, i) => f.format(new Date(2020, i, 1)));
  });

  const daysInMonth = (y, m) => new Date(y, m, 0).getDate();

  /** Buckets for the current view: every slot in range, even the empty ones, so the
      rail reads as a real axis rather than a list of hits. */
  const buckets = $derived.by(() => {
    if (view.level === 'years') {
      const years = dated.map((x) => x.y);
      if (years.length === 0) return [];
      // Always include the current year so "now" has a place on the axis.
      const lo = Math.min(...years, nowY);
      const hi = Math.max(...years, nowY);
      return Array.from({ length: hi - lo + 1 }, (_, i) => {
        const y = lo + i;
        const hits = dated.filter((x) => x.y === y);
        return {
          key: `y${y}`,
          label: String(y),
          count: hits.length,
          openCount: hits.filter((x) => !x.done).length,
          now: y === nowY,
          target: { level: 'year', y }
        };
      });
    }
    if (view.level === 'months') {
      return monthNames.map((name, i) => {
        const m = i + 1;
        const hits = dated.filter((x) => x.y === view.y && x.m === m);
        return {
          key: `m${m}`,
          label: name,
          count: hits.length,
          openCount: hits.filter((x) => !x.done).length,
          now: view.y === nowY && m === nowM,
          target: { level: 'month', y: view.y, m }
        };
      });
    }
    const n = daysInMonth(view.y, view.m);
    return Array.from({ length: n }, (_, i) => {
      const d = i + 1;
      const hits = dated.filter((x) => x.y === view.y && x.m === view.m && x.d === d);
      return {
        key: `d${d}`,
        label: String(d),
        count: hits.length,
        openCount: hits.filter((x) => !x.done).length,
        now: view.y === nowY && view.m === nowM && d === nowD,
        weekend: [0, 6].includes(new Date(view.y, view.m - 1, d).getDay()),
        target: { level: 'day', y: view.y, m: view.m, d }
      };
    });
  });

  const max = $derived(Math.max(1, ...buckets.map((b) => b.count)));

  const sameBucket = (b) =>
    range !== null &&
    range.level === b.target.level &&
    range.y === b.target.y &&
    range.m === b.target.m &&
    range.d === b.target.d;

  /** One rung up the year → month → day ladder; null past the top. */
  const parentOf = (r) =>
    r === null
      ? null
      : r.level === 'day'
        ? { level: 'month', y: r.y, m: r.m }
        : r.level === 'month'
          ? { level: 'year', y: r.y }
          : null;

  function pick(b) {
    // Clicking the active bucket again steps back out, so a second click is "undo".
    range = sameBucket(b) ? parentOf(b.target) : b.target;
  }

  const zoomOut = () => (range = parentOf(range));

  // Breadcrumb trail: what is currently selected, each crumb clickable to jump back.
  const crumbs = $derived.by(() => {
    const out = [{ label: $t('todos.timeline.all'), range: null }];
    if (range === null) return out;
    out.push({ label: String(range.y), range: { level: 'year', y: range.y } });
    if (range.level !== 'year')
      out.push({
        label: monthNames[range.m - 1],
        range: { level: 'month', y: range.y, m: range.m }
      });
    if (range.level === 'day')
      out.push({ label: String(range.d), range: { ...range } });
    return out;
  });

  const total = $derived(buckets.reduce((s, b) => s + b.count, 0));
</script>

<aside class="tl" aria-label={$t('todos.timeline.title')}>
  <div class="tl-head">
    <nav class="crumbs">
      {#each crumbs as c, i (i)}
        {#if i > 0}<span class="sep">/</span>{/if}
        <button
          class="crumb"
          class:last={i === crumbs.length - 1}
          onclick={() => (range = c.range)}
          disabled={i === crumbs.length - 1}
        >
          {c.label}
        </button>
      {/each}
    </nav>
    <button
      class="out"
      onclick={zoomOut}
      disabled={range === null}
      title={$t('todos.timeline.zoomOut')}
      aria-label={$t('todos.timeline.zoomOut')}
    >
      <Icon name="minus" size={13} />
    </button>
  </div>

  {#if buckets.length === 0}
    <p class="tl-empty">{$t('todos.timeline.empty')}</p>
  {:else}
    <ul class="rail" class:dense={view.level === 'days'}>
      {#each buckets as b (b.key)}
        <li>
          <button
            class="slot"
            class:active={sameBucket(b)}
            class:now={b.now}
            class:weekend={b.weekend}
            class:empty={b.count === 0}
            onclick={() => pick(b)}
            title={$t('todos.timeline.slotTitle', { label: b.label, count: b.count })}
            aria-pressed={sameBucket(b)}
          >
            <span class="tick" aria-hidden="true"></span>
            <span class="lab">{b.label}</span>
            <span class="bar-wrap">
              <span class="bar" style="width:{(b.count / max) * 100}%">
                <span class="bar-open" style="width:{b.count ? (b.openCount / b.count) * 100 : 0}%"></span>
              </span>
            </span>
            <span class="n">{b.count || ''}</span>
          </button>
        </li>
      {/each}
    </ul>
    <p class="tl-foot">{$t('todos.timeline.dated', { count: total })}</p>
  {/if}
</aside>

<style>
  .tl {
    flex: none;
    width: 200px;
    align-self: flex-start;
    position: sticky;
    top: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
  }
  .tl-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    min-height: 22px;
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
    flex-wrap: wrap;
  }
  .crumb {
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .crumb:hover:not(:disabled) {
    color: var(--text);
  }
  .crumb.last {
    color: var(--text);
    font-weight: var(--fw-medium);
    cursor: default;
  }
  .sep {
    font-size: var(--text-xs);
    color: var(--dim, var(--muted));
  }
  .out {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .out:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--accent);
  }
  .out:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .rail {
    list-style: none;
    display: flex;
    flex-direction: column;
    /* The vertical axis: one hairline the ticks hang off. */
    border-left: 1px solid var(--border);
    padding-left: 0;
    margin-left: 3px;
    max-height: 420px;
    overflow-y: auto;
    scrollbar-width: thin;
  }
  .slot {
    position: relative;
    display: grid;
    /* Label gets the room it needs (a 4-digit year must never truncate); the bar takes
       what is left, capped so it stays a gauge rather than the dominant element. */
    grid-template-columns: auto minmax(0, 1fr) 14px;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 3px var(--space-1) 3px var(--space-2);
    background: transparent;
    border: none;
    border-radius: var(--radius);
    cursor: pointer;
    text-align: left;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .rail.dense .slot {
    padding-top: 1px;
    padding-bottom: 1px;
  }
  .slot:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .slot.active {
    background: var(--surface-2);
    color: var(--text);
  }
  /* The tick sits on the rail itself, left of the row. */
  .tick {
    position: absolute;
    left: -4px;
    width: 7px;
    height: 1px;
    background: var(--border);
  }
  .slot.active .tick,
  .slot:hover .tick {
    background: var(--accent);
    height: 2px;
  }
  .slot.now .tick {
    background: var(--amber);
    height: 2px;
  }
  .lab {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .slot.now .lab {
    color: var(--amber);
    font-weight: var(--fw-medium);
  }
  .slot.active .lab {
    font-weight: var(--fw-medium);
  }
  .slot.weekend .lab {
    opacity: 0.6;
  }
  .slot.empty .lab {
    opacity: 0.45;
  }
  .bar-wrap {
    min-width: 0;
    /* Right-aligned and capped: the label keeps its full width, the gauge stays subordinate. */
    justify-self: end;
    width: 100%;
    max-width: 84px;
    height: 4px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--border) 55%, transparent);
  }
  .bar {
    display: block;
    height: 100%;
    min-width: 0;
    border-radius: 2px;
    /* Done share stays muted; the open share is repainted on top. */
    background: color-mix(in srgb, var(--accent) 30%, transparent);
    transition: width 0.18s ease;
  }
  .bar-open {
    display: block;
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
  }
  .slot.active .bar-open {
    background: var(--accent);
  }
  .n {
    text-align: right;
    font-variant-numeric: tabular-nums;
    opacity: 0.7;
  }
  .tl-empty,
  .tl-foot {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .tl-foot {
    padding-top: var(--space-1);
    border-top: 0.5px solid var(--border);
  }

  @media (max-width: 900px) {
    .tl {
      width: 100%;
      position: static;
    }
    .rail {
      max-height: 220px;
    }
  }
</style>
