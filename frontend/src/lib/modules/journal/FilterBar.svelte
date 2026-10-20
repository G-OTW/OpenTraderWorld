<script>
  // The trade-filter bar shared by Breakdown, Analytics and Compare. One definition of
  // "what a filtered scope is", so the three screens can never drift apart on it.
  //
  // `value` is the payload the API expects (already date-bounded); the caller binds it
  // and re-fetches when it changes. Filters persist per `storageKey` so a screen keeps
  // its own scope across refreshes.
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { ASSET_CLASSES } from './api.js';
  import { t } from '$lib/i18n';

  let {
    categoryId = '',
    strategies = [],
    tags = [],
    suggestions = { tickers: [], exchanges: [], signals: [] },
    storageKey = 'otw.journal.filters.v1',
    // Bindable: the filter payload, rebuilt on every edit.
    value = $bindable({}),
    // A unique id prefix for the datalists (two bars on one page must not collide).
    idPrefix = 'jf'
  } = $props();

  let fSide = $state('');
  let fTicker = $state('');
  let fStrategy = $state('');
  let fSignal = $state('');
  let fAsset = $state('');
  let fTag = $state('');
  let fSince = $state(''); // yyyy-mm-dd (date input)
  let fUntil = $state('');

  let loaded = $state(false);
  (function load() {
    try {
      const p = JSON.parse(localStorage.getItem(storageKey) || '{}');
      if (['', 'long', 'short'].includes(p.fSide)) fSide = p.fSide;
      if (typeof p.fTicker === 'string') fTicker = p.fTicker;
      if (typeof p.fStrategy === 'string') fStrategy = p.fStrategy;
      if (typeof p.fSignal === 'string') fSignal = p.fSignal;
      if (typeof p.fAsset === 'string') fAsset = p.fAsset;
      if (typeof p.fTag === 'string') fTag = p.fTag;
      if (typeof p.fSince === 'string') fSince = p.fSince;
      if (typeof p.fUntil === 'string') fUntil = p.fUntil;
    } catch {
      /* corrupt — ignore */
    }
    loaded = true;
  })();

  $effect(() => {
    const snap = { fSide, fTicker, fStrategy, fSignal, fAsset, fTag, fSince, fUntil };
    if (!loaded) return;
    try {
      localStorage.setItem(storageKey, JSON.stringify(snap));
    } catch {
      /* non-fatal */
    }
  });

  // Date inputs become RFC3339 day bounds; the scope's category rides along so the
  // caller has one object to send.
  $effect(() => {
    value = {
      category_id: categoryId || undefined,
      side: fSide,
      ticker: fTicker,
      strategy_id: fStrategy,
      signal_name: fSignal,
      asset_class: fAsset,
      tag_id: fTag,
      since: fSince ? new Date(fSince + 'T00:00:00').toISOString() : '',
      until: fUntil ? new Date(fUntil + 'T23:59:59').toISOString() : ''
    };
  });

  // The ticker filter offers what the journal actually holds. A persisted ticker that no
  // longer appears in any trade stays listed, so reloading a saved scope can't silently
  // widen it to "any ticker".
  const tickerOptions = $derived([
    { value: '', label: $t('journal.breakdown.filter.anyTicker') },
    ...(suggestions.tickers.includes(fTicker) || !fTicker ? [] : [{ value: fTicker, label: fTicker }]),
    ...suggestions.tickers.map((v) => ({ value: v, label: v }))
  ]);

  const activeCount = $derived(
    [fSide, fTicker, fStrategy, fSignal, fAsset, fTag, fSince, fUntil].filter(Boolean).length
  );

  export function clear() {
    fSide = fTicker = fStrategy = fSignal = fAsset = fTag = fSince = fUntil = '';
  }
</script>

<datalist id="{idPrefix}-signals">
  {#each suggestions.signals as v}<option value={v}></option>{/each}
</datalist>

<section class="filter-bar">
  <Dropdown
    bind:value={fSide}
    title={$t('journal.breakdown.filter.side')}
    ariaLabel={$t('journal.breakdown.filter.side')}
    options={[
      { value: '', label: $t('journal.breakdown.filter.anySide') },
      { value: 'long', label: $t('journal.side.long') },
      { value: 'short', label: $t('journal.side.short') }
    ]}
  />
  <Dropdown
    bind:value={fTicker}
    title={$t('journal.breakdown.filter.ticker')}
    ariaLabel={$t('journal.breakdown.filter.ticker')}
    options={tickerOptions}
  />
  <Dropdown
    bind:value={fStrategy}
    title={$t('journal.breakdown.filter.strategy')}
    ariaLabel={$t('journal.breakdown.filter.strategy')}
    options={[
      { value: '', label: $t('journal.breakdown.filter.anyStrategy') },
      ...strategies.map((s) => ({ value: s.id, label: s.name }))
    ]}
  />
  <input
    placeholder={$t('journal.breakdown.filter.signal')}
    list="{idPrefix}-signals"
    autocomplete="off"
    bind:value={fSignal}
  />
  <Dropdown
    bind:value={fAsset}
    title={$t('journal.breakdown.filter.assetClass')}
    ariaLabel={$t('journal.breakdown.filter.assetClass')}
    options={[
      { value: '', label: $t('journal.breakdown.filter.anyClass') },
      ...ASSET_CLASSES.map((a) => ({ value: a.id, label: a.label }))
    ]}
  />
  {#if tags.length}
    <Dropdown
      bind:value={fTag}
      title={$t('journal.breakdown.filter.tag')}
      ariaLabel={$t('journal.breakdown.filter.tag')}
      options={[
        { value: '', label: $t('journal.breakdown.filter.anyTag') },
        ...tags.map((t) => ({ value: t.id, label: t.name }))
      ]}
    />
  {/if}
  <label class="date">{$t('journal.breakdown.filter.from')} <input type="date" bind:value={fSince} /></label>
  <label class="date">{$t('journal.breakdown.filter.to')} <input type="date" bind:value={fUntil} /></label>
  {#if activeCount > 0}
    <div class="filter-clear">
      <Button size="sm" icon="x" onclick={clear}>
        {$t('journal.breakdown.filter.clear', { count: activeCount })}
      </Button>
    </div>
  {/if}
</section>

<style>
  /* .filter-bar / .filter-clear / .date come from theme/components.css (shared with
     the Trades list) — only the free-text input width is local. */
  .filter-bar input:not([type]) {
    width: 110px;
  }
</style>
