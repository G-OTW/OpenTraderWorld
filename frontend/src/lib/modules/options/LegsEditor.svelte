<script>
  // The legs of the strategy being built. Strike and expiry come from the loaded chain; a
  // contract change reprices the leg from the chain (ask to buy, bid to sell) and refills its
  // IV, both still editable.
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { chainStrikes, chainExpiries, findRow } from './chain.js';
  import { legId, legPrice } from './store.svelte.js';
  import { price } from './columns.js';
  import { t } from '$lib/i18n';

  let { legs = $bindable([]), chain = null, spot = null } = $props();

  const rows = $derived(chain?.rows ?? []);
  const expiries = $derived(chainExpiries(rows));
  const kinds = $derived(['call', 'put', 'stock'].map((k) => ({ value: k, label: $t(`options.leg.${k}`) })));

  function strikeOptions(leg) {
    const list = chainStrikes(rows, leg.expiry);
    if (leg.strike != null && !list.includes(leg.strike)) list.push(leg.strike);
    return list.sort((a, b) => a - b).map((k) => ({ value: k, label: price(k) }));
  }
  function expiryOptions(leg) {
    const list = [...expiries];
    if (leg.expiry && !list.includes(leg.expiry)) list.push(leg.expiry);
    return list.sort().map((d) => ({ value: d, label: d }));
  }

  function reprice(leg) {
    if (leg.kind === 'stock') {
      leg.strike = null;
      leg.expiry = null;
      leg.iv = null;
      if (spot != null) leg.price = spot;
      return;
    }
    if (!leg.expiry) leg.expiry = expiries[0] ?? null;
    if (leg.strike == null && spot != null) {
      const list = chainStrikes(rows, leg.expiry);
      leg.strike = list.reduce((b, k) => (b == null || Math.abs(k - spot) < Math.abs(b - spot) ? k : b), null);
    }
    const row = findRow(rows, leg.expiry, leg.strike, leg.kind === 'call');
    if (row) {
      leg.price = legPrice(row, leg.qty);
      leg.iv = row.iv_mid ?? row.provider_iv ?? leg.iv;
    }
  }

  // Stock counts shares and an option counts contracts of 100: crossing between them keeps the
  // side and resets the size to one lot (100 shares, 1 contract), and drops the old price.
  function setKind(leg, kind) {
    const crossed = (leg.kind === 'stock') !== (kind === 'stock');
    leg.kind = kind;
    if (crossed) {
      leg.qty = (Math.sign(leg.qty) || 1) * (kind === 'stock' ? 100 : 1);
      leg.price = null;
    }
    reprice(leg);
  }

  function add(kind) {
    const leg = { id: legId(), kind, strike: null, expiry: null, qty: kind === 'stock' ? 100 : 1, price: null, iv: null };
    reprice(leg);
    legs.push(leg);
  }

  const ivPct = (leg) => (leg.iv == null ? '' : +(leg.iv * 100).toFixed(2));
  function setIv(leg, v) {
    leg.iv = v === '' || !Number.isFinite(Number(v)) ? null : Number(v) / 100;
  }
  function setQty(leg, v) {
    const n = Math.trunc(Number(v));
    if (!Number.isFinite(n) || n === 0) return;
    const flipped = Math.sign(n) !== Math.sign(leg.qty);
    leg.qty = n;
    if (flipped) reprice(leg);
  }
</script>

<section class="qp-block">
  <div class="head">
    <h3>{$t('options.legs.title')}</h3>
    <div class="actions">
      <button class="ghost" onclick={() => add('call')}><Icon name="plus" size={14} /> {$t('options.leg.call')}</button>
      <button class="ghost" onclick={() => add('put')}><Icon name="plus" size={14} /> {$t('options.leg.put')}</button>
      <button class="ghost" onclick={() => add('stock')}><Icon name="plus" size={14} /> {$t('options.leg.stock')}</button>
      <button class="ghost danger" onclick={() => (legs = [])} disabled={!legs.length}>{$t('options.legs.clear')}</button>
    </div>
  </div>
  {#if legs.length}
    <div class="qp-scroll">
      <table class="qp-table legs">
        <thead>
          <tr>
            <th>{$t('options.legs.kind')}</th>
            <th>{$t('options.col.strike')}</th>
            <th>{$t('options.legs.expiry')}</th>
            <th class="num">{$t('options.legs.qty')}</th>
            <th class="num">{$t('options.legs.price')}</th>
            <th class="num">{$t('options.legs.iv')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each legs as leg (leg.id)}
            <tr class:short={leg.qty < 0}>
              <td data-label={$t('options.legs.kind')}><Dropdown value={leg.kind} options={kinds} float ariaLabel={$t('options.legs.kind')} onpick={(v) => setKind(leg, v)} /></td>
              <td data-label={$t('options.col.strike')}>
                {#if leg.kind !== 'stock'}
                  <Dropdown value={leg.strike} options={strikeOptions(leg)} float searchable ariaLabel={$t('options.col.strike')} onpick={(v) => ((leg.strike = v), reprice(leg))} />
                {/if}
              </td>
              <td data-label={$t('options.legs.expiry')}>
                {#if leg.kind !== 'stock'}
                  <Dropdown value={leg.expiry} options={expiryOptions(leg)} float ariaLabel={$t('options.legs.expiry')} onpick={(v) => ((leg.expiry = v), reprice(leg))} />
                {/if}
              </td>
              <td class="num" data-label={$t('options.legs.qty')}><input type="number" step="1" value={leg.qty} onchange={(e) => setQty(leg, e.currentTarget.value)} aria-label={$t('options.legs.qty')} /></td>
              <td class="num" data-label={$t('options.legs.price')}><input type="number" step="0.01" min="0" bind:value={leg.price} aria-label={$t('options.legs.price')} /></td>
              <td class="num" data-label={$t('options.legs.iv')}>
                {#if leg.kind !== 'stock'}
                  <input type="number" step="0.1" min="0" value={ivPct(leg)} onchange={(e) => setIv(leg, e.currentTarget.value)} aria-label={$t('options.legs.iv')} />
                {/if}
              </td>
              <td class="rm"><button class="icon danger-hover" onclick={() => (legs = legs.filter((l) => l.id !== leg.id))} aria-label={$t('options.legs.remove')}><Icon name="x" size={14} /></button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="qp-hint">{$t('options.legs.hint')}</p>
  {:else}
    <p class="qp-hint">{$t('options.legs.empty')}</p>
  {/if}
</section>

<style>
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .legs td {
    vertical-align: middle;
  }
  .legs td:nth-child(1) {
    min-width: 100px;
  }
  .legs td:nth-child(2),
  .legs td:nth-child(3) {
    min-width: 130px;
  }
  .legs input {
    width: 96px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  tr.short td:first-child {
    box-shadow: inset 2px 0 0 var(--red);
  }
  tr:not(.short) td:first-child {
    box-shadow: inset 2px 0 0 var(--green);
  }
  /* Phone: each leg is a card of labelled fields, two by two, the side marked on its edge.
     The header row stays for screen readers only. */
  @media (max-width: 767px) {
    .actions button {
      min-height: 44px;
      height: auto;
    }
    .legs,
    .legs tbody {
      display: block;
    }
    .legs thead {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .legs tbody tr {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--space-2) var(--space-3);
      padding: var(--space-3);
      border-bottom: var(--hairline) solid var(--border);
      box-shadow: inset 2px 0 0 var(--green);
    }
    .legs tbody tr.short {
      box-shadow: inset 2px 0 0 var(--red);
    }
    .legs tbody tr:hover {
      background: none;
    }
    .legs td,
    .legs td:nth-child(1),
    .legs td:nth-child(2),
    .legs td:nth-child(3) {
      min-width: 0;
      padding: 0;
      border: none;
      text-align: left;
    }
    tr.short td:first-child,
    tr:not(.short) td:first-child {
      box-shadow: none;
    }
    .legs td[data-label]::before {
      content: attr(data-label);
      display: block;
      margin-bottom: var(--space-1);
      font-size: var(--fs-metric-label);
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--muted);
    }
    .legs td[data-label]:empty {
      display: none;
    }
    .legs input {
      width: 100%;
      min-height: 44px;
    }
    .legs td :global(.trigger) {
      min-height: 44px;
    }
    .legs td.rm {
      grid-column: 1 / -1;
      display: flex;
      justify-content: flex-end;
    }
    .legs td.rm button.icon {
      width: 44px;
      height: 44px;
    }
  }
</style>
