<script>
  // Plot style picker: a search field over the types in their groups, two columns, each
  // type with a small drawing of what it looks like.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { CHART_TYPE_GROUPS } from './indicators.js';

  let { value = $bindable(''), ariaLabel = '', onpick } = $props();

  /** One glyph per type, 16×16, stroked in the current color. */
  const GLYPH = {
    ohlc: '<path d="M5 2v12M3 5h2M5 10h2M11 3v10M9 6h2M11 11h2"/>',
    candlestick: '<path d="M5 2v2M5 11v3M11 3v3M11 11v2"/><rect x="3.5" y="4" width="3" height="7" fill="currentColor"/><rect x="9.5" y="6" width="3" height="5"/>',
    hollow: '<path d="M5 2v2M5 11v3M11 3v3M11 11v2"/><rect x="3.5" y="4" width="3" height="7"/><rect x="9.5" y="6" width="3" height="5"/>',
    volcandles: '<path d="M4 3v2M4 11v2M11 2v3M11 12v2"/><rect x="3" y="5" width="2" height="6"/><rect x="8.5" y="5" width="5" height="7" fill="currentColor"/>',
    line: '<path d="M2 12l4-5 3 2 5-6"/>',
    linemarkers: '<path d="M2 12l4-5 3 2 5-6"/><circle cx="6" cy="7" r="1.3" fill="currentColor"/><circle cx="9" cy="9" r="1.3" fill="currentColor"/>',
    step: '<path d="M2 12h4V7h4V4h4"/>',
    area: '<path d="M2 12l4-5 3 2 5-6v11H2z" fill="currentColor" fill-opacity=".25"/><path d="M2 12l4-5 3 2 5-6"/>',
    hlcarea: '<path d="M2 7l4-3 4 2 4-3v6l-4 3-4-2-4 3z" fill="currentColor" fill-opacity=".25"/><path d="M2 10l4-3 4 2 4-3"/>',
    baseline: '<path d="M2 8h12" stroke-dasharray="1.5 1.5"/><path d="M2 11l3-5 3 4 3-7 3 6"/>',
    columns: '<path d="M3 14V9M6 14V6M9 14V8M12 14V3" stroke-width="2"/>',
    highlow: '<path d="M3 4v7M6 6v6M9 3v6M12 5v8" stroke-width="2"/>',
    heikinashi: '<path d="M5 2v3M5 11v3M11 2v3M11 11v3"/><rect x="3.5" y="5" width="3" height="6" fill="currentColor"/><rect x="9.5" y="5" width="3" height="6" fill="currentColor"/>',
    renko: '<rect x="2" y="10" width="3" height="3"/><rect x="5" y="7" width="3" height="3"/><rect x="8" y="4" width="3" height="3" fill="currentColor"/><rect x="11" y="7" width="3" height="3"/>',
    linebreak: '<rect x="2.5" y="9" width="3" height="4"/><rect x="6.5" y="5" width="3" height="4" fill="currentColor"/><rect x="10.5" y="3" width="3" height="5"/>',
    kagi: '<path d="M2 13V6h4v5h3V3h3v7h2"/>',
    pnf: '<path d="M3 3l3 3M6 3L3 6M3 9l3 3M6 9l-3 3"/><circle cx="11" cy="5" r="1.8"/><circle cx="11" cy="10.5" r="1.8"/>',
    range: '<path d="M3 5v6M6 4v6M9 6v6M12 5v6" stroke-width="2"/>'
  };

  let open = $state(false);
  let btn = $state(null);
  let field = $state(null);
  let at = $state(null);
  let q = $state('');

  const label = (ty) => $t(`histviz.page.type.${ty}`);
  const groups = $derived.by(() => {
    const s = q.trim().toLowerCase();
    return CHART_TYPE_GROUPS.map(([g, list]) => [g, s ? list.filter((ty) => label(ty).toLowerCase().includes(s)) : list]).filter(
      ([, list]) => list.length
    );
  });

  function measure() {
    if (!btn) return;
    const r = btn.getBoundingClientRect();
    const w = 440;
    const below = window.innerHeight - r.bottom;
    const up = below < 380 && r.top > below;
    at = {
      left: Math.max(4, Math.min(r.left, window.innerWidth - w - 4)),
      top: up ? null : r.bottom + 4,
      bottom: up ? window.innerHeight - r.top + 4 : null,
      max: Math.max(220, (up ? r.top : below) - 12)
    };
  }
  function toggle() {
    if (open) return (open = false);
    q = '';
    measure();
    open = true;
    queueMicrotask(() => field?.focus());
  }
  function pick(ty) {
    open = false;
    if (ty === value) return;
    value = ty;
    onpick?.(ty);
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

{#snippet glyph(ty)}
  <svg class="gl" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <!-- eslint-disable-next-line svelte/no-at-html-tags — static, app-defined markup -->
    {@html GLYPH[ty] ?? ''}
  </svg>
{/snippet}

<div class="typ" use:clickOutside={() => (open = false)}>
  <button
    type="button"
    class="trig"
    class:open
    bind:this={btn}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-label={ariaLabel}
    title={ariaLabel}
    onclick={toggle}
  >
    <span class="tg">{@render glyph(value)}</span>
    <span class="tv">{value ? label(value) : ''}</span>
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
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          open = false;
          btn?.focus();
        }
      }}
    >
      <div class="search">
        <Icon name="search" size={14} />
        <input
          bind:this={field}
          bind:value={q}
          placeholder={$t('histviz.typePick.search')}
          aria-label={$t('histviz.typePick.search')}
          onkeydown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter' && groups.length) pick(groups[0][1][0]);
            else if (e.key === 'Escape') {
              open = false;
              btn?.focus();
            }
          }}
        />
      </div>
      <div class="body">
        {#each groups as [g, list] (g)}
          <p class="head">{$t(`histviz.page.typeGroup.${g}`)}</p>
          <div class="grid">
            {#each list as ty (ty)}
              <button type="button" class="item" class:on={ty === value} onclick={() => pick(ty)}>
                {@render glyph(ty)}
                <span>{label(ty)}</span>
              </button>
            {/each}
          </div>
        {:else}
          <p class="none">{$t('histviz.typePick.none')}</p>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .typ {
    display: inline-flex;
  }
  .trig {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: var(--control-h, 28px);
    padding: 0 var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    white-space: nowrap;
    cursor: pointer;
  }
  .trig :global(svg) {
    color: var(--muted);
  }
  .trig .tg :global(svg) {
    color: var(--accent);
  }
  .tg {
    display: inline-flex;
  }
  .trig:hover,
  .trig.open {
    background: var(--surface-2);
  }
  .pop {
    position: fixed;
    z-index: var(--z-dropdown);
    display: flex;
    flex-direction: column;
    width: min(440px, calc(100vw - 8px));
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
    padding: 0 var(--space-3);
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
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-1) var(--space-2) var(--space-2);
  }
  .head {
    margin: var(--space-3) var(--space-2) var(--space-1);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .head:first-child {
    margin-top: var(--space-2);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 30px;
    padding: 0 var(--space-2);
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .item .gl {
    flex: none;
    color: var(--muted);
  }
  .item:hover {
    background: var(--surface-2);
  }
  .item.on {
    background: color-mix(in srgb, var(--accent) 16%, var(--surface));
  }
  .item.on .gl {
    color: var(--accent);
  }
  .none {
    margin: var(--space-3) var(--space-2);
    color: var(--muted);
    font-size: var(--text-xs);
  }
</style>
