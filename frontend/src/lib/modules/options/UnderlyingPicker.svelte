<script>
  // The underlying is picked from the symbol search of the chosen connector, never typed and
  // sent as is: the provider only ever sees a symbol it listed itself.
  import { optionsApi } from './api.js';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { t } from '$lib/i18n';

  // `kinds`: what the connector lists. Alpaca files ETFs under equity and has no index.
  let { connector = null, kinds = ['etf', 'equity', 'index'], assetType = $bindable('etf'), value = $bindable(null), onpick = () => {} } = $props();

  let q = $state(value?.symbol ?? '');
  let results = $state([]);
  let notes = $state([]);
  let open = $state(false);
  let busy = $state(false);
  let error = $state('');
  let active = $state(0);
  let menu = $state(null);
  let timer;
  let ticket = 0;

  function search(text) {
    clearTimeout(timer);
    if (!text.trim() || !connector) {
      results = [];
      notes = [];
      return;
    }
    timer = setTimeout(async () => {
      const mine = ++ticket;
      busy = true;
      error = '';
      try {
        const r = await optionsApi.symbols({ q: text.trim(), asset_type: assetType, connector_id: connector });
        if (mine !== ticket) return;
        results = r.results ?? [];
        notes = r.notes ?? [];
        active = 0;
        open = true;
      } catch (e) {
        if (mine === ticket) error = e.message;
      } finally {
        if (mine === ticket) busy = false;
      }
    }, 300);
  }

  function oninput(e) {
    q = e.currentTarget.value;
    if (value && q !== value.symbol) value = null;
    search(q);
  }

  function pick(hit) {
    value = { symbol: hit.symbol, name: hit.name, asset_type: hit.asset_type, exchange: hit.exchange };
    q = hit.symbol;
    open = false;
    onpick(value);
  }

  function setType(k) {
    if (assetType === k) return;
    assetType = k;
    value = null;
    search(q);
  }

  $effect(() => {
    if (!kinds.includes(assetType)) setType(kinds[0]);
  });

  function onkeydown(e) {
    if (e.key === 'Escape') {
      open = false;
    } else if (open && results.length && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      e.preventDefault();
      active = (active + (e.key === 'ArrowDown' ? 1 : -1) + results.length) % results.length;
      menu?.children[active]?.scrollIntoView({ block: 'nearest' });
    } else if (open && e.key === 'Enter' && results[active]) {
      e.preventDefault();
      pick(results[active]);
    }
  }
</script>

<div class="qp-field type">
  <span class="qp-label">{$t('options.underlying.type')}</span>
  <div class="qp-seg">
    {#each kinds as k (k)}
      <button class:active={assetType === k} onclick={() => setType(k)}>{$t(`options.underlying.${k}`)}</button>
    {/each}
  </div>
</div>
<div class="qp-field up" use:clickOutside={() => (open = false)}>
  <span class="qp-label">{$t('options.underlying.label')}</span>
  <div class="box">
    <input
      type="text"
      class="search"
      value={q}
      {oninput}
      onfocus={() => results.length && (open = true)}
      {onkeydown}
      placeholder={connector ? $t('options.underlying.placeholder') : $t('options.underlying.noSource')}
      disabled={!connector}
      aria-invalid={q && !value ? 'true' : undefined}
    />
    {#if busy}<span class="qp-hint state">…</span>{:else if value}<span class="qp-good state">✓</span>{/if}
    {#if open && (results.length || notes.length)}
      <div class="menu" role="listbox" bind:this={menu}>
        {#each results as r, i (r.connector_id + r.symbol + r.exchange)}
          <button role="option" class:on={i === active} aria-selected={value?.symbol === r.symbol} onpointerenter={() => (active = i)} onclick={() => pick(r)}>
            <span class="sym">{r.symbol}</span>
            <span class="name">{r.name ?? ''}</span>
            <span class="ex">{r.exchange ?? ''}</span>
          </button>
        {/each}
        {#if !results.length}<p class="qp-hint pad">{$t('options.underlying.none')}</p>{/if}
        {#each notes as n, i (i)}<p class="qp-hint pad qp-warn">{n.message}</p>{/each}
      </div>
    {/if}
    <!-- Out of the flow: the row is bottom-aligned, so a line here would lift the whole field. -->
    {#if error}<span class="under err" role="alert">{error}</span>{:else if q && !value && !busy && !open}<span class="under qp-hint">{$t('options.underlying.pickHint')}</span>{/if}
  </div>
</div>

<style>
  .qp-field.type {
    flex: 0 0 auto;
  }
  .qp-field.up {
    flex: 1 1 240px;
    max-width: 340px;
  }
  .box {
    position: relative;
  }
  .box input {
    width: 100%;
    padding-right: var(--space-8);
  }
  .state {
    position: absolute;
    right: var(--space-3);
    top: 50%;
    transform: translateY(-50%);
    pointer-events: none;
  }
  .under {
    position: absolute;
    top: calc(100% + 2px);
    left: 0;
    width: max-content;
    max-width: 100%;
    font-size: var(--fs-desc);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .menu {
    position: absolute;
    z-index: 20;
    top: calc(100% + var(--space-1));
    left: 0;
    width: max(100%, min(480px, calc(100vw - 2 * var(--space-6))));
    max-height: 336px;
    overflow-y: auto;
    padding: var(--space-1);
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    box-shadow: var(--shadow-3);
    display: flex;
    flex-direction: column;
  }
  .menu button {
    flex: none;
    display: grid;
    grid-template-columns: 64px minmax(0, 1fr) auto;
    gap: var(--space-3);
    align-items: center;
    min-height: 32px;
    text-align: left;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    padding: 0 var(--space-2);
    color: var(--text);
    cursor: pointer;
  }
  .menu button.on {
    background: var(--surface-2);
  }
  .sym {
    font-family: var(--mono);
    font-weight: var(--fw-medium);
  }
  .name {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ex {
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--fs-metric-label);
    letter-spacing: 0.04em;
  }
  .pad {
    padding: var(--space-2);
  }
  .err {
    color: var(--red);
  }
  /* Phone: the type and the search take a line each, the hint below flows under the field,
     and a result reads symbol and exchange on one line, the full name under them. */
  @media (max-width: 767px) {
    .qp-field.type,
    .qp-field.up {
      flex: 1 1 100%;
      max-width: none;
    }
    .qp-seg button,
    .box input {
      min-height: 44px;
    }
    .under {
      position: static;
      display: block;
      width: auto;
      margin-top: var(--space-1);
      white-space: normal;
      overflow-wrap: anywhere;
    }
    .menu {
      width: 100%;
    }
    .menu button {
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 0 var(--space-3);
      min-height: 44px;
      padding: var(--space-1) var(--space-2);
    }
    .sym {
      overflow-wrap: anywhere;
    }
    .name {
      grid-column: 1 / -1;
      grid-row: 2;
      white-space: normal;
      overflow-wrap: anywhere;
    }
  }
</style>
