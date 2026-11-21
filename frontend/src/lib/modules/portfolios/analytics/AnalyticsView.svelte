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
  import SegChips from '../SegChips.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { portfoliosApi } from '../api.js';
  import { PANELS, WINDOWS, panelFor, isSelf } from './registry.js';

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
  const WINDOWED = ['performance', 'risk', 'benchmark', 'costs', 'compare'];
  const showWindow = $derived(WINDOWED.includes(block));

  $effect(() => {
    const id = portfolio?.id;
    const k = key;
    // Bumped by the page after a rebuild or a ledger edit: the ledger is upstream of all
    // seven blocks, so a write anywhere invalidates every cached answer.
    const rk = reloadKey;
    if (!id || isSelf(block)) return;
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
    <div class="names">
      {#each PANELS as p (p.key)}
        <button class:active={block === p.key} aria-current={block === p.key ? 'page' : undefined} onclick={() => (block = p.key)}>
          {$t(`portfolios.analytics.block.${p.key}`)}
        </button>
      {/each}
    </div>
    <div class="tools">
      {#if showWindow}
        <SegChips
          options={WINDOWS.map((w) => ({ key: w, label: $t(`portfolios.analytics.window.${w}`) }))}
          bind:value={window_}
          mono
          ariaLabel={$t('portfolios.headline.periodAria')}
        />
      {/if}
      <!-- Pinned to the right edge whether or not the window picker is there: it must not
           hop when switching to a block that shows one. -->
      <button
        class="icon"
        onclick={() => onfix?.('history')}
        title={$t('portfolios.analytics.measure')}
        aria-label={$t('portfolios.analytics.measure')}
      >
        <Icon name="settings" size={14} />
      </button>
    </div>
  </nav>

  <ErrorText {error} copyable />

  <div class="body">
    {#if isSelf(block)}
      {@const Panel = panelFor(block)}
      {#key `${block}:${window_}:${reloadKey}`}
        <Panel portfolioId={portfolio?.id} {currency} window={window_} {onfix} />
      {/key}
    {:else if loading && !current}
      <div class="pa-stack">
        <div class="pa-stats">
          {#each Array(4) as _, i (i)}<Skeleton height="64px" />{/each}
        </div>
        <Skeleton height="280px" />
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
  .analytics {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .blocks {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2) var(--space-4);
  }
  .names {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
  }
  .names button {
    height: 28px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background-color 160ms ease,
      color 160ms ease;
  }
  .names button:hover {
    color: var(--text);
  }
  .names button.active {
    background: var(--surface-2);
    color: var(--text);
  }
  .tools {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: auto;
  }

  /* Layout and table conventions shared by every panel of the tab: one definition, scoped
     to the analysis view. */
  .analytics :global(.pa-stack) {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .analytics :global(.pa-stats) {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--space-3);
  }
  .analytics :global(.pa-stats .statcard) {
    background: var(--surface);
    border-radius: var(--radius);
  }
  .analytics :global(.pa-cols) {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 360px), 1fr));
    gap: var(--space-3);
    align-items: start;
  }
  .analytics :global(.pa-scroll) {
    overflow-x: auto;
    min-width: 0;
  }
  .analytics :global(table.pa-table) {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  .analytics :global(.pa-table th),
  .analytics :global(.pa-table td) {
    height: 36px;
    padding: 0 var(--space-3);
    text-align: left;
    white-space: nowrap;
    border-bottom: var(--hairline) solid var(--border);
  }
  .analytics :global(.pa-table th) {
    height: 32px;
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .analytics :global(.pa-table tbody tr:last-child td) {
    border-bottom: 0;
  }
  .analytics :global(.pa-table tbody tr) {
    transition: background-color 140ms ease;
  }
  .analytics :global(.pa-table tbody tr:hover) {
    background: var(--surface-hover);
  }
  .analytics :global(.pa-table .r) {
    text-align: right;
  }
  .analytics :global(.pa-table td.r),
  .analytics :global(.pa-table td.num) {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .analytics :global(.pa-table .num) {
    text-align: right;
  }
  .analytics :global(.pa-note) {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--dim);
  }
  .analytics :global(.pa-warn) {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--amber-ink);
  }
  .analytics :global(.pa-up) {
    color: var(--green);
  }
  .analytics :global(.pa-down) {
    color: var(--red);
  }
  .analytics :global(.pa-muted) {
    color: var(--muted);
  }

  /* Phone: the window picker takes its own line and wraps; the gear keeps the right edge. */
  @media (max-width: 767px) {
    .names button {
      min-height: 44px;
    }
    .tools {
      flex: 1 1 100%;
      justify-content: space-between;
    }
  }
</style>
