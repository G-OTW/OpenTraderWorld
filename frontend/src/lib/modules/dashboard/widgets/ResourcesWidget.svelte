<script>
  // Resources widget: a scrollable list of bookmarks from a chosen category (or all).
  // Config: { category_id }. Each entry links out to its URL.
  import { resourcesApi, linkHost } from '$lib/modules/resources/api.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import RankBars from './parts/RankBars.svelte';
  import Donut from './parts/Donut.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'bookmarks');
  const catId = $derived(item.config?.category_id || null);
  const order = $derived(item.config?.order === 'recent' ? 'recent' : 'saved');
  const limit = $derived(Math.max(1, Math.min(50, item.config?.limit ?? 10)));

  let all = $state(null);
  let categories = $state([]);
  let err = $state('');

  async function load() {
    err = '';
    try {
      [all, categories] = await Promise.all([resourcesApi.list(), resourcesApi.listCategories()]);
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const catName = (id) => categories.find((c) => c.id === id)?.name ?? $t('dashboard.widgets.resources.uncategorized');

  const perCategory = $derived.by(() => {
    const counts = new Map();
    for (const r of all ?? []) counts.set(r.category_id, (counts.get(r.category_id) ?? 0) + 1);
    return [...counts]
      .map(([id, value]) => ({ label: catName(id), value, display: String(value) }))
      .sort((a, b) => b.value - a.value)
      .slice(0, limit);
  });

  // "Needs enrichment" is the module's own definition of an unfinished entry: no
  // description, or no thumbnail to show in the gallery.
  const needsWork = $derived(
    (all ?? []).filter((r) => !r.description?.trim() || !r.thumb_url?.trim()).slice(0, limit)
  );

  const domains = $derived.by(() => {
    const counts = new Map();
    for (const r of all ?? []) {
      const host = linkHost(r.link);
      if (!host) continue;
      counts.set(host, (counts.get(host) ?? 0) + 1);
    }
    const rows = [...counts].map(([label, value]) => ({ label, value })).sort((a, b) => b.value - a.value);
    const top = rows.slice(0, 5);
    const rest = rows.slice(5).reduce((sum, r) => sum + r.value, 0);
    if (rest > 0) top.push({ label: $t('dashboard.widgets.resources.others'), value: rest });
    return top;
  });

  const shown = $derived.by(() => {
    const rows = (all ?? []).filter((r) => !catId || r.category_id === catId);
    if (order === 'recent') {
      rows.sort((a, b) => String(b.created_at ?? b.updated_at ?? '').localeCompare(String(a.created_at ?? a.updated_at ?? '')));
    }
    return rows.slice(0, limit);
  });
</script>

<WidgetState
  {editing}
  error={err}
  loading={all === null}
  empty={['bookmarks', 'recent'].includes(variant) && shown.length === 0}
  preview={$t('dashboard.widgets.resources.preview')}
  emptyText={$t('dashboard.widgets.resources.empty')}
  rows={4}
>
  {#if variant === 'counts'}
    <div class="counts">
      <div class="cell">
        <span class="fig">{(all ?? []).length}</span>
        <span class="w-sub">{$t('dashboard.widgets.resources.totalResources')}</span>
      </div>
      <div class="cell">
        <span class="fig">{categories.length}</span>
        <span class="w-sub">{$t('dashboard.widgets.resources.categories')}</span>
      </div>
    </div>
  {:else if variant === 'perCategory'}
    <RankBars rows={perCategory} />
  {:else if variant === 'enrich'}
    {#if needsWork.length === 0}
      <p class="w-state">{$t('dashboard.widgets.resources.allEnriched')}</p>
    {:else}
      <div class="w-list">
        {#each needsWork as r (r.id)}
          <a class="w-row" href={r.link} target="_blank" rel="noreferrer noopener">
            <Icon name="pencil" size={14} />
            <span class="w-name name">{r.name || $t('dashboard.widgets.resources.untitled')}</span>
            <span class="w-sub host">
              {!r.description?.trim() ? $t('dashboard.widgets.resources.noDescription') : $t('dashboard.widgets.resources.noThumb')}
            </span>
          </a>
        {/each}
      </div>
    {/if}
  {:else if variant === 'domains'}
    <Donut segments={domains} centerLabel={$t('dashboard.widgets.resources.resources')} />
  {:else}
  <div class="w-list">
    {#each shown as r (r.id)}
      <a class="w-row" href={r.link} target="_blank" rel="noreferrer noopener">
        <span class="w-name name">{r.name}</span>
        {#if linkHost(r.link)}<span class="w-sub host">{linkHost(r.link)}</span>{/if}
      </a>
    {/each}
  </div>
  {/if}
</WidgetState>

<style>
  .counts {
    display: flex;
    gap: var(--space-6);
  }
  .cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 26px;
    font-weight: var(--fw-medium);
    letter-spacing: -0.015em;
    color: var(--text);
  }
  .w-row :global(svg) {
    color: var(--amber);
    flex-shrink: 0;
  }
  .name {
    flex: 1;
    font-weight: var(--fw-medium);
  }
  a:hover .name {
    color: var(--accent);
  }
  /* The host stays on the same line as the name: a bookmark is one fact, and the
     domain is how you tell two similarly named ones apart. */
  .host {
    flex-shrink: 0;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
