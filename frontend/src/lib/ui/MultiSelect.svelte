<script>
  import Icon from './Icon.svelte';
  import { clickOutside } from './clickOutside.js';
  import { t } from '$lib/i18n';

  // Checkbox dropdown: pick none, one or many values. Drawn in HTML so it matches the
  // other project dropdowns instead of getting the OS-native <select> popup.
  // Empty selection = "all" — the caller reads that as "no filter".
  // props: value (bindable string[]), options ([{value,label}] or string[]),
  //        allLabel (shown when nothing is picked), onchange
  let {
    value = $bindable([]),
    options = [],
    allLabel = '',
    width = '160px',
    flat = false, // square + hairline border, to sit in a row of flat table controls
    disabled = false,
    onchange = () => {}
  } = $props();

  let open = $state(false);
  let root = $state(null);

  // Accept both `['Cash','Stocks']` and `[{value,label}]`.
  const items = $derived(
    options.map((o) => (typeof o === 'object' && o !== null ? o : { value: o, label: String(o) }))
  );

  // One picked → show it; several → count. Keeps the button from growing.
  const summary = $derived.by(() => {
    if (value.length === 0) return allLabel;
    if (value.length === 1) return items.find((o) => o.value === value[0])?.label ?? value[0];
    return $t('common.nSelected', { count: value.length });
  });

  function toggle(v) {
    value = value.includes(v) ? value.filter((x) => x !== v) : [...value, v];
    onchange(value);
  }
  function clear() {
    value = [];
    onchange(value);
  }
</script>

<!-- Escape closes and hands focus back to the toggle, so keyboard users don't land at the
     top of the page. -->
<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape' && open) {
      open = false;
      root?.querySelector('.ms-toggle')?.focus();
    }
  }}
/>

<!-- Closes on click-away (action), on tab-out (focusout) and on Escape.
     focusout only closes on a real move to another focusable element: clicking the menu's
     own padding blurs to nothing (relatedTarget null), which must NOT close a multi-select
     the user is still ticking through. Outside clicks are the action's job. -->
<div
  class="ms"
  style:width
  bind:this={root}
  use:clickOutside={() => (open = false)}
  onfocusout={(e) => {
    if (e.relatedTarget && !e.currentTarget.contains(e.relatedTarget)) open = false;
  }}
>
  <button
    class="ms-toggle"
    class:flat
    class:on={value.length > 0}
    {disabled}
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <span class="ms-label">{summary}</span>
    <span class="caret"><Icon name="chevron-down" size={12} /></span>
  </button>
  {#if open}
    <div class="ms-menu">
      {#if value.length}
        <button class="ms-clear" onclick={clear}>{$t('common.clearSelection')}</button>
      {/if}
      {#each items as o (o.value)}
        <label class="ms-opt">
          <input type="checkbox" checked={value.includes(o.value)} onchange={() => toggle(o.value)} />
          <span>{o.label}</span>
        </label>
      {/each}
      {#if items.length === 0}
        <p class="ms-empty">{$t('common.noOptions')}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* The wrapper stretches with the row; the toggle has to fill it, otherwise it sizes to
     its own padding and sits shorter than the inputs and buttons beside it. */
  .ms {
    position: relative;
    display: flex;
  }
  .ms-toggle {
    display: flex;
    align-items: center;
    height: 100%;
    gap: 6px;
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 6px 8px;
    white-space: nowrap;
  }
  .ms-toggle:hover:not(:disabled) {
    background: var(--surface-2);
    border-color: var(--border-control);
  }
  .ms-toggle:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  /* Match a row of flat controls (hairline border, square corners) exactly, so the
     toggles line up with the buttons and inputs beside them. */
  .ms-toggle.flat {
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: 6px 10px;
  }
  /* An active filter reads at a glance without opening the menu. Listed after .flat so it
     keeps the accent border in both styles. */
  .ms-toggle.on,
  .ms-toggle.flat.on {
    border-color: var(--accent);
    color: var(--accent);
  }
  .ms-label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .caret {
    color: var(--muted);
    margin-left: auto;
    display: inline-flex;
  }
  .ms-menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: var(--z-dropdown);
    margin-top: 4px;
    min-width: 100%;
    width: max-content;
    max-width: 280px;
    max-height: 50vh;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    padding: 4px;
  }
  .ms-opt {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px;
    border-radius: var(--radius);
    cursor: pointer;
    color: var(--text);
    font-size: var(--text-sm);
    white-space: nowrap;
  }
  .ms-opt:hover {
    background: var(--surface-2);
  }
  .ms-opt input {
    width: auto;
    margin: 0;
  }
  .ms-clear {
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 5px 6px;
    margin-bottom: 4px;
  }
  .ms-clear:hover {
    color: var(--text);
  }
  .ms-empty {
    color: var(--muted);
    font-size: var(--text-sm);
    margin: 0;
    padding: 6px;
  }
</style>
