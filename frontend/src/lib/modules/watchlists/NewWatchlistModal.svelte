<script>
  // Create a watchlist three ways: blank, from a curated template (seeded and quoted
  // server-side, so it opens populated), or mirroring a Portfolio Tracker portfolio.
  import Modal from '$lib/ui/Modal.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Field from '$lib/ui/Field.svelte';
  import { watchlistsApi } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), oncreated = () => {} } = $props();

  let tab = $state('blank');
  let error = $state('');
  let busy = $state(''); // '' | 'blank' | template id | portfolio id

  let name = $state('');
  let description = $state('');

  let templates = $state([]);
  let portfolios = $state([]);

  // Lazy-load the pickers the first time the modal opens.
  let loadedSources = $state(false);
  $effect(() => {
    if (!open || loadedSources) return;
    loadedSources = true;
    watchlistsApi.templates().then((r) => (templates = r)).catch(() => {});
    watchlistsApi.portfolios().then((r) => (portfolios = r)).catch(() => {});
  });

  function done(wl) {
    open = false;
    name = '';
    description = '';
    error = '';
    oncreated(wl);
  }

  async function createBlank() {
    if (busy || !name.trim()) return;
    busy = 'blank';
    error = '';
    try {
      done(await watchlistsApi.create({ name: name.trim(), description: description.trim() }));
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function createFromTemplate(tpl) {
    if (busy) return;
    busy = tpl.id;
    error = '';
    try {
      done(await watchlistsApi.create({ template: tpl.id }));
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function createFromPortfolio(pf) {
    if (busy) return;
    busy = pf.id;
    error = '';
    try {
      const wl = await watchlistsApi.create({ name: pf.name });
      await watchlistsApi.importPortfolio(wl.id, pf.id);
      done(wl);
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }
</script>

<Modal bind:open size="md" title={$t('watchlists.new.title')}>
  <Tabs
    bind:value={tab}
    ariaLabel={$t('watchlists.new.title')}
    tabs={[
      { id: 'blank', label: $t('watchlists.new.blank') },
      { id: 'template', label: $t('watchlists.new.template') },
      { id: 'portfolio', label: $t('watchlists.new.fromPortfolio') }
    ]}
  />

  <div class="body">
    <ErrorText error={error} copyable />

    {#if tab === 'blank'}
      <form class="blank" onsubmit={(e) => (e.preventDefault(), createBlank())}>
        <Field id="wl-new-name" label={$t('watchlists.detail.nameLabel')} required>
          <!-- svelte-ignore a11y_autofocus -->
          <input id="wl-new-name" autofocus placeholder={$t('watchlists.new.namePlaceholder')} bind:value={name} />
        </Field>
        <Field id="wl-new-desc" label={$t('watchlists.detail.descLabel')}>
          <input id="wl-new-desc" placeholder={$t('watchlists.new.descPlaceholder')} bind:value={description} />
        </Field>
        <div class="actions">
          <Button variant="primary" type="submit" loading={busy === 'blank'} disabled={!name.trim()}>
            {$t('watchlists.new.create')}
          </Button>
        </div>
      </form>
    {:else if tab === 'template'}
      <p class="hint">{$t('watchlists.new.templateHint')}</p>
      <div class="cards">
        {#each templates as tpl (tpl.id)}
          <button class="tplcard" disabled={!!busy} class:busy={busy === tpl.id} onclick={() => createFromTemplate(tpl)}>
            <span class="tpltop">
              <span class="tplname">{tpl.name}</span>
              <span class="tplcount">{$t('watchlists.new.symbolCount', { count: tpl.count })}</span>
            </span>
            <span class="tplsyms" title={tpl.symbols.join(' · ')}>{tpl.symbols.slice(0, 8).join(' · ')}{tpl.symbols.length > 8 ? ' …' : ''}</span>
            {#if busy === tpl.id}<span class="spinner" aria-hidden="true"></span>{/if}
          </button>
        {/each}
      </div>
    {:else}
      <p class="hint">{$t('watchlists.new.portfolioHint')}</p>
      {#if portfolios.length === 0}
        <p class="hint">{$t('watchlists.new.noPortfolios')}</p>
      {:else}
        <div class="cards">
          {#each portfolios as pf (pf.id)}
            <button class="tplcard" disabled={!!busy} class:busy={busy === pf.id} onclick={() => createFromPortfolio(pf)}>
              <span class="tpltop">
                <span class="tplname">{pf.name}</span>
                <span class="tplcount">{$t('watchlists.new.symbolCount', { count: pf.asset_count })}</span>
              </span>
              {#if pf.description}<span class="tplsyms" title={pf.description}>{pf.description}</span>{/if}
              {#if busy === pf.id}<span class="spinner" aria-hidden="true"></span>{/if}
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</Modal>

<style>
  .body {
    margin-top: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .blank {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .blank :global(input) {
    width: 100%;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .hint {
    font-size: 13px;
    color: var(--dim);
    margin: 0;
  }
  /* Tiles in a grid: one hairline frame each, nothing nested inside. */
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--space-2);
  }
  .tplcard {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
    text-align: left;
    background: transparent;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-3);
    color: var(--text);
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }
  .tplcard:hover:not(:disabled) {
    background: var(--surface-hover);
    border-color: var(--border-control);
  }
  .tplcard:disabled:not(.busy) {
    opacity: 0.6;
    cursor: default;
  }
  .tpltop {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    padding-right: var(--space-4);
  }
  .tplname {
    font-size: 13px;
    font-weight: 600;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tplcount {
    flex: none;
    font-size: 11px;
    color: var(--dim);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .tplsyms {
    font-size: 11px;
    color: var(--muted);
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spinner {
    position: absolute;
    top: var(--space-3);
    right: var(--space-3);
    width: 14px;
    height: 14px;
    border: 2px solid var(--accent);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 600ms linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
