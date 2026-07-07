<script>
  // News feed widget: shows the latest items from a chosen feed (or all feeds) as a
  // scrollable list or grid. Items link out to their article. Config: { feed_id, view }.
  import { newsApi } from '$lib/modules/news/api.js';
  import { t } from '$lib/i18n';

  // Compact "Jul 3, 14:20"-style stamp (null-safe).
  function fmtDate(s) {
    if (!s) return '';
    const d = new Date(s);
    if (isNaN(d)) return '';
    return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  let { item, editing } = $props();
  const view = $derived(item.config?.view === 'grid' ? 'grid' : 'list');

  let items = $state(null);
  let err = $state('');

  $effect(() => {
    const feedId = item.config?.feed_id || '';
    if (editing) return; // don't fetch behind the config affordance
    let alive = true;
    items = null;
    err = '';
    newsApi
      .listItems({ feed_id: feedId, limit: 10 })
      .then((r) => alive && (items = r))
      .catch((e) => alive && (err = e.message));
    return () => (alive = false);
  });
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.news.preview')}</p>
{:else if err}
  <p class="err">{err}</p>
{:else if items === null}
  <p class="hint">{$t('common.loading')}</p>
{:else if items.length === 0}
  <p class="hint">{$t('dashboard.widgets.news.empty')}</p>
{:else}
  <div class={view}>
    {#each items as it (it.id)}
      <a class="it" href={it.url} target="_blank" rel="noreferrer noopener">
        <span class="t">{it.title || $t('dashboard.widgets.news.untitled')}</span>
        <span class="meta">{it.source_name}{#if it.published_at || it.fetched_at} · {fmtDate(it.published_at) || fmtDate(it.fetched_at)}{/if}</span>
      </a>
    {/each}
  </div>
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
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: var(--space-2);
  }
  .it {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) 0;
    text-decoration: none;
    color: var(--text);
    border-bottom: 1px solid var(--border);
  }
  .list .it:last-child {
    border-bottom: none;
  }
  .grid .it {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
  }
  .it:hover .t {
    color: var(--accent);
  }
  .t {
    font-size: 0.82rem;
    line-height: 1.3;
    font-weight: 500;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .meta {
    font-size: 0.72rem;
    color: var(--muted);
  }
</style>
