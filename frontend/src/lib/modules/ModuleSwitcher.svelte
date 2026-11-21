<script>
  import { moduleForPath, visibleModules } from './registry';
  import { installedIds } from './installed.js';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { pref } from '$lib/prefs.svelte.js';

  // Modules by family, in the order the families are shown. A module missing here (a new
  // or community one) lands in "other", so it is never hidden.
  const GROUPS = [
    { key: 'trading', tone: 'green', ids: ['histviz', 'backtest', 'robots', 'scanner', 'options', 'watchlists', 'journal', 'routines'] },
    { key: 'research', tone: 'blue', ids: ['findb', 'fundamentals', 'economics', 'histdata', 'mportfolios', 'quant', 'news', 'mailbox', 'community-docs', 'resources'] },
    { key: 'wealth', tone: 'teal', ids: ['portfolios', 'wealth', 'subscriptions', 'taxcalc', 'goals'] },
    { key: 'assistant', tone: 'violet', ids: ['agent', 'prompt-store', 'editor', 'mindset'] },
    { key: 'organize', tone: 'amber', ids: ['calendar', 'todos', 'time', 'remindme'] },
    { key: 'workspace', tone: 'green', ids: ['dashboard', 'automator', 'webhooks'] }
  ];
  const groupOf = new Map(GROUPS.flatMap((g) => g.ids.map((id) => [id, g])));
  const OTHER = { key: 'other', tone: 'blue' };
  const RECENT_MAX = 3;

  let open = $state(false);
  let filter = $state('');
  let filterEl = $state();
  let listEl = $state();
  // Index of the keyboard-highlighted tile in the flat order (groups, then tiles).
  let cursor = $state(0);
  const recentPref = pref('otw.switcher.recent', []);

  const current = $derived(moduleForPath(page.url.pathname));
  const modules = $derived(visibleModules($installedIds));
  const desc = (m) => {
    const s = m.descKey ? $t(m.descKey) : m.home ? $t('switcher.homeDesc') : '';
    return s.replace(/\.$/, '');
  };

  const shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    if (!f) return modules;
    return modules.filter((m) => m.name.toLowerCase().includes(f) || desc(m).toLowerCase().includes(f));
  });
  const groups = $derived.by(() => {
    const by = new Map();
    for (const m of shown) {
      const g = groupOf.get(m.id) ?? OTHER;
      if (!by.has(g)) by.set(g, []);
      by.get(g).push(m);
    }
    const order = [...GROUPS, OTHER];
    return [...by.entries()]
      .sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]))
      .map(([g, list]) => ({
        ...g,
        list: list.slice().sort((a, b) => g.ids?.indexOf(a.id) - g.ids?.indexOf(b.id) || a.name.localeCompare(b.name))
      }));
  });
  const flat = $derived(groups.flatMap((g) => g.list));
  const indexOf = $derived(new Map(flat.map((m, i) => [m.id, i])));
  const recents = $derived(
    (Array.isArray(recentPref.value) ? recentPref.value : [])
      .filter((id) => id !== current.id)
      .map((id) => modules.find((m) => m.id === id))
      .filter(Boolean)
      .slice(0, RECENT_MAX)
  );

  // Every module visited goes to the front of the recent list.
  $effect(() => {
    const id = current.id;
    const list = Array.isArray(recentPref.value) ? recentPref.value : [];
    if (list[0] === id) return;
    recentPref.value = [id, ...list.filter((x) => x !== id)].slice(0, 8);
  });

  // Typing narrows the list under the cursor; snap it back to the top match.
  $effect(() => {
    filter;
    cursor = 0;
  });

  function toggle() {
    open = !open;
    filter = '';
    cursor = Math.max(0, flat.findIndex((m) => m.id === current.id));
    if (open) setTimeout(() => filterEl?.focus(), 0);
  }

  function pick(m) {
    open = false;
    if (m.base !== current.base) goto(m.base);
  }

  /** The tile next to the cursor on screen: left/right in reading order, up/down the
      nearest tile in the row above or below, closest to the same column. */
  function step(dir) {
    const tiles = [...(listEl?.querySelectorAll('[data-i]') ?? [])];
    if (!tiles.length) return;
    if (dir === 'left' || dir === 'right') {
      const n = flat.length;
      cursor = (cursor + (dir === 'right' ? 1 : -1) + n) % n;
    } else {
      const from = tiles.find((el) => +el.dataset.i === cursor)?.getBoundingClientRect();
      if (!from) return;
      const down = dir === 'down';
      let best = null;
      let bestScore = Infinity;
      for (const el of tiles) {
        const r = el.getBoundingClientRect();
        const dy = down ? r.top - from.top : from.top - r.top;
        if (dy <= 1) continue;
        const score = dy * 1000 + Math.abs(r.left - from.left);
        if (score < bestScore) {
          bestScore = score;
          best = el;
        }
      }
      if (best) cursor = +best.dataset.i;
      else cursor = down ? 0 : flat.length - 1;
    }
    queueMicrotask(() => listEl?.querySelector(`[data-i="${cursor}"]`)?.scrollIntoView({ block: 'nearest' }));
  }

  function onFilterKey(e) {
    const keys = { ArrowDown: 'down', ArrowUp: 'up', ArrowLeft: 'left', ArrowRight: 'right' };
    if (e.key === 'Escape') {
      open = false;
    } else if (keys[e.key]) {
      // Left/right keep moving the caret while there is text to edit.
      if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && filter) return;
      e.preventDefault();
      step(keys[e.key]);
    } else if (e.key === 'Enter' && flat[cursor]) {
      pick(flat[cursor]);
    }
  }

  // Hover follows the pointer only when it really moves: a keyboard scroll slides tiles
  // under a still pointer, and taking that as a hover would bounce the highlight.
  let lastPt = '';
  function hover(e, i) {
    const pt = `${e.clientX},${e.clientY}`;
    if (pt === lastPt) return;
    lastPt = pt;
    if (cursor !== i) cursor = i;
  }

  function onWindowClick(e) {
    if (!e.target.closest('.switcher')) open = false;
  }
</script>

<svelte:window on:click={onWindowClick} />

<div class="switcher">
  <button class="current" onclick={toggle} aria-haspopup="listbox" aria-expanded={open}>
    <span class="icon"><Icon name={current.icon} /></span>
    <span class="name">{current.name}</span>
    <span class="chevron" class:open><Icon name="chevron-down" size={14} /></span>
  </button>

  {#if open}
    <div class="menu">
      <!-- One frame for the field: the input inside is borderless. -->
      <div class="find">
        <Icon name="search" size={16} />
        <!-- The input keeps focus; the highlighted tile is announced through
             aria-activedescendant, which is how a combobox is meant to work. -->
        <input
          class="filter"
          placeholder={$t('switcher.find')}
          role="combobox"
          aria-expanded="true"
          aria-controls="switcher-list"
          aria-activedescendant={flat[cursor] ? `switcher-opt-${flat[cursor].id}` : undefined}
          bind:this={filterEl}
          bind:value={filter}
          onkeydown={onFilterKey}
        />
      </div>

      <div class="body" bind:this={listEl}>
        {#if !filter.trim() && recents.length}
          <div class="gh"><span>{$t('switcher.recent')}</span><span class="gl"></span></div>
          <div class="recents">
            {#each recents as m (m.id)}
              <button type="button" class="chip" tabindex="-1" onclick={() => pick(m)}>
                <Icon name={m.icon} size={14} />
                <span>{m.name}</span>
              </button>
            {/each}
          </div>
        {/if}

        <div id="switcher-list" role="listbox">
          {#each groups as g (g.key)}
            <div class="gh"><span>{$t(`switcher.group.${g.key}`)}</span><span class="gl"></span></div>
            <div class="grid">
              {#each g.list as m (m.id)}
                {@const i = indexOf.get(m.id)}
                <button
                  id="switcher-opt-{m.id}"
                  class="item"
                  class:active={m.id === current.id}
                  class:cursor={i === cursor}
                  data-i={i}
                  onclick={() => pick(m)}
                  onpointermove={(e) => hover(e, i)}
                  role="option"
                  tabindex="-1"
                  aria-selected={m.id === current.id}
                >
                  <span class="ic t-{g.tone}"><Icon name={m.icon} size={16} /></span>
                  <span class="label">
                    <span class="item-name">{m.name}</span>
                    {#if desc(m)}<span class="item-desc">{desc(m)}</span>{/if}
                  </span>
                  <span class="check" class:on={m.id === current.id}><Icon name="check" size={14} /></span>
                </button>
              {/each}
            </div>
          {:else}
            <p class="none">{$t('switcher.none')}</p>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .switcher {
    position: relative;
  }

  .current {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    background: transparent;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius);
    color: var(--text);
    padding: 6px 10px;
    cursor: pointer;
    font-size: 15px;
    font-weight: var(--fw-medium);
    transition: background var(--dur-fast) var(--ease);
  }
  .current:hover,
  .current[aria-expanded='true'] {
    background: var(--surface-2);
  }
  .current .icon {
    color: var(--accent);
    display: inline-flex;
    flex-shrink: 0;
  }
  .current .name {
    flex: 1;
    min-width: 0;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chevron {
    color: var(--muted);
    display: inline-flex;
    flex-shrink: 0;
    transition: transform var(--dur-fast) var(--ease);
  }
  .chevron.open {
    transform: rotate(180deg);
  }

  .menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: var(--z-dropdown);
    width: min(460px, calc(100vw - 2 * var(--space-4)));
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-2);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .find {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex: none;
    height: 48px;
    padding: 0 var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
    color: var(--muted);
  }
  .filter {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    box-shadow: none;
    color: var(--text);
    font-size: var(--text-base);
  }
  .filter:focus,
  .filter:focus-visible {
    border: none;
    box-shadow: none;
    outline: none;
  }
  .filter::placeholder {
    color: var(--dim);
  }
  .body {
    max-height: min(70vh, 560px);
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: var(--space-1) var(--space-2) var(--space-3);
  }
  .gh {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: var(--space-3) var(--space-2) var(--space-2);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .gl {
    flex: 1;
    height: var(--hairline);
    background: var(--border);
  }
  .recents {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    padding: 0 var(--space-1);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .chip :global(svg) {
    color: var(--muted);
  }
  .chip:hover {
    background: color-mix(in srgb, var(--text) 8%, var(--surface-2));
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
  }
  .none {
    margin: var(--space-4) var(--space-2);
    color: var(--muted);
    font-size: var(--text-sm);
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    padding: 6px var(--space-2);
    background: transparent;
    border: var(--hairline) solid transparent;
    border-radius: var(--radius-lg);
    color: var(--text);
    cursor: pointer;
    text-align: left;
  }
  .item.cursor {
    background: var(--surface-2);
  }
  .item.active {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }
  .ic {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: var(--radius);
  }
  .t-green {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
  }
  .t-amber {
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    color: var(--amber);
  }
  .t-blue {
    background: color-mix(in srgb, #6ea8fe 14%, transparent);
    color: #8fb8f5;
  }
  .t-violet {
    background: color-mix(in srgb, #a78bfa 14%, transparent);
    color: #b9a5f5;
  }
  .t-teal {
    background: color-mix(in srgb, #4fd1c5 14%, transparent);
    color: #6fd6cc;
  }
  .label {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .item-name,
  .item-desc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-name {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .item-desc {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  /* Always laid out, shown only on the current module: the tiles keep one width. */
  .check {
    display: inline-flex;
    flex: none;
    color: var(--accent);
    visibility: hidden;
  }
  .check.on {
    visibility: visible;
  }
  @media (max-width: 520px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
