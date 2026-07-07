<script>
  // Config editor for a single widget. Switches on widget type to show the right fields.
  // Edits a local draft of `item.config` and commits it via `onsave(config)` so the parent
  // controls persistence. Every widget shares an optional custom `title`.
  import Modal from '$lib/ui/Modal.svelte';
  import { widgetByType } from './registry.js';
  import { newsApi } from '$lib/modules/news/api.js';
  import { resourcesApi } from '$lib/modules/resources/api.js';
  import { portfoliosApi } from '$lib/modules/portfolios/api.js';
  import { timeApi } from '$lib/modules/time/api.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    item = null, // { type, config }
    onsave = () => {}
  } = $props();

  const def = $derived(item ? widgetByType(item.type) : null);
  let cfg = $state({});

  // Option lists loaded on demand per widget type.
  let feeds = $state(null);
  let categories = $state(null);
  let portfolios = $state(null);
  let projects = $state(null);
  let loadErr = $state('');

  $effect(() => {
    if (!open || !item) return;
    cfg = { ...item.config };
    loadErr = '';
    loadOptions(item.type);
  });

  async function loadOptions(type) {
    try {
      if (type === 'news' && feeds === null) feeds = await newsApi.listFeeds();
      if (type === 'resources' && categories === null) categories = await resourcesApi.listCategories();
      if (type === 'portfolios' && portfolios === null) portfolios = await portfoliosApi.list();
      if (type === 'time' && projects === null) projects = await timeApi.listProjects();
    } catch (e) {
      loadErr = e.message;
    }
  }

  function set(key, val) {
    cfg = { ...cfg, [key]: val };
  }

  function submit() {
    onsave({ ...cfg });
    open = false;
  }
</script>

<Modal bind:open size="sm" title={$t('dashboard.widgets.config.title', { label: def?.label ?? $t('dashboard.widgets.config.widget') })}>
  <div class="form">
    {#if loadErr}<p class="err">{loadErr}</p>{/if}

    <label>
      <span>{$t('dashboard.widgets.config.titleField')} <small>{$t('dashboard.widgets.config.optional')}</small></span>
      <input value={cfg.title ?? ''} oninput={(e) => set('title', e.currentTarget.value)}
        placeholder={def?.label} />
    </label>

    <label>
      <span>{$t('dashboard.widgets.config.height')}</span>
      <select value={cfg.height ?? 'standard'} onchange={(e) => set('height', e.currentTarget.value)}>
        <option value="compact">{$t('dashboard.widgets.config.compact')}</option>
        <option value="standard">{$t('dashboard.widgets.config.standard')}</option>
        <option value="tall">{$t('dashboard.widgets.config.tall')}</option>
      </select>
    </label>

    {#if item?.type === 'text'}
      <label>
        <span>{$t('dashboard.widgets.config.text')}</span>
        <textarea rows="6" value={cfg.body ?? ''} oninput={(e) => set('body', e.currentTarget.value)}
          placeholder={$t('dashboard.widgets.config.textPlaceholder')}></textarea>
      </label>

    {:else if item?.type === 'news'}
      <label>
        <span>{$t('dashboard.widgets.config.feed')}</span>
        {#if feeds === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <select value={cfg.feed_id ?? ''} onchange={(e) => set('feed_id', e.currentTarget.value || null)}>
            <option value="">{$t('dashboard.widgets.config.allFeeds')}</option>
            {#each feeds as f}<option value={f.id}>{f.name}</option>{/each}
          </select>
        {/if}
      </label>
      <label>
        <span>{$t('dashboard.widgets.config.layout')}</span>
        <select value={cfg.view ?? 'list'} onchange={(e) => set('view', e.currentTarget.value)}>
          <option value="list">{$t('dashboard.widgets.config.list')}</option>
          <option value="grid">{$t('dashboard.widgets.config.grid')}</option>
        </select>
      </label>

    {:else if item?.type === 'resources'}
      <label>
        <span>{$t('dashboard.widgets.config.category')}</span>
        {#if categories === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <select value={cfg.category_id ?? ''} onchange={(e) => set('category_id', e.currentTarget.value || null)}>
            <option value="">{$t('dashboard.widgets.config.allCategories')}</option>
            {#each categories as c}<option value={c.id}>{c.name}</option>{/each}
          </select>
        {/if}
      </label>

    {:else if item?.type === 'portfolios'}
      <label>
        <span>{$t('dashboard.widgets.config.portfolio')}</span>
        {#if portfolios === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <select value={cfg.portfolio_id ?? ''} onchange={(e) => set('portfolio_id', e.currentTarget.value || null)}>
            <option value="">{$t('dashboard.widgets.config.firstPortfolio')}</option>
            {#each portfolios as p}<option value={p.id}>{p.name}</option>{/each}
          </select>
        {/if}
      </label>

    {:else if item?.type === 'time'}
      <label>
        <span>{$t('dashboard.widgets.config.project')}</span>
        {#if projects === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <select value={cfg.project_id ?? ''} onchange={(e) => set('project_id', e.currentTarget.value || null)}>
            <option value="">{$t('dashboard.widgets.config.allProjects')}</option>
            {#each projects as p}<option value={p.id}>{p.name}</option>{/each}
          </select>
        {/if}
      </label>

    {:else if item?.type === 'goals' || item?.type === 'todos' || item?.type === 'calendar'}
      <label>
        <span>{$t('dashboard.widgets.config.maxItems')}</span>
        <input type="number" min="1" max="50" value={cfg.limit ?? 8}
          oninput={(e) => set('limit', Math.max(1, Math.min(50, +e.currentTarget.value || 8)))} />
      </label>

    {:else}
      <p class="muted">{$t('dashboard.widgets.config.noOptions')}</p>
    {/if}
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={submit}>{$t('common.save')}</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: 0.85rem;
    color: var(--muted);
  }
  small {
    font-weight: 400;
  }
  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .err {
    color: var(--red);
    font-size: 0.8rem;
  }
</style>
