<script>
  // Editor widgets — the cards of the Editor mockup. Everything comes from the module's
  // one `list()` call (document metadata, newest edit first); the quick-create card adds
  // a page through the same API the module uses.
  //
  // Config: { variant, limit }.
  import { docsApi } from '$lib/modules/editor/api.js';
  
  import { t } from '$lib/i18n';
import { ago } from '$lib/format.js';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import WidgetState from './WidgetState.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'recent');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 6)));

  let docs = $state(null);
  let err = $state('');
  let title = $state('');
  let saving = $state(false);
  let savedId = $state('');

  async function load() {
    err = '';
    try {
      docs = await docsApi.list();
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    if (!editing) load();
  });

  const byId = $derived(new Map((docs ?? []).map((d) => [d.id, d])));
  // "/Finance/Trading" — the folder chain above a page, which is how the mockup
  // distinguishes two pages that share a name.
  function pathOf(d) {
    const parts = [];
    let p = d.parent_id ? byId.get(d.parent_id) : null;
    let guard = 0;
    while (p && guard++ < 10) {
      parts.unshift(p.title);
      p = p.parent_id ? byId.get(p.parent_id) : null;
    }
    return parts.length ? `/${parts.join('/')}` : '';
  }

  const pages = $derived((docs ?? []).filter((d) => d.kind !== 'folder'));
  const folders = $derived((docs ?? []).filter((d) => d.kind === 'folder'));
  const recent = $derived(
    [...pages].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, limit)
  );

  async function create() {
    const name = title.trim();
    if (!name) return;
    saving = true;
    err = '';
    try {
      const doc = await docsApi.create(null, 'page', name);
      savedId = doc?.id ?? '';
      title = '';
      await load();
    } catch (e) {
      err = e.message;
    } finally {
      saving = false;
    }
  }
</script>

{#if variant === 'quick'}
  <!-- A live control, so it stays usable while the list behind it is still loading. -->
  {#if editing}
    <p class="w-state">{$t('dashboard.widgets.editor.preview')}</p>
  {:else}
    <div class="w-body">
      <ErrorText error={err} compact />
      <form class="quick" onsubmit={(e) => { e.preventDefault(); create(); }}>
        <input bind:value={title} placeholder={$t('dashboard.widgets.editor.notePlaceholder')} disabled={saving} />
        <button class="primary" disabled={saving || !title.trim()}>
          <Icon name="plus" size={13} /> {$t('dashboard.widgets.editor.createNote')}
        </button>
      </form>
      {#if savedId}
        <a class="w-foot" href="/editor">{$t('dashboard.widgets.editor.open')} <Icon name="chevron-right" size={12} /></a>
      {/if}
    </div>
  {/if}
{:else}
  <WidgetState
    {editing}
    error={err}
    loading={docs === null}
    empty={variant === 'recent' && recent.length === 0}
    preview={$t('dashboard.widgets.editor.preview')}
    emptyText={$t('dashboard.widgets.editor.empty')}
    rows={4}
  >
    {#if variant === 'counts'}
      <div class="counts">
        <div class="cell wide">
          <span class="fig">{(docs ?? []).length}</span>
          <span class="w-sub">{$t('dashboard.widgets.editor.totalDocs')}</span>
        </div>
        <div class="row">
          <div class="cell">
            <span class="fig sm">{pages.length}</span>
            <span class="w-sub">{$t('dashboard.widgets.editor.pages')}</span>
          </div>
          <div class="cell">
            <span class="fig sm">{folders.length}</span>
            <span class="w-sub">{$t('dashboard.widgets.editor.folders')}</span>
          </div>
        </div>
      </div>
    {:else}
      <div class="w-list">
        {#each recent as d (d.id)}
          <a class="w-row stack" href="/editor">
            <span class="line">
              <span class="w-name title">{d.title || $t('dashboard.widgets.editor.untitled')}</span>
              <span class="w-sub when">{$ago(d.updated_at)}</span>
            </span>
            {#if pathOf(d)}<span class="w-name w-sub path">{pathOf(d)}</span>{/if}
          </a>
        {/each}
      </div>
    {/if}
  </WidgetState>
{/if}

<style>
  .quick {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .quick input {
    width: 100%;
  }
  .quick button {
    width: 100%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
  }
  .line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    min-width: 0;
  }
  .title {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .when {
    flex-shrink: 0;
  }
  .path {
    font-family: var(--mono);
    font-size: 11px;
  }
  a:hover .title {
    color: var(--accent);
  }
  .counts {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-6);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 28px;
    font-weight: var(--fw-medium);
    letter-spacing: -0.015em;
    color: var(--text);
    line-height: 1.08;
  }
  .fig.sm {
    font-size: 19px;
  }
</style>
