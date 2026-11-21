<script>
  // Timeframe picker: a grid by unit (minutes, hours, days and more) under the user's
  // favourites, a field that takes a timeframe as typed (7m, 2h, 3D), and a row that adds a
  // custom one. A custom timeframe (consolidated, not served) carries an amber dot.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { consolidationBase, byDuration } from './pane.js';

  let {
    value = '',
    /** What the chart can show: served plus usable custom ones, sorted. */
    timeframes = [],
    /** What the provider serves as is. */
    served = [],
    /** The user's custom timeframes. */
    custom = [],
    /** Favourite timeframes; null = the default set. */
    favorites = null,
    disabled = false,
    placeholder = '',
    ariaLabel = '',
    onpick,
    /** The new custom list, when one is added from here. */
    oncustom,
    onfavorites,
    /** Open the custom timeframes' manager. */
    onmanage
  } = $props();

  const DEFAULT_FAV = ['1m', '5m', '15m', '1h', '4h'];
  const UNITS = ['m', 'h', 'd', 'w'];

  let open = $state(false);
  let btn = $state(null);
  let field = $state(null);
  let at = $state(null);
  let q = $state('');
  let count = $state(7);
  let unit = $state('m');
  /** The cell under the pointer or the focus: what F toggles. */
  let hot = $state(null);

  const favs = $derived((favorites ?? DEFAULT_FAV).filter((tf) => timeframes.includes(tf)));
  const unitOf = (tf) => /^\d+([mhdw])$/.exec(tf)?.[1] ?? 'x';
  /** Minutes and hours read as bare numbers under their heading; days and more keep their
   *  unit, upper case, as a trader writes them (1D, 1W). */
  const short = (tf) => {
    const u = unitOf(tf);
    if (u === 'm' || u === 'h') return tf.slice(0, -1);
    return u === 'x' ? tf : tf.toUpperCase();
  };
  const full = (tf) => (unitOf(tf) === 'd' || unitOf(tf) === 'w' ? tf.toUpperCase() : tf);

  const groups = $derived(
    [
      ['minutes', timeframes.filter((tf) => unitOf(tf) === 'm')],
      ['hours', timeframes.filter((tf) => unitOf(tf) === 'h')],
      ['days', timeframes.filter((tf) => !['m', 'h'].includes(unitOf(tf)))]
    ].filter(([, list]) => list.length)
  );

  /** A typed timeframe in canonical form: `7 min` → 7m, `3D` → 3d. */
  const norm = (s) =>
    String(s ?? '')
      .trim()
      .toLowerCase()
      .replace(/\s+/g, '')
      .replace(/(mn|min|mins|minutes?)$/, 'm')
      .replace(/(hr|hrs|hours?)$/, 'h')
      .replace(/(days?|j)$/, 'd')
      .replace(/(wk|weeks?|sem)$/, 'w');
  const typed = $derived(norm(q));
  const matches = $derived(typed ? timeframes.filter((tf) => tf.startsWith(typed)) : []);

  /** Why `tf` cannot be added as a custom timeframe, or '' when it can. */
  function problemOf(tf) {
    if (!/^\d+[mhdw]$/.test(tf) || !(parseInt(tf, 10) > 0)) return $t('histviz.customTf.badCount');
    if (served.includes(tf)) return $t('histviz.customTf.served', { tf });
    if (custom.includes(tf)) return $t('histviz.customTf.exists', { tf });
    if (!consolidationBase(served, tf)) return $t('histviz.customTf.noBase', { tf });
    return '';
  }
  const typedNew = $derived(typed && !timeframes.includes(typed) && !problemOf(typed) ? typed : '');
  const footTf = $derived(Number.isInteger(+count) && +count > 0 ? `${+count}${unit}` : '');
  const footProblem = $derived(
    footTf && timeframes.includes(footTf) ? '' : problemOf(footTf)
  );

  function measure() {
    if (!btn) return;
    const r = btn.getBoundingClientRect();
    const w = 280;
    const below = window.innerHeight - r.bottom;
    const up = below < 360 && r.top > below;
    at = {
      left: Math.max(4, Math.min(r.left, window.innerWidth - w - 4)),
      top: up ? null : r.bottom + 4,
      bottom: up ? window.innerHeight - r.top + 4 : null,
      max: Math.max(200, (up ? r.top : below) - 12)
    };
  }
  function toggle() {
    if (disabled) return;
    if (open) return close();
    q = '';
    measure();
    open = true;
    queueMicrotask(() => field?.focus());
  }
  function close() {
    open = false;
    hot = null;
  }
  function pick(tf) {
    close();
    if (tf && tf !== value) onpick?.(tf);
  }
  function add(tf) {
    if (!tf) return;
    if (timeframes.includes(tf)) return pick(tf);
    if (problemOf(tf)) return;
    oncustom?.([...custom, tf].sort(byDuration));
    pick(tf);
  }
  function toggleFav(tf) {
    const list = favorites ?? DEFAULT_FAV;
    onfavorites?.(list.includes(tf) ? list.filter((x) => x !== tf) : [...list, tf].sort(byDuration));
  }

  function onkey(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      btn?.focus();
    } else if ((e.key === 'f' || e.key === 'F') && hot && e.target?.tagName !== 'INPUT' && e.target?.tagName !== 'SELECT') {
      e.preventDefault();
      toggleFav(hot);
    }
  }

  $effect(() => {
    if (!open) return;
    const on = () => measure();
    window.addEventListener('scroll', on, true);
    window.addEventListener('resize', on);
    return () => {
      window.removeEventListener('scroll', on, true);
      window.removeEventListener('resize', on);
    };
  });
</script>

{#snippet cell(tf, label)}
  {@const base = served.includes(tf) ? null : consolidationBase(served, tf)}
  <button
    type="button"
    class="cell"
    class:on={tf === value}
    title={[full(tf), base ? $t('histviz.customTf.fromShort', { base }) : '', $t('histviz.tfPick.favHint')].filter(Boolean).join(' · ')}
    onclick={() => pick(tf)}
    oncontextmenu={(e) => {
      e.preventDefault();
      toggleFav(tf);
    }}
    onpointerenter={() => (hot = tf)}
    onpointerleave={() => (hot = null)}
    onfocus={() => (hot = tf)}
  >
    {label}
    {#if base}<span class="cdot" aria-hidden="true"></span>{/if}
  </button>
{/snippet}

<div class="tfp" use:clickOutside={() => open && close()}>
  <button
    type="button"
    class="trig"
    class:open
    bind:this={btn}
    {disabled}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-label={ariaLabel}
    title={ariaLabel}
    onclick={toggle}
  >
    <span class="tv">{value ? full(value) : placeholder}</span>
    <Icon name={open ? 'chevron-up' : 'chevron-down'} size={12} />
  </button>

  {#if open && at}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="pop"
      role="dialog"
      aria-label={ariaLabel}
      tabindex="-1"
      style="left:{at.left}px;{at.top != null ? `top:${at.top}px` : `bottom:${at.bottom}px`};max-height:{at.max}px"
      onkeydown={onkey}
    >
      <div class="search">
        <Icon name="search" size={14} />
        <input
          bind:this={field}
          bind:value={q}
          placeholder={$t('histviz.tfPick.search')}
          aria-label={$t('histviz.tfPick.search')}
          onkeydown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') {
              e.preventDefault();
              if (matches.includes(typed)) pick(typed);
              else if (typedNew) add(typedNew);
              else if (matches.length) pick(matches[0]);
            } else if (e.key === 'Escape') onkey(e);
          }}
        />
        {#if onmanage}
          <button type="button" class="mng" title={$t('histviz.customTf.title')} aria-label={$t('histviz.customTf.title')} onclick={() => (close(), onmanage())}>
            <Icon name="settings" size={13} />
          </button>
        {/if}
      </div>

      <div class="body">
        {#if typed}
          <div class="grid">
            {#each matches as tf (tf)}
              {@render cell(tf, full(tf))}
            {/each}
            {#if typedNew}
              <button type="button" class="cell add" onclick={() => add(typedNew)}>
                <Icon name="plus" size={12} />{full(typedNew)}
              </button>
            {/if}
          </div>
          {#if !matches.length && !typedNew}
            <p class="none">{problemOf(typed)}</p>
          {/if}
        {:else}
          {#if favs.length}
            <p class="head"><Icon name="star" size={12} />{$t('histviz.tfPick.favorites')}</p>
            <div class="grid">
              {#each favs as tf (tf)}
                {@render cell(tf, full(tf))}
              {/each}
            </div>
          {/if}
          {#each groups as [key, list] (key)}
            <p class="head">{$t(`histviz.tfPick.${key}`)}</p>
            <div class="grid">
              {#each list as tf (tf)}
                {@render cell(tf, short(tf))}
              {/each}
            </div>
          {/each}
        {/if}
      </div>

      <div class="foot">
        <input
          class="cnt"
          type="number"
          min="1"
          step="1"
          bind:value={count}
          aria-label={$t('histviz.customTf.count')}
          onkeydown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') add(footTf);
          }}
        />
        <select class="unit" bind:value={unit} aria-label={$t('histviz.customTf.unit')}>
          {#each UNITS as u (u)}
            <option value={u}>{$t(`histviz.tfPick.unit.${u}`)}</option>
          {/each}
        </select>
        <button type="button" class="addb" disabled={!!footProblem} title={footProblem || $t('histviz.customTf.builtFrom', { tf: footTf, base: consolidationBase(served, footTf) ?? footTf })} onclick={() => add(footTf)}>
          <Icon name="plus" size={13} />{$t('histviz.customTf.add')}
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .tfp {
    display: inline-flex;
  }
  .trig {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: var(--control-h, 28px);
    padding: 0 var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    white-space: nowrap;
    cursor: pointer;
  }
  .trig :global(svg) {
    color: var(--muted);
  }
  .trig:hover:not(:disabled),
  .trig.open {
    background: var(--surface-2);
  }
  .trig:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .pop {
    position: fixed;
    z-index: var(--z-dropdown);
    display: flex;
    flex-direction: column;
    width: 280px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: 0 8px 28px rgb(0 0 0 / 0.32);
    overflow: hidden;
    outline: none;
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    padding: 0 var(--space-2) 0 var(--space-3);
    border-bottom: 1px solid var(--border);
    color: var(--muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    height: 40px;
    padding: 0;
    border: none;
    background: transparent;
    box-shadow: none;
    color: var(--text);
    font-size: var(--text-sm);
  }
  .search input:focus,
  .search input:focus-visible {
    outline: none;
    box-shadow: none;
  }
  .mng {
    display: inline-grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .mng:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-2) var(--space-2) var(--space-1);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: var(--space-2) var(--space-1) 6px;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .head:first-child {
    margin-top: 2px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 4px;
  }
  .cell {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    height: 30px;
    padding: 0 4px;
    border: none;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .cell:hover {
    background: color-mix(in srgb, var(--text) 10%, var(--surface-2));
  }
  .cell.on {
    background: color-mix(in srgb, var(--accent) 18%, var(--surface));
    color: var(--text);
  }
  .cell.add {
    color: var(--accent);
  }
  .cdot {
    position: absolute;
    top: 5px;
    right: 6px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--amber);
  }
  .none {
    margin: var(--space-2) var(--space-1);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    margin: var(--space-1) var(--space-2) 0;
    padding: var(--space-2) 0;
    border-top: 1px solid var(--border);
  }
  .cnt {
    width: 52px;
    height: 30px;
    padding: 0 var(--space-2);
    text-align: center;
    font-family: var(--mono);
    font-size: var(--text-sm);
  }
  .unit {
    width: auto;
    height: 30px;
    padding: 0 26px 0 var(--space-2);
    border-color: transparent;
    background-color: var(--surface-2);
    font-size: var(--text-sm);
  }
  .addb {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
    height: 30px;
    padding: 0 var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--accent);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .addb:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .addb:disabled {
    color: var(--muted);
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
