<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';

  // Single-choice dropdown drawn in HTML, so it matches the multi-select source
  // picker next to it instead of getting the OS-native <select> popup.
  // props: value (bindable), options ([{value,label}]), title, onchange
  let { value = $bindable(''), options = [], title = '', onchange = () => {} } = $props();

  let open = $state(false);
  const current = $derived(options.find((o) => o.value === value) || options[0] || null);

  function pick(v) {
    open = false;
    if (v === value) return;
    value = v;
    onchange(v);
  }
</script>

<!-- Closes on click-away (action) and on tab-out (focusout). -->
<div
  class="pick"
  use:clickOutside={() => (open = false)}
  onfocusout={(e) => {
    if (!e.currentTarget.contains(e.relatedTarget)) open = false;
  }}
>
  <button class="pick-toggle" {title} onclick={() => (open = !open)}>
    <span class="pick-label">{current?.label ?? ''}</span>
    <span class="caret"><Icon name="chevron-down" size={12} /></span>
  </button>
  {#if open}
    <div class="pick-menu">
      {#each options as o (o.value)}
        <button class="pick-opt" class:active={o.value === value} onclick={() => pick(o.value)}>
          {o.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .pick {
    position: relative;
  }
  .pick-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 6px 8px;
    max-width: 180px;
    white-space: nowrap;
  }
  .pick-toggle:hover {
    background: var(--surface-2);
    border-color: var(--border-control);
  }
  .pick-label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .caret {
    color: var(--muted);
    margin-left: auto;
    display: inline-flex;
  }
  .pick-menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: var(--z-dropdown);
    margin-top: 4px;
    min-width: 100%;
    max-height: 50vh;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    padding: 4px;
  }
  .pick-opt {
    display: block;
    width: 100%;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 5px 6px;
    text-align: left;
    white-space: nowrap;
  }
  .pick-opt:hover {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--text);
  }
  .pick-opt.active {
    color: var(--accent);
  }
</style>
