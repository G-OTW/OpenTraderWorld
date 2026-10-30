<script>
  // "How do I write this symbol?": the small (?) next to every box where a symbol is typed
  // by hand. One shared table (symbolFormats.js), one modal, every module: the same
  // instrument is BTCUSDT at Binance, XBTUSD at Kraken and BTC-USD at Coinbase, and getting
  // that wrong is the most common reason a download or an enrichment comes back empty.
  //
  // `provider` (a connector's provider, or a broker account's broker) opens the modal on
  // that house's card; without one the list opens on the first provider.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { SYMBOL_FORMATS, formatFor } from '$lib/ui/symbolFormats.js';
  import { t } from '$lib/i18n';

  let { provider = '', size = 13 } = $props();

  let open = $state(false);
  let picked = $state('');

  // The provider the caller knows about wins each time the helper is opened, so the modal
  // never reopens on whatever was browsed last.
  function show() {
    picked = formatFor(provider)?.provider ?? picked ?? '';
    open = true;
  }

  const current = $derived(
    SYMBOL_FORMATS.find((f) => f.provider === picked) ?? SYMBOL_FORMATS[0]
  );
</script>

<button
  type="button"
  class="symhelp"
  onclick={show}
  aria-label={$t('symbolHelp.aria')}
  title={$t('symbolHelp.aria')}
>
  <Icon name="help-circle" {size} />
</button>

<Modal bind:open title={$t('symbolHelp.title')} size="lg">
  <p class="intro">{$t('symbolHelp.intro')}</p>

  <div class="chips" role="tablist" aria-label={$t('symbolHelp.providers')}>
    {#each SYMBOL_FORMATS as f (f.provider)}
      <button
        type="button"
        role="tab"
        class="chip"
        class:on={f.provider === current.provider}
        aria-selected={f.provider === current.provider}
        onclick={() => (picked = f.provider)}
      >
        {f.label}
      </button>
    {/each}
  </div>

  <div class="card">
    <div class="head">
      <span class="name">{current.label}</span>
      <a class="docs" href={current.docs} target="_blank" rel="noreferrer noopener">
        {$t('symbolHelp.docs')}<Icon name="external-link" size={11} />
      </a>
    </div>
    <ul class="rows">
      {#each current.rows as r (r.asset + r.examples[0])}
        <li>
          <span class="asset">{$t(`symbolHelp.asset.${r.asset}`)}</span>
          <span class="ex">
            {#each r.examples as e (e)}<code>{e}</code>{/each}
          </span>
          <span class="hint">{$t(`symbolHelp.hint.${r.hint}`)}</span>
        </li>
      {/each}
    </ul>
  </div>

  <p class="foot">{$t('symbolHelp.foot')}</p>
</Modal>

<style>
  /* The trigger is a glyph on a label line, never a control of its own: no frame, no box. */
  .symhelp {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    padding: 0;
    margin-left: var(--space-1);
    color: var(--muted);
    cursor: pointer;
    vertical-align: middle;
  }
  .symhelp:hover {
    color: var(--text);
  }
  .intro {
    margin: 0 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: 1.5;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin-bottom: var(--space-3);
  }
  .chip {
    background: transparent;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text);
  }
  .chip.on {
    color: var(--text);
    border-color: var(--accent);
  }
  .card {
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 0.5px solid var(--border);
  }
  .name {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .docs {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--muted);
    text-decoration: none;
  }
  .docs:hover {
    color: var(--accent);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .rows li {
    display: grid;
    grid-template-columns: 90px minmax(0, 1fr);
    gap: 2px var(--space-3);
    padding: var(--space-2) var(--space-3);
  }
  .rows li + li {
    border-top: 0.5px solid var(--border);
  }
  .asset {
    grid-row: span 2;
    font-size: var(--text-xs);
    color: var(--muted);
    padding-top: 2px;
  }
  .ex {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .ex code {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--text);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: 1px var(--space-1);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.5;
  }
  .foot {
    margin: var(--space-3) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.5;
  }
  @media (max-width: 560px) {
    .rows li {
      grid-template-columns: 1fr;
    }
    .asset {
      grid-row: auto;
    }
  }
</style>
