<script>
  // Align this portfolio on what a broker account actually holds.
  //
  // A balance sheet, not a trade history: the account says what it owns, and the import
  // writes the one operation that makes the ledger agree. Every line is the user's call,
  // because the difference between the broker and the book is a transaction that happened
  // on a day nobody here knows.
  //
  // The price is the honest part. Interactive Brokers publishes a cost basis and it arrives
  // filled in; a crypto exchange publishes a quantity and nothing else, so the field starts
  // at today's price and says so, out loud, above the table.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import BrokerButton from '$lib/brokers/BrokerButton.svelte';
  import { brokersApi, isReady } from '$lib/brokers/api.js';
  import ImportSymbols from './ImportSymbols.svelte';
  import { portfoliosApi, fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  const MODULE = 'portfolios';

  let {
    open = $bindable(false),
    portfolioId,
    portfolioCurrency = 'USD',
    assets = [],
    onimported = () => {}
  } = $props();

  let accounts = $state([]);
  let accountId = $state('');
  let busy = $state(false);
  let error = $state('');
  let preview = $state(null);
  let report = $state(null);
  /** Per broker symbol: `{ skip, price }`. The user's edits, kept apart from the server's
   *  answer so a fresh preview never silently drops what was typed. */
  let edits = $state({});
  /** What each unmatched symbol is, answered here and sent at commit. */
  let symbolChoices = $state({});
  /** Symbols whose price is being read from the venue right now. */
  let quoting = $state({});
  /** The market that answered, per symbol: `{ pair, currency }`. */
  let quoted = $state({});

  const key = (s) => String(s ?? '').trim().toUpperCase();
  const account = $derived(accounts.find((a) => a.id === accountId) ?? null);
  const cap = $derived(account?.capability ?? null);
  const canQuote = $derived(cap?.quotes ?? false);
  const accountOpts = $derived(
    accounts.map((a) => ({
      value: a.id,
      label: isReady(a) ? a.name : `${a.name} · ${$t('brokers.incomplete')}`
    }))
  );

  /** The server's line plus what the user has since decided about it. */
  const rows = $derived(
    (preview?.lines ?? []).map((l) => {
      const k = key(l.source);
      const choice = symbolChoices[k];
      const state = choice
        ? choice.kind === 'skip'
          ? 'skip'
          : choice.kind === 'asset'
            ? 'chosen'
            : 'new'
        : l.state;
      const edit = edits[k] ?? {};
      // A symbol answered here points somewhere else than the server's match: the units
      // held are the newly chosen asset's, and a brand-new one holds none yet.
      const picked = choice?.kind === 'asset' ? assets.find((a) => a.id === choice.asset_id) : null;
      const held = choice ? (picked?.quantity ?? 0) : l.held_qty;
      const delta = l.broker_qty - held;
      const price = edit.price ?? (l.price == null ? '' : l.price);
      const resolved = state === 'matched' || state === 'chosen' || state === 'new';
      return {
        ...l,
        k,
        state,
        name: picked ? picked.name || picked.symbol : (choice?.name ?? l.name),
        currency: picked?.currency ?? choice?.currency ?? l.currency,
        held_qty: held,
        delta,
        price,
        skip: edit.skip ?? false,
        resolved,
        aligned: Math.abs(delta) < 1e-12,
        writes:
          resolved && !(edit.skip ?? false) && Math.abs(delta) >= 1e-12 && `${price}`.trim() !== ''
      };
    })
  );

  const willWrite = $derived(rows.filter((r) => r.writes).length);
  const needPrice = $derived(
    rows.filter(
      (r) => r.resolved && !r.skip && Math.abs(r.delta) >= 1e-12 && `${r.price}`.trim() === ''
    ).length
  );
  /** Lines about to be filed at today's price because the broker publishes no cost basis. */
  const atSpot = $derived(rows.filter((r) => r.writes && r.price_source !== 'broker').length);

  $effect(() => {
    if (open) boot();
  });

  async function boot() {
    error = '';
    report = null;
    try {
      accounts = await brokersApi.list(MODULE);
      const last = await portfoliosApi.brokerLast(portfolioId);
      const wanted = last?.account_id;
      accountId = accounts.some((a) => a.id === wanted) ? wanted : (accounts[0]?.id ?? '');
    } catch (e) {
      error = e.message;
    }
  }

  function onAccountChange(id) {
    accountId = id;
    preview = null;
    report = null;
    edits = {};
    symbolChoices = {};
    quoted = {};
  }

  async function runPreview() {
    if (!accountId) return;
    busy = true;
    error = '';
    report = null;
    try {
      preview = await portfoliosApi.brokerPreview(portfolioId, {
        account_id: accountId,
        symbols: symbolChoices
      });
    } catch (e) {
      error = e.message;
      preview = null;
    } finally {
      busy = false;
    }
  }

  async function runImport() {
    busy = true;
    error = '';
    try {
      report = await portfoliosApi.brokerCommit(portfolioId, {
        account_id: accountId,
        symbols: symbolChoices,
        lines: rows
          .filter((r) => r.resolved)
          .map((r) => ({
            symbol: r.source,
            skip: r.skip || !r.writes,
            price: `${r.price}`.trim() === '' ? null : Number(r.price)
          }))
      });
      preview = null;
      onimported();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  /** Ask the venue what this asset trades at, with the account's own credentials.
   *  The answer lands in the field and stays the user's to overwrite: an exchange prices
   *  BTC in USDT, so the market that answered is shown under the line rather than folded
   *  into a dollar figure nobody can check. */
  async function fetchPrice(row) {
    quoting = { ...quoting, [row.k]: true };
    error = '';
    try {
      const res = await portfoliosApi.brokerPrices(portfolioId, {
        account_id: accountId,
        symbols: [row.source]
      });
      const q = (res.quotes ?? []).find((x) => key(x.symbol) === row.k);
      if (!q) {
        throw new Error(
          $t('portfolios.broker.noQuote', { broker: cap?.label ?? '', symbol: row.source })
        );
      }
      setPrice(row.k, String(q.price));
      quoted = { ...quoted, [row.k]: { pair: q.pair, currency: q.currency } };
    } catch (e) {
      error = e.message;
    } finally {
      quoting = { ...quoting, [row.k]: false };
    }
  }

  function setPrice(k, value) {
    edits = { ...edits, [k]: { ...(edits[k] ?? {}), price: value } };
  }
  function toggleSkip(k) {
    edits = { ...edits, [k]: { ...(edits[k] ?? {}), skip: !(edits[k]?.skip ?? false) } };
  }

  function choose(source, choice) {
    const k = key(source);
    const next = { ...symbolChoices };
    if (choice) next[k] = choice;
    else delete next[k];
    symbolChoices = next;
  }

  const signed = (n) => `${n > 0 ? '+' : ''}${fmtNum(n, 8)}`;
</script>

<Modal bind:open size="lg" title={$t('portfolios.broker.title')}>
  <div class="wrap">
    {#if !accounts.length}
      <EmptyState
        icon="briefcase"
        title={$t('portfolios.broker.noAccount')}
        description={$t('portfolios.broker.noAccountHelp')}
      >
        {#snippet action()}
          <BrokerButton module={MODULE} onchanged={boot} />
        {/snippet}
      </EmptyState>
    {:else}
      <div class="head">
        <div class="fld">
          <span class="lbl">{$t('portfolios.broker.account')}</span>
          <Dropdown
            value={accountId}
            options={accountOpts}
            ariaLabel={$t('portfolios.broker.account')}
            onpick={onAccountChange}
          />
        </div>
        <BrokerButton module={MODULE} onchanged={boot} />
      </div>

      {#if cap}
        <p class="note">
          <Icon name="eye" size={12} />
          <span>{$t('portfolios.broker.readOnly')}</span>
        </p>
      {/if}

      <div class="actions">
        <Button onclick={runPreview} disabled={!accountId || busy}>
          {busy && !report ? $t('portfolios.broker.reading') : $t('portfolios.broker.read')}
        </Button>
        {#if preview && willWrite > 0}
          <Button variant="primary" onclick={runImport} disabled={busy}>
            {$t('portfolios.broker.import', { count: willWrite })}
          </Button>
        {/if}
      </div>

      <ErrorText {error} copyable />

      {#if report}
        <div class="report">
          <p class="big">
            <Icon name="check-circle" size={16} />
            {$t('portfolios.broker.done', {
              imported: report.imported,
              created: report.assets_created
            })}
          </p>
          <p class="sub">
            {$t('portfolios.broker.doneDetail', {
              skipped: report.skipped,
              duplicates: report.duplicates
            })}
          </p>
          {#each report.errors ?? [] as e}
            <p class="warn"><Icon name="alert-triangle" size={11} /> {e}</p>
          {/each}
        </div>
      {/if}

      {#if preview}
        <!-- Always: a symbol the server matched by itself is still the user's to point
             somewhere else. -->
        <ImportSymbols symbols={preview.symbols} {assets} {portfolioCurrency} onchoose={choose} />

        <!-- The one thing this import cannot know, said once above the table rather than
             whispered on every row. -->
        {#if atSpot}
          <p class="alert">
            <Icon name="alert-triangle" size={13} />
            <span>{$t('portfolios.broker.priceAlert', { count: atSpot })}</span>
          </p>
        {/if}
        {#if needPrice}
          <p class="alert need">
            <Icon name="alert-triangle" size={13} />
            <span>{$t('portfolios.broker.priceMissing', { count: needPrice })}</span>
          </p>
        {/if}

        {#if rows.length}
          <div class="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>{$t('portfolios.broker.asset')}</th>
                  <th class="num">{$t('portfolios.broker.atBroker')}</th>
                  <th class="num">{$t('portfolios.broker.inPortfolio')}</th>
                  <th class="num">{$t('portfolios.broker.difference')}</th>
                  <th class="num">{$t('portfolios.broker.price')}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {#each rows as r (r.k)}
                  <tr class:off={r.skip || !r.resolved || r.aligned}>
                    <td>
                      <span class="sym">{r.source}</span>
                      {#if r.name}<span class="nm">{r.name}</span>{/if}
                      {#if r.state === 'unresolved'}
                        <span class="pill warn">{$t('portfolios.broker.unmatched')}</span>
                      {:else if r.state === 'new'}
                        <span class="pill">{$t('portfolios.broker.willCreate')}</span>
                      {/if}
                    </td>
                    <td class="num">{fmtNum(r.broker_qty, 8)}</td>
                    <td class="num dim">{fmtNum(r.held_qty, 8)}</td>
                    <td class="num" class:up={r.delta > 0} class:down={r.delta < 0}>
                      {r.aligned ? '—' : signed(r.delta)}
                    </td>
                    <td class="num">
                      {#if r.aligned || !r.resolved}
                        <span class="dim">—</span>
                      {:else}
                        <span class="field">
                          <span class="price">
                            <input
                              type="number"
                              step="any"
                              min="0"
                              value={r.price}
                              oninput={(e) => setPrice(r.k, e.currentTarget.value)}
                            />
                            <span class="ccy">{r.currency}</span>
                          </span>
                          {#if canQuote}
                            <button
                              class="quote"
                              type="button"
                              title={$t('portfolios.broker.fetchPrice')}
                              aria-label={$t('portfolios.broker.fetchPrice')}
                              disabled={busy || quoting[r.k]}
                              onclick={() => fetchPrice(r)}
                            >
                              <Icon name="refresh-cw" size={14} />
                            </button>
                          {/if}
                        </span>
                        {#if quoted[r.k]}
                          <span class="src live">
                            {$t('portfolios.broker.quoted', {
                              broker: cap?.label ?? '',
                              pair: quoted[r.k].pair
                            })}
                          </span>
                          {#if quoted[r.k].currency !== r.currency}
                            <span class="src">
                              {$t('portfolios.broker.quoteCcy', {
                                quoted: quoted[r.k].currency,
                                asset: r.currency
                              })}
                            </span>
                          {/if}
                        {:else}
                          <span class="src" class:broker={r.price_source === 'broker'}>
                            {r.price_source === 'broker'
                              ? $t('portfolios.broker.fromBroker')
                              : $t('portfolios.broker.todayPrice')}
                          </span>
                        {/if}
                      {/if}
                    </td>
                    <td class="act">
                      {#if r.aligned}
                        <span class="pill ok">{$t('portfolios.broker.inSync')}</span>
                      {:else if r.resolved}
                        <label class="chk">
                          <input
                            type="checkbox"
                            checked={!r.skip}
                            onchange={() => toggleSkip(r.k)}
                          />
                          {$t('portfolios.broker.align')}
                        </label>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <p class="note">{$t('portfolios.broker.empty')}</p>
        {/if}
      {/if}
    {/if}
  </div>
</Modal>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 220px;
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .note,
  .alert {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.5;
  }
  .alert {
    padding: var(--space-2) var(--space-3);
    border: var(--hairline) solid var(--amber);
    border-radius: var(--radius);
    color: var(--amber);
  }
  .alert.need {
    border-color: var(--red);
    color: var(--red);
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
  .table-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  th {
    text-align: left;
    font-weight: var(--fw-medium);
    font-size: var(--text-xs);
    color: var(--muted);
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
  }
  td {
    padding: var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    vertical-align: top;
  }
  .num {
    text-align: right;
    font-family: var(--mono, ui-monospace, monospace);
  }
  tr.off td {
    opacity: 0.55;
  }
  .sym {
    font-weight: var(--fw-medium);
  }
  .nm {
    display: block;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .dim {
    color: var(--muted);
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .field {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }
  .price {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: var(--control-h);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    padding: 0 var(--space-2);
    transition: border-color var(--dur-fast) var(--ease);
  }
  /* One control, one frame: the box owns the hairline and the focus colour, the field
     inside it owns neither, not even while focused. */
  .price:focus-within {
    border-color: var(--accent);
    box-shadow: var(--ring);
  }
  .price input {
    width: 92px;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    text-align: right;
    font-family: var(--mono, ui-monospace, monospace);
    font-size: var(--text-sm);
  }
  .price input:focus {
    outline: none;
    box-shadow: none;
    border-color: transparent;
  }
  /* The native spinner is a second control inside the frame, and a price is typed. */
  .price input {
    appearance: textfield;
    -moz-appearance: textfield;
  }
  .price input::-webkit-outer-spin-button,
  .price input::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .quote {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--control-h);
    height: var(--control-h);
    padding: 0;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--muted);
    cursor: pointer;
    transition: border-color var(--dur-fast) var(--ease);
  }
  .quote:hover:not(:disabled) {
    color: var(--accent);
    border-color: var(--accent);
  }
  .quote:disabled {
    opacity: 0.5;
    cursor: progress;
  }
  .src.live {
    color: var(--green);
  }
  .ccy {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .src {
    display: block;
    margin-top: 2px;
    font-size: var(--text-xs);
    color: var(--amber);
  }
  .src.broker {
    color: var(--muted);
  }
  .pill {
    display: inline-block;
    margin-left: var(--space-1);
    padding: 0 var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 999px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .pill.warn {
    border-color: var(--amber);
    color: var(--amber);
  }
  .pill.ok {
    border-color: var(--green);
    color: var(--green);
  }
  .act {
    white-space: nowrap;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--text);
    cursor: pointer;
  }
  .report {
    border: var(--hairline) solid var(--green);
    border-radius: var(--radius);
    padding: var(--space-3);
  }
  .report .big {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    color: var(--green);
  }
  .report .sub {
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .warn {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--amber);
  }
</style>
