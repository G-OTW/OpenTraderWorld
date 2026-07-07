<script>
  // Resources widget: a scrollable list of bookmarks from a chosen category (or all).
  // Config: { category_id }. Each entry links out to its URL.
  import { resourcesApi, linkHost } from '$lib/modules/resources/api.js';
  import { t } from '$lib/i18n';

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
  <p class="err">{err}</p>
{:else if all === null}
  <p class="hint">{$t('common.loading')}</p>
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
  .hint,
  .err {
    font-size: 0.82rem;
    color: var(--muted);
  }
  .err {
    color: var(--red);
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
    font-size: 0.85rem;
    font-weight: 500;
  }
  li a:hover .n {
    color: var(--accent);
  }
  .h {
    font-size: 0.72rem;
    color: var(--muted);
  }
</style>
