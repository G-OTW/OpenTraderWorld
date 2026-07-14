<script>
  // Resources widget: a scrollable list of bookmarks from a chosen category (or all).
  // Config: { category_id }. Each entry links out to its URL.
  import { resourcesApi, linkHost } from '$lib/modules/resources/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();
  const catId = $derived(item.config?.category_id || null);

  let all = $state(null);
  let err = $state('');

  async function load() {
    err = '';
    try {
      all = await resourcesApi.list();
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const shown = $derived((all ?? []).filter((r) => !catId || r.category_id === catId));
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.resources.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if all === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else if shown.length === 0}
  <p class="hint">{$t('dashboard.widgets.resources.empty')}</p>
{:else}
  <ul class="list">
    {#each shown as r (r.id)}
      <li>
        <a href={r.link} target="_blank" rel="noreferrer noopener">
          <span class="n">{r.name}</span>
          {#if linkHost(r.link)}<span class="h">{linkHost(r.link)}</span>{/if}
        </a>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .sk {
    padding: var(--space-1) 0;
  }
  /* Preview, loading and empty text — not an error. This was grouped with a
     now-removed .err rule and inherited its red. */
  .hint {
    color: var(--muted);
  }
  .list {
    display: flex;
    flex-direction: column;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li a {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: var(--space-2) 0;
    text-decoration: none;
    color: var(--text);
    border-bottom: 1px solid var(--border);
  }
  li:last-child a {
    border-bottom: none;
  }
  .n {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  li a:hover .n {
    color: var(--accent);
  }
  .h {
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
