<script>
  // Create a watchlist three ways: blank, from a curated template (seeded and quoted
  // server-side, so it opens populated), or mirroring a Portfolio Tracker portfolio.
  import Modal from '$lib/ui/Modal.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
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
        <!-- svelte-ignore a11y_autofocus -->
        <input autofocus placeholder={$t('watchlists.new.namePlaceholder')} bind:value={name} />
        <input placeholder={$t('watchlists.new.descPlaceholder')} bind:value={description} />
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
            <span class="tplname">{tpl.name}</span>
            <span class="tplcount">{$t('watchlists.new.symbolCount', { count: tpl.count })}</span>
            <span class="tplsyms">{tpl.symbols.slice(0, 8).join(' · ')}{tpl.symbols.length > 8 ? ' …' : ''}</span>
            {#if busy === tpl.id}<span class="spinner" aria-hidden="true"></span>{/if}
          </button>
        {/each}
      </div>
    {:else}
      <p class="hint">{$t('watchlists.new.portfolioHint')}</p>
      {#if portfolios.length === 0}
        <p class="hint muted">{$t('watchlists.new.noPortfolios')}</p>
      {:else}
        <div class="cards">
          {#each portfolios as pf (pf.id)}
            <button class="tplcard" disabled={!!busy} class:busy={busy === pf.id} onclick={() => createFromPortfolio(pf)}>
              <span class="tplname">{pf.name}</span>
              <span class="tplcount">{$t('watchlists.new.symbolCount', { count: pf.asset_count })}</span>
              {#if pf.description}<span class="tplsyms">{pf.description}</span>{/if}
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
    margin-top: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .blank {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .blank input {
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .hint {
    font-size: var(--text-sm);
    color: var(--muted);
  }
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
    text-align: left;
    background: transparent;
    border: 0.5px solid var(--border);
    border-radius: 0;
    padding: var(--space-3);
    color: var(--text);
    cursor: pointer;
  }
  .tplcard:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .tplcard:disabled:not(.busy) {
    opacity: 0.6;
    cursor: default;
  }
  .tplname {
    font-weight: var(--fw-medium);
  }
  .tplcount {
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
  }
  .tplsyms {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: var(--muted);
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
