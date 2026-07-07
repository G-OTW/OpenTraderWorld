<script>
  import Icon from './Icon.svelte';
  import { t } from '$lib/i18n';

  // Searchable single-select dropdown: type to filter the options by substring, then pick
  // with click or Enter. Empty value = "any". Closes on outside-click / Escape / blur.
  let {
    value = $bindable(''),
    options = [],
    label = '',
    placeholder = 'any',
    onchange = () => {}
  } = $props();

  let open = $state(false);
  let query = $state('');
  let active = $state(0); // highlighted index into `filtered`
  let root = $state(null);
  let input = $state(null);

  // The "clear" entry plus matching options. Substring, case-insensitive.
  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const opts = q ? options.filter((o) => o.toLowerCase().includes(q)) : options;
    return [{ value: '', label: `${label || 'Filter'}: any` }, ...opts.map((o) => ({ value: o, label: o }))];
  });

  function openMenu() {
    open = true;
    query = '';
    active = 0;
    queueMicrotask(() => input?.focus());
  }
  function pick(v) {
    value = v;
    onchange(v);
    open = false;
  }
  function onKey(e) {
    if (!open) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(active + 1, filtered.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (filtered[active]) pick(filtered[active].value);
    } else if (e.key === 'Escape') {
      open = false;
    }
  }
  // Reset highlight to the top whenever the filtered list changes.
  $effect(() => {
    query;
    active = 0;
  });

  function onDocClick(e) {
    if (root && !root.contains(e.target)) open = false;
  }
</script>

<svelte:document onclick={onDocClick} />

<div class="combo" class:set={value} bind:this={root} onkeydown={onKey} role="presentation">
  <button type="button" class="trigger" onclick={() => (open ? (open = false) : openMenu())}>
    <span class="trig-label">{value || `${label}: any`}</span>
    <span class="caret"><Icon name="chevron-down" size={13} /></span>
  </button>
  {#if open}
    <div class="menu">
      <input
        class="combo-search"
        bind:this={input}
        bind:value={query}
        {placeholder}
        aria-label={`Search ${label}`}
      />
      <ul class="opts">
        {#each filtered as o, i (o.value)}
          <li>
            <button
              type="button"
              class="opt"
              class:active={i === active}
              class:sel={o.value === value}
              onmouseenter={() => (active = i)}
              onclick={() => pick(o.value)}
            >
              {o.label}
            </button>
          </li>
        {/each}
        {#if filtered.length === 1}
          <li class="none">{$t('common.noMatches')}</li>
        {/if}
      </ul>
    </div>
  {/if}
</div>

<style>
  .combo {
    position: relative;
    display: inline-block;
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    height: var(--control-h);
    padding: 0 var(--space-3);
    font: inherit;
    font-size: var(--control-fs);
    cursor: pointer;
    max-width: 200px;
    transition: border-color 0.12s ease, box-shadow 0.12s ease;
  }
  .trigger:hover {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
  }
  .combo.set .trigger {
    border-color: var(--accent);
    color: var(--accent);
  }
  .trig-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .caret {
    color: var(--muted);
    display: inline-flex;
  }
  .menu {
    position: absolute;
    z-index: 20;
    top: calc(100% + 4px);
    left: 0;
    min-width: 220px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
    padding: var(--space-2);
  }
  .combo-search {
    width: 100%;
    margin-bottom: var(--space-2);
  }
  .opts {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 240px;
    overflow-y: auto;
  }
  .opt {
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--text);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    cursor: pointer;
    font: inherit;
    font-size: 0.85rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .opt.active {
    background: var(--surface-2);
  }
  .opt.sel {
    color: var(--accent);
  }
  .none {
    color: var(--muted);
    font-size: 0.85rem;
    padding: var(--space-2) var(--space-3);
  }
</style>
