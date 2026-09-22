<script>
  // Fill the tax form from a broker account: the realized gains of one tax year, per line.
  //
  // Read only, and stored nowhere. The server pulls a window of fills, folds them into
  // closed positions and adds up what each disposal realized; this applies the three
  // numbers to the form, which the user saves as usual.
  //
  // The window is the subtle part, and it is the user's to set. A sale made in March was
  // bought at some earlier date, and without that purchase there is no cost basis: the
  // pull therefore starts where the field says, defaulting to 1 January of the year, and a
  // sale whose purchase is outside it comes back as an error naming the fix rather than a
  // gain measured against nothing.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import SymbolHelp from '$lib/ui/SymbolHelp.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import BrokerButton from '$lib/brokers/BrokerButton.svelte';
  import { brokersApi, isReady } from '$lib/brokers/api.js';
  import { taxcalcApi, fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  const MODULE = 'taxcalc';

  let { open = $bindable(false), profileCurrency = '', onapply = () => {} } = $props();

  let accounts = $state([]);
  let accountId = $state('');
  let taxYear = $state(new Date().getFullYear());
  let from = $state('');
  let symbols = $state([]);
  let symbolDraft = $state('');
  let suggestions = $state([]);
  let loadingSymbols = $state(false);
  let busy = $state(false);
  let error = $state('');
  let preview = $state(null);

  const account = $derived(accounts.find((a) => a.id === accountId) ?? null);
  const cap = $derived(account?.capability ?? null);
  const accountOpts = $derived(
    accounts.map((a) => ({
      value: a.id,
      label: isReady(a) ? a.name : `${a.name} · ${$t('brokers.incomplete')}`
    }))
  );
  const ready = $derived(!!accountId && (!cap?.needs_symbols || symbols.length > 0));

  $effect(() => {
    if (open) boot();
  });

  async function boot() {
    error = '';
    try {
      accounts = await brokersApi.list(MODULE);
      if (!accounts.some((a) => a.id === accountId)) accountId = accounts[0]?.id ?? '';
      if (!from) from = `${taxYear}-01-01`;
    } catch (e) {
      error = e.message;
    }
  }

  function onAccountChange(id) {
    accountId = id;
    preview = null;
    suggestions = [];
  }

  function onYearChange() {
    preview = null;
    from = `${taxYear}-01-01`;
  }

  async function loadSuggestions() {
    if (!accountId) return;
    loadingSymbols = true;
    error = '';
    try {
      suggestions = await brokersApi.symbols(accountId);
    } catch (e) {
      error = e.message;
    } finally {
      loadingSymbols = false;
    }
  }

  function addSymbol(raw) {
    const s = (raw ?? symbolDraft).trim().toUpperCase();
    if (!s || symbols.includes(s)) {
      symbolDraft = '';
      return;
    }
    symbols = [...symbols, s];
    symbolDraft = '';
  }
  const dropSymbol = (s) => (symbols = symbols.filter((x) => x !== s));

  async function read() {
    busy = true;
    error = '';
    preview = null;
    try {
      preview = await taxcalcApi.brokerPreview({
        account_id: accountId,
        tax_year: Number(taxYear),
        from: from || null,
        symbols,
        currency: profileCurrency || null
      });
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  function apply() {
    if (!preview) return;
    onapply({
      realized_capital_gains: preview.capital_gains,
      derivative_gains: preview.derivative_gains,
      crypto_gains: preview.crypto_gains,
      currency: preview.currency
    });
    open = false;
  }

  const day = (iso) => (iso ? String(iso).slice(0, 10) : '');
</script>

<Modal bind:open title={$t('taxcalc.broker.title')} size="lg">
  <div class="wrap">
    {#if !accounts.length}
      <EmptyState
        icon="briefcase"
        title={$t('taxcalc.broker.noAccount')}
        description={$t('taxcalc.broker.noAccountHelp')}
      >
        {#snippet action()}
          <BrokerButton module={MODULE} onchanged={boot} />
        {/snippet}
      </EmptyState>
    {:else}
      <div class="row">
        <div class="grow fld">
          <span>{$t('taxcalc.broker.account')}</span>
          <Dropdown
            value={accountId}
            options={accountOpts}
            ariaLabel={$t('taxcalc.broker.account')}
            onpick={onAccountChange}
          />
        </div>
        <label class="fld">
          <span>{$t('taxcalc.broker.taxYear')}</span>
          <input type="number" bind:value={taxYear} onchange={onYearChange} />
        </label>
        <label class="fld">
          <span>{$t('taxcalc.broker.readFrom')}</span>
          <input type="date" bind:value={from} onchange={() => (preview = null)} />
        </label>
        <BrokerButton module={MODULE} onchanged={boot} />
      </div>

      <p class="note">
        <Icon name="info" size={12} />
        <span>{$t('taxcalc.broker.windowHint')}</span>
      </p>

      {#if cap?.needs_symbols || symbols.length}
        <div class="fld">
          <span>
            {$t('taxcalc.broker.symbols')}
            {#if cap?.needs_symbols}<span class="req">*</span>{/if}
            <!-- The broker answers its own spelling, so the shapes it takes sit here. -->
            <SymbolHelp provider={account?.broker ?? ''} />
          </span>
          <div class="tagbox">
            {#each symbols as s (s)}
              <span class="tag">
                {s}
                <button class="drop" onclick={() => dropSymbol(s)} aria-label={$t('common.remove')}>
                  <Icon name="x" size={10} />
                </button>
              </span>
            {/each}
            <input
              bind:value={symbolDraft}
              placeholder={$t('taxcalc.broker.symbolPlaceholder')}
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ',') {
                  e.preventDefault();
                  addSymbol();
                }
              }}
              onblur={() => addSymbol()}
            />
          </div>
          <div class="suggest">
            <Button variant="ghost" onclick={loadSuggestions} disabled={loadingSymbols}>
              {loadingSymbols ? $t('common.loading') : $t('taxcalc.broker.suggest')}
            </Button>
            {#each suggestions.slice(0, 40) as s (s)}
              {#if !symbols.includes(s)}
                <button class="chip" onclick={() => addSymbol(s)}>{s}</button>
              {/if}
            {/each}
          </div>
        </div>
      {/if}

      <div class="acts">
        <Button onclick={read} disabled={!ready || busy}>
          {busy ? $t('taxcalc.broker.reading') : $t('taxcalc.broker.read')}
        </Button>
      </div>

      <ErrorText {error} copyable />

      {#if preview}
        <div class="result">
          <p class="muted small">
            {$t('taxcalc.broker.summary', {
              closed: preview.closed,
              year: preview.tax_year,
              executions: preview.executions
            })}
          </p>
          <table class="tbl">
            <tbody>
              <tr>
                <td>{$t('taxcalc.loadJournal.capitalGains')}</td>
                <td class="num">{fmtMoney(preview.capital_gains, preview.currency)}</td>
              </tr>
              <tr>
                <td>{$t('taxcalc.loadJournal.derivativeGains')}</td>
                <td class="num">{fmtMoney(preview.derivative_gains, preview.currency)}</td>
              </tr>
              <tr>
                <td>{$t('taxcalc.loadJournal.cryptoGains')}</td>
                <td class="num">{fmtMoney(preview.crypto_gains, preview.currency)}</td>
              </tr>
              <tr class="sub">
                <td>{$t('taxcalc.broker.feesDeducted')}</td>
                <td class="num">{fmtMoney(preview.fees, preview.currency)}</td>
              </tr>
            </tbody>
          </table>

          {#if preview.still_open || preview.before_year}
            <p class="muted small">
              {$t('taxcalc.broker.leftOut', {
                open: preview.still_open,
                before: preview.before_year
              })}
            </p>
          {/if}

          <!-- A gain in the wrong money is worse than a gain nobody counted, so an
               unconverted disposal is named and left out of the totals. -->
          {#if preview.unconverted?.length}
            <p class="warn small">
              <Icon name="alert-triangle" size={11} />
              {$t('taxcalc.broker.unconverted', {
                count: preview.unconverted.length,
                currency: preview.currency
              })}
              <span class="mono">{preview.unconverted.slice(0, 8).join(', ')}</span>
            </p>
          {/if}

          {#if preview.errors?.length}
            <div class="errs">
              <p class="warn small">
                <Icon name="alert-triangle" size={11} />
                {$t('taxcalc.broker.fillErrors', { count: preview.errors.length })}
              </p>
              {#each preview.errors.slice(0, 6) as e}
                <p class="errline">{e}</p>
              {/each}
            </div>
          {/if}

          {#if preview.lines?.length}
            <details>
              <summary>{$t('taxcalc.broker.showLines', { count: preview.lines.length })}</summary>
              <div class="table-wrap">
                <table class="tbl lines">
                  <thead>
                    <tr>
                      <th>{$t('taxcalc.broker.instrument')}</th>
                      <th>{$t('taxcalc.broker.closedOn')}</th>
                      <th class="num">{$t('taxcalc.broker.realized')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each preview.lines as l (l.ticker + l.closed_at)}
                      <tr>
                        <td>
                          {l.ticker}
                          <span class="muted">{l.asset_class}</span>
                        </td>
                        <td class="muted">{day(l.closed_at)}</td>
                        <td class="num" class:up={l.pnl > 0} class:down={l.pnl < 0}>
                          {fmtMoney(l.pnl, l.currency)}
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </details>
          {/if}

          <p class="muted small">{$t('taxcalc.broker.applyHint')}</p>
        </div>
      {/if}
    {/if}
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={apply} disabled={!preview}>
      {$t('taxcalc.broker.applyToForm')}
    </button>
  {/snippet}
</Modal>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
    align-items: flex-end;
    flex-wrap: wrap;
  }
  .grow {
    flex: 1;
    min-width: 180px;
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .req {
    color: var(--red);
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.5;
  }
  /* One control, one frame: the field inside the tag box is borderless. */
  .tagbox {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    align-items: center;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-1);
  }
  .tagbox input {
    flex: 1;
    min-width: 110px;
    border: none;
    background: transparent;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 1px var(--space-2);
    border-radius: var(--radius);
    background: var(--surface-2);
    font-family: var(--mono, ui-monospace, monospace);
    font-size: var(--text-xs);
  }
  .drop {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
  }
  .suggest {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    align-items: center;
    margin-top: var(--space-1);
  }
  .chip {
    padding: 1px var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text);
  }
  .acts {
    display: flex;
    gap: var(--space-2);
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .tbl {
    width: 100%;
    border-collapse: collapse;
  }
  .tbl td,
  .tbl th {
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--text-sm);
  }
  .tbl th {
    text-align: left;
    font-size: var(--text-xs);
    color: var(--muted);
    font-weight: var(--fw-medium);
  }
  .tbl tr.sub td {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-xs);
  }
  .mono {
    font-family: var(--mono, ui-monospace, monospace);
  }
  .warn {
    display: flex;
    align-items: baseline;
    gap: var(--space-1);
    flex-wrap: wrap;
    color: var(--amber);
    margin: 0;
  }
  .errs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .errline {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .table-wrap {
    max-height: 260px;
    overflow: auto;
  }
  summary {
    font-size: var(--text-xs);
    color: var(--muted);
    cursor: pointer;
  }
</style>
