<script>
  // Show/hide + reorder editor for a short list (page sections, company tabs).
  // props: items ([{ id, label }]) in current order, hidden (ids), onchange(order, hidden)
  import Icon from '$lib/ui/Icon.svelte';
  import { move } from './prefs.svelte.js';
  import { t } from '$lib/i18n';

  let { items = [], hidden = [], onchange } = $props();

  const order = $derived(items.map((i) => i.id));

  function toggle(id) {
    onchange?.(order, hidden.includes(id) ? hidden.filter((h) => h !== id) : [...hidden, id]);
  }
</script>

<ul>
  {#each items as it, i (it.id)}
    <li class:off={hidden.includes(it.id)}>
      <label>
        <input type="checkbox" checked={!hidden.includes(it.id)} onchange={() => toggle(it.id)} />
        {it.label}
      </label>
      <span class="arrows">
        <button class="icon" disabled={i === 0} onclick={() => onchange?.(move(order, it.id, -1), hidden)} aria-label={$t('fundamentals.cust.up')}>
          <Icon name="arrow-up" size={13} />
        </button>
        <button class="icon" disabled={i === items.length - 1} onclick={() => onchange?.(move(order, it.id, 1), hidden)} aria-label={$t('fundamentals.cust.down')}>
          <Icon name="arrow-down" size={13} />
        </button>
      </span>
    </li>
  {/each}
</ul>

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
  }
  li:last-child {
    border-bottom: 0;
  }
  li.off label {
    color: var(--dim);
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .arrows {
    display: flex;
    gap: var(--space-1);
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .icon:disabled {
    opacity: 0.3;
    cursor: default;
  }
</style>
