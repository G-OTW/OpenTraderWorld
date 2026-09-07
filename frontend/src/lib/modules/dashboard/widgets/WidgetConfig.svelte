<script>
  // Config editor for a single widget. Switches on widget type to show the right fields.
  // Edits a local draft of `item.config` and commits it via `onsave(config)` so the parent
  // controls persistence. Every widget shares an optional custom `title`.
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
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

  // Dropdown works on strings; '' is the "none / all" row and stores back as null.
  const idOpts = (list, allLabel) => [
    { value: '', label: allLabel },
    ...(list ?? []).map((x) => ({ value: String(x.id), label: x.name }))
  ];
  const setId = (key) => (v) => set(key, v || null);

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

    <div class="fld">
      <span>{$t('dashboard.widgets.config.height')}</span>
      <Dropdown
        value={cfg.height ?? 'standard'}
        onpick={(v) => set('height', v)}
        ariaLabel={$t('dashboard.widgets.config.height')}
        options={[
          { value: 'title', label: $t('dashboard.widgets.config.titleHeight') },
          { value: 'compact', label: $t('dashboard.widgets.config.compact') },
          { value: 'standard', label: $t('dashboard.widgets.config.standard') },
          { value: 'tall', label: $t('dashboard.widgets.config.tall') }
        ]}
      />
    </div>

    {#if item?.type === 'text'}
      <label>
        <span>{$t('dashboard.widgets.config.text')}</span>
        <textarea rows="6" value={cfg.body ?? ''} oninput={(e) => set('body', e.currentTarget.value)}
          placeholder={$t('dashboard.widgets.config.textPlaceholder')}></textarea>
      </label>

    {:else if item?.type === 'news'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.feed')}</span>
        {#if feeds === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.feed_id == null ? '' : String(cfg.feed_id)}
            onpick={setId('feed_id')}
            ariaLabel={$t('dashboard.widgets.config.feed')}
            options={idOpts(feeds, $t('dashboard.widgets.config.allFeeds'))}
          />
        {/if}
      </div>
      <div class="fld">
        <span>{$t('dashboard.widgets.config.layout')}</span>
        <Dropdown
          value={cfg.view ?? 'list'}
          onpick={(v) => set('view', v)}
          ariaLabel={$t('dashboard.widgets.config.layout')}
          options={[
            { value: 'list', label: $t('dashboard.widgets.config.list') },
            { value: 'grid', label: $t('dashboard.widgets.config.grid') }
          ]}
        />
      </div>

    {:else if item?.type === 'resources'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.category')}</span>
        {#if categories === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.category_id == null ? '' : String(cfg.category_id)}
            onpick={setId('category_id')}
            ariaLabel={$t('dashboard.widgets.config.category')}
            options={idOpts(categories, $t('dashboard.widgets.config.allCategories'))}
          />
        {/if}
      </div>

    {:else if item?.type === 'portfolios'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.portfolio')}</span>
        {#if portfolios === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.portfolio_id == null ? '' : String(cfg.portfolio_id)}
            onpick={setId('portfolio_id')}
            ariaLabel={$t('dashboard.widgets.config.portfolio')}
            options={idOpts(portfolios, $t('dashboard.widgets.config.firstPortfolio'))}
          />
        {/if}
      </div>

    {:else if item?.type === 'time'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.project')}</span>
        {#if projects === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.project_id == null ? '' : String(cfg.project_id)}
            onpick={setId('project_id')}
            ariaLabel={$t('dashboard.widgets.config.project')}
            options={idOpts(projects, $t('dashboard.widgets.config.allProjects'))}
          />
        {/if}
      </div>

    {:else if item?.type === 'watchlists'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.watchlist')}</span>
        {#if watchlists === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.watchlist_id == null ? '' : String(cfg.watchlist_id)}
            onpick={setId('watchlist_id')}
            ariaLabel={$t('dashboard.widgets.config.watchlist')}
            options={idOpts(watchlists, $t('dashboard.widgets.config.firstWatchlist'))}
          />
        {/if}
      </div>

    {:else if item?.type === 'economics'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.importance')}</span>
        <Dropdown
          value={cfg.importance ?? '-1,0,1'}
          onpick={(v) => set('importance', v)}
          ariaLabel={$t('dashboard.widgets.config.importance')}
          options={[
            { value: '-1,0,1', label: $t('dashboard.widgets.config.importanceAll') },
            { value: '0,1', label: $t('dashboard.widgets.config.importanceMedHigh') },
            { value: '1', label: $t('dashboard.widgets.config.importanceHigh') }
          ]}
        />
      </div>
      <label>
        <span>{$t('dashboard.widgets.config.countries')} <small>{$t('dashboard.widgets.config.optional')}</small></span>
        <input value={cfg.countries ?? ''} oninput={(e) => set('countries', e.currentTarget.value)}
          placeholder={$t('dashboard.widgets.config.countriesPlaceholder')} />
      </label>

    {:else if item?.type === 'wealth'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.window')}</span>
        <Dropdown
          value={String(cfg.months ?? 12)}
          onpick={(v) => set('months', +v)}
          ariaLabel={$t('dashboard.widgets.config.window')}
          options={[
            { value: '6', label: $t('dashboard.widgets.config.months', { count: 6 }) },
            { value: '12', label: $t('dashboard.widgets.config.months', { count: 12 }) },
            { value: '24', label: $t('dashboard.widgets.config.months', { count: 24 }) }
          ]}
        />
      </div>

    {:else if item?.type === 'subscriptions'}
      <label>
        <span>{$t('dashboard.widgets.config.maxRenewals')}</span>
        <input type="number" min="1" max="20" value={cfg.limit ?? 5}
          oninput={(e) => set('limit', Math.max(1, Math.min(20, +e.currentTarget.value || 5)))} />
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
  label,
  .fld {
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
