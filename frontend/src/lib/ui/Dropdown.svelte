<script>
  // A themed single-select. Replaces the native <select>, whose popup is drawn by the OS
  // and so ignores the app's palette entirely — a light-grey macOS menu over a dark UI.
  //
  // Not ComboSelect: that one is a *filter* control (it always offers an "any" entry and
  // takes plain strings). This is a form picker over {value,label,color} options, so a
  // category can carry its dot into both the button and the menu.
  //
  // The menu scrolls past ~8 rows, and past `searchThreshold` options it grows a search
  // field: long lists (a symbol list, an indicator library) are unusable otherwise. Pass
  // `searchable` to force the field on or off regardless of the count.
  //
  // Keyboard: Enter/Space/↓ opens, ↑/↓ moves (skipping group headers and scrolling the
  // highlight into view), Enter picks, Escape closes and restores focus to the button. With
  // the search field open, typing filters and the arrows still drive the list.
  //
  // The current value is marked with color, not a check glyph: the row already carries a
  // dot column in some callers, and a trailing tick shifts the label metrics of one row.
  //
  // An option may carry `disabled: true` (shown, dimmed, not pickable) or `header: true`
  // (a non-selectable group label); the highlight walks over both.
  import Icon from '$lib/ui/Icon.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';

  // `onpick` is for callers whose real state isn't the option value itself (a number
  // inside a schedule object, say) and so can't simply bind to it.
  let {
    value = $bindable(''),
    options = [], // [{ value, label, color? }]
    placeholder = '',
    ariaLabel = '',
    title = '',
    disabled = false,
    searchable = null, // null = decide from the option count
    searchThreshold = 8,
    searchPlaceholder = '',
    // `float`: pin the popup to the viewport instead of to the trigger's box. An absolutely
    // positioned menu is clipped by any scrolling ancestor, and a toolbar that scrolls its
    // overflow (the chart pane's) is exactly such an ancestor: without this the list opens as
    // a 30px sliver. Off by default, so every existing caller keeps its current behaviour.
    float = false,
    onpick = null
  } = $props();

  let open = $state(false);
  let active = $state(0); // highlighted index into `view` while the menu is open
  let q = $state('');
  let btn = $state(null);
  /** Where a floating popup sits: measured from the trigger when the menu opens, and
   *  remeasured while it is open so a scroll or a resize cannot leave it behind. */
  let anchor = $state(null);
  let menu = $state(null);
  let field = $state(null);
  let rows = $state([]); // row elements, to scroll the highlight into view

  const uid = `dd-${Math.random().toString(36).slice(2, 8)}`;

  const current = $derived(options.find((o) => o.value === value && !o.header) ?? null);
  const shown = $derived(current?.label ?? placeholder);

  const pickable = $derived(options.filter((o) => !o.header && !o.disabled).length);
  const withSearch = $derived(searchable ?? pickable > searchThreshold);

  // Accent- and case-insensitive contains, so "prefere" finds "Préféré".
  const norm = (s) =>
    (s ?? '')
      .toString()
      .normalize('NFD')
      .replace(/\p{Diacritic}/gu, '')
      .toLowerCase();

  /** The rows actually rendered: everything when the query is empty, else the matches with
   *  the group headers that still have children under them. */
  const view = $derived.by(() => {
    const needle = norm(q.trim());
    if (!needle) return options;
    const hit = options.map((o) => !o.header && norm(o.label).includes(needle));
    return options.filter((o, i) => {
      if (!o.header) return hit[i];
      for (let j = i + 1; j < options.length && !options[j].header; j++) if (hit[j]) return true;
      return false;
    });
  });

  /** Label split around the matched run, so the menu shows *why* a row is listed. */
  function parts(label) {
    const needle = q.trim();
    if (!needle) return [label];
    const at = norm(label).indexOf(norm(needle));
    if (at < 0) return [label];
    return [label.slice(0, at), label.slice(at, at + needle.length), label.slice(at + needle.length)];
  }

  // `header: true` marks a non-selectable group label (the <optgroup> of this listbox);
  // the highlight walks over those rather than landing on them.
  const step = (from, dir) => {
    const n = view.length;
    if (!n) return -1;
    for (let i = 1; i <= n; i++) {
      const j = (from + dir * i + n * i) % n;
      if (!view[j]?.header && !view[j]?.disabled) return j;
    }
    return from;
  };

  /** Viewport coordinates for the popup: under the trigger, flipped above it when the menu
   *  would not fit below, and never wider than the window. */
  function measure() {
    if (!float || !btn) return;
    const r = btn.getBoundingClientRect();
    const below = window.innerHeight - r.bottom;
    const up = below < 300 && r.top > below;
    anchor = {
      left: Math.max(4, Math.min(r.left, window.innerWidth - r.width - 4)),
      width: Math.max(r.width, 140),
      top: up ? null : r.bottom + 2,
      bottom: up ? window.innerHeight - r.top + 2 : null,
      max: Math.max(140, (up ? r.top : below) - 12)
    };
  }

  function openMenu() {
    q = '';
    measure();
    active = view.findIndex((o) => o.value === value && !o.header);
    if (active < 0) active = step(-1, 1);
    open = true;
  }
  function close({ refocus = false } = {}) {
    open = false;
    q = '';
    if (refocus) btn?.focus();
  }
  function pick(v) {
    value = v;
    onpick?.(v);
    close({ refocus: true });
  }

  function onButtonKeydown(e) {
    if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      openMenu();
    }
  }

  // One handler for the whole popup: the search field is inside it, so typing keeps working
  // while the arrows keep driving the list.
  function onMenuKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close({ refocus: true });
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = step(active, 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = step(active, -1);
    } else if (e.key === 'Home' && !withSearch) {
      e.preventDefault();
      active = step(-1, 1);
    } else if (e.key === 'End' && !withSearch) {
      e.preventDefault();
      active = step(view.length, -1);
    } else if (e.key === 'Enter' || (e.key === ' ' && !withSearch)) {
      e.preventDefault();
      if (view[active] && !view[active].header && !view[active].disabled) pick(view[active].value);
    } else if (e.key === 'Tab') {
      close();
    }
  }

  // Filtering moves the rows under the highlight; land it on the first match instead.
  function onInput() {
    active = step(-1, 1);
  }

  // Focus the search field (or the list) when it opens so the keys work without a second click.
  $effect(() => {
    if (!open) return;
    if (withSearch) field?.focus();
    else menu?.focus();
  });

  // Keep the highlight visible in a scrolled menu.
  $effect(() => {
    if (open) rows[active]?.scrollIntoView({ block: 'nearest' });
  });

  // A pinned popup is outside the flow, so it has to follow the trigger by hand.
  $effect(() => {
    if (!open || !float) return;
    const on = () => measure();
    window.addEventListener('scroll', on, true);
    window.addEventListener('resize', on);
    return () => {
      window.removeEventListener('scroll', on, true);
      window.removeEventListener('resize', on);
    };
  });
</script>

<div class="dd" use:clickOutside={() => close()}>
  <button
    bind:this={btn}
    type="button"
    class="trigger"
    class:placeholder={!current}
    {disabled}
    title={title || undefined}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={ariaLabel || undefined}
    onclick={() => (open ? close() : openMenu())}
    onkeydown={onButtonKeydown}
  >
    {#if current?.color}
      <span class="dot" style:background={current.color} aria-hidden="true"></span>
    {/if}
    <span class="lbl">{shown}</span>
    <Icon name={open ? 'chevron-up' : 'chevron-down'} size={13} />
  </button>

  {#if open}
    <!-- Key handling lives on the popup so it works from the search field too. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="pop"
      class:float
      style={float && anchor
        ? `left:${anchor.left}px;width:${anchor.width}px;` +
          (anchor.top != null ? `top:${anchor.top}px;` : `bottom:${anchor.bottom}px;`) +
          `--pop-max:${anchor.max}px`
        : undefined}
      onkeydown={onMenuKeydown}
      role="presentation"
    >
      {#if withSearch}
        <div class="search">
          <Icon name="search" size={12} />
          <!-- svelte-ignore a11y_autofocus -->
          <input
            bind:this={field}
            bind:value={q}
            oninput={onInput}
            type="text"
            spellcheck="false"
            autocomplete="off"
            placeholder={searchPlaceholder || ariaLabel || '…'}
            aria-controls={uid}
            aria-activedescendant={view[active] && !view[active].header ? `${uid}-${active}` : undefined}
          />
          {#if q}
            <button type="button" class="clear" onclick={() => ((q = ''), onInput(), field?.focus())}>
              <Icon name="x" size={11} />
            </button>
          {/if}
        </div>
      {/if}

      <ul
        bind:this={menu}
        id={uid}
        class="menu"
        role="listbox"
        tabindex="-1"
        aria-label={ariaLabel || undefined}
      >
        {#each view as o, i (o.header ? `h${i}` : o.value)}
          {#if o.header}
            <li class="grp" role="presentation">{o.label}</li>
          {:else}
            <!-- The listbox pattern puts focus and key handling on the container, not on each
                 option, so the rows carry no handler of their own by design. -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <li
              bind:this={rows[i]}
              id="{uid}-{i}"
              role="option"
              aria-selected={o.value === value}
              class:active={i === active}
              class:sel={o.value === value}
              class:off={o.disabled}
              aria-disabled={o.disabled || undefined}
              onclick={() => !o.disabled && pick(o.value)}
              onmouseenter={() => !o.disabled && (active = i)}
            >
              {#if o.color}
                <span class="dot" style:background={o.color} aria-hidden="true"></span>
              {:else if options.some((x) => x.color)}
                <!-- Keeps labels on one column when only some options carry a dot. -->
                <span class="dot ghost" aria-hidden="true"></span>
              {/if}
              <span class="lbl">
                {#each parts(o.label) as part, pi}{#if pi === 1}<b class="hit">{part}</b>{:else}{part}{/if}{/each}
              </span>
            </li>
          {/if}
        {/each}
        {#if !view.length}
          <li class="none" role="presentation">—</li>
        {/if}
      </ul>
    </div>
  {/if}
</div>

<style>
  .dd {
    position: relative;
  }

  .trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    background: transparent;
    border: var(--hairline, 0.5px) solid var(--border-control);
    border-radius: 0;
    height: var(--control-h);
    padding: 0 var(--space-3);
    color: var(--text);
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
  .trigger.placeholder .lbl {
    color: var(--faint, var(--muted));
  }
  .trigger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .trigger:disabled:hover {
    background: transparent;
  }

  .lbl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  .dot.ghost {
    background: transparent;
  }

  .pop.float {
    position: fixed;
    top: auto;
    left: auto;
    right: auto;
  }
  .pop.float .menu {
    max-height: min(260px, var(--pop-max, 260px));
  }
  .pop {
    position: absolute;
    z-index: var(--z-dropdown, 50);
    top: calc(100% + 2px);
    left: 0;
    right: 0;
    background: var(--surface);
    border: var(--hairline, 0.5px) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg, 0 8px 24px rgb(0 0 0 / 0.35));
    overflow: hidden;
  }

  /* The field stays put while the list scrolls under it. */
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-2);
    height: var(--control-h);
    border-bottom: var(--hairline, 0.5px) solid var(--border);
    color: var(--faint, var(--muted));
  }
  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-body);
  }
  .search input:focus {
    outline: none;
  }
  .clear {
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    padding: 2px;
    color: var(--muted);
    cursor: pointer;
  }
  .clear:hover {
    color: var(--text);
  }

  .menu {
    max-height: 260px;
    overflow-y: auto;
    overscroll-behavior: contain;
    margin: 0;
    padding: var(--space-1);
    list-style: none;
    outline: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-body);
    cursor: pointer;
  }
  /* Hover and keyboard share one highlight, so the mouse and the arrows never disagree
     about which row is "current". */
  li.active {
    background: var(--surface-2);
    color: var(--text);
  }
  /* After .active on purpose: the picked value keeps its accent even while the cursor
     highlight sits on it — highlight says "where you are", color says "what is set". */
  li.sel {
    color: var(--accent);
    font-weight: var(--fw-medium);
  }
  /* The matched run, so a filtered list explains itself at a glance. */
  .hit {
    color: var(--accent);
    font-weight: var(--fw-medium);
  }
  li.active .hit {
    color: inherit;
    text-decoration: underline;
  }

  li.off {
    color: var(--faint, var(--muted));
    opacity: 0.55;
    cursor: not-allowed;
  }
  li.grp {
    padding: var(--space-2) var(--space-2) var(--space-1);
    color: var(--faint, var(--muted));
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    cursor: default;
  }
  li.none {
    color: var(--faint, var(--muted));
    justify-content: center;
    cursor: default;
  }
</style>
