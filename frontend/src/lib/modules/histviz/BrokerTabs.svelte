<script>
  // The order panel's account tabs: Positions, Orders, Account and Logs of the accounts with
  // trading on, all of them or the one picked in the broker filter.
  //
  // Positions and orders are the broker's own, as the page last read them (it follows every
  // trading account, see `follow` on the page); an order this page is still sending shows
  // before the broker lists it. Logs are the order audit log, read from the server while the
  // tab is open, and so is the Account tab's money (equity, margin, what is available), the same
  // figures for every broker.
  import { onDestroy } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';
  import { brokersApi } from '$lib/brokers/api.js';
  import AccountSummary from '$lib/brokers/AccountSummary.svelte';
  import { poll } from '$lib/brokers/poll.js';
  import { makePriceFmt, priceDigitsOfValues } from './format.js';

  let {
    tab = 'positions', // positions | orders | account | logs
    accounts = [], // the trading accounts
    books = {}, // accountId -> { book, at, error }
    livePrices = {}, // `${accountId}|${symbol}` -> a chart's live price
    trailPrices = {}, // `${accountId}|${orderId}` -> where a trailing stop trails now, as a chart works it out
    marks = {}, // accountId -> { native_id: the broker's live mark }
    symbol = '', // the pane's pair: the Account tab shows its two assets where the broker holds them by name
    broker = $bindable(''), // '' = every account
    orders = [], // this page's own orders (their status while sending)
    oncancelline, // (localId)
    oncancelbroker, // (accountId, orderId)
    onresendunsent, // (accountId, clientIds): send again orders the broker never received
    ondismissunsent, // (accountId, clientIds): drop them
    oneditline, // (localId): change it in the ticket
    oneditbroker, // (accountId, orderId)
    oncloseposition, // (accountId, position): close it at the market
    onprotectposition, // (accountId, position): open the ticket with exits for it
    oncancelall
  } = $props();

  const shownAccounts = $derived(broker ? accounts.filter((a) => a.id === broker) : accounts);
  const nameOf = (id) => accounts.find((a) => a.id === id)?.name ?? '';
  const many = $derived(shownAccounts.length > 1);
  const filterOptions = $derived([
    { value: '', label: $t('histviz.trade.allBrokers') },
    ...accounts.map((a) => ({ value: a.id, label: a.name }))
  ]);
  const errors = $derived(shownAccounts.map((a) => (books[a.id]?.error ? `${a.name}: ${books[a.id].error}` : '')).filter(Boolean));

  const positions = $derived(
    shownAccounts.flatMap((a) => (books[a.id]?.book?.positions ?? []).map((p) => live({ ...p, accountId: a.id })))
  );
  /** A position marked live, the way the chart's own label is: at the price of a chart showing
   *  its instrument, else at the broker's live mark. Without either it keeps the figures of
   *  the last book read. */
  function live(p) {
    const last = livePrices[`${p.accountId}|${p.symbol}`] ?? (p.native_id ? marks[p.accountId]?.[p.native_id] : null);
    const avg = Number(p.avg_price);
    if (!(last > 0) || !(avg > 0)) return p;
    const dir = p.side === 'short' ? -1 : 1;
    const mult = Number(p.multiplier) > 0 ? Number(p.multiplier) : 1;
    return { ...p, market_price: last, unrealized_pnl: (last - avg) * Math.abs(Number(p.qty) || 0) * mult * dir };
  }
  /** The broker's working orders, then this page's orders it does not list yet. */
  const rows = $derived.by(() => {
    const out = [];
    const listed = new Set();
    for (const a of shownAccounts) {
      for (const o of books[a.id]?.book?.orders ?? []) {
        listed.add(`${a.id}:${o.id}`);
        const mine = orders.find((m) => m.account === a.id && String(m.ref) === String(o.id));
        out.push({
          key: `b:${a.id}:${o.id}`,
          accountId: a.id,
          id: String(o.id),
          symbol: o.symbol,
          side: o.side,
          type: o.order_type,
          qty: o.qty,
          filled: o.filled_qty,
          limit: o.limit_price,
          stop: trailPrices[`${a.id}|${o.id}`] ?? o.stop_price,
          // The level is the chart's estimate: the broker reports only where it was placed.
          estimated: trailPrices[`${a.id}|${o.id}`] != null,
          status: mine?.status === 'sending' ? 'sending' : 'working'
        });
      }
    }
    const ids = new Set(shownAccounts.map((a) => a.id));
    for (const m of orders) {
      if ((broker && !ids.has(m.account)) || (m.ref && listed.has(`${m.account}:${m.ref}`))) continue;
      if (m.ref && m.status === 'working') continue; // listed on the next read
      out.push({
        key: `l:${m.id}`,
        local: m.id,
        accountId: m.account,
        id: m.ref ?? '',
        symbol: m.symbol,
        side: m.side,
        type: $t(`histviz.trade.type.${m.type}`),
        qty: m.qty,
        filled: 0,
        limit: m.limit,
        stop: m.stop,
        status: m.status ?? 'draft'
      });
    }
    return out;
  });

  /** Orders the broker answered it never received, one ticket per row (an entry and its
   *  exits): sent again together or dropped together, never left to block silently. */
  const unsent = $derived.by(() => {
    const out = [];
    for (const a of shownAccounts) {
      const legs = books[a.id]?.book?.unsent ?? [];
      const ids = new Set(legs.map((l) => l.client_id));
      for (const root of legs.filter((l) => !l.parent || !ids.has(l.parent))) {
        const group = [root, ...legs.filter((l) => l.parent === root.client_id)];
        out.push({ key: `${a.id}:${root.client_id}`, accountId: a.id, symbol: root.ticker, ids: group.map((l) => l.client_id), legs: group.map((l) => l.request ?? {}) });
      }
    }
    return out;
  });
  /** One leg as the row names it: side, type, size and its price. */
  const legName = (r) =>
    [String(r.side ?? '').toUpperCase(), String(r.type ?? '').replace('_', ' '), r.qty, Number(r.limit) > 0 ? `@ ${r.limit}` : Number(r.stop) > 0 ? `@ ${r.stop}` : '']
      .filter(Boolean)
      .join(' ');
  let unsentBusy = $state('');
  async function unsentAction(u, fn) {
    unsentBusy = u.key;
    try {
      await fn?.(u.accountId, u.ids);
    } finally {
      unsentBusy = '';
    }
  }

  // ── Account ──
  // Read while the tab is open and the page visible, every few seconds (see `poll.js`).
  let funds = $state({}); // accountId -> { rows, error }
  async function readMoney() {
    const ids = shownAccounts.map((a) => a.id);
    const next = {};
    await Promise.all(
      ids.map(async (id) => {
        try {
          next[id] = { rows: await brokersApi.account(id, 'histviz', symbol || null), error: '' };
        } catch (e) {
          next[id] = { rows: funds[id]?.rows ?? [], error: e.message };
        }
      })
    );
    funds = next;
  }
  $effect(() => {
    void broker;
    void symbol;
    void shownAccounts.length;
    if (tab !== 'account') return;
    return poll(readMoney);
  });

  // ── Logs ──
  let logs = $state([]);
  let logError = $state('');
  let logTimer = null;
  async function readLogs() {
    clearTimeout(logTimer);
    try {
      logs = await brokersApi.orderLog(broker || null, 200);
      logError = '';
    } catch (e) {
      logError = e.message;
    }
    logTimer = setTimeout(readLogs, 5000);
  }
  $effect(() => {
    void broker;
    if (tab !== 'logs') return;
    readLogs();
    return () => clearTimeout(logTimer);
  });
  onDestroy(() => clearTimeout(logTimer));

  /** What a logged call asked, in one line. */
  function asked(e) {
    const r = e.request ?? {};
    if (e.action === 'place') {
      return (r.legs ?? [])
        .map((l) => {
          const px = [l.limit, l.stop].filter((v) => v != null).join(' / ');
          return `${$t(`histviz.trade.${l.side}`)} ${$t(`histviz.trade.type.${l.type}`)} ${l.qty}${px ? ` @ ${px}` : ''}`;
        })
        .join(', ')
        .concat(r.ticker ? ` · ${r.ticker}` : '');
    }
    if (e.action === 'modify') return `#${r.order} → ${[r.limit, r.stop].filter((v) => v != null).join(' / ')}`;
    if (e.action === 'cancel') return `#${r.order}`;
    if (e.action === 'close') return `${$t('histviz.trade.contract')} ${r.native_id}`;
    if (e.action === 'reconcile') {
      const s = r?.subject ?? {};
      return `${$t(`histviz.trade.reconcile.${r?.kind}`)} · ${s.symbol ?? s.client_id ?? s.id ?? s.position ?? ''}`;
    }
    return '';
  }
  /** What the broker answered, in one line. */
  function answered(e) {
    const r = e.response;
    if (!e.ok) return r?.error ?? '';
    if (e.action === 'place') return (Array.isArray(r) ? r : []).map((p) => `#${p.id}`).join(' ');
    if (e.action === 'cancel_all') return $t('histviz.trade.cancelledN', { n: r ?? 0 });
    if (e.action === 'close') return r?.id ? `#${r.id}` : 'OK';
    if (e.action === 'reconcile') {
      if (r?.before != null) return `${r.before} → ${r.after}`;
      if (r?.was) return `${r.was} → ${r.now}`;
      return r?.error ?? r?.why ?? '';
    }
    return 'OK';
  }
  const when = (iso) => new Date(iso).toLocaleTimeString();
  const amount = (v) => Math.abs(v).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const money = (v) => `${v > 0 ? '+' : v < 0 ? '−' : ''}${Math.abs(v).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
  const fmtRow = (o) => makePriceFmt(priceDigitsOfValues([o.limit, o.stop, o.avg_price].map(Number).filter((v) => v > 0)));
  const opened = (p) => (p.opened_at ? new Date(p.opened_at).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' }) : '–');
  const fee = (p) => (p.entry_fee != null ? `${amount(Number(p.entry_fee))} ${p.fee_currency ?? ''}` : '–');
  // The venue's leverage and margin mode on the instrument (Binance USDⓈ-M), read only.
  const lever = (p) =>
    [p.leverage != null ? `${Number(p.leverage)}×` : '', p.margin_mode ? $t(`histviz.trade.marginMode.${p.margin_mode}`) : ''].filter(Boolean).join(' ') || '–';
  const leveraged = $derived(positions.some((p) => p.leverage != null || p.margin_mode));

  // Position columns sized in mono `ch` to their full value plus a margin, from what only a
  // fill moves: a live tick never resizes them, and a narrow panel scrolls sideways instead.
  const posCols = $derived.by(() => {
    const w = (head, vals) => Math.max(head.length, ...vals.map((v) => String(v).length)) + 2;
    const avg = positions.map((p) => (Number(p.avg_price) > 0 ? fmtRow({ limit: p.market_price, avg_price: p.avg_price })(Number(p.avg_price)) : ''));
    return [
      ...(many ? [w($t('histviz.trade.account'), positions.map((p) => nameOf(p.accountId)))] : []),
      w($t('histviz.trade.symbol'), positions.map((p) => p.symbol)),
      w($t('histviz.trade.side'), [$t('histviz.book.side.long'), $t('histviz.book.side.short')]),
      w($t('histviz.trade.qty'), positions.map((p) => p.qty)),
      w($t('histviz.trade.avgPrice'), avg),
      // The market price may show more decimals than the fill: two characters of headroom.
      w($t('histviz.trade.marketPrice'), avg.map((s) => `${s}00`)),
      // Room for a six-digit P&L, whatever it reads now.
      w($t('histviz.trade.pnl'), positions.map((p) => `${money(-999999.99)} ${p.currency ?? ''}`)),
      w($t('histviz.trade.opened'), positions.map(opened)),
      w($t('histviz.trade.fees'), positions.map(fee)),
      ...(leveraged ? [w($t('histviz.trade.leverage'), positions.map(lever))] : []),
    ];
  });
</script>

<div class="bt">
  <div class="filter">
    <Dropdown bind:value={broker} options={filterOptions} ariaLabel={$t('histviz.trade.brokerFilter')} float />
  </div>
  {#each errors as e (e)}
    <p class="note err"><Icon name="alert-triangle" size={12} /> {e}</p>
  {/each}

  {#if tab === 'positions'}
    {#if !positions.length}
      <p class="note">{$t('histviz.trade.noPositions')}</p>
    {:else}
      <div class="scroll">
      <table class="fixed">
        <colgroup>
          {#each posCols as n, i (i)}<col style:width="calc({n}ch + 2 * var(--space-2))" />{/each}
          <col style:width="32px" />
        </colgroup>
        <thead>
          <tr>
            {#if many}<th>{$t('histviz.trade.account')}</th>{/if}
            <th>{$t('histviz.trade.symbol')}</th>
            <th>{$t('histviz.trade.side')}</th>
            <th class="n">{$t('histviz.trade.qty')}</th>
            <th class="n">{$t('histviz.trade.avgPrice')}</th>
            <th class="n">{$t('histviz.trade.marketPrice')}</th>
            <th class="n">{$t('histviz.trade.pnl')}</th>
            <th>{$t('histviz.trade.opened')}</th>
            <th class="n">{$t('histviz.trade.fees')}</th>
            {#if leveraged}<th title={$t('histviz.trade.leverageHint')}>{$t('histviz.trade.leverage')}</th>{/if}
            <th class="acts"></th>
          </tr>
        </thead>
        <tbody>
          {#each positions as p (`${p.accountId}:${p.symbol}:${p.side}:${p.account}`)}
            {@const f = fmtRow({ limit: p.market_price, avg_price: p.avg_price })}
            <tr>
              {#if many}<td class="muted">{nameOf(p.accountId)}</td>{/if}
              <td class="mono">{p.symbol}</td>
              <td class={p.side === 'short' ? 'sell' : 'buy'}>{$t(`histviz.book.side.${p.side === 'short' ? 'short' : 'long'}`)}</td>
              <td class="n mono">{p.qty}</td>
              <td class="n mono">{Number(p.avg_price) > 0 ? f(Number(p.avg_price)) : ''}</td>
              <td class="n mono">{Number(p.market_price) > 0 ? f(Number(p.market_price)) : ''}</td>
              <td class="n mono" class:buy={p.unrealized_pnl > 0} class:sell={p.unrealized_pnl < 0}>
                {p.unrealized_pnl != null ? `${money(p.unrealized_pnl)} ${p.currency ?? ''}` : ''}
              </td>
              <td class="mono muted" title={p.opened_at ? new Date(p.opened_at).toLocaleString() : ''}>{opened(p)}</td>
              <td class="n mono muted">{fee(p)}</td>
              {#if leveraged}<td class="mono muted" title={$t('histviz.trade.leverageHint')}>{lever(p)}</td>{/if}
              <td class="acts">
                <div>
                  {#if onprotectposition}
                    <button
                      class="ibtn"
                      title={$t('histviz.trade.protectPosition')}
                      aria-label={$t('histviz.trade.protectPosition')}
                      onclick={() => onprotectposition(p.accountId, { symbol: p.symbol, side: p.side, qty: p.qty, account: p.account })}
                    >
                      <Icon name="shield" size={11} />
                    </button>
                  {/if}
                  {#if p.native_id}
                    <button
                      class="ibtn close"
                      title={$t('histviz.trade.closePosition')}
                      aria-label={$t('histviz.trade.closePosition')}
                      onclick={() =>
                        oncloseposition?.(p.accountId, { nativeId: p.native_id, account: p.account, symbol: p.symbol, side: p.side, qty: p.qty })}
                    >
                      <Icon name="x" size={11} />
                    </button>
                  {/if}
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      </div>
    {/if}
  {:else if tab === 'orders'}
    {#each unsent as u (u.key)}
      <div class="unsent">
        <Icon name="alert-triangle" size={12} />
        <div class="what">
          <span>{$t('histviz.trade.unsent', { symbol: u.symbol })}{#if many} · {nameOf(u.accountId)}{/if}</span>
          <span class="mono muted">{u.legs.map(legName).join(' · ')}</span>
        </div>
        <button type="button" class="ghost" disabled={unsentBusy === u.key} onclick={() => unsentAction(u, onresendunsent)}>{$t('histviz.trade.unsentResend')}</button>
        <button type="button" class="ghost" disabled={unsentBusy === u.key} onclick={() => unsentAction(u, ondismissunsent)}>{$t('histviz.trade.unsentDismiss')}</button>
      </div>
    {/each}
    {#if !rows.length && !unsent.length}
      <p class="note">{$t('histviz.trade.noOrders')}</p>
    {:else}
      <div class="scroll">
      <table>
        <thead>
          <tr>
            {#if many}<th>{$t('histviz.trade.account')}</th>{/if}
            <th>{$t('histviz.trade.symbol')}</th>
            <th>{$t('histviz.trade.side')}</th>
            <th>{$t('histviz.trade.typeLabel')}</th>
            <th class="n">{$t('histviz.trade.qty')}</th>
            <th class="n">{$t('histviz.trade.limitPrice')}</th>
            <th class="n">{$t('histviz.trade.stopPrice')}</th>
            <th>{$t('histviz.trade.status')}</th>
            <th class="acts"></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as o (o.key)}
            {@const f = fmtRow(o)}
            <tr>
              {#if many}<td class="muted">{nameOf(o.accountId)}</td>{/if}
              <td class="mono">{o.symbol}</td>
              <td class={o.side}>{$t(`histviz.trade.${o.side}`)}</td>
              <td>{o.type}</td>
              <td class="n mono">{o.filled ? `${o.filled}/${o.qty}` : o.qty}</td>
              <td class="n mono">{Number(o.limit) > 0 ? f(Number(o.limit)) : ''}</td>
              <td class="n mono" title={o.estimated ? $t('histviz.trade.trailEstimated') : null}>{Number(o.stop) > 0 ? `${o.estimated ? '≈ ' : ''}${f(Number(o.stop))}` : ''}</td>
              <td class="muted">{$t(`histviz.trade.state.${o.status}`)}{#if o.id} <span class="mono">#{o.id}</span>{/if}</td>
              <td class="acts">
                <div>
                  <button
                    class="ibtn"
                    title={$t('histviz.trade.editOrder')}
                    aria-label={$t('histviz.trade.editOrder')}
                    disabled={o.status !== 'working' || !o.id}
                    onclick={() => (o.local ? oneditline?.(o.local) : oneditbroker?.(o.accountId, o.id))}
                  >
                    <Icon name="pencil" size={11} />
                  </button>
                  <button
                    class="ibtn close"
                    title={$t('histviz.trade.cancel')}
                    aria-label={$t('histviz.trade.cancel')}
                    disabled={o.status === 'sending'}
                    onclick={() => (o.local ? oncancelline?.(o.local) : oncancelbroker?.(o.accountId, o.id))}
                  >
                    <Icon name="x" size={11} />
                  </button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      </div>
      <div class="foot">
        <button type="button" class="ghost" onclick={() => oncancelall?.()}>{$t('histviz.trade.cancelAll')}</button>
      </div>
    {/if}
  {:else if tab === 'account'}
    {#if !shownAccounts.length}
      <p class="note">{$t('histviz.trade.noAccounts')}</p>
    {/if}
    {#each shownAccounts as a (a.id)}
      {@const m = funds[a.id]}
      <section class="acct">
        {#if many || m?.rows?.length > 1}<h4>{a.name}</h4>{/if}
        {#if m?.error}<p class="note err"><Icon name="alert-triangle" size={12} /> {m.error}</p>{/if}
        {#if !m}
          <p class="note">{$t('histviz.trade.reading')}</p>
        {:else}
          {#each m.rows as row (row.account)}
            {#if m.rows.length > 1}<span class="sub mono">{row.account}</span>{/if}
            <AccountSummary summary={row} />
          {/each}
        {/if}
      </section>
    {/each}
  {:else}
    {#if logError}<p class="note err">{logError}</p>{/if}
    {#if !logs.length}
      <p class="note">{$t('histviz.trade.noLogs')}</p>
    {:else}
      <table>
        <thead>
          <tr>
            <th>{$t('histviz.trade.time')}</th>
            {#if many}<th>{$t('histviz.trade.account')}</th>{/if}
            <th>{$t('histviz.trade.action')}</th>
            <th>{$t('histviz.trade.request')}</th>
            <th>{$t('histviz.trade.answer')}</th>
            <th class="n">{$t('histviz.trade.latency')}</th>
          </tr>
        </thead>
        <tbody>
          {#each logs as e (e.id)}
            <tr>
              <td class="mono muted" title={new Date(e.at).toLocaleString()}>{when(e.at)}</td>
              {#if many}<td class="muted">{e.account}</td>{/if}
              <td>{$t(`histviz.trade.log.${e.action}`)}</td>
              <td class="mono wrap">{asked(e)}</td>
              <td class="wrap" class:sell={!e.ok}>{answered(e)}</td>
              <td class="n mono">{e.latency_ms} ms</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}
</div>

<style>
  .acct {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    margin-top: var(--space-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .acct h4 {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .sub {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .bt {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .filter {
    max-width: 220px;
  }
  .note {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    color: var(--muted);
  }
  .note.err {
    color: var(--red);
  }
  .unsent {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--amber);
    border-radius: var(--radius);
    color: var(--amber);
  }
  .unsent .what {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    gap: 2px;
  }
  .ghost:disabled {
    opacity: 0.4;
    cursor: default;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    padding: 2px var(--space-2);
    text-align: left;
    white-space: nowrap;
    border-bottom: 1px solid var(--border);
  }
  td.wrap {
    white-space: normal;
  }
  /* Too wide for the panel: the table scrolls sideways, the action column stays pinned right. */
  .scroll {
    overflow-x: auto;
  }
  /* Live P&L ticks change cell text widths; a fixed layout keeps the columns still. */
  table.fixed {
    table-layout: fixed;
  }
  /* `ch` read in the mono face: never narrower than a proportional label of the same length. */
  col {
    font-family: var(--mono);
  }
  .acts {
    position: sticky;
    right: 0;
    background: var(--surface);
  }
  th {
    color: var(--muted);
    font-weight: 500;
  }
  .n {
    text-align: right;
  }
  .mono {
    font-family: var(--mono);
  }
  .muted {
    color: var(--muted);
  }
  .buy {
    color: var(--accent);
  }
  .sell {
    color: var(--red);
  }
  .ibtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .ibtn:hover:not(:disabled) {
    background: var(--surface);
    color: var(--text);
  }
  .ibtn.close:hover:not(:disabled) {
    color: var(--red);
  }
  .acts > div {
    display: flex;
    gap: 2px;
  }
  .ibtn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
  }
  .ghost {
    padding: 2px var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    font-size: var(--text-xs);
    cursor: pointer;
  }
</style>
