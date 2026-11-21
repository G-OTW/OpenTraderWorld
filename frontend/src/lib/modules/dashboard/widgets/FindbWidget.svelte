<script>
  // FinanceDatabase widgets — the cards of the FinDB mockup, each a `variant`. Reads the
  // module's favourites, folders and catalog status; the update check is the module's own
  // cached call, so the widget never hits upstream on its own.
  //
  // Config: { variant, limit }.
  import { findbApi } from '$lib/modules/findb/api.js';
  import { fmtNum, fmtDateTime } from '$lib/format';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Donut from './parts/Donut.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'assetTypes');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 8)));

  const needsFavorites = $derived(['assetTypes', 'folders', 'stale'].includes(variant));

  let favorites = $state(null);
  let folders = $state([]);
  let update = $state.raw(null);
  let err = $state('');

  $effect(() => {
    if (editing || !needsFavorites) return;
    let alive = true;
    Promise.all([findbApi.listFavorites(), findbApi.listFolders()])
      .then(([f, d]) => { if (alive) { favorites = f; folders = d; } })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });
  $effect(() => {
    if (editing || variant !== 'update') return;
    let alive = true;
    findbApi.updateCheck()
      .then((u) => { if (alive) update = u; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const byType = $derived.by(() => {
    const counts = new Map();
    for (const f of favorites ?? []) counts.set(f.asset_type || '—', (counts.get(f.asset_type || '—') ?? 0) + 1);
    return [...counts].map(([label, value]) => ({ label, value })).sort((a, b) => b.value - a.value);
  });

  const byFolder = $derived.by(() => {
    const counts = new Map();
    for (const f of favorites ?? []) counts.set(f.folder_id ?? '', (counts.get(f.folder_id ?? '') ?? 0) + 1);
    const rows = folders.map((d) => ({ id: d.id, name: d.name, color: d.color, count: counts.get(d.id) ?? 0 }));
    const loose = counts.get('') ?? 0;
    if (loose > 0) rows.push({ id: '', name: $t('dashboard.widgets.findb.unfiled'), color: '', count: loose });
    return rows.sort((a, b) => b.count - a.count).slice(0, limit);
  });

  // A favourite that lost its instrument row no longer resolves after a re-import: it is
  // stale, and the module cannot price or search it until it is re-pointed.
  const stale = $derived((favorites ?? []).filter((f) => f.instrument_id == null).slice(0, limit));
</script>

<WidgetState
  {editing}
  error={err}
  loading={(needsFavorites && favorites === null) || (variant === 'update' && update === null)}
  empty={needsFavorites && (favorites ?? []).length === 0}
  preview={$t('dashboard.widgets.findb.preview')}
  emptyText={$t('dashboard.widgets.findb.empty')}
  rows={4}
>
  {#if variant === 'assetTypes'}
    <Donut segments={byType} centerLabel={$t('dashboard.widgets.findb.favorites')} />
  {:else if variant === 'folders'}
    <div class="w-body">
      <span class="w-sub">
        {$t('dashboard.widgets.findb.foldersMeta', { folders: folders.length, items: (favorites ?? []).length })}
      </span>
      <ul class="w-list">
        {#each byFolder as f (f.id)}
          <li>
            <a class="w-row" href="/findb">
              <Icon name="folder" size={15} />
              <span class="w-name grow">{f.name}</span>
              <span class="w-num">{f.count}</span>
              <Icon name="chevron-right" size={13} />
            </a>
          </li>
        {/each}
      </ul>
    </div>
  {:else if variant === 'stale'}
    {#if stale.length === 0}
      <p class="w-state">{$t('dashboard.widgets.findb.noStale')}</p>
    {:else}
      <ul class="w-list">
        {#each stale as f (f.id)}
          <li class="w-row">
            <span class="sym">{f.symbol || '—'}</span>
            <span class="stackcol grow">
              <span class="w-name">{f.name || f.symbol}</span>
              <span class="w-sub">{$t('dashboard.widgets.findb.noMatch')}</span>
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if variant === 'update'}
    <div class="w-body">
      <div class="banner" class:on={update.update_available}>
        <span class="w-dot" class:on={update.update_available}></span>
        <div class="btext">
          <span class="btitle">
            {update.update_available
              ? $t('dashboard.widgets.findb.updateAvailable')
              : $t('dashboard.widgets.findb.upToDate')}
          </span>
          {#if update.latest}
            <span class="w-sub">{$t('dashboard.widgets.findb.latestVersion', { version: update.latest })}</span>
          {/if}
        </div>
      </div>
      <ul class="w-list">
        <li class="w-row">
          <span class="w-name grow">{$t('dashboard.widgets.findb.installed')}</span>
          <span class="w-num">{update.current || '—'}</span>
        </li>
        {#if update.built_at}
          <li class="w-row">
            <span class="w-name grow">{$t('dashboard.widgets.findb.builtAt')}</span>
            <span class="w-num">{fmtDateTime(update.built_at)}</span>
          </li>
        {/if}
        {#if update.size}
          <li class="w-row">
            <span class="w-name grow">{$t('dashboard.widgets.findb.snapshotSize')}</span>
            <span class="w-num">{fmtNum(update.size / 1e6, 1)} MB</span>
          </li>
        {/if}
      </ul>
    </div>
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
    min-width: 0;
  }
  .w-row :global(svg) {
    color: var(--faint);
    flex-shrink: 0;
  }
  .stackcol {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  /* The symbol keeps a fixed chip so a list of them reads as a column. */
  .sym {
    flex-shrink: 0;
    min-width: 52px;
    padding: 3px 6px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-family: var(--mono);
    font-size: 10.5px;
    text-align: center;
    color: var(--dim);
  }
  .banner {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-3);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .banner.on {
    background: var(--accent-soft);
  }
  .banner .w-dot {
    margin-top: 5px;
    background: var(--faint);
  }
  .banner .w-dot.on {
    background: var(--accent);
  }
  .btext {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .btitle {
    color: var(--text);
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
  }
</style>
