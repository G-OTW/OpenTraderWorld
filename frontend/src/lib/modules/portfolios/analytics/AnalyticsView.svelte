<script>
  // The analysis tab: one window picker, one block nav, and whatever the server answered.
  //
  // It requests **only the block being looked at**. That is not an optimization detail, it
  // is the registry's contract made visible: `book` costs one ledger walk and no market data,
  // while `stress` pays for bars. Asking for all seven to render one would make the cheap
  // ones as slow as the expensive one.
  import { untrack } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { portfoliosApi } from '../api.js';
  import { PANELS, WINDOWS, panelFor } from './registry.js';

  let { portfolio = null, currency = 'USD', onfix = null, reloadKey = 0 } = $props();

  let block = $state('book');
  let window_ = $state('inception');
  let loading = $state(false);
  let error = $state(null);
  // Keyed by block+window: switching back to a tab already seen must not re-query.
  let panels = $state({});
  let seenReload = 0;

  const key = $derived(`${block}:${window_}`);
  const current = $derived(panels[key] ?? null);
  // Blocks that do not read the curve are unaffected by the window, so the picker hides
  // rather than offering a control that changes nothing.
  const WINDOWED = ['performance', 'risk', 'benchmark', 'costs'];
  const showWindow = $derived(WINDOWED.includes(block));

  $effect(() => {
    const id = portfolio?.id;
    const k = key;
    // Bumped by the page after a rebuild or a ledger edit: the ledger is upstream of all
    // seven blocks, so a write anywhere invalidates every cached answer.
    const rk = reloadKey;
    if (!id) return;
    // The cache is read and written here, so the read is untracked: tracking it would make
    // the effect its own trigger.
    untrack(() => {
      if (rk !== seenReload) {
        seenReload = rk;
        panels = {};
      }
      if (panels[k]) return;
      loading = true;
      error = null;
      portfoliosApi
        .analytics(id, { blocks: [block], window: window_ })
        .then((r) => {
          panels = { ...panels, [k]: r.blocks?.[block] ?? null };
        })
        .catch((e) => (error = e.message))
        .finally(() => (loading = false));
    });
  });
</script>

<div class="analytics">
  <nav class="blocks" aria-label={$t('portfolios.analytics.nav')}>
    {#each PANELS as p (p.key)}
      <button class:active={block === p.key} onclick={() => (block = p.key)}>
        {$t(`portfolios.analytics.block.${p.key}`)}
      </button>
    {/each}
    {#if showWindow}
      <div class="windows">
        {#each WINDOWS as w (w)}
          <button class="win" class:active={window_ === w} onclick={() => (window_ = w)}>
            {$t(`portfolios.analytics.window.${w}`)}
          </button>
        {/each}
      </div>
    {/if}
    <button class="btn sm ghost gear" onclick={() => onfix?.('history')} title={$t('portfolios.analytics.measure')}>
      <Icon name="settings" size={13} />
    </button>
  </nav>

  <ErrorText {error} copyable />

  <div class="body">
    {#if loading && !current}
      <div class="skel">
        {#each Array(4) as _, i (i)}<Skeleton height="72px" />{/each}
        <Skeleton height="180px" />
      </div>
    {:else if current}
      {@const Panel = panelFor(block)}
      {#if Panel}
        <Panel block={current} {currency} portfolioId={portfolio?.id} {onfix} />
      {/if}
    {/if}
  </div>
</div>

<style>
  .blocks {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--space-4);
  }
  .blocks > button {
    padding: var(--space-1) var(--space-3);
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .blocks > button:hover {
    color: var(--text);
  }
  .blocks > button.active {
    background: var(--surface-2);
    color: var(--text);
  }
  .windows {
    display: flex;
    gap: 2px;
    margin-left: auto;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .win {
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 600;
  }
  .win.active {
    background: var(--accent);
    color: #fff;
  }
  /* The gear is pinned to the right edge whether or not the window picker is there:
     it must not hop when switching to a block that shows one. */
  .gear {
    margin-left: auto;
  }
  .windows + .gear {
    margin-left: var(--space-2);
  }
  .skel {
    display: grid;
    gap: var(--space-3);
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  }
  .skel :global(> :last-child) {
    grid-column: 1 / -1;
  }
</style>
