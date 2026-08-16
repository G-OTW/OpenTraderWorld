<script>
  // Picker shown in edit mode to add a tile to a row: a live widget or a module link.
  // Rows are mixed — either kind can sit in any row. picking calls onpickWidget(type) or
  // onpickModule(moduleId) and closes.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { availableWidgets } from './registry.js';
  import { visibleModules } from '$lib/modules/registry';
  import { installedIds } from '$lib/modules/installed.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    onpickWidget = () => {},
    onpickModule = () => {}
  } = $props();

  const widgets = $derived(availableWidgets($installedIds));
  const mods = $derived(visibleModules($installedIds).filter((m) => !m.home));

  function pickWidget(type) {
    onpickWidget(type);
    open = false;
  }
  function pickModule(id) {
    onpickModule(id);
    open = false;
  }
</script>

<Modal bind:open size="md" title={$t('dashboard.widgets.picker.title')}>
  <h3 class="sec">{$t('dashboard.widgets.picker.widgets')}</h3>
  <div class="grid">
    {#each widgets as w (w.type)}
      <button class="card" onclick={() => pickWidget(w.type)}>
        <span class="ci"><Icon name={w.icon} size={18} /></span>
        <span class="cn">{w.label}</span>
        <span class="cb">{w.blurb}</span>
      </button>
    {/each}
  </div>

  <h3 class="sec">{$t('dashboard.widgets.picker.moduleLinks')}</h3>
  <div class="grid links">
    {#each mods as m (m.id)}
      <button class="card link" onclick={() => pickModule(m.id)}>
        <span class="ci"><Icon name={m.icon} size={18} /></span>
        <span class="cn">{m.name}</span>
      </button>
    {/each}
  </div>
</Modal>

<style>
  .sec {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    margin: var(--space-4) 0 var(--space-2);
  }
  .sec:first-child {
    margin-top: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-3);
  }
  .grid.links {
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  }
  .card.link {
    grid-template-rows: auto;
    align-items: center;
  }
  .card.link .ci {
    grid-row: 1;
  }
  .card {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: auto auto;
    gap: 2px var(--space-3);
    text-align: left;
    padding: var(--space-3);
    background: var(--bg);
    border: 0.5px solid var(--border);
    border-radius: 0;
    color: var(--text);
    cursor: pointer;
    font-family: inherit;
  }
  .card:hover {
    background: var(--surface-2);
  }
  .ci {
    grid-row: 1 / 3;
    display: inline-flex;
    align-items: center;
    color: var(--muted);
  }
  .cn {
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
  }
  .cb {
    font-size: var(--text-xs);
    color: var(--dim);
    line-height: 1.35;
  }
</style>
