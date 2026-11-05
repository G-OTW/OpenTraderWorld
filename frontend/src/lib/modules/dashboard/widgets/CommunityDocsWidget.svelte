<script>
  // Community Docs dashboard presentations. They deliberately reuse the offline library
  // and favorites endpoints; the widget does not create a parallel notion of saved links.
  import { communityDocsApi, groupByCategory } from '$lib/modules/community-docs/api.js';
  import { relativeTime } from '$lib/format';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Donut from './parts/Donut.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'library');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 6)));

  let docs = $state(null);
  let favorites = $state(null);
  let err = $state('');

  $effect(() => {
    if (editing) return;
    let alive = true;
    docs = null;
    favorites = null;
    err = '';
    Promise.all([communityDocsApi.list(), communityDocsApi.favorites()])
      .then(([all, saved]) => {
        if (!alive) return;
        docs = Array.isArray(all) ? all : [];
        favorites = Array.isArray(saved) ? saved : [];
      })
      .catch((e) => {
        if (alive) err = e.message;
      });
    return () => (alive = false);
  });

  const categories = $derived.by(() =>
    groupByCategory(docs ?? [])
      .map((group) => ({ label: group.category, value: group.docs.length, docs: group.docs }))
      .sort((a, b) => b.value - a.value)
  );
  const recent = $derived(
    [...(docs ?? [])]
      .sort((a, b) => String(b.synced_at ?? '').localeCompare(String(a.synced_at ?? '')))
      .slice(0, limit)
  );
  const quick = $derived((favorites ?? []).slice(0, limit));
</script>

<WidgetState
  {editing}
  error={err}
  loading={docs === null || favorites === null}
  empty={(docs ?? []).length === 0}
  preview="Community Docs preview"
  emptyText="No community docs saved yet."
  rows={4}
>
  {#if variant === 'library'}
    <div class="w-body">
      <div class="summary">
        <span class="figure">{docs.length}</span>
        <span class="w-eyebrow">community docs</span>
      </div>
      <ul class="w-list">
        <li class="w-row">
          <span class="w-name grow">Saved references</span>
          <span class="w-num">{favorites.length}</span>
        </li>
        <li class="w-row">
          <span class="w-name grow">Categories</span>
          <span class="w-num">{categories.length}</span>
        </li>
      </ul>
    </div>
  {:else if variant === 'category'}
    <ul class="w-list">
      {#each categories.slice(0, limit) as group (group.label)}
        <li>
          <a class="w-row" href="/community-docs">
            <Icon name="folder" size={15} />
            <span class="w-name grow">{group.label}</span>
            <span class="w-num">{group.value}</span>
            <Icon name="chevron-right" size={13} />
          </a>
        </li>
      {/each}
    </ul>
  {:else if variant === 'mix'}
    <Donut segments={categories} center={String(docs.length)} centerLabel="references" />
  {:else}
    {@const shown = variant === 'quick' ? quick : recent}
    {#if shown.length === 0}
      <p class="w-state">{variant === 'quick' ? 'No quick-access references yet.' : 'No recent references yet.'}</p>
    {:else}
      <ul class="w-list">
        {#each shown as doc (doc.slug)}
          {@const when = relativeTime(doc.synced_at)}
          <li>
            <a class="w-row stack" href="/community-docs">
              <span class="line">
                <span class="w-name grow">{doc.title}</span>
                {#if doc.favorited || variant === 'quick'}<Icon name="star" size={13} />{/if}
              </span>
              <span class="w-sub">{doc.categories?.[0] || 'Uncategorized'} · {$t(when.key, when.params)}</span>
            </a>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
    min-width: 0;
  }
  .summary {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .figure {
    color: var(--text);
    font-family: var(--mono);
    font-size: 28px;
    font-weight: var(--fw-medium);
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    width: 100%;
  }
  .w-row :global(svg) {
    color: var(--faint);
    flex-shrink: 0;
  }
  .w-row:hover .w-name {
    color: var(--accent);
  }
</style>
