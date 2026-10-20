<script>
  // What each symbol of the file is, in this portfolio.
  //
  // This is the one question the file cannot answer: "VT" is a string, an asset here is a
  // price source (provider + id). A symbol the portfolio already holds answers itself and
  // is marked as such; every other one waits for the user — search it, point it at an
  // asset already held, or leave its rows out. Nothing is created until the import runs,
  // and an undecided symbol blocks the import rather than being invented into one.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { portfoliosApi } from './api.js';
  import { t } from '$lib/i18n';

  let {
    symbols = [],
    assets = [],
    portfolioCurrency = 'USD',
    /** (sourceSymbol, choice | null) — null clears the choice (back to auto-matching). */
    onchoose = () => {}
  } = $props();

  let openKey = $state('');
  let kind = $state('stock'); // 'crypto' | 'stock'
  let query = $state('');
  let results = $state([]);
  let searching = $state(false);
  let searchError = $state('');

  // Symbols the user has just answered. The answer only shows up in `symbols` after the
  // next analyze round-trip (~300 ms), and without this the auto-open below would reopen
  // the picker on the symbol that was *just* resolved — asking the same question twice.
  let answered = $state(new Set());

  const unresolved = $derived(
    symbols.filter((s) => s.state === 'unresolved' && !answered.has(s.source))
  );

  // Land on the first undecided symbol: it is what the user is here to answer.
  $effect(() => {
    if (!openKey && unresolved.length) openPicker(unresolved[0]);
  });

  function markAnswered(source) {
    answered = new Set(answered).add(source);
  }

  function openPicker(sym) {
    openKey = sym.source;
    query = sym.source;
    results = [];
    searchError = '';
    // A ticker of 3-5 upper-case letters is a listing far more often than a coin.
    kind = /^[A-Z0-9.]{1,6}$/.test(sym.source.trim()) ? 'stock' : 'crypto';
    runSearch();
  }

  let debounce;
  function queueSearch() {
    clearTimeout(debounce);
    debounce = setTimeout(runSearch, 250);
  }

  async function runSearch() {
    if (!query.trim()) {
      results = [];
      return;
    }
    searching = true;
    searchError = '';
    try {
      results = (await portfoliosApi.search(kind, query)).slice(0, 8);
    } catch (e) {
      searchError = e.message;
      results = [];
    } finally {
      searching = false;
    }
  }

  function chooseHit(sym, hit) {
    onchoose(sym.source, {
      kind: 'new',
      asset_class: hit.asset_class,
      provider: hit.provider,
      provider_id: hit.provider_id,
      symbol: hit.symbol,
      name: hit.name,
      // Create it in the currency the file quotes it in — the file is the only source
      // for that; fall back to the portfolio's when it says nothing.
      currency: sym.file_currency ?? portfolioCurrency
    });
    markAnswered(sym.source);
    openKey = '';
  }

  function chooseAsset(sym, assetId) {
    if (!assetId) return;
    onchoose(sym.source, { kind: 'asset', asset_id: assetId });
    markAnswered(sym.source);
    openKey = '';
  }

  function skip(sym) {
    onchoose(sym.source, { kind: 'skip' });
    markAnswered(sym.source);
    openKey = '';
  }

  function clear(sym) {
    onchoose(sym.source, null);
    answered = new Set([...answered].filter((s) => s !== sym.source));
    openPicker(sym);
  }

  const stateLabel = (s) => $t(`portfolios.import.symbols.state.${s}`);
</script>

<section class="symbols">
  <div class="head">
    <h2>{$t('portfolios.import.symbols.title')}</h2>
    <span class="dim small">
      {unresolved.length
        ? $t('portfolios.import.symbols.pending', { n: unresolved.length })
        : $t('portfolios.import.symbols.allResolved')}
    </span>
  </div>

  <ul class="list">
    {#each symbols as sym (sym.source)}
      <li class:pending={sym.state === 'unresolved'} class:off={sym.state === 'skip'}>
        <div class="row">
          <span class="src mono">{sym.source}</span>
          <span class="rows mono">{$t('portfolios.import.symbols.rows', { n: sym.rows })}</span>
          <span class="arrow"><Icon name="chevron-right" size={13} /></span>
          <span class="target">
            {#if sym.state === 'unresolved'}
              <span class="chip pending">{stateLabel('unresolved')}</span>
            {:else if sym.state === 'skip'}
              <span class="chip">{stateLabel('skip')}</span>
            {:else}
              <span class="tsym mono">{sym.symbol}</span>
              <span class="tname">{sym.name || ''}</span>
              <span class="chip {sym.state}">{stateLabel(sym.state)}</span>
              {#if sym.currency_clash}
                <span class="chip warn" title={$t('portfolios.import.symbols.currencyClashHint')}>
                  {sym.currency_clash} → {sym.currency}
                </span>
              {/if}
            {/if}
          </span>
          <span class="act">
            {#if openKey === sym.source}
              <Button size="sm" onclick={() => (openKey = '')}>{$t('common.cancel')}</Button>
            {:else if sym.state === 'matched'}
              <Button size="sm" onclick={() => openPicker(sym)}>
                {$t('portfolios.import.symbols.change')}
              </Button>
            {:else if sym.state === 'unresolved'}
              <Button size="sm" variant="primary" onclick={() => openPicker(sym)}>
                {$t('portfolios.import.symbols.resolve')}
              </Button>
            {:else}
              <Button size="sm" onclick={() => clear(sym)}>
                {$t('portfolios.import.symbols.change')}
              </Button>
            {/if}
          </span>
        </div>

        {#if openKey === sym.source}
          <div class="picker">
            {#if assets.length}
              <div class="pick-row">
                <span>{$t('portfolios.import.symbols.existing')}</span>
                <Dropdown
                  value=""
                  onpick={(v) => chooseAsset(sym, v)}
                  ariaLabel={$t('portfolios.import.symbols.existing')}
                  options={[
                    { value: '', label: $t('portfolios.import.symbols.existingNone') },
                    ...assets.map((a) => ({ value: a.id, label: `${a.symbol} — ${a.name || a.provider_id}` }))
                  ]}
                />
              </div>
            {/if}

            <div class="pick-row">
              <span>{$t('portfolios.import.symbols.search')}</span>
              <div class="searchbar">
                <span class="kinds">
                  <button class:active={kind === 'stock'} onclick={() => ((kind = 'stock'), runSearch())}>
                    {$t('portfolios.addAsset.stockEtf')}
                  </button>
                  <button class:active={kind === 'crypto'} onclick={() => ((kind = 'crypto'), runSearch())}>
                    {$t('portfolios.addAsset.crypto')}
                  </button>
                </span>
                <input
                  type="search"
                  bind:value={query}
                  oninput={queueSearch}
                  placeholder={$t('portfolios.import.symbols.searchPlaceholder')}
                  autocomplete="off"
                />
                <Button size="sm" onclick={() => skip(sym)}>
                  {$t('portfolios.import.symbols.skipRows')}
                </Button>
              </div>
            </div>

            <div class="hits">
              {#if searchError}
                <p class="err small">{searchError}</p>
              {:else if searching}
                <p class="dim small">{$t('portfolios.addAsset.searching')}</p>
              {:else if results.length === 0}
                <p class="dim small">{$t('portfolios.addAsset.noMatches')}</p>
              {:else}
                {#each results as r (r.provider + r.provider_id)}
                  <button class="hit" onclick={() => chooseHit(sym, r)}>
                    <span class="sym mono">{r.symbol}</span>
                    <span class="name">{r.name}</span>
                    <span class="cls">{r.asset_class}</span>
                    <span class="prov dim">{r.provider}</span>
                  </button>
                {/each}
              {/if}
            </div>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
</section>

<style>
  .symbols {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .head h2 {
    font-family: var(--mono);
    font-size: var(--fs-section);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--dim);
  }
  .list {
    list-style: none;
    border: 0.5px solid var(--border);
    max-height: 42vh;
    overflow-y: auto;
  }
  .list li {
    border-bottom: 0.5px solid var(--border);
    border-left: 1.5px solid transparent;
  }
  .list li:last-child {
    border-bottom: none;
  }
  /* The undecided ones are the work left to do — mark them, and only them. */
  .list li.pending {
    border-left-color: var(--amber);
  }
  .list li.off {
    opacity: 0.55;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 5px var(--space-3);
  }
  .src {
    font-weight: var(--fw-medium);
    min-width: 88px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rows {
    color: var(--dim);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .arrow {
    color: var(--dim);
    display: inline-flex;
  }
  .target {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    overflow: hidden;
  }
  .tsym {
    font-weight: var(--fw-medium);
  }
  .tname {
    color: var(--muted);
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    border: 0.5px solid var(--border);
    padding: 0 var(--space-1);
    white-space: nowrap;
  }
  .chip.pending {
    color: var(--amber-ink, var(--amber));
    border-color: color-mix(in srgb, var(--amber) 45%, transparent);
  }
  .chip.new {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .chip.warn {
    color: var(--amber-ink, var(--amber));
    border-color: color-mix(in srgb, var(--amber) 45%, transparent);
    text-transform: none;
    font-family: var(--mono);
  }
  .act {
    flex-shrink: 0;
  }
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3) var(--space-3);
    background: var(--surface-2);
    border-top: 0.5px solid var(--border);
  }
  .pick-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .pick-row > span {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    width: 96px;
    flex-shrink: 0;
  }
  .pick-row :global(.dd) {
    flex: 1;
    min-width: 0;
  }
  .searchbar {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .searchbar input {
    flex: 1;
    min-width: 0;
  }
  .kinds {
    display: inline-flex;
    height: var(--control-h);
    border: var(--hairline) solid var(--border-control);
    flex-shrink: 0;
  }
  .kinds button {
    background: transparent;
    border: none;
    color: var(--muted);
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .kinds button + button {
    border-left: 0.5px solid var(--border-control);
  }
  .kinds button.active {
    background: var(--surface);
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .hits {
    display: flex;
    flex-direction: column;
    max-height: 200px;
    overflow-y: auto;
  }
  .hit {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    text-align: left;
    background: transparent;
    border: none;
    border-bottom: 0.5px solid var(--border);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    padding: 4px var(--space-2);
    cursor: pointer;
  }
  .hit:last-child {
    border-bottom: none;
  }
  .hit:hover {
    background: var(--surface);
  }
  .sym {
    font-weight: var(--fw-medium);
    min-width: 72px;
  }
  .name {
    flex: 1;
    min-width: 0;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cls,
  .prov {
    font-size: var(--text-xs);
    text-transform: uppercase;
    color: var(--muted);
  }
  .mono {
    font-family: var(--mono);
  }
  .dim {
    color: var(--dim);
  }
  .small {
    font-size: var(--text-sm);
  }
  .err {
    color: var(--red);
  }
</style>
