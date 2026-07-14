<script>
  // Set-up-auto-refresh reconciliation. On open we check every asset against its price source
  // (server /reconcile) and show a per-asset status: ✓ ok (live price shown) / ⚠ unresolved / ●
  // manual. Unresolved rows expand an inline fixer — pick a source + type the provider's exact
  // ticker + Re-check, or Mark manual to opt the asset out. Auto-refresh can be enabled once no
  // asset is unresolved (manual is allowed). If everything is already ok on the first check the
  // parent skips this modal entirely and enables auto-refresh directly.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { portfoliosApi, SPOT_SOURCES, fmtMoney } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let {
    open = $bindable(false),
    portfolioId,
    assets = [], // [{ id, symbol, name, asset_class, spot_provider, spot_symbol, provider }]
    onenabled = () => {} // called after auto-refresh is turned on
  } = $props();

  // Per-asset reconcile result keyed by asset_id: { status, source, price_usd, note }.
  let results = $state({});
  // Per-asset edit form for the inline fixer: { source, symbol, saving }.
  let edits = $state({});
  let checking = $state(false);
  let enabling = $state(false);
  let error = $state('');

  const meta = $derived(Object.fromEntries(assets.map((a) => [a.id, a])));
  const rows = $derived(assets.map((a) => ({ ...a, ...(results[a.id] ?? {}) })));
  const unresolved = $derived(rows.filter((r) => r.status === 'unresolved').length);
  const canEnable = $derived(!checking && rows.length > 0 && unresolved === 0);

  // Run the full reconcile when the modal opens.
  $effect(() => {
    if (open) checkAll();
  });

  async function checkAll() {
    checking = true;
    error = '';
    try {
      const r = await portfoliosApi.reconcile(portfolioId);
      results = Object.fromEntries((r.results ?? []).map((x) => [x.asset_id, x]));
    } catch (e) {
      error = e.message;
    } finally {
      checking = false;
    }
  }

  function sourcesFor(assetClass) {
    return SPOT_SOURCES[assetClass] ?? SPOT_SOURCES.crypto;
  }

  function startEdit(a) {
    const opts = sourcesFor(a.asset_class);
    edits[a.id] = {
      source: a.spot_provider ?? opts[0].id,
      symbol: a.spot_symbol || a.symbol || '',
      saving: false
    };
  }

  // Save the override for one asset, then re-reconcile just that asset (reuse the full endpoint;
  // it's one small portfolio and keeps the code simple).
  async function recheck(a) {
    const e = edits[a.id];
    if (!e) return;
    e.saving = true;
    error = '';
    try {
      // A source equal to the asset's default provider with no custom symbol → clear the override.
      const isDefault = e.source === a.provider && !e.symbol.trim();
      await portfoliosApi.updateAsset(a.id, {
        spot_provider: isDefault ? null : e.source,
        spot_symbol: e.symbol.trim(),
        recon_status: 'ok' // let the check below decide the real status
      });
      await checkAll();
      if (results[a.id]?.status === 'ok') delete edits[a.id];
    } catch (err) {
      error = err.message;
    } finally {
      if (edits[a.id]) edits[a.id].saving = false;
    }
  }

  async function markManual(a) {
    error = '';
    try {
      await portfoliosApi.updateAsset(a.id, { recon_status: 'manual' });
      await checkAll();
      delete edits[a.id];
    } catch (err) {
      error = err.message;
    }
  }

  async function clearManual(a) {
    error = '';
    try {
      await portfoliosApi.updateAsset(a.id, { recon_status: 'ok' });
      await checkAll();
    } catch (err) {
      error = err.message;
    }
  }

  async function enable() {
    enabling = true;
    error = '';
    try {
      await portfoliosApi.update(portfolioId, { auto_refresh: true });
      open = false;
      onenabled();
    } catch (err) {
      error = err.message;
    } finally {
      enabling = false;
    }
  }
</script>

<Modal bind:open size="lg" title={$t('portfolios.reconcile.title')}>
  <p class="lead">
    {@html $t('portfolios.reconcile.lead')}
  </p>

  <ErrorText error={error} copyable />

  <div class="rows">
    {#each rows as a (a.id)}
      {@const status = a.status ?? (checking ? 'checking' : '—')}
      <div class="row" class:ok={status === 'ok'} class:bad={status === 'unresolved'}>
        <span class="dot {status}"></span>
        <span class="sym">{a.symbol}</span>
        <span class="name">{a.name}</span>
        <span class="src">{a.source ?? a.spot_provider ?? a.provider}</span>
        <span class="price">{a.price_usd != null ? fmtMoney(a.price_usd, 'USD') : '—'}</span>
        <span class="stat {status}">
          {#if status === 'ok'}<Icon name="check" size={13} /> {$t('portfolios.reconcile.status.ok')}
          {:else if status === 'unresolved'}<Icon name="alert-triangle" size={13} /> {$t('portfolios.reconcile.status.unresolved')}
          {:else if status === 'manual'}{$t('portfolios.reconcile.status.manual')}
          {:else if status === 'checking'}{$t('portfolios.reconcile.status.checking')}
          {:else}—{/if}
        </span>
        <span class="act">
          {#if status === 'unresolved'}
            {#if edits[a.id]}
              <button class="link" onclick={() => delete edits[a.id]}>{$t('portfolios.reconcile.cancel')}</button>
            {:else}
              <button class="link" onclick={() => startEdit(a)}>{$t('portfolios.reconcile.fix')}</button>
              <button class="link" onclick={() => markManual(a)}>{$t('portfolios.reconcile.markManual')}</button>
            {/if}
          {:else if status === 'manual'}
            <button class="link" onclick={() => clearManual(a)}>{$t('portfolios.reconcile.recheck')}</button>
          {/if}
        </span>
      </div>

      {#if edits[a.id]}
        {@const opts = sourcesFor(a.asset_class)}
        {@const hint = opts.find((o) => o.id === edits[a.id].source)?.hint ?? ''}
        <div class="fixer">
          <label>
            {$t('portfolios.reconcile.source')}
            <select bind:value={edits[a.id].source}>
              {#each opts as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
            </select>
          </label>
          <label class="tk">
            {$t('portfolios.reconcile.ticker')}
            <input
              type="text"
              placeholder={hint}
              bind:value={edits[a.id].symbol}
              onkeydown={(e) => e.key === 'Enter' && recheck(a)}
            />
          </label>
          <button class="btn sm primary" disabled={edits[a.id].saving} onclick={() => recheck(a)}>
            {edits[a.id].saving ? $t('portfolios.reconcile.checking') : $t('portfolios.reconcile.recheck')}
          </button>
        </div>
      {/if}
    {/each}

    {#if rows.length === 0}
      <p class="muted">{$t('portfolios.reconcile.noAssets')}</p>
    {/if}
  </div>

  {#snippet footer()}
    <div class="foot">
      <span class="tally">
        {#if checking}{$t('portfolios.reconcile.checking')}{:else}
          {$t('portfolios.reconcile.tally', {
            ok: rows.filter((r) => r.status === 'ok').length,
            unresolved,
            manual: rows.filter((r) => r.status === 'manual').length
          })}
        {/if}
      </span>
      <div class="spacer"></div>
      <button class="btn" onclick={checkAll} disabled={checking}>{$t('portfolios.reconcile.recheckAll')}</button>
      <button class="btn primary" onclick={enable} disabled={!canEnable || enabling}>
        {enabling ? $t('portfolios.reconcile.enabling') : $t('portfolios.reconcile.enable')}
      </button>
    </div>
  {/snippet}
</Modal>

<style>
  .lead {
    color: var(--muted);
    font-size: var(--text-base);
    margin-bottom: var(--space-3);
  }
  .warn {
    color: var(--amber);
    font-weight: var(--fw-semibold);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 52vh;
    overflow-y: auto;
  }
  .row {
    display: grid;
    grid-template-columns: 14px 72px 1fr auto auto auto auto;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--radius);
  }
  .row.bad {
    border-color: color-mix(in srgb, var(--amber) 40%, transparent);
    background: color-mix(in srgb, var(--amber) 8%, transparent);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.ok {
    background: var(--green);
  }
  .dot.unresolved {
    background: var(--amber);
  }
  .dot.manual {
    background: var(--muted);
  }
  .dot.checking {
    background: var(--border);
  }
  .sym {
    font-weight: var(--fw-semibold);
  }
  .name {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .src {
    font-size: var(--text-xs);
    text-transform: uppercase;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1px var(--space-1);
  }
  .price {
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .stat {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .stat.ok {
    color: var(--green);
  }
  .stat.unresolved {
    color: var(--amber);
  }
  .act {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
  }
  .link {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
  }
  .fixer {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3) var(--space-3);
    margin: 0 0 var(--space-1) 26px;
    border-left: 2px solid var(--amber);
  }
  .fixer label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .fixer .tk {
    flex: 1;
  }
  .fixer select,
  .fixer input {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    color: var(--text);
    font-size: var(--text-base);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
  }
  .tally {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-base);
  }
  .btn.sm {
    padding: var(--space-1) var(--space-2);
  }
  .btn.primary {
    border-color: var(--accent);
    color: var(--accent);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .muted {
    color: var(--muted);
  }
</style>
