<script>
  // News feed widget: shows the latest items from a chosen feed (or all feeds) as a
  // scrollable list or grid. Items link out to their article. Config: { feed_id, view }.
  import { newsApi } from '$lib/modules/news/api.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import { livePulse, LIVE } from '../live.svelte.js';

  // Compact "Jul 3, 14:20"-style stamp (null-safe).
  function fmtDate(s) {
    if (!s) return '';
    const d = new Date(s);
    if (isNaN(d)) return '';
    return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  let { item, editing } = $props();
  const view = $derived(item.config?.view === 'grid' ? 'grid' : 'list');
  const limit = $derived(Math.max(1, Math.min(50, item.config?.limit ?? 10)));

  let items = $state(null);
  let err = $state('');

  const live = livePulse(LIVE.inbox);
  // Blank to the loading state only when the chosen feed changes; a periodic refresh keeps
  // the current items on screen until the new ones land.
  let asked = '';
  $effect(() => {
    const feedId = item.config?.feed_id || '';
    const n = limit;
    live.n;
    if (editing) return; // don't fetch behind the config affordance
    let alive = true;
    if (`${feedId}|${n}` !== asked) {
      asked = `${feedId}|${n}`;
      items = null;
    }
    err = '';
    newsApi
      .listItems({ feed_id: feedId, limit })
      .then((r) => alive && (items = r))
      .catch((e) => alive && (err = e.message));
    return () => (alive = false);
  });
</script>

<WidgetState
  {editing}
  error={err}
  loading={items === null}
  empty={items?.length === 0}
  preview={$t('dashboard.widgets.news.preview')}
  emptyText={$t('dashboard.widgets.news.empty')}
  rows={4}
>
  {#if view === 'grid'}
    <div class="grid">
      {#each items as it (it.id)}
        <a class="card" href={it.url} target="_blank" rel="noreferrer noopener">
          <span class="title w-clamp">{it.title || $t('dashboard.widgets.news.untitled')}</span>
          <span class="w-sub">{it.source_name}</span>
        </a>
      {/each}
    </div>
  {:else}
    <div class="w-list">
      {#each items as it (it.id)}
        <a class="w-row stack" href={it.url} target="_blank" rel="noreferrer noopener">
          <span class="title w-clamp">{it.title || $t('dashboard.widgets.news.untitled')}</span>
          <span class="w-sub">
            <span class="src">{it.source_name}</span>
            {#if it.published_at || it.fetched_at}
              · {fmtDate(it.published_at) || fmtDate(it.fetched_at)}
            {/if}
          </span>
        </a>
      {/each}
    </div>
  {/if}
</WidgetState>

<style>
  .title {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    line-height: 1.35;
    color: var(--text);
  }
  a:hover .title {
    color: var(--accent);
  }
  /* The source is what you scan by, so it carries the weight in the metadata line. */
  .src {
    color: var(--muted);
  }
  /* The grid variant is the one place a widget draws its own boxes: without them the
     tiles lose their reading order once the text lengths differ. */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-2);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3);
    border-radius: var(--radius);
    background: var(--surface-2);
    text-decoration: none;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .card:hover {
    background: var(--surface-3);
  }
  .card:focus-visible {
    outline: none;
    box-shadow: var(--ring);
  }
</style>
