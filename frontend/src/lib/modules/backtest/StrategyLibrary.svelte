<script>
  // Dock tab 2 — the saved strategies. The search box matches names *and* tags (that is what
  // tags are for here), so "btc scalp" narrows to strategies tagged either. Clicking a row
  // loads it into the wizard; the row that is currently loaded is marked.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let { strategies = [], query = '', activeId = null, onopen, onduplicate, onremove } = $props();

  const filtered = $derived.by(() => {
    const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (!terms.length) return strategies;
    // Every term must hit somewhere (name or one tag) — narrowing, not widening.
    return strategies.filter((s) => {
      const hay = `${s.name} ${(s.tags ?? []).join(' ')}`.toLowerCase();
      return terms.every((w) => hay.includes(w));
    });
  });

  const fmtDate = (iso) => (iso ? String(iso).slice(0, 10) : '');
</script>

{#if !strategies.length}
  <p class="empty">{$t('backtest.dock.noStrategies')}</p>
{:else if !filtered.length}
  <p class="empty">{$t('backtest.dock.noMatch', { q: query })}</p>
{:else}
  <div class="rows">
    {#each filtered as s (s.id)}
      <div class="row" class:on={s.id === activeId}>
        <button class="main" onclick={() => onopen?.(s)} title={$t('backtest.dock.loadStrategy')}>
          <span class="nm">{s.name}</span>
          <span class="tags">
            {#each s.tags ?? [] as tag (tag)}<span class="tag">{tag}</span>{/each}
          </span>
          <span class="when">{fmtDate(s.updated_at)}</span>
        </button>
        <button class="act" title={$t('backtest.dock.duplicate')} onclick={() => onduplicate?.(s)}><Icon name="copy" size={12} /></button>
        <button class="act danger" title={$t('common.remove')} onclick={() => onremove?.(s)}><Icon name="x" size={12} /></button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-3);
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    border-bottom: var(--hairline) solid var(--border);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.on {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 260px) minmax(0, 1fr) 90px;
    align-items: center;
    gap: var(--space-3);
    background: transparent;
    border: none;
    color: var(--text);
    text-align: left;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .nm {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tags {
    display: flex;
    gap: var(--space-1);
    overflow: hidden;
    flex-wrap: nowrap;
  }
  .tag {
    font-size: 0.68rem;
    color: var(--muted);
    border: var(--hairline) solid var(--border);
    padding: 1px var(--space-2);
    white-space: nowrap;
  }
  .when {
    color: var(--muted);
    font-size: var(--text-xs);
    text-align: right;
  }
  .act {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-2);
    display: inline-flex;
  }
  .act:hover {
    color: var(--text);
  }
  .act.danger:hover {
    color: var(--red);
  }
</style>
