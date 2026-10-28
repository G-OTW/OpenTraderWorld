<script>
  // Symbol picker shared by Company, Alternative data and ETF. The chips are the favourites,
  // then the RECENT_SYMBOLS most recently opened. The field searches every stored symbol;
  // `lookup` adds a provider search below the stored matches, and a ticker found nowhere is
  // handed to the page as typed, which opens it (resolve, store, fetch) as before.
  //
  // props: items ([{ticker, name, followed, opened}]), value (the open ticker), placeholder,
  //        fallback (tickers shown while nothing is stored), lookup (async q => [{ticker,
  //        name, detail}]), onpick(ticker), onfollow(ticker, followed), busy
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { RECENT_SYMBOLS } from './constants.js';
  import { t } from '$lib/i18n';

  let {
    items = [],
    value = '',
    placeholder = '',
    fallback = [],
    lookup = null,
    onpick,
    onfollow,
    busy = false
  } = $props();

  let q = $state('');
  let open = $state(false);
  let found = $state([]);
  let active = $state(-1);
  let list = $state(null);

  const norm = (s) => (s ?? '').toString().toLowerCase();

  const chips = $derived.by(() => {
    if (!items.length) return fallback.map((ticker) => ({ ticker, suggested: true }));
    const fav = items.filter((i) => i.followed).sort((a, b) => a.ticker.localeCompare(b.ticker));
    const recent = items
      .filter((i) => !i.followed)
      .sort((a, b) => (b.opened ?? 0) - (a.opened ?? 0))
      .slice(0, RECENT_SYMBOLS);
    return [...fav, ...recent];
  });

  // Stored matches: an exact ticker first, then favourites, then by ticker.
  const matches = $derived.by(() => {
    const n = norm(q.trim());
    const hit = n ? items.filter((i) => norm(i.ticker).includes(n) || norm(i.name).includes(n)) : items;
    const rank = (i) => (norm(i.ticker) === n ? 0 : i.followed ? 1 : 2);
    return [...hit].sort((a, b) => rank(a) - rank(b) || a.ticker.localeCompare(b.ticker));
  });

  $effect(() => {
    const s = q.trim();
    found = [];
    if (!lookup || !s) return;
    const timer = setTimeout(() => {
      lookup(s)
        .then((r) => {
          if (q.trim() === s) found = r.filter((x) => !items.some((i) => i.ticker === x.ticker));
        })
        .catch(() => {});
    }, 250);
    return () => clearTimeout(timer);
  });

  // One list for the keyboard: stored matches, provider matches, then the typed ticker.
  const typed = $derived(q.trim().toUpperCase());
  const rows = $derived([
    ...matches.map((i) => ({ ...i, kind: 'stored' })),
    ...found.map((i) => ({ ...i, kind: 'found' })),
    ...(typed && ![...matches, ...found].some((i) => i.ticker === typed) ? [{ ticker: typed, kind: 'typed' }] : [])
  ]);

  $effect(() => {
    rows;
    active = -1;
  });

  function pick(tk) {
    q = '';
    open = false;
    active = -1;
    onpick?.(tk.toUpperCase());
  }

  function submit(e) {
    e.preventDefault();
    if (active >= 0 && rows[active]) pick(rows[active].ticker);
    else if (typed) pick(typed);
  }

  function onkeydown(e) {
    if (e.key === 'Escape') {
      open = false;
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    open = true;
    if (!rows.length) return;
    active = e.key === 'ArrowDown' ? (active + 1) % rows.length : (active - 1 + rows.length) % rows.length;
    list?.children[active + headerOffset(active)]?.scrollIntoView({ block: 'nearest' });
  }

  // Group headers sit in the list between rows: count those above row `i`.
  const headerOffset = (i) => (matches.length ? 1 : 0) + (found.length && i >= matches.length ? 1 : 0);
</script>

<div class="sp">
  <form class="search" onsubmit={submit} use:clickOutside={() => (open = false)}>
    <div class="box">
      <input
        type="search"
        {placeholder}
        bind:value={q}
        disabled={busy}
        autocomplete="off"
        spellcheck="false"
        onfocus={() => (open = true)}
        oninput={() => (open = true)}
        {onkeydown}
      />
      {#if open && rows.length}
        <ul class="menu" bind:this={list} role="listbox">
          {#each rows as r, i (r.kind + r.ticker)}
            {#if i === 0 && r.kind === 'stored'}
              <li class="grp" role="presentation">{$t('fundamentals.picker.stored')}</li>
            {:else if r.kind === 'found' && (i === 0 || rows[i - 1].kind !== 'found')}
              <li class="grp" role="presentation">{$t('fundamentals.picker.found')}</li>
            {/if}
            <li class="row" class:active={i === active} role="option" aria-selected={i === active}>
              <button type="button" class="main" onclick={() => pick(r.ticker)}>
                {#if r.kind === 'typed'}
                  <Icon name="download" size={13} />
                  <span>{$t('fundamentals.picker.open', { ticker: r.ticker })}</span>
                {:else}
                  <strong>{r.ticker}</strong>
                  <span class="nm">{r.name ?? ''}</span>
                  {#if r.detail}<span class="dt">{r.detail}</span>{/if}
                {/if}
              </button>
              {#if r.kind === 'stored' && onfollow}
                <button
                  type="button"
                  class="star"
                  class:on={r.followed}
                  title={r.followed ? $t('fundamentals.picker.unfavorite') : $t('fundamentals.picker.favorite')}
                  aria-label={r.followed ? $t('fundamentals.picker.unfavorite') : $t('fundamentals.picker.favorite')}
                  onclick={() => onfollow(r.ticker, !r.followed)}
                >
                  <Icon name="star" size={13} />
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <Button type="submit" icon="search" disabled={busy || !typed}>{$t('fundamentals.open')}</Button>
  </form>

  <div class="chips">
  {#each chips as c (c.ticker)}
    <span class="chip sym" class:active={value === c.ticker}>
      <button type="button" class="tk" onclick={() => pick(c.ticker)} title={c.name ?? undefined}>{c.ticker}</button>
      {#if !c.suggested && onfollow}
        <button
          type="button"
          class="star"
          class:on={c.followed}
          title={c.followed ? $t('fundamentals.picker.unfavorite') : $t('fundamentals.picker.favorite')}
          aria-label={c.followed ? $t('fundamentals.picker.unfavorite') : $t('fundamentals.picker.favorite')}
          onclick={() => onfollow(c.ticker, !c.followed)}
        >
          <Icon name="star" size={11} />
        </button>
      {/if}
    </span>
  {/each}
  </div>
</div>

<style>
  .sp {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
    min-width: 0;
  }
  .search {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--space-2);
  }
  /* One line that scrolls sideways: the chips never wrap nor push the page wider. */
  .chips {
    flex: 1 1 240px;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    overflow-x: auto;
    scrollbar-width: thin;
    padding: 2px 0;
  }
  .chips > :global(*) {
    flex: none;
  }
  .box {
    position: relative;
  }
  .box input {
    width: 260px;
    max-width: 100%;
  }
  .menu {
    position: absolute;
    z-index: var(--z-dropdown, 50);
    top: calc(100% + 2px);
    left: 0;
    width: min(480px, 90vw);
    max-height: 320px;
    overflow-y: auto;
    overscroll-behavior: contain;
    list-style: none;
    margin: 0;
    padding: var(--space-1);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
  }
  .grp {
    padding: var(--space-2) var(--space-2) var(--space-1);
    color: var(--muted);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .row {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
  }
  .row.active,
  .row:hover {
    background: var(--surface-2);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    padding: var(--space-2);
    border: 0;
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }
  .nm,
  .dt {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .nm {
    color: var(--muted);
  }
  .dt {
    margin-left: auto;
    color: var(--muted);
    font-size: var(--text-xs);
  }

  /* One frame per chip: the chip owns the border, its two buttons are bare. */
  .sym {
    padding: 0;
    gap: 0;
  }
  .sym .tk {
    padding: 4px 4px 4px 12px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .sym .tk:last-child {
    padding-right: 12px;
  }
  .star {
    display: inline-flex;
    align-items: center;
    padding: 4px 8px 4px 4px;
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .row .star {
    padding: var(--space-2);
  }
  .sym .star:not(.on) {
    opacity: 0;
  }
  .sym:hover .star,
  .star:focus-visible {
    opacity: 1;
  }
  .star:hover {
    color: var(--amber);
  }
  .star.on {
    color: var(--amber);
  }
  .star.on :global(svg) {
    fill: currentColor;
  }
  @media (max-width: 767px) {
    .search {
      display: grid;
      grid-template-columns: minmax(0, 1fr);
      width: 100%;
      min-width: 0;
    }
    .box {
      min-width: 0;
    }
    .box input {
      width: 100%;
    }
    .search > :global(button) {
      width: 100%;
      min-height: 44px;
    }
    .chips {
      flex: 1 1 100%;
      flex-wrap: wrap;
      overflow: visible;
    }
    .sym .tk,
    .sym .star {
      min-height: 44px;
    }
    .sym .star:not(.on) {
      opacity: 1;
    }
    .menu .main {
      flex-wrap: wrap;
      align-items: flex-start;
    }
    .menu .nm,
    .menu .dt {
      overflow: visible;
      text-overflow: clip;
      white-space: normal;
      overflow-wrap: anywhere;
    }
  }
</style>
