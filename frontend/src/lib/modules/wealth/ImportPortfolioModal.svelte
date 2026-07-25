<script>
  // Import assets from a Portfolio Tracker portfolio into MyWealth. Pick a portfolio, tick
  // the assets to bring over (all preselected except ones already imported), and each becomes
  // a wealth asset (category = portfolio name) seeded with a first revision at today's price.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { portfoliosApi } from '$lib/modules/portfolios/api.js';
  import { wealthApi, fmtMoney } from './api.js';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { dateKey } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    /** Existing wealth assets, to flag already-imported ones (same name + category). */
    existing = [],
    onimported = () => {}
  } = $props();

  let portfolios = $state([]);
  let pfId = $state('');
  let detail = $state(null); // { portfolio, assets } of the chosen portfolio
  let selected = $state(new Set());
  let loading = $state(false);
  let importing = $state(false);
  let error = $state('');

  // Reset and (re)load the portfolio list each time the modal opens.
  $effect(() => {
    if (open) init();
  });

  async function init() {
    pfId = '';
    detail = null;
    selected = new Set();
    error = '';
    loading = true;
    try {
      portfolios = await portfoliosApi.list();
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  async function choose(e) {
    pfId = e.target.value;
    detail = null;
    selected = new Set();
    if (!pfId) return;
    error = '';
    loading = true;
    try {
      detail = await portfoliosApi.detail(pfId);
      // Preselect everything still held and not already imported.
      selected = new Set(
        detail.assets.filter((a) => a.quantity > 0 && !isDup(a)).map((a) => a.id)
      );
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  function assetName(a) {
    return a.name || a.symbol;
  }
  function isDup(a) {
    const cat = detail?.portfolio?.name ?? '';
    return existing.some((w) => w.name === assetName(a) && (w.category || '') === cat);
  }
  // crypto → crypto; stocks and ETFs → stock; anything else → other.
  function mapType(assetClass) {
    if (assetClass === 'crypto') return 'crypto';
    if (assetClass === 'stock' || assetClass === 'etf') return 'stock';
    return 'other';
  }

  function toggle(id) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }
  const allSelected = $derived(detail != null && detail.assets.length > 0 && detail.assets.every((a) => selected.has(a.id)));
  function toggleAll() {
    selected = allSelected ? new Set() : new Set(detail.assets.map((a) => a.id));
  }

  async function doImport() {
    const pf = detail.portfolio;
    const picked = detail.assets.filter((a) => selected.has(a.id));
    // Local day, not UTC — the seeded revision is dated for the user, not for Greenwich.
    const today = dateKey();
    importing = true;
    error = '';
    try {
      for (const a of picked) {
        const id = await wealthApi.addAsset({
          template_id: null,
          name: assetName(a),
          asset_type: mapType(a.asset_class),
          currency: pf.currency,
          category: pf.name
        });
        // Seed a first revision from the tracker's current position, when priced.
        if (a.price != null) {
          await wealthApi.addRevision(id, {
            valued_at: today,
            price: a.price,
            quantity: a.quantity,
            fields: {},
            note: null
          });
        }
      }
      open = false;
      onimported();
    } catch (e) {
      error = e.message;
    }
    importing = false;
  }
</script>

<Modal bind:open size="md" title={$t('wealth.import.title')}>
  <div class="imp">
    <label class="field">
      <span>{$t('wealth.import.portfolio')}</span>
      <select value={pfId} onchange={choose} disabled={importing}>
        <option value="">{$t('wealth.import.choose')}</option>
        {#each portfolios as pf (pf.id)}
          <option value={pf.id}>{pf.name} ({pf.currency})</option>
        {/each}
      </select>
    </label>

    {#if loading}
      <p class="muted">{$t('common.loading')}</p>
    {:else if !pfId && portfolios.length === 0}
      <p class="muted">{$t('wealth.import.noPortfolios')}</p>
    {:else if detail}
      {#if detail.assets.length === 0}
        <p class="muted">{$t('wealth.import.noAssets')}</p>
      {:else}
        <p class="hint">{$t('wealth.import.categoryHint', { name: detail.portfolio.name })}</p>
        <div class="list-head">
          <button type="button" class="link" onclick={toggleAll}>
            {allSelected ? $t('wealth.import.deselectAll') : $t('wealth.import.selectAll')}
          </button>
          <span class="count">{$t('wealth.import.selectedCount', { count: selected.size, total: detail.assets.length })}</span>
        </div>
        <ul class="assets">
          {#each detail.assets as a (a.id)}
            <li>
              <label class="row" class:off={!selected.has(a.id)}>
                <input
                  type="checkbox"
                  checked={selected.has(a.id)}
                  onchange={() => toggle(a.id)}
                  disabled={importing}
                />
                <span class="sym">{a.symbol}</span>
                <span class="name">{assetName(a)}</span>
                {#if isDup(a)}<span class="dup">{$t('wealth.import.alreadyAdded')}</span>{/if}
                <span class="val">
                  {#if a.market_value != null}{fmtMoney(a.market_value, detail.portfolio.currency)}{:else}—{/if}
                </span>
              </label>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    <ErrorText {error} />

    <div class="actions">
      <button type="button" class="ghost" onclick={() => (open = false)} disabled={importing}>
        {$t('common.cancel')}
      </button>
      <button
        type="button"
        class="primary"
        onclick={doImport}
        disabled={importing || selected.size === 0}
      >
        {#if importing}{$t('wealth.import.importing')}{:else}
          <Icon name="download" size={15} /> {$t('wealth.import.importBtn', { count: selected.size })}
        {/if}
      </button>
    </div>
  </div>
</Modal>

<style>
  .imp {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .link {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
  }
  .link:hover {
    color: var(--text);
  }
  .count {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
  }
  .assets {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 320px;
    overflow-y: auto;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 6px 9px;
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: 0;
    cursor: pointer;
    font-size: var(--text-base);
  }
  .row.off {
    opacity: 0.55;
  }
  .sym {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
    min-width: 60px;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }
  .dup {
    font-size: 0.65rem;
    text-transform: uppercase;
    color: var(--amber-ink);
    border: 0.5px solid color-mix(in srgb, var(--amber) 45%, transparent);
    border-radius: 0;
    padding: 1px 6px;
    white-space: nowrap;
  }
  .val {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-base);
  }
</style>
