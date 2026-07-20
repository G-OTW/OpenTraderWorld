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
  import { watchlistsApi } from '$lib/modules/watchlists/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

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
  let watchlists = $state(null);
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
      if (type === 'watchlists' && watchlists === null) watchlists = await watchlistsApi.list();
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
    <ErrorText error={loadErr} compact />

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

    {:else if item?.type === 'watchlists'}
      <label>
        <span>{$t('dashboard.widgets.config.watchlist')}</span>
        {#if watchlists === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <select value={cfg.watchlist_id ?? ''} onchange={(e) => set('watchlist_id', e.currentTarget.value || null)}>
            <option value="">{$t('dashboard.widgets.config.firstWatchlist')}</option>
            {#each watchlists as w}<option value={w.id}>{w.name}</option>{/each}
          </select>
        {/if}
      </label>

    {:else if item?.type === 'economics'}
      <label>
        <span>{$t('dashboard.widgets.config.importance')}</span>
        <select value={cfg.importance ?? '-1,0,1'} onchange={(e) => set('importance', e.currentTarget.value)}>
          <option value="-1,0,1">{$t('dashboard.widgets.config.importanceAll')}</option>
          <option value="0,1">{$t('dashboard.widgets.config.importanceMedHigh')}</option>
          <option value="1">{$t('dashboard.widgets.config.importanceHigh')}</option>
        </select>
      </label>
      <label>
        <span>{$t('dashboard.widgets.config.countries')} <small>{$t('dashboard.widgets.config.optional')}</small></span>
        <input value={cfg.countries ?? ''} oninput={(e) => set('countries', e.currentTarget.value)}
          placeholder={$t('dashboard.widgets.config.countriesPlaceholder')} />
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
    font-size: var(--text-base);
    color: var(--dim);
  }
  small {
    font-weight: var(--fw-normal);
  }
  .muted {
    color: var(--dim);
    font-size: var(--text-base);
  }
</style>
