<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Project-wide provider credentials. Write-only: we only ever know which secret names are
  // set, never their values. Keyless providers (Binance, Coinbase, Kraken, Yahoo) show as ready.
  // Master/detail: left pane lists providers, right pane edits the selected one's settings.
  import { histdataApi } from './api.js';
  import { t } from '$lib/i18n';

  let { providers = [], onchanged } = $props();

  // Remember which provider was open across refresh (per-browser).
  const SEL_KEY = 'otw.histdata.provider.v1';
  let selectedId = $state(
    (() => {
      try {
        return localStorage.getItem(SEL_KEY);
      } catch {
        return null;
      }
    })()
  );
  $effect(() => {
    try {
      if (selectedId) localStorage.setItem(SEL_KEY, selectedId);
    } catch {
      /* non-fatal */
    }
  });
  const selected = $derived(
    providers.find((p) => p.provider === selectedId) ?? providers[0] ?? null
  );

  // A provider is "ready" if keyless or all required secrets are set.
  const ready = (p) => !p.required_secrets.length || p.required_secrets.every((n) => p.set_secrets.includes(n));

  // Draft values per "provider:secretName".
  let drafts = $state({});

  async function save(provider, name) {
    const key = `${provider}:${name}`;
    const value = drafts[key];
    if (!value) return;
    await histdataApi.setSecret(provider, name, value);
    drafts[key] = '';
    onchanged?.();
  }
  async function clear(provider, name) {
    await histdataApi.deleteSecret(provider, name);
    onchanged?.();
  }
</script>

<div class="split">
  <ul class="list">
    {#each providers as p (p.provider)}
      <li>
        <button
          class="row"
          class:active={selected?.provider === p.provider}
          onclick={() => (selectedId = p.provider)}
        >
          <span class="dot" class:ok={ready(p)}></span>
          <span class="nm">{p.label}</span>
          {#if p.paid}<span class="tag paid">{$t('histdata.providers.paid')}</span>{/if}
          {#if !p.required_secrets.length}<span class="tag free">{$t('histdata.providers.keyless')}</span>{/if}
        </button>
      </li>
    {/each}
  </ul>

  <div class="detail">
    {#if !selected}
      <p class="note">{$t('histdata.providers.none')}</p>
    {:else}
      {@const p = selected}
      <div class="head">
        <a class="name" href={p.website} target="_blank" rel="noreferrer">{p.label}</a>
        {#if p.paid}<span class="tag paid">{$t('histdata.providers.paid')}</span>{/if}
        {#if !p.required_secrets.length}<span class="tag free">{$t('histdata.providers.keyless')}</span>{/if}
        {#if p.rate_limit}
          <span class="info" title={p.rate_limit}>ⓘ {$t('histdata.providers.rateLimits')}</span>
        {/if}
        {#if p.docs_url}
          <a class="docs" href={p.docs_url} target="_blank" rel="noreferrer" title={$t('histdata.providers.openDocs')}>{$t('histdata.providers.apiDocs')} <Icon name="external-link" size={11} /></a>
        {/if}
      </div>
      {#if p.rate_limit}<p class="rate">{p.rate_limit}</p>{/if}
      {#if p.required_secrets.length}
        <div class="secrets">
          {#each p.required_secrets as name (name)}
            {@const isSet = p.set_secrets.includes(name)}
            <div class="secret">
              <span class="sname">{name}</span>
              <span class="state" class:set={isSet}>{isSet ? $t('histdata.providers.set') : $t('histdata.providers.notSet')}</span>
              <input
                type="password"
                placeholder={isSet ? $t('histdata.providers.replacePlaceholder') : $t('histdata.providers.enterValuePlaceholder')}
                bind:value={drafts[`${p.provider}:${name}`]}
              />
              <button onclick={() => save(p.provider, name)}>{$t('common.save')}</button>
              {#if isSet}
                <button class="danger" onclick={() => clear(p.provider, name)}>{$t('common.clear')}</button>
              {/if}
            </div>
          {/each}
        </div>
      {:else}
        <p class="note">{$t('histdata.providers.noCredentials')}</p>
      {/if}
    {/if}
  </div>
</div>

<style>
  .split {
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: var(--space-4);
    align-items: start;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    list-style: none;
    border-right: 1px solid var(--border);
    padding-right: var(--space-3);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    cursor: pointer;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.active {
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--red);
    flex: none;
  }
  .dot.ok {
    background: var(--green);
  }
  .nm {
    font-weight: 600;
    flex: 1;
  }
  .detail {
    min-height: 120px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }
  .name {
    font-weight: 700;
    font-size: 1.05rem;
    color: var(--text);
    text-decoration: none;
  }
  .tag {
    font-size: 0.7rem;
    padding: 1px 6px;
    border-radius: var(--radius);
  }
  .paid {
    background: var(--amber);
    color: #000;
  }
  .free {
    background: var(--green);
    color: #000;
  }
  .info {
    font-size: 0.72rem;
    color: var(--muted);
    cursor: help;
    border-bottom: 1px dotted var(--muted);
  }
  .docs {
    font-size: 0.72rem;
    color: var(--accent);
    text-decoration: none;
    margin-left: auto;
  }
  .rate {
    color: var(--muted);
    font-size: 0.78rem;
    line-height: 1.4;
    margin-bottom: var(--space-3);
    max-width: 60ch;
  }
  .note {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .secrets {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .secret {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .sname {
    width: 110px;
    font-size: 0.85rem;
  }
  .state {
    width: 60px;
    font-size: 0.75rem;
    color: var(--red);
  }
  .state.set {
    color: var(--green);
  }
  input {
    flex: 1;
  }
  button {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    font-size: 0.8rem;
    color: var(--text);
    cursor: pointer;
  }
  .danger {
    color: var(--red);
    border-color: var(--red);
  }
</style>
