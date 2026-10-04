<script>
  // Broker book popover: pick an account, sync it, and the charts draw what it holds.
  //
  // The sync is an act, not a subscription: what is on screen is the snapshot the user asked
  // for, stamped with the moment it was read. Nothing here can open, change or cancel
  // anything; the whole feature is a set of horizontal lines.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import BrokerButton from '$lib/brokers/BrokerButton.svelte';
  import { isReady } from '$lib/brokers/api.js';
  import { t, locale } from '$lib/i18n';
  import { MODULE, bookSymbols, sameInstrument, unpricedPositions } from './book.js';

  let {
    accounts = [],
    accountId = '',
    book = null,
    syncedAt = 0,
    busy = false,
    error = '',
    /** Tickers the workspace currently has on screen, so a symbol that is in the book but on
     *  no chart can say so instead of quietly drawing nothing. */
    tickers = [],
    onsync,
    onclear,
    onpickaccount,
    onaccounts,
    onclose
  } = $props();

  const accountOpts = $derived(
    accounts.map((a) => ({
      value: a.id,
      label: isReady(a) ? a.name : `${a.name} · ${$t('brokers.incomplete')}`
    }))
  );
  const account = $derived(accounts.find((a) => a.id === accountId) ?? null);
  const cap = $derived(account?.capability ?? null);
  const counts = $derived({
    positions: book?.positions?.length ?? 0,
    orders: book?.orders?.length ?? 0
  });
  const unpriced = $derived(unpricedPositions(book));
  const symbols = $derived(
    bookSymbols(book).map((s) => ({ symbol: s, shown: tickers.some((tk) => sameInstrument(s, tk)) }))
  );
  const stamp = $derived(
    syncedAt
      ? new Date(syncedAt).toLocaleTimeString($locale, { hour: '2-digit', minute: '2-digit' })
      : ''
  );
</script>

<div class="popover">
  <div class="head">
    <span>{$t('histviz.book.title')}</span>
    <button class="x" title={$t('histviz.settings.close')} onclick={() => onclose?.()}>
      <Icon name="x" size={13} />
    </button>
  </div>

  {#if accounts.length}
    <div class="field">
      <span>{$t('histviz.book.account')}</span>
      <!-- Picking another account drops the overlay: lines from account A under a picker
           showing account B would be a lie the chart cannot correct by itself. -->
      <Dropdown
        value={accountId}
        options={accountOpts}
        ariaLabel={$t('histviz.book.account')}
        float
        onpick={(v) => onpickaccount?.(v)}
      />
    </div>
    {#if cap && !cap.positions && !cap.orders}
      <p class="note">{$t('histviz.book.nothingToRead', { broker: cap.label })}</p>
    {:else if cap && !cap.positions}
      <p class="note">{$t('histviz.book.ordersOnly', { broker: cap.label })}</p>
    {:else if cap && !cap.orders}
      <p class="note">{$t('histviz.book.positionsOnly', { broker: cap.label })}</p>
    {/if}

    <div class="acts">
      <Button variant="primary" onclick={() => onsync?.()} disabled={!accountId || busy}>
        <Icon name="refresh-cw" size={12} />
        {busy ? $t('histviz.book.syncing') : $t('histviz.book.sync')}
      </Button>
      {#if book}
        <Button variant="ghost" onclick={() => onclear?.()} disabled={busy}>
          {$t('histviz.book.clear')}
        </Button>
      {/if}
    </div>

    {#if book}
      <p class="stamp">
        {$t('histviz.book.synced', {
          positions: counts.positions,
          orders: counts.orders,
          time: stamp
        })}
      </p>
      {#if unpriced}
        <p class="note">{$t('histviz.book.unpriced', { count: unpriced })}</p>
      {/if}
      {#each book.warnings ?? [] as w}
        <p class="warn"><Icon name="alert-triangle" size={11} /> {w}</p>
      {/each}
      {#if symbols.length}
        <div class="syms">
          {#each symbols as s (s.symbol)}
            <span class="sym" class:on={s.shown} title={s.shown ? '' : $t('histviz.book.offChart')}>
              {s.symbol}
            </span>
          {/each}
        </div>
        <p class="hint">{$t('histviz.book.matchHint')}</p>
      {:else}
        <p class="note">{$t('histviz.book.empty')}</p>
      {/if}
    {/if}
  {:else}
    <p class="note">{$t('histviz.book.noAccount')}</p>
  {/if}

  <ErrorText {error} compact copyable />

  <div class="foot">
    <BrokerButton module={MODULE} size="sm" onchanged={() => onaccounts?.()} />
  </div>
</div>

<style>
  .popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: var(--z-dropdown);
    width: 300px;
    max-height: min(72vh, 620px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .head span {
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .x:hover {
    color: var(--text);
  }
  .field {
    display: grid;
    grid-template-columns: 1fr 150px;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--fs-body);
  }
  .field > span {
    color: var(--dim);
  }
  .acts {
    display: flex;
    gap: var(--space-2);
  }
  .note,
  .hint,
  .stamp {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.45;
  }
  .stamp {
    color: var(--dim);
  }
  .warn {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    margin: 0;
    font-size: var(--text-xs);
    color: var(--amber);
    line-height: 1.45;
  }
  .syms {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  /* A symbol on a chart right now is lit; the others are what to open to see their levels. */
  .sym {
    padding: 1px var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    font-family: var(--mono, ui-monospace, monospace);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .sym.on {
    color: var(--accent);
    border-color: var(--accent);
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    border-top: var(--hairline) solid var(--border);
    padding-top: var(--space-2);
  }
</style>
